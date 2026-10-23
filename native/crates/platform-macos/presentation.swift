import AppKit
import ScreenCaptureKit

private final class PresentationReply<Value>: @unchecked Sendable {
    private let lock=NSLock()
    private var result:Result<Value,Error>?
    private var expired=false
    func complete(_ value:Result<Value,Error>) -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard !expired else { return false }
        result=value; return true
    }
    func read() -> Result<Value,Error>? {
        lock.lock(); defer { lock.unlock() }; return result
    }
    func expire() { lock.lock(); expired=true; lock.unlock() }
}

private func waitForPresentation<Value>(_ reply:PresentationReply<Value>, seconds:Double=3) throws -> Value {
    let deadline=Date().addingTimeInterval(seconds)
    while Date()<deadline {
        if let result=reply.read() { return try result.get() }
        RunLoop.current.run(mode:.default,before:Date().addingTimeInterval(0.01))
        pumpPresentationEvents()
    }
    reply.expire()
    throw failure("presentation_timeout", "Window presentation did not respond before its deadline")
}

func pumpPresentationEvents() {
    guard let app=NSApp else { return }
    for _ in 0..<16 {
        guard let event=app.nextEvent(matching:.any,until:Date(),inMode:.default,dequeue:true) else { break }
        app.sendEvent(event)
    }
    app.updateWindows()
}

private func presentationError(_ error:Error) {
    fputs("macOS presentation: \(error)\n",stderr)
}

private final class CursorPanel:NSPanel {
    override var canBecomeKey:Bool { false }
    override var canBecomeMain:Bool { false }
}

private final class AgentPointer:NSView {
    override var isFlipped:Bool { true }
    override func draw(_ dirtyRect:NSRect) {
        let path=NSBezierPath()
        path.move(to:NSPoint(x:2,y:2))
        for point in [NSPoint(x:2,y:27),NSPoint(x:9,y:21),NSPoint(x:14,y:32),
                      NSPoint(x:19,y:30),NSPoint(x:14,y:19),NSPoint(x:25,y:19)] { path.line(to:point) }
        path.close(); NSColor.systemPurple.setFill(); path.fill()
        path.lineWidth=2; path.lineJoinStyle = .round; NSColor.white.setStroke(); path.stroke()
    }
}

private final class WindowPresentation {
    var window:[String:Any]
    var stream:SCStream?
    var frames:SharedWindowFrames?
    var captureDimensions:CGSize?
    var panel:CursorPanel?
    var point:CGPoint?
    var checkedAt=Date.distantPast
    init(_ window:[String:Any]) { self.window=window }
}

private var activePresentation:WindowPresentation?
private var presentationApplicationReady=false
private var presentationInterruption:BridgeError?

func sharingFailure(_ error:Error) -> BridgeError {
    let error=error as NSError
    let userStopped=error.domain==SCStreamErrorDomain && error.code == -3817
    return failure(userStopped ? "user_interruption":"capture_stream_stopped",
                   "Window sharing stopped (\(error.domain), code \(error.code)): \(error.localizedDescription). Input is disabled; call computer_start explicitly for a new session")
}

func presentationInterruptionError() -> BridgeError? {
    presentationInterruption ?? activePresentation?.frames?.stoppedError().map(sharingFailure)
}

func presentationWasInterrupted() -> Bool {
    presentationInterruptionError() != nil
}

private func requirePresentationThread() throws {
    guard Thread.isMainThread else { throw failure("presentation_unavailable", "Window presentation requires the helper main thread") }
}

private func shareableWindow(_ window:[String:Any]) throws -> SCWindow {
    let id=try windowID(window)
    let reply=PresentationReply<SCShareableContent>()
    let completion:(SCShareableContent?,Error?)->Void = { content,error in
        if let error=error { _=reply.complete(.failure(error)) }
        else if let content=content { _=reply.complete(.success(content)) }
        else { _=reply.complete(.failure(failure("capture_failed", "No shareable window information returned"))) }
    }
    if (window["pid"] as? Int32)==getpid(),#available(macOS 14.4,*) {
        SCShareableContent.getCurrentProcessShareableContent(completionHandler:completion)
    } else {
        guard CGPreflightScreenCaptureAccess() else { throw failure("permission_denied", "Screen Recording permission is required for window sharing") }
        SCShareableContent.getExcludingDesktopWindows(false,onScreenWindowsOnly:true,completionHandler:completion)
    }
    let content=try waitForPresentation(reply)
    guard let target=content.windows.first(where:{$0.windowID==id && $0.owningApplication?.processID==(window["pid"] as? Int32)}) else {
        throw failure("invalid_window", "Selected window is not available for sharing")
    }
    return target
}

