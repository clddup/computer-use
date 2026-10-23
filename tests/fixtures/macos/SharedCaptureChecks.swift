import AppKit
import CoreGraphics

private final class CaptureView:NSView {
    var color=NSColor.systemRed
    override func draw(_ dirtyRect:NSRect) { color.setFill(); bounds.fill() }
}

@main struct SharedCaptureChecks {
    static func check(_ value:Bool,_ message:String) throws {
        if !value { throw failure("test_failed",message) }
    }
    static func descriptor(_ window:NSWindow) throws -> [String:Any] {
        guard let info=windowInfos().first(where:{($0[kCGWindowNumber as String] as? Int)==window.windowNumber}),
              let data=info[kCGWindowBounds as String] as? [String:Any],
              let frame=CGRect(dictionaryRepresentation:data as CFDictionary),let identity=processIdentity(getpid()) else {
            throw failure("test_failed","Background synthetic window missing")
        }
        return ["id":String(window.windowNumber),"pid":getpid(),"process_identity":identity,"bounds":rect(frame),"title":window.title]
    }
    static func image(_ window:[String:Any],_ name:String) throws -> NSBitmapImageRep {
        let capture=try captureSharedWindow(window)
        try check(capture["capture_method"] as? String == "screencapturekit-stream","Not captured from the sharing stream")
        let data=Data(base64Encoded:capture["data"] as! String)!
        try data.write(to:URL(fileURLWithPath:CommandLine.arguments[1]).appendingPathComponent(name+".png"))
        return NSBitmapImageRep(data:data)!
    }
    private static func changedImage(_ selected:[String:Any]) throws -> NSBitmapImageRep {
        let deadline=Date().addingTimeInterval(3)
        var attempt=0
        repeat {
            let result=try image(selected,"blue-attempt-\(attempt)")
            let color=result.colorAt(x:40,y:result.pixelsHigh/2)!.usingColorSpace(.deviceRGB)!
            if color.blueComponent>color.redComponent+0.3 {
                try result.representation(using:.png,properties:[:])!.write(to:URL(fileURLWithPath:CommandLine.arguments[1]).appendingPathComponent("shared-blue.png"))
                return result
            }
            attempt+=1
        } while Date()<deadline
        throw failure("test_failed","Shared frame did not reflect changed window content before the deadline")
    }
    static func main() throws {
        guard #available(macOS 14.4,*) else { throw failure("unsupported_environment","Self-window sharing requires macOS 14.4+") }
        guard CommandLine.arguments.count==2 else { throw failure("invalid_params","Pass the evidence directory") }
        do { try requireInteractiveDesktop() }
        catch {
            let result:[String:Any]=["status":"blocked","error":bridgeFailureData(error),"screen_permission_preflight":CGPreflightScreenCaptureAccess()]
            let data=try JSONSerialization.data(withJSONObject:result,options:[.prettyPrinted,.sortedKeys])
            try data.write(to:URL(fileURLWithPath:CommandLine.arguments[1]).appendingPathComponent("results.json"))
            fputs("Shared-window check blocked: \(error)\n",stderr); exit(2)
        }
        let app=NSApplication.shared; app.setActivationPolicy(.accessory)
        let window=NSWindow(contentRect:NSRect(x:180,y:220,width:360,height:240),styleMask:[.titled,.closable],backing:.buffered,defer:false)
        window.title="Background shared-frame fixture"; window.isReleasedWhenClosed=false
        let view=CaptureView(frame:window.contentView!.bounds); window.contentView=view
        window.orderBack(nil); window.makeKey(); view.display()
        let ready=Date().addingTimeInterval(0.35)
        while Date()<ready { RunLoop.current.run(mode:.default,before:Date().addingTimeInterval(0.01)); pumpPresentationEvents() }
        do { try verify(window,view) }
        catch { fputs("Shared-window verification failed: \(error)\n",stderr); exit(1) }
    }
    private static func verify(_ window:NSWindow,_ view:CaptureView) throws {
        defer { window.close(); do {try closePresentation()} catch {fputs("Cleanup failed: \(error)\n",stderr)} }
        let selected=try descriptor(window)
        let foreground=NSWorkspace.shared.frontmostApplication?.processIdentifier
        try check(foreground != getpid(),"Synthetic fixture was already foreground before capture")
        let first=try image(selected,"shared-red")
        try check(first.pixelsWide==720 && first.pixelsHigh<600,"Captured desktop instead of selected window")
        let red=first.colorAt(x:40,y:first.pixelsHigh/2)!.usingColorSpace(.deviceRGB)!
        try check(red.redComponent>red.blueComponent+0.3,"Background window red pixels missing")
        try check(window.hasActiveWindowSharingSession,"No actual system sharing session")
        for _ in 0..<4 { _=try captureSharedWindow(selected) }
        view.color = .systemBlue; view.display()
        let second=try changedImage(selected)
        let blue=second.colorAt(x:40,y:second.pixelsHigh/2)!.usingColorSpace(.deviceRGB)!
        try check(blue.blueComponent>blue.redComponent+0.3,"Shared frame did not reflect changed window content")
        do { _=try captureSharedWindow(selected,cancelled:{true}); throw failure("test_failed","Cancelled capture returned an image") }
        catch let error as BridgeError { try check(error.code=="cancelled","Cancellation lost its error code") }
        try closePresentation(); try closePresentation()
        try check(!window.hasActiveWindowSharingSession,"Stop did not release the sharing stream")
        let foregroundAfter=NSWorkspace.shared.frontmostApplication?.processIdentifier
        try check(foregroundAfter != getpid(),"Background capture activated the synthetic application")
        let result:[String:Any]=["passed":true,"one_background_window":true,"stream_only":true,"idle_recapture_count":4,"changed_pixels_verified":true,"cancel_does_not_return_image":true,"stop_idempotent":true,"foreground_unchanged_sample":foreground==foregroundAfter,"target_not_activated":true,"screen_permission_preflight":CGPreflightScreenCaptureAccess()]
        let data=try JSONSerialization.data(withJSONObject:result,options:[.prettyPrinted,.sortedKeys])
        try data.write(to:URL(fileURLWithPath:CommandLine.arguments[1]).appendingPathComponent("results.json"))
        print(String(data:data,encoding:.utf8)!)
    }
}
