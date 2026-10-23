import AppKit
import CoreGraphics

func settle(_ seconds:Double=0.35) {
    let end=Date().addingTimeInterval(seconds)
    while Date()<end { RunLoop.current.run(mode:.default,before:Date().addingTimeInterval(0.01)); maintainPresentation(cancelled:false) }
}
func fixture(_ title:String,_ x:CGFloat) -> NSWindow {
    let window=NSWindow(contentRect:NSRect(x:x,y:350,width:500,height:180),styleMask:[.titled,.closable,.resizable],backing:.buffered,defer:false)
    window.title=title; window.isReleasedWhenClosed=false
    window.contentView?.addSubview(NSTextField(labelWithString:"Synthetic presentation test only."))
    window.orderFrontRegardless(); settle()
    return window
}
func descriptor(_ window:NSWindow) throws -> [String:Any] {
    guard let info=windowInfos().first(where:{($0[kCGWindowNumber as String] as? Int)==window.windowNumber}),
          let data=info[kCGWindowBounds as String] as? [String:Any],
          let frame=CGRect(dictionaryRepresentation:data as CFDictionary),let identity=processIdentity(getpid()) else { throw failure("test_failed","Synthetic window not visible") }
    return ["id":String(window.windowNumber),"pid":getpid(),"process_identity":identity,"bounds":rect(frame),"title":window.title]
}
func saveView(_ window:NSWindow,_ name:String) throws {
    let view=window.contentView!.superview!
    let bitmap=view.bitmapImageRepForCachingDisplay(in:view.bounds)!
    view.cacheDisplay(in:view.bounds,to:bitmap)
    try bitmap.representation(using:.png,properties:[:])!.write(to:URL(fileURLWithPath:CommandLine.arguments[1]).appendingPathComponent("\(name).png"))
}
func check(_ condition:Bool,_ message:String) throws {
    if !condition { throw failure("test_failed",message) }
}
@main struct PresentationTest {
    static func main() throws {
        guard #available(macOS 14.4,*) else {throw failure("unsupported_environment","Self-window sharing checks require macOS 14.4+")}
        guard CommandLine.arguments.count==2 else {throw failure("invalid_params","Pass an evidence output directory")}
        let app=NSApplication.shared; app.setActivationPolicy(.regular); app.finishLaunching()
        let first=fixture("Presentation fixture one",400)
        defer { first.close(); do {try closePresentation()} catch {fputs("Presentation cleanup failed: \(error)\n",stderr)} }
        let descriptorOne=try descriptor(first)
        let frame=try bounds(descriptorOne),point=CGPoint(x:frame.minX+100,y:frame.minY+80)
        let physicalBefore=CGEvent(source:nil)!.location
        let foregroundBefore=NSWorkspace.shared.frontmostApplication?.processIdentifier
        try presentWindow(descriptorOne,capture:false)
        try presentCursor(point,window:descriptorOne)
        let cursorUnchanged=CGEvent(source:nil)!.location==physicalBefore
        let foregroundUnchanged=NSWorkspace.shared.frontmostApplication?.processIdentifier==foregroundBefore
        settle()
        let cursor=app.windows.first(where:{$0.title=="Computer Use agent pointer"})!
        try check(cursor.isVisible,"Independent cursor not visible")
        try check(cursor.ignoresMouseEvents && !cursor.canBecomeKey && !cursor.canBecomeMain,"Pointer must not receive input or focus")
        try check(!first.hasActiveWindowSharingSession,"capture=false must not produce sharing badge")
        try saveView(cursor,"pointer")
        try presentWindow(descriptorOne,capture:true); settle(2)
        try check(first.hasActiveWindowSharingSession,"Real SCStream did not produce system sharing indicator")
        try saveView(first,"sharing")
        try presentWindow(descriptorOne,capture:false); settle()
        try check(first.hasActiveWindowSharingSession,"Metadata-only observe stopped existing capture stream")
        let second=fixture("Presentation fixture two",930)
        defer { second.close() }
        try presentWindow(try descriptor(second),capture:true); settle(2)
        try check(!first.hasActiveWindowSharingSession && second.hasActiveWindowSharingSession,"Window switch did not transfer capture stream")
        maintainPresentation(cancelled:true); settle()
        try check(!second.hasActiveWindowSharingSession,"Cancellation did not stop capture stream")
        try check(!cursor.isVisible,"Cancellation did not remove cursor")
        try presentWindow(try descriptor(second),capture:true); settle(2)
        second.close(); maintainPresentation(cancelled:false); settle()
        try check(!second.hasActiveWindowSharingSession,"Closed target retained capture stream")
        try check(cursorUnchanged,"Presentation moved physical cursor during presentation call")
        try check(foregroundUnchanged,"Presentation changed foreground application during presentation call")
        let result:[String:Any]=["passed":true,"capture_false_no_badge":true,"independent_cursor_visible":true,"mouse_passthrough":true,"system_sharing_indicator":true,"window_switch_stops_old_stream":true,"cancel_stops_stream_and_cursor":true,"target_close_stops_stream":true,"physical_cursor_unchanged":true,"foreground_unchanged":true,"screen_permission_preflight":CGPreflightScreenCaptureAccess(),"scope":"Cursor and foreground compared across presentWindow+presentCursor calls only; user may move the pointer or change apps during later async capture tests"]
        let data=try JSONSerialization.data(withJSONObject:result,options:[.sortedKeys,.prettyPrinted])
        try data.write(to:URL(fileURLWithPath:CommandLine.arguments[1]).appendingPathComponent("results.json"))
        print(String(data:data,encoding:.utf8)!)
    }
}
