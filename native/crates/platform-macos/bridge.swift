import Foundation
import AppKit
import ApplicationServices
import ScreenCaptureKit

struct BridgeError: Error { let code: String; let message: String; var mayHaveExecuted=false }
func failure(_ code: String, _ message: String) -> BridgeError { BridgeError(code: code, message: message) }
func afterDispatch<T>(_ operation:() throws -> T) throws -> T {
    do { return try operation() }
    catch let error as BridgeError {
        throw BridgeError(code:error.code,message:error.message,mayHaveExecuted:true)
    }
    catch { throw BridgeError(code:"os_error",message:String(describing:error),mayHaveExecuted:true) }
}
func bridgeFailureData(_ error:Error) -> [String:Any] {
    let failure=error as? BridgeError ?? BridgeError(code:"os_error",message:String(describing:error))
    return ["code":failure.code,"message":failure.message,"may_have_executed":failure.mayHaveExecuted]
}
private func now() -> UInt64 { UInt64(Date().timeIntervalSince1970 * 1000) }
private func identifier() -> String { UUID().uuidString }
func rect(_ value: CGRect) -> [String: Any] { ["x": value.origin.x, "y": value.origin.y, "width": value.width, "height": value.height] }
func bounds(_ value: [String: Any]) throws -> CGRect {
    guard let b = value["bounds"] as? [String: Any],
          let nx=b["x"] as? NSNumber, let ny=b["y"] as? NSNumber,
          let nw=b["width"] as? NSNumber, let nh=b["height"] as? NSNumber else { throw failure("invalid_window", "Missing window bounds") }
    guard [nx,ny,nw,nh].allSatisfy({CFGetTypeID($0) != CFBooleanGetTypeID()}) else { throw failure("invalid_window", "Window bounds cannot contain booleans") }
    let x=nx.doubleValue,y=ny.doubleValue,w=nw.doubleValue,h=nh.doubleValue
    guard x.isFinite, y.isFinite, w.isFinite, h.isFinite, w>0, h>0 else { throw failure("invalid_window", "Window bounds must be finite and positive") }
    return CGRect(x:x,y:y,width:w,height:h)
}
func windowID(_ value: [String: Any]) throws -> CGWindowID {
    guard let id = value["id"] as? String, let number=UInt32(id) else { throw failure("invalid_window", "Invalid window ID") }
    return number
}
func windowInfos() -> [[String: Any]] {
    CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
}
func requireInteractiveDesktop() throws {
    let session=CGSessionCopyCurrentDictionary() as? [String:Any]
    if (session?["CGSSessionScreenIsLocked"] as? Bool)==true ||
       NSWorkspace.shared.frontmostApplication?.bundleIdentifier=="com.apple.loginwindow" {
        throw failure("desktop_unavailable","The macOS desktop is locked or showing the login screen. The user must unlock it; do not capture or control the login screen")
    }
}
func processIdentity(_ pid:Int32) -> String? {
    var info=proc_bsdinfo()
    let size=Int32(MemoryLayout<proc_bsdinfo>.size)
    guard proc_pidinfo(pid,PROC_PIDTBSDINFO,0,&info,size)==size else { return nil }
    return "\(pid):\(info.pbi_start_tvsec):\(info.pbi_start_tvusec)"
}
func isApplicationWindow(_ info:[String:Any], regularPIDs:Set<Int32>) -> Bool {
    guard let layer=info[kCGWindowLayer as String] as? Int,
          let pid=info[kCGWindowOwnerPID as String] as? Int32,
          (info[kCGWindowAlpha as String] as? Double ?? 1)>0 else { return false }
    return layer==0 || layer>0 && regularPIDs.contains(pid)
}
func listWindows() -> [[String: Any]] {
    let regularPIDs=Set(NSWorkspace.shared.runningApplications.filter { $0.activationPolicy == .regular }.map(\.processIdentifier))
    return windowInfos().compactMap { info in
        guard isApplicationWindow(info,regularPIDs:regularPIDs),
              let pid=info[kCGWindowOwnerPID as String] as? Int32,
              let id=info[kCGWindowNumber as String] as? UInt32,
              !isPresentationWindow(id),
              let data=info[kCGWindowBounds as String] as? [String:Any],
              let frame=CGRect(dictionaryRepresentation:data as CFDictionary), frame.width>0, frame.height>0,
              let identity=processIdentity(pid) else { return nil }
        return ["id":String(id), "pid":pid, "process_identity":identity,
                "title":info[kCGWindowName as String] as? String ?? "", "bounds":rect(frame)]
    }
}
func validate(_ window: [String: Any]) throws {
    try requireInteractiveDesktop()
    let id=try windowID(window)
    guard let current=listWindows().first(where: { ($0["id"] as? String)==String(id) }),
          (current["pid"] as? Int32)==(window["pid"] as? Int32),
          (current["process_identity"] as? String)==(window["process_identity"] as? String),
          try bounds(current)==bounds(window) else { throw failure("stale_window", "Window identity or geometry changed") }
}
func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
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
func axWindow(_ window: [String: Any]) throws -> AXUIElement {
    try requireAccessibility()
    guard let pid=window["pid"] as? Int32 else { throw failure("invalid_window", "Missing PID") }
    let app=AXUIElementCreateApplication(pid); AXUIElementSetMessagingTimeout(app,0.25)
    guard let windows=attribute(app,kAXWindowsAttribute) as? [AXUIElement] else { throw failure("accessibility_unavailable", "Application does not expose AX windows") }
    let target=try bounds(window)
    let candidates=windows.filter { element in
        guard let b=axBounds(element) else { return false }
        return abs(b.minX-target.minX)<2 && abs(b.minY-target.minY)<2 && abs(b.width-target.width)<2 && abs(b.height-target.height)<2
    }
    guard !candidates.isEmpty else { throw failure("stale_window", "No AX window matches the current geometry; re-list and observe after the window settles") }
    guard candidates.count==1 else { throw failure("ambiguous_window", "More than one AX window matches the current geometry") }
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
func requireAccessibility() throws {
    guard AXIsProcessTrusted() else { throw failure("permission_denied", "Accessibility permission must be granted by the user in System Settings") }
}
private func isForeground(_ window: [String:Any]) throws -> Bool {
    let id=try windowID(window)
    guard let pid=window["pid"] as? Int32,
          NSWorkspace.shared.frontmostApplication?.processIdentifier==pid,
          let first=listWindows().first(where:{($0["pid"] as? Int32)==pid}) else { return false }
    return (first["id"] as? String)==String(id)
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
private func applicationURL(_ target:String) throws -> URL {
    let url:URL?
    if target.hasPrefix("/") {
        url=URL(fileURLWithPath:target).standardizedFileURL.resolvingSymlinksInPath()
    } else { url=NSWorkspace.shared.urlForApplication(withBundleIdentifier:target) }
    guard let url, url.pathExtension=="app", let bundle=Bundle(url:url),
          bundle.executableURL != nil else { throw failure("application_not_found", "Use an installed application's bundle identifier or absolute .app path") }
    return url
}
private func listApplications(_ profiles:[[String:Any]]) throws -> [[String:Any]] {
    let windows=listWindows()
    let running=NSWorkspace.shared.runningApplications.filter { $0.activationPolicy == .regular }
    if profiles.isEmpty {
        return running.compactMap { app in
            guard let path=app.bundleURL?.path else { return nil }
            return ["id":app.bundleIdentifier ?? path,"display_name":app.localizedName ?? path,"is_running":true,
                    "windows":windows.filter { ($0["pid"] as? Int32)==app.processIdentifier }]
        }
    }
    return try profiles.map { profile in
        guard let id=profile["id"] as? String, let target=profile["executable"] as? String else { throw failure("invalid_params", "Invalid application profile") }
        let url=try applicationURL(target)
        let app=running.first { $0.bundleURL?.standardizedFileURL.resolvingSymlinksInPath()==url }
        return ["id":id,"display_name":profile["display_name"] as? String ?? id,"is_running":app != nil,
                "windows":app.map { app in windows.filter { ($0["pid"] as? Int32)==app.processIdentifier } } ?? []]
    }
}
private final class ApplicationLaunchBox: @unchecked Sendable {
    var app:NSRunningApplication?
    var error:Error?
}
private func launchApplication(_ profile:[String:Any]) throws -> [[String:Any]] {
    guard let target=profile["executable"] as? String,
          let args=profile["args"] as? [String] else { throw failure("invalid_params", "Invalid application launch") }
    let url=try applicationURL(target)
    let config=NSWorkspace.OpenConfiguration()
    config.arguments=args
    config.activates=false
    config.createsNewApplicationInstance=false
    let box=ApplicationLaunchBox()
    return try afterDispatch {
        NSWorkspace.shared.openApplication(at:url,configuration:config) { app,error in
            box.app=app; box.error=error
        }
        let deadline=ProcessInfo.processInfo.systemUptime+3
        repeat {
            RunLoop.current.run(until:Date().addingTimeInterval(0.02))
            if let error=box.error { throw error }
            if let app=box.app {
                let windows=listWindows().filter { ($0["pid"] as? Int32)==app.processIdentifier }
                if !windows.isEmpty { return windows }
            }
        } while ProcessInfo.processInfo.systemUptime<deadline
        return []
    }
}
private func prepare(_ window:[String:Any]) throws -> Bool {
    try requireAccessibility()
    if try isForeground(window) { return true }
    let element=try axWindow(window)
    guard let pid=window["pid"] as? Int32, let app=NSRunningApplication(processIdentifier:pid) else { throw failure("invalid_window", "Application exited") }
    return try afterDispatch {
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
}
private func query(_ request: [String: Any]) throws -> Any {
    let method=request["method"] as? String ?? ""
    if method=="presentation_error" { return presentationInterruptionError().map(bridgeFailureData) as Any? ?? NSNull() }
    if method=="close_presentation" { try closePresentation(); return true }
    if method=="list_apps" { return try listApplications(request["apps"] as? [[String:Any]] ?? []) }
    if method=="launch_app" {
        guard let app=request["app"] as? [String:Any] else { throw failure("invalid_params", "Missing application") }
        return try launchApplication(app)
    }
    if method=="permissions" {
        let requestAccessibility=request["request_accessibility"] as? Bool ?? false
        let requestScreenRecording=request["request_screen_recording"] as? Bool ?? false
        let accessibility:Bool
        if requestAccessibility && !AXIsProcessTrusted() {
            let options=[kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String:true] as CFDictionary
            accessibility=AXIsProcessTrustedWithOptions(options)
        } else { accessibility=AXIsProcessTrusted() }
        let screenRecording=CGPreflightScreenCaptureAccess()
        if requestScreenRecording && !screenRecording { _ = CGRequestScreenCaptureAccess() }
        return ["accessibility":accessibility,"screen_recording":CGPreflightScreenCaptureAccess()]
    }
    if method=="capabilities" {
        let desktopAvailable=(try? requireInteractiveDesktop()) != nil
        let reason:Any = !desktopAvailable ? "The macOS desktop is locked or showing the login screen" : AXIsProcessTrusted() ? NSNull() as Any : "Accessibility permission is missing" as Any
        return ["protocol_version":1,"platform":"macos","backend":"ax-screencapturekit-coregraphics","available":AXIsProcessTrusted() && desktopAvailable,
                "reason":reason,
                "actions":["activate_window","click","type_text","key","scroll","drag"],"screenshot_methods":["screencapturekit-stream"],
                "limitations":["macOS 14+; Accessibility and Screen Recording permissions are separate","Visible windows and application popups require unambiguous AX association; no set_value","Window-targeted input uses dynamically resolved CGEventSetWindowLocation; no global HID fallback","An independent pointer does not guarantee isolated keyboard focus in every application"]]
    }
    if method=="list" { return listWindows() }
    guard let window=request["window"] as? [String:Any] else { throw failure("invalid_params", "Missing window") }
    try validate(window)
    if method=="present" {
        try presentWindow(window,capture:request["screenshot"] as? Bool ?? false)
        return true
    }
    if method=="validate" { return true }
    if method=="prepare" { return try prepare(window) }
    if method=="observe" {
        let elements=(request["accessibility"] as? Bool ?? false) ? try tree(window) : []
        let images=(request["screenshot"] as? Bool ?? false) ? [try captureSharedWindow(window)] : []
        try validate(window); let timestamp=now()
        var warnings=["AX traversal is bounded; AX and capture are not an atomic snapshot"]
        if let front=listWindows().first(where:{($0["pid"] as? Int32)==(window["pid"] as? Int32)}),
           let id=front["id"] as? String, id != window["id"] as? String {
            warnings.append("Another application window is in front (id \(id)). Window-only screenshots exclude separate popups. Call computer_list_windows and observe that window before interacting with it.")
        }
        return ["id":identifier(),"helper_epoch":"","window":window,"captured_at_ms":timestamp,"expires_at_ms":timestamp+30_000,
                "elements":elements,"images":images,"warnings":warnings]
    }
    throw failure("invalid_params", "Unknown bridge query")
}

private func dispatch(_ input:UnsafePointer<CChar>,_ operation:([String:Any]) throws -> Any)->UnsafeMutablePointer<CChar>? {
    let response:[String:Any]
    do { let data=Data(String(cString:input).utf8);guard let request=try JSONSerialization.jsonObject(with:data) as? [String:Any] else { throw failure("invalid_params", "Invalid JSON request") };response=["result":try operation(request)] }
    catch { response=["error":bridgeFailureData(error)] }
    guard let data=try? JSONSerialization.data(withJSONObject:response,options:[.fragmentsAllowed]),let string=String(data:data,encoding:.utf8) else { return nil }
    return strdup(string)
}
@_cdecl("cu_macos_query") public func cuMacosQuery(_ input:UnsafePointer<CChar>)->UnsafeMutablePointer<CChar>? { dispatch(input,query) }
@_cdecl("cu_macos_perform") public func cuMacosPerform(_ input:UnsafePointer<CChar>,_ context:UnsafeRawPointer?,_ cancelled:@convention(c)(UnsafeRawPointer?)->Int32)->UnsafeMutablePointer<CChar>? { dispatch(input) { try perform($0,context,cancelled) } }
@_cdecl("cu_macos_capture") public func cuMacosCapture(_ input:UnsafePointer<CChar>,_ context:UnsafeRawPointer?,_ cancelled:@convention(c)(UnsafeRawPointer?)->Int32)->UnsafeMutablePointer<CChar>? {
    dispatch(input) { try captureSharedWindow($0,cancelled:{cancelled(context) != 0}) }
}
@_cdecl("cu_macos_free") public func cuMacosFree(_ value:UnsafeMutablePointer<CChar>) { free(value) }
@_cdecl("cu_macos_maintain") public func cuMacosMaintain(_ cancelled:Int32) -> Int32 {
    maintainPresentation(cancelled:cancelled != 0)
    return presentationWasInterrupted() ? 1:0
}
@_cdecl("cu_macos_is_presentation_window") public func cuMacosIsPresentationWindow(_ id:UInt32) -> Int32 { isPresentationWindow(id) ? 1:0 }
