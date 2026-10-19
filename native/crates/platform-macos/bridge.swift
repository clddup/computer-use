import Foundation
import AppKit
import ApplicationServices
import ScreenCaptureKit

private struct BridgeError: Error { let code: String; let message: String }
private func failure(_ code: String, _ message: String) -> BridgeError { BridgeError(code: code, message: message) }
private func now() -> UInt64 { UInt64(Date().timeIntervalSince1970 * 1000) }
private func identifier() -> String { UUID().uuidString }
private func rect(_ value: CGRect) -> [String: Any] { ["x": value.origin.x, "y": value.origin.y, "width": value.width, "height": value.height] }
func bounds(_ value: [String: Any]) throws -> CGRect {
    guard let b = value["bounds"] as? [String: Any],
          let nx=b["x"] as? NSNumber, let ny=b["y"] as? NSNumber,
          let nw=b["width"] as? NSNumber, let nh=b["height"] as? NSNumber else { throw failure("invalid_window", "Missing window bounds") }
    guard [nx,ny,nw,nh].allSatisfy({CFGetTypeID($0) != CFBooleanGetTypeID()}) else { throw failure("invalid_window", "Window bounds cannot contain booleans") }
    let x=nx.doubleValue,y=ny.doubleValue,w=nw.doubleValue,h=nh.doubleValue
    guard x.isFinite, y.isFinite, w.isFinite, h.isFinite, w>0, h>0 else { throw failure("invalid_window", "Window bounds must be finite and positive") }
    return CGRect(x:x,y:y,width:w,height:h)
}
private func windowID(_ value: [String: Any]) throws -> CGWindowID {
    guard let id = value["id"] as? String, let number=UInt32(id) else { throw failure("invalid_window", "Invalid window ID") }
    return number
}
private func windowInfos() -> [[String: Any]] {
    CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
}
func processIdentity(_ pid:Int32) -> String? {
    var info=proc_bsdinfo()
    let size=Int32(MemoryLayout<proc_bsdinfo>.size)
    guard proc_pidinfo(pid,PROC_PIDTBSDINFO,0,&info,size)==size else { return nil }
    return "\(pid):\(info.pbi_start_tvsec):\(info.pbi_start_tvusec)"
}
private func listWindows() -> [[String: Any]] {
    windowInfos().compactMap { info in
        guard (info[kCGWindowLayer as String] as? Int) == 0,
              let pid=info[kCGWindowOwnerPID as String] as? Int32,
              let id=info[kCGWindowNumber as String] as? UInt32,
              let data=info[kCGWindowBounds as String] as? [String:Any],
              let frame=CGRect(dictionaryRepresentation:data as CFDictionary), frame.width>0, frame.height>0,
              let identity=processIdentity(pid) else { return nil }
        return ["id":String(id), "pid":pid, "process_identity":identity,
                "title":info[kCGWindowName as String] as? String ?? "", "bounds":rect(frame)]
    }
}
private func validate(_ window: [String: Any]) throws {
    let id=try windowID(window)
    guard let current=listWindows().first(where: { ($0["id"] as? String)==String(id) }),
          (current["pid"] as? Int32)==(window["pid"] as? Int32),
          (current["process_identity"] as? String)==(window["process_identity"] as? String),
          try bounds(current)==bounds(window) else { throw failure("stale_window", "Window identity or geometry changed") }
}
private func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var result: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element,name as CFString,&result) == .success else { return nil }
    return result
}
private func axBounds(_ element: AXUIElement) -> CGRect? {
    guard let p=attribute(element,kAXPositionAttribute), CFGetTypeID(p)==AXValueGetTypeID(),
          let s=attribute(element,kAXSizeAttribute), CFGetTypeID(s)==AXValueGetTypeID() else { return nil }
    var point=CGPoint.zero; var size=CGSize.zero
    guard AXValueGetValue(p as! AXValue,.cgPoint,&point), AXValueGetValue(s as! AXValue,.cgSize,&size) else { return nil }
    return CGRect(origin:point,size:size)
}
private func axWindow(_ window: [String: Any]) throws -> AXUIElement {
    try requireAccessibility()
    guard let pid=window["pid"] as? Int32 else { throw failure("invalid_window", "Missing PID") }
    let app=AXUIElementCreateApplication(pid); AXUIElementSetMessagingTimeout(app,0.25)
    guard let windows=attribute(app,kAXWindowsAttribute) as? [AXUIElement] else { throw failure("accessibility_unavailable", "Application does not expose AX windows") }
    let target=try bounds(window)
    let candidates=windows.filter { element in
        guard let b=axBounds(element) else { return false }
        return abs(b.minX-target.minX)<2 && abs(b.minY-target.minY)<2 && abs(b.width-target.width)<2 && abs(b.height-target.height)<2
    }
    guard candidates.count==1 else { throw failure("ambiguous_window", "AX window association is not unique") }
    return candidates[0]
}
private func tree(_ window: [String: Any]) throws -> [[String: Any]] {
    let root=try axWindow(window); var stack:[(AXUIElement,Int,String)]=[(root,0,"root")]; var elements:[[String:Any]]=[]
    let deadline=Date().addingTimeInterval(3)
    while let (element,depth,path)=stack.popLast(), elements.count<512, Date()<deadline {
        AXUIElementSetMessagingTimeout(element,0.25)
        let role=attribute(element,kAXRoleAttribute) as? String ?? "unknown"
        let subrole=attribute(element,kAXSubroleAttribute) as? String ?? ""
        let password=subrole=="AXSecureTextField" || (attribute(element,"AXProtectedContent") as? Bool ?? false)
        var settable=DarwinBoolean(false)
        AXUIElementIsAttributeSettable(element,kAXValueAttribute as CFString,&settable)
        let value:Any = password ? NSNull() : (attribute(element,kAXValueAttribute) as? String).map { String($0.prefix(4096)) } as Any? ?? NSNull()
        elements.append(["id":identifier(),"role":role,"name":password ? "[password]" : (attribute(element,kAXTitleAttribute) as? String ?? attribute(element,kAXDescriptionAttribute) as? String ?? ""),
                         "value":value,"bounds":axBounds(element).map(rect) as Any? ?? NSNull(),"editable":settable.boolValue && !password,"password":password,"depth":depth,"locator":path])
        if depth>=16 || password { continue }
        let children=attribute(element,kAXChildrenAttribute) as? [AXUIElement] ?? []
        for (index,child) in children.prefix(128).enumerated().reversed() { stack.append((child,depth+1,"\(path)/\(index)")) }
    }
    return elements
}
private func requireAccessibility() throws {
    guard AXIsProcessTrusted() else { throw failure("permission_denied", "Accessibility permission must be granted by the user in System Settings") }
}
private func isForeground(_ window: [String:Any]) throws -> Bool {
    let id=try windowID(window)
    guard let pid=window["pid"] as? Int32,
          NSWorkspace.shared.frontmostApplication?.processIdentifier==pid,
          let first=windowInfos().first(where:{($0[kCGWindowLayer as String] as? Int)==0 && ($0[kCGWindowOwnerPID as String] as? Int32)==pid}) else { return false }
    return (first[kCGWindowNumber as String] as? UInt32)==id
}
func captureSize(_ frame:CGRect) throws -> (Int,Int) {
    let width=(frame.width*2).rounded(.up),height=(frame.height*2).rounded(.up)
    guard width.isFinite,height.isFinite,width>=1,height>=1,width*height<=16_000_000 else {
        throw failure("capture_limit", "Requested capture exceeds pixel limit")
    }
    return (Int(width),Int(height))
}
func boundedInteger(_ value:Double,_ minimum:Int,_ maximum:Int) throws -> Int {
    guard value.isFinite,value>=Double(minimum),value<=Double(maximum),value.rounded()==value else {
        throw failure("invalid_params", "Expected an integer within \(minimum)..\(maximum)")
    }
    return Int(value)
}
func pointerEvent(_ type:CGEventType,_ button:CGMouseButton,_ position:CGPoint,_ clickState:Int64=1) throws -> CGEvent {
    guard let event=CGEvent(mouseEventSource:nil,mouseType:type,mouseCursorPosition:position,mouseButton:button) else {
        throw failure("input_failed", "Cannot allocate pointer event")
    }
    event.setIntegerValueField(.mouseEventClickState,value:clickState)
    return event
}
private func capture(_ window: [String: Any]) throws -> [String: Any] {
    guard CGPreflightScreenCaptureAccess() else { throw failure("permission_denied", "Screen Recording permission must be granted by the user") }
    let id=try windowID(window); let frame=try bounds(window)
    let (pixelWidth,pixelHeight)=try captureSize(frame)
    let semaphore=DispatchSemaphore(value:0)
    let box=CaptureBox()
    let task=Task {
        do {
            let content=try await SCShareableContent.excludingDesktopWindows(false,onScreenWindowsOnly:true)
            guard let target=content.windows.first(where:{$0.windowID==id}) else { throw failure("invalid_window", "Window is not capturable") }
            try Task.checkCancellation()
            let filter=SCContentFilter(desktopIndependentWindow:target)
            let configuration=SCStreamConfiguration()
            configuration.width=pixelWidth; configuration.height=pixelHeight
            configuration.showsCursor=false
            configuration.ignoreShadowsSingleWindow=true
            configuration.scalesToFit=true
            let image=try await SCScreenshotManager.captureImage(contentFilter:filter,configuration:configuration)
            try Task.checkCancellation()
            guard image.width*image.height<=16_000_000,
                  let data=NSBitmapImageRep(cgImage:image).representation(using:.png,properties:[:]),data.count<=9_000_000 else { throw failure("capture_limit", "Capture exceeds size limit") }
            box.result = .success(["id":identifier(),"mime_type":"image/png","data":data.base64EncodedString(),"width":image.width,"height":image.height,"screen_bounds":rect(frame),"capture_method":"screencapturekit"])
        } catch { box.result = .failure(error) }
        semaphore.signal()
    }
    guard semaphore.wait(timeout:.now()+8) == .success, let result=box.result else {
        task.cancel()
        throw failure("capture_timeout", "ScreenCaptureKit timed out")
    }
    return try result.get()
}
private final class CaptureBox: @unchecked Sendable { var result: Result<[String:Any],Error>? }
private func query(_ request: [String: Any]) throws -> Any {
    let method=request["method"] as? String ?? ""
    if method=="permissions" {
        let request=request["request"] as? Bool ?? false
        let accessibility:Bool
        if request {
            let options=[kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String:true] as CFDictionary
            accessibility=AXIsProcessTrustedWithOptions(options)
        } else { accessibility=AXIsProcessTrusted() }
        let screenRecording=CGPreflightScreenCaptureAccess()
        if request && !screenRecording { _ = CGRequestScreenCaptureAccess() }
        return ["accessibility":accessibility,"screen_recording":CGPreflightScreenCaptureAccess()]
    }
    if method=="capabilities" {
        return ["protocol_version":1,"platform":"macos","backend":"ax-screencapturekit-coregraphics","available":AXIsProcessTrusted(),
                "reason":AXIsProcessTrusted() ? NSNull() : "Accessibility permission is missing" as Any,
                "actions":["activate_window","click","type_text","key","scroll","drag"],"screenshot_methods":["screencapturekit"],
                "limitations":["macOS 14+; Accessibility and Screen Recording permissions are separate","Only visible windows with unambiguous AX association; no set_value","Input requires selected foreground window; no protected desktop"]]
    }
    if method=="list" { return listWindows() }
    guard let window=request["window"] as? [String:Any] else { throw failure("invalid_params", "Missing window") }
    try validate(window)
    if method=="validate" { return true }
    if method=="prepare" {
        let element=try axWindow(window)
        guard let pid=window["pid"] as? Int32, let app=NSRunningApplication(processIdentifier:pid) else { throw failure("invalid_window", "Application exited") }
        guard app.activate(options:[]) else { throw failure("focus_failed", "Could not activate target application") }
        guard AXUIElementPerformAction(element,kAXRaiseAction as CFString) == .success else { throw failure("focus_failed", "Could not raise target AX window") }
        let deadline=ProcessInfo.processInfo.systemUptime+1.5
        repeat {
            try validate(window)
            if try isForeground(window) { return true }
            RunLoop.current.run(until:Date().addingTimeInterval(0.01))
        } while ProcessInfo.processInfo.systemUptime<deadline
        throw failure("focus_failed", "Target window did not become foreground before the activation deadline")
    }
    if method=="observe" {
        let elements=(request["accessibility"] as? Bool ?? false) ? try tree(window) : []
        let images=(request["screenshot"] as? Bool ?? false) ? [try capture(window)] : []
        try validate(window); let timestamp=now()
        return ["id":identifier(),"helper_epoch":"","window":window,"captured_at_ms":timestamp,"expires_at_ms":timestamp+30_000,
                "elements":elements,"images":images,"warnings":["AX traversal is bounded; AX and capture are not an atomic snapshot"]]
    }
    throw failure("invalid_params", "Unknown bridge query")
}

