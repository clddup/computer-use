import Foundation
import AppKit
import ApplicationServices

private typealias WindowLocationSetter = @convention(c) (CGEvent, CGPoint) -> Void
private let windowLocationSetter:WindowLocationSetter? = {
    guard let library=dlopen("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics",RTLD_LAZY),
          let symbol=dlsym(library,"CGEventSetWindowLocation") else { return nil }
    return unsafeBitCast(symbol,to:WindowLocationSetter.self)
}()

func targetedInputAvailable() -> Bool { windowLocationSetter != nil }
func locateTargetedEvent(_ event:CGEvent, point:CGPoint, window:[String:Any]) throws {
    guard let setLocation=windowLocationSetter else {
        throw failure("unsupported_environment","Targeted input requires CGEventSetWindowLocation; global input fallback is disabled")
    }
    let frame=try bounds(window), id=try windowID(window)
    event.location=point
    event.setIntegerValueField(.mouseEventWindowUnderMousePointer,value:Int64(id))
    event.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent,value:Int64(id))
    // NSEvent's window number is distinct from CG's hit-test window IDs, including for scroll events.
    event.setIntegerValueField(CGEventField(rawValue:51)!,value:Int64(id))
    setLocation(event,CGPoint(x:point.x-frame.minX,y:point.y-frame.minY))
    guard NSEvent(cgEvent:event)?.windowNumber==Int(id) else {
        throw failure("unsupported_environment","The OS did not preserve the targeted event's window number")
    }
}
func targetedPointerEvent(_ type:CGEventType,_ button:CGMouseButton,_ point:CGPoint,
                          _ clickState:Int64,window:[String:Any],flags:CGEventFlags=[]) throws -> CGEvent {
    let frame=try bounds(window)
    let local=CGPoint(x:point.x-frame.minX,y:frame.maxY-point.y)
    guard let kind=NSEvent.EventType(rawValue:UInt(type.rawValue)),
          let native=NSEvent.mouseEvent(with:kind,location:local,modifierFlags:NSEvent.ModifierFlags(rawValue:UInt(flags.rawValue)),
              timestamp:ProcessInfo.processInfo.systemUptime,windowNumber:Int(try windowID(window)),context:nil,
              eventNumber:0,clickCount:Int(clickState),pressure:1),
          let event=native.cgEvent else { throw failure("input_failed","Cannot allocate window-targeted pointer event") }
    event.flags=flags
    try locateTargetedEvent(event,point:point,window:window)
    return event
}

private let keycodes:[String:CGKeyCode] = ["a":0,"s":1,"d":2,"f":3,"h":4,"g":5,"z":6,"x":7,"c":8,"v":9,"b":11,"q":12,"w":13,"e":14,"r":15,"y":16,"t":17,"1":18,"2":19,"3":20,"4":21,"6":22,"5":23,"9":25,"7":26,"8":28,"0":29,"o":31,"u":32,"i":34,"p":35,"l":37,"j":38,"k":40,"n":45,"m":46,"enter":36,"return":36,"tab":48,"space":49,"backspace":51,"escape":53,"esc":53,"meta":55,"command":55,"shift":56,"alt":58,"control":59,"ctrl":59,"left":123,"right":124,"down":125,"up":126,"delete":117,"home":115,"end":119,"pageup":116,"pagedown":121]
private func modifier(_ code:CGKeyCode)->CGEventFlags {
    switch code {case 55:return .maskCommand;case 56:return .maskShift;case 58:return .maskAlternate;case 59:return .maskControl;default:return []}
}

private final class TargetedInput {
    let window:[String:Any]
    let pid:Int32
    let checkCancellation:()->Bool
    var heldKeys:[CGKeyCode]=[]
    var flags:CGEventFlags=[]
    var heldMouse:CGMouseButton?
    var position=CGPoint.zero
    var dispatched=false