private func beginSharing(_ presentation:WindowPresentation) throws {
    let target=try shareableWindow(presentation.window)
    let (width,height)=try captureSize(bounds(presentation.window))
    let configuration=SCStreamConfiguration()
    configuration.width=width; configuration.height=height
    configuration.minimumFrameInterval=CMTime(value:1,timescale:15)
    configuration.showsCursor=false; configuration.capturesAudio=false
    configuration.ignoreShadowsSingleWindow=true; configuration.scalesToFit=true
    configuration.ignoreGlobalClipSingleWindow=true
    configuration.pixelFormat=kCVPixelFormatType_32BGRA; configuration.queueDepth=3
    let frames=SharedWindowFrames()
    let stream=SCStream(filter:SCContentFilter(desktopIndependentWindow:target),configuration:configuration,delegate:frames)
    try stream.addStreamOutput(frames,type:.screen,sampleHandlerQueue:DispatchQueue(label:"cu.presentation.frames"))
    let reply=PresentationReply<Bool>()
    stream.startCapture { error in
        let accepted=reply.complete(error.map { .failure($0) } ?? .success(true))
        if !accepted { stream.stopCapture { error in if let error=error { presentationError(error) } } }
    }
    _=try waitForPresentation(reply)
    presentation.stream=stream; presentation.frames=frames
    presentation.captureDimensions=CGSize(width:width,height:height)
}

private func endSharing(_ presentation:WindowPresentation) throws {
    guard let stream=presentation.stream else { return }
    let alreadyStopped=presentation.frames?.stoppedError() != nil
    presentation.stream=nil; presentation.frames=nil; presentation.captureDimensions=nil
    if alreadyStopped { return }
    let reply=PresentationReply<Bool>()
    stream.stopCapture { error in
        if let error=error as NSError?,error.domain==SCStreamErrorDomain,error.code == -3808 {
            _=reply.complete(.success(true))
        } else { _=reply.complete(error.map { .failure($0) } ?? .success(true)) }
    }
    _=try waitForPresentation(reply)
}

func sharedWindowFrames(_ window:[String:Any]) throws -> SharedWindowFrames {
    try presentWindow(window,capture:true)
    guard let frames=activePresentation?.frames else { throw failure("capture_stream_stopped","Selected window has no active sharing stream") }
    return frames
}

func presentWindow(_ window:[String:Any],capture:Bool) throws {
    try requirePresentationThread(); try validate(window)
    if let error=presentationInterruptionError() { throw error }
    let current=activePresentation
    if current?.window["id"] as? String != window["id"] as? String ||
       current?.window["process_identity"] as? String != window["process_identity"] as? String {
        try closePresentation()
        if !presentationApplicationReady {
            _=NSApplication.shared
            if NSApp.activationPolicy() == .prohibited { NSApp.setActivationPolicy(.accessory) }
            NSApp.finishLaunching(); presentationApplicationReady=true
        }
        activePresentation=WindowPresentation(window)
    }
    guard let presentation=activePresentation else { throw failure("presentation_unavailable", "No selected window") }
    presentation.window=window
    if capture,let dimensions=presentation.captureDimensions {
        let (width,height)=try captureSize(bounds(window))
        if dimensions != CGSize(width:width,height:height) { try endSharing(presentation) }
    }
    if capture && presentation.stream==nil { try beginSharing(presentation) }
}

private func makeCursorPanel() -> CursorPanel {
    let panel=CursorPanel(contentRect:NSRect(x:0,y:0,width:30,height:36),styleMask:[.borderless,.nonactivatingPanel],backing:.buffered,defer:false)
    panel.title="Computer Use agent pointer"; panel.isOpaque=false; panel.backgroundColor = .clear
    panel.hasShadow=true; panel.ignoresMouseEvents=true; panel.hidesOnDeactivate=false
    panel.collectionBehavior=[.canJoinAllSpaces,.fullScreenAuxiliary,.stationary]
    panel.isReleasedWhenClosed=false; panel.contentView=AgentPointer(frame:NSRect(x:0,y:0,width:30,height:36))
    return panel
}