private let keycodes:[String:CGKeyCode] = ["a":0,"s":1,"d":2,"f":3,"h":4,"g":5,"z":6,"x":7,"c":8,"v":9,"b":11,"q":12,"w":13,"e":14,"r":15,"y":16,"t":17,"1":18,"2":19,"3":20,"4":21,"6":22,"5":23,"9":25,"7":26,"8":28,"0":29,"o":31,"u":32,"i":34,"p":35,"l":37,"j":38,"k":40,"n":45,"m":46,"enter":36,"return":36,"tab":48,"space":49,"backspace":51,"escape":53,"esc":53,"meta":55,"command":55,"shift":56,"alt":58,"control":59,"ctrl":59,"left":123,"right":124,"down":125,"up":126,"delete":117,"home":115,"end":119,"pageup":116,"pagedown":121]
private func modifier(_ code:CGKeyCode)->CGEventFlags { switch code { case 55:return .maskCommand; case 56:return .maskShift;case 58:return .maskAlternate;case 59:return .maskControl;default:return [] } }
private func perform(_ request:[String:Any],_ context:UnsafeRawPointer?,_ cancelled:@convention(c)(UnsafeRawPointer?)->Int32) throws -> String {
    guard let window=request["window"] as? [String:Any],let action=request["action"] as? [String:Any],let kind=action["kind"] as? String,
          window["pid"] as? Int32 != nil else { throw failure("invalid_params", "Missing input arguments") }
    if kind=="set_value" { throw failure("unsupported_action", "Use focus and type_text on macOS") }
    func check() throws {
        if cancelled(context) != 0 { throw failure("cancelled", "Session stopped") }
        try requireAccessibility()
        try validate(window)
        guard try isForeground(window) else { throw failure("focus_changed", "Selected window is not foreground") }
    }
    try check()
    let physical=CGEventSource.flagsState(.combinedSessionState)
    if !physical.intersection([.maskCommand,.maskShift,.maskControl,.maskAlternate]).isEmpty { throw failure("user_input_active", "Release physically held modifiers") }
    var heldKeys:[CGKeyCode]=[];var flags:CGEventFlags=[];var heldMouse:CGMouseButton?=nil;var position=CGPoint.zero
    func key(_ code:CGKeyCode,_ down:Bool,_ text:String?=nil) throws {
        guard let event=CGEvent(keyboardEventSource:nil,virtualKey:code,keyDown:down) else { throw failure("input_failed", "Cannot allocate keyboard event") }
        event.flags=flags
        if let text=text {let units=Array(text.utf16);units.withUnsafeBufferPointer { event.keyboardSetUnicodeString(stringLength:units.count,unicodeString:$0.baseAddress!) }}
        event.post(tap:.cghidEventTap)
    }
    func mouse(_ type:CGEventType,_ button:CGMouseButton,_ p:CGPoint,_ clickState:Int64=1) throws {
        try pointerEvent(type,button,p,clickState).post(tap:.cghidEventTap)
    }
    defer {
        for code in heldKeys.reversed() { flags.subtract(modifier(code)); try? key(code,false) }
        if let button=heldMouse { try? mouse(button == .left ? .leftMouseUp : button == .right ? .rightMouseUp : .otherMouseUp,button,position) }
    }
    func move(_ x:Double,_ y:Double,_ dragged:Bool=false) throws {
        try check();let point=CGPoint(x:x,y:y)
        guard try bounds(window).contains(point) else { throw failure("out_of_bounds", "Pointer outside window") }
        let system=AXUIElementCreateSystemWide()
        AXUIElementSetMessagingTimeout(system,0.25)
        var hit:AXUIElement?
        guard AXUIElementCopyElementAtPosition(system,Float(point.x),Float(point.y),&hit) == .success,
              let hit=hit else { throw failure("target_occluded", "Cannot determine the accessibility target at pointer position") }
        var hitPID:Int32=0
        guard AXUIElementGetPid(hit,&hitPID) == .success,hitPID==(window["pid"] as? Int32) else {
            throw failure("target_occluded", "Pointer target belongs to another application")
        }
        let hitWindow=attribute(hit,kAXWindowAttribute).flatMap { value -> AXUIElement? in
            CFGetTypeID(value)==AXUIElementGetTypeID() ? (value as! AXUIElement) : nil
        } ?? hit
        guard CFEqual(hitWindow,try axWindow(window)) else { throw failure("target_occluded", "Pointer target belongs to another window") }
        position=point;try mouse(dragged ? .leftMouseDragged : .mouseMoved,.left,point)
    }
    func number(_ name:String)->Double { action[name] as? Double ?? 0 }
    switch kind {
    case "click":
        try move(number("x"),number("y"));let name=action["button"] as? String ?? "left"
        let button:CGMouseButton=name=="right" ? .right : name=="middle" ? .center : .left
        let count=try boundedInteger(number("click_count"),1,3)
        for index in 1...count {
            try check(); heldMouse=button;try mouse(button == .left ? .leftMouseDown : button == .right ? .rightMouseDown : .otherMouseDown,button,position,Int64(index))
            try mouse(button == .left ? .leftMouseUp : button == .right ? .rightMouseUp : .otherMouseUp,button,position,Int64(index));heldMouse=nil
            if count > 1 { Thread.sleep(forTimeInterval:0.02) }
        }
    case "type_text":
        for character in action["text"] as? String ?? "" { try check();heldKeys.append(0);try key(0,true,String(character));try key(0,false,String(character));heldKeys.removeLast() }
    case "key":
        let names=action["keys"] as? [String] ?? []
        let codes=try names.map { name -> CGKeyCode in guard let code=keycodes[name.lowercased()] else { throw failure("unsupported_key", "Unsupported key \(name)") };return code }
        for code in codes {try check();heldKeys.append(code);flags.formUnion(modifier(code));try key(code,true)}
        for code in codes.reversed() {flags.subtract(modifier(code));try key(code,false);heldKeys.removeLast()}
    case "scroll":
        let dx=try boundedInteger(number("dx"),-20,20),dy=try boundedInteger(number("dy"),-20,20)
        try move(number("x"),number("y"));guard let event=CGEvent(scrollWheelEvent2Source:nil,units:.line,wheelCount:2,wheel1:-Int32(dy),wheel2:-Int32(dx),wheel3:0) else { throw failure("input_failed", "Cannot allocate scroll event") };event.location=position;event.post(tap:.cghidEventTap)
    case "drag":
        let duration=try boundedInteger(number("duration_ms"),50,3000)
        try move(number("from_x"),number("from_y"));heldMouse = .left;try mouse(.leftMouseDown,.left,position)
        let steps=duration/10
        for step in 1...steps { let t=Double(step)/Double(steps);try move(number("from_x")+(number("to_x")-number("from_x"))*t,number("from_y")+(number("to_y")-number("from_y"))*t,true);Thread.sleep(forTimeInterval:0.01) }
        try mouse(.leftMouseUp,.left,position);heldMouse=nil
    default: throw failure("unsupported_action", "Unknown input action")
    }
    return "coregraphics"
}
private func dispatch(_ input:UnsafePointer<CChar>,_ operation:([String:Any]) throws -> Any)->UnsafeMutablePointer<CChar>? {
    let response:[String:Any]
    do { let data=Data(String(cString:input).utf8);guard let request=try JSONSerialization.jsonObject(with:data) as? [String:Any] else { throw failure("invalid_params", "Invalid JSON request") };response=["result":try operation(request)] }
    catch let error as BridgeError { response=["error":["code":error.code,"message":error.message]] }
    catch { response=["error":["code":"os_error","message":String(describing:error)]] }
    guard let data=try? JSONSerialization.data(withJSONObject:response,options:[.fragmentsAllowed]),let string=String(data:data,encoding:.utf8) else { return nil }
    return strdup(string)
}
@_cdecl("cu_macos_query") public func cuMacosQuery(_ input:UnsafePointer<CChar>)->UnsafeMutablePointer<CChar>? { dispatch(input,query) }
@_cdecl("cu_macos_perform") public func cuMacosPerform(_ input:UnsafePointer<CChar>,_ context:UnsafeRawPointer?,_ cancelled:@convention(c)(UnsafeRawPointer?)->Int32)->UnsafeMutablePointer<CChar>? { dispatch(input) { try perform($0,context,cancelled) } }
@_cdecl("cu_macos_free") public func cuMacosFree(_ value:UnsafeMutablePointer<CChar>) { free(value) }