    init(window:[String:Any],checkCancellation:@escaping ()->Bool) throws {
        guard let pid=window["pid"] as? Int32 else { throw failure("invalid_params","Missing target PID") }
        self.window=window;self.pid=pid;self.checkCancellation=checkCancellation
        guard targetedInputAvailable() else { throw failure("unsupported_environment","Window-targeted input is unavailable; no global fallback") }
        try check()
    }
    func check() throws {
        if checkCancellation() { throw failure("cancelled","Session stopped") }
        if let error=presentationInterruptionError() { throw error }
        try requireAccessibility();try validate(window)
        _ = try axWindow(window)
    }
    func post(_ event:CGEvent) {
        dispatched=true
        event.postToPid(pid)
    }
    func key(_ code:CGKeyCode,_ down:Bool,_ text:String?=nil) throws {
        guard let event=CGEvent(keyboardEventSource:nil,virtualKey:code,keyDown:down) else { throw failure("input_failed","Cannot allocate keyboard event") }
        event.flags=flags
        if let text {
            let units=Array(text.utf16)
            units.withUnsafeBufferPointer {event.keyboardSetUnicodeString(stringLength:units.count,unicodeString:$0.baseAddress!)}
        }
        post(event)
    }
    func mouse(_ type:CGEventType,_ button:CGMouseButton,_ point:CGPoint,_ count:Int64=1) throws {
        let event=try targetedPointerEvent(type,button,point,count,window:window,flags:flags)
        post(event)
    }
    func move(_ point:CGPoint,dragged:Bool=false) throws {
        try check()
        guard try bounds(window).contains(point) else { throw failure("out_of_bounds","Pointer outside selected window") }
        let app=AXUIElementCreateApplication(pid);AXUIElementSetMessagingTimeout(app,0.25)
        var hit:AXUIElement?
        guard AXUIElementCopyElementAtPosition(app,Float(point.x),Float(point.y),&hit) == .success,let hit else {
            throw failure("target_occluded","Cannot resolve the target application's accessibility hit")
        }
        let root=attribute(hit,kAXWindowAttribute).flatMap { value -> AXUIElement? in
            CFGetTypeID(value)==AXUIElementGetTypeID() ? (value as! AXUIElement):nil
        } ?? hit
        guard CFEqual(root,try axWindow(window)) else { throw failure("target_occluded","Pointer target belongs to another application window") }
        position=point
        try presentCursor(point,window:window)
        try mouse(dragged ? .leftMouseDragged:.mouseMoved,.left,point)
    }
    func release() {
        guard processIdentity(pid)==window["process_identity"] as? String else { return }
        do {
            for code in heldKeys.reversed() {flags.subtract(modifier(code));try key(code,false)}
            if let button=heldMouse {try mouse(button == .left ? .leftMouseUp:button == .right ? .rightMouseUp:.otherMouseUp,button,position)}
        } catch {fputs("Targeted input cleanup failed: \(error)\n",stderr)}
    }
    func run(_ action:[String:Any]) throws -> String {
        defer { release() }
        do {
            switch action["kind"] as? String {
            case "click":try click(action)
            case "key":try pressKeys(action)
            case "type_text":try typeText(action["text"] as? String ?? "")
            case "scroll":try scroll(action)
            case "drag":try drag(action)
            default:throw failure("unsupported_action","Unsupported targeted input action")
            }
            return "coregraphics-targeted"
        } catch {
            if dispatched {return try afterDispatch {throw error}}
            throw error
        }
    }
    func click(_ action:[String:Any]) throws {
        try move(CGPoint(x:number(action,"x"),y:number(action,"y")))
        let name=action["button"] as? String ?? "left"
        let button:CGMouseButton=name=="right" ? .right:name=="middle" ? .center:.left
        let count=try boundedInteger(number(action,"click_count"),1,3)
        for index in 1...count {
            try check();heldMouse=button
            try mouse(button == .left ? .leftMouseDown:button == .right ? .rightMouseDown:.otherMouseDown,button,position,Int64(index))
            try mouse(button == .left ? .leftMouseUp:button == .right ? .rightMouseUp:.otherMouseUp,button,position,Int64(index))
            heldMouse=nil
            if count>1 {Thread.sleep(forTimeInterval:0.02)}
        }
    }
    func pressKeys(_ action:[String:Any]) throws {
        try checkKeyboardWindow()
        let codes=try (action["keys"] as? [String] ?? []).map {name -> CGKeyCode in
            guard let code=keycodes[name.lowercased()] else {throw failure("unsupported_key","Unsupported key \(name)")};return code
        }
        for code in codes {try check();try checkKeyboardWindow();heldKeys.append(code);flags.formUnion(modifier(code));try key(code,true)}
        for code in codes.reversed() {flags.subtract(modifier(code));try key(code,false);heldKeys.removeLast()}
    }
    func typeText(_ text:String) throws {
        try checkKeyboardWindow()
        for character in text {try check();try checkKeyboardWindow();heldKeys.append(0);try key(0,true,String(character));try key(0,false,String(character));heldKeys.removeLast()}
    }
    func checkKeyboardWindow() throws {
        let app=AXUIElementCreateApplication(pid);AXUIElementSetMessagingTimeout(app,0.25)
        guard let focused=attribute(app,kAXFocusedWindowAttribute),CFGetTypeID(focused)==AXUIElementGetTypeID(),
              CFEqual(focused,try axWindow(window)) else {
            throw failure("focus_changed","The target application's keyboard window changed; select and observe its focused window")
        }
    }
    func scroll(_ action:[String:Any]) throws {
        let dx=try boundedInteger(number(action,"dx"),-20,20),dy=try boundedInteger(number(action,"dy"),-20,20)
        try move(CGPoint(x:number(action,"x"),y:number(action,"y")))
        guard let event=CGEvent(scrollWheelEvent2Source:nil,units:.line,wheelCount:2,wheel1:-Int32(dy),wheel2:-Int32(dx),wheel3:0) else {
            throw failure("input_failed","Cannot allocate scroll event")
        }
        try locateTargetedEvent(event,point:position,window:window);post(event)
    }
    func drag(_ action:[String:Any]) throws {
        let duration=try boundedInteger(number(action,"duration_ms"),50,3000)
        let from=CGPoint(x:number(action,"from_x"),y:number(action,"from_y"))
        let to=CGPoint(x:number(action,"to_x"),y:number(action,"to_y"))
        try move(from);heldMouse = .left;try mouse(.leftMouseDown,.left,position)
        let steps=duration/10
        for step in 1...steps {
            let t=Double(step)/Double(steps)
            try move(CGPoint(x:from.x+(to.x-from.x)*t,y:from.y+(to.y-from.y)*t),dragged:true)
            Thread.sleep(forTimeInterval:0.01)
        }
        try mouse(.leftMouseUp,.left,position);heldMouse=nil
    }
    func number(_ action:[String:Any],_ key:String)->Double {action[key] as? Double ?? 0}
}

func perform(_ request:[String:Any],_ context:UnsafeRawPointer?,_ cancelled:@convention(c)(UnsafeRawPointer?)->Int32) throws -> String {
    guard let window=request["window"] as? [String:Any],let action=request["action"] as? [String:Any] else {
        throw failure("invalid_params","Missing input arguments")
    }
    return try TargetedInput(window:window,checkCancellation:{cancelled(context) != 0}).run(action)
}