private func updatePointer(_ presentation:WindowPresentation) {
    guard let panel=presentation.panel,let point=presentation.point else { return }
    guard let id=try? windowID(presentation.window),
          let target=windowInfos().first(where:{($0[kCGWindowNumber as String] as? UInt32)==id}),
          let level=target[kCGWindowLayer as String] as? Int else { panel.orderOut(nil); return }
    let topY=NSScreen.screens.first?.frame.maxY ?? CGDisplayBounds(CGMainDisplayID()).height
    panel.level=NSWindow.Level(rawValue:level)
    panel.setFrameOrigin(NSPoint(x:point.x-2,y:topY-point.y-34))
    panel.order(.above,relativeTo:Int(id))
    panel.displayIfNeeded()
}

func isPresentationWindow(_ id:UInt32) -> Bool {
    activePresentation?.panel?.windowNumber==Int(id)
}

func presentCursor(_ point:CGPoint,window:[String:Any]) throws {
    try requirePresentationThread(); try validate(window)
    guard point.x.isFinite,point.y.isFinite,try bounds(window).contains(point) else {
        throw failure("out_of_bounds", "Agent pointer is outside the selected window")
    }
    if activePresentation?.window["id"] as? String != window["id"] as? String ||
       activePresentation?.window["process_identity"] as? String != window["process_identity"] as? String {
        try presentWindow(window,capture:false)
    }
    guard let presentation=activePresentation else { throw failure("presentation_unavailable", "No selected window") }
    presentation.window=window; presentation.point=point
    if presentation.panel==nil { presentation.panel=makeCursorPanel() }
    updatePointer(presentation)
    RunLoop.current.run(mode:.default,before:Date())
    pumpPresentationEvents()
}

func maintainPresentation(cancelled:Bool) {
    guard Thread.isMainThread else { presentationError(failure("presentation_unavailable", "Presentation maintenance requires the helper main thread")); return }
    RunLoop.current.run(mode:.default,before:Date())
    pumpPresentationEvents()
    guard let presentation=activePresentation else { return }
    if cancelled {
        do { try closePresentation() } catch { presentationError(error) }; return
    }
    guard Date().timeIntervalSince(presentation.checkedAt)>0.2 else { return }
    presentation.checkedAt=Date()
    do { try requireInteractiveDesktop() }
    catch let error as BridgeError {
        presentationInterruption=error
        do { try closePresentation() } catch { presentationError(error) }
        return
    } catch { presentationError(error) }
    if let error=presentation.frames?.stoppedError() {
        presentationInterruption=sharingFailure(error)
        presentation.stream=nil
        do {try closePresentation()} catch {presentationError(error)}
        return
    }
    do { try refreshPresentation(presentation); updatePointer(presentation) }
    catch {
        do { try closePresentation() } catch { presentationError(error) }
    }
}

private func refreshPresentation(_ presentation:WindowPresentation) throws {
    let window=presentation.window,id=try windowID(window)
    guard let pid=window["pid"] as? Int32,
          processIdentity(pid)==(window["process_identity"] as? String),
          let info=windowInfos().first(where:{($0[kCGWindowNumber as String] as? UInt32)==id && ($0[kCGWindowOwnerPID as String] as? Int32)==pid}),
          let data=info[kCGWindowBounds as String] as? [String:Any],
          let frame=CGRect(dictionaryRepresentation:data as CFDictionary) else {
        throw failure("stale_window", "Presentation target is no longer visible")
    }
    if frame != (try bounds(window)) {
        presentation.window["bounds"]=rect(frame)
        presentation.point=nil; presentation.panel?.orderOut(nil)
    }
}

func closePresentation() throws {
    try requirePresentationThread()
    guard let presentation=activePresentation else { return }
    activePresentation=nil
    presentation.panel?.orderOut(nil); presentation.panel?.close(); presentation.panel=nil
    try endSharing(presentation)
}
