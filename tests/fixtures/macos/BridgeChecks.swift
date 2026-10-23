import Foundation
import CoreGraphics
import AppKit
import ScreenCaptureKit

// Compile with bridge.swift. These checks allocate events but never post input or capture a screen.
@main
struct BridgeChecks {
    static func expect(_ value: Bool) { precondition(value) }

    static func rejects(_ operation: () throws -> Void) {
        do { try operation(); fatalError("Expected rejection") }
        catch { }
    }

    static func dispatchErrors() {
        let error=NSError(domain:"SyntheticActivation",code:1)
        expect(bridgeFailureData(error)["may_have_executed"] as? Bool == false)
        do { try afterDispatch { throw error } }
        catch { expect(bridgeFailureData(error)["may_have_executed"] as? Bool == true) }
        print("PASS activation dispatch-boundary errors without activating a window")
    }

    static func popupLayers() {
        let normal:[String:Any]=[kCGWindowLayer as String:0,kCGWindowOwnerPID as String:Int32(42)]
        var popup=normal; popup[kCGWindowLayer as String]=24
        expect(isApplicationWindow(normal,regularPIDs:[]))
        expect(isApplicationWindow(popup,regularPIDs:[42]))
        expect(!isApplicationWindow(popup,regularPIDs:[99]))
        popup[kCGWindowAlpha as String]=0.0
        expect(!isApplicationWindow(popup,regularPIDs:[42]))
        print("PASS nonzero-layer application popups without admitting system overlays")
    }
    static func targetedEvents() throws {
        let window:[String:Any]=["id":"123","pid":Int32(42),"bounds":["x":-500,"y":20,"width":660,"height":540]]
        let cursor=CGEvent(source:nil)?.location
        for count in 1...3 {
            for type in [CGEventType.leftMouseDown, .leftMouseUp] {
                let event = try targetedPointerEvent(type, .left, CGPoint(x: -50, y: 100), Int64(count),window:window)
                expect(event.getIntegerValueField(.mouseEventClickState) == count)
                expect(event.type == type)
                expect(event.location == CGPoint(x: -50, y: 100))
                expect(event.getIntegerValueField(.mouseEventWindowUnderMousePointer)==123)
                expect(event.getIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent)==123)
                expect(NSEvent(cgEvent:event)?.windowNumber==123)
            }
        }
        let scroll=CGEvent(scrollWheelEvent2Source:nil,units:.line,wheelCount:2,wheel1:-3,wheel2:0,wheel3:0)!
        try locateTargetedEvent(scroll,point:CGPoint(x:-50,y:100),window:window)
        expect(NSEvent(cgEvent:scroll)?.windowNumber==123)
        typealias WindowLocationGetter = @convention(c) (CGEvent) -> CGPoint
        let library=dlopen("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics",RTLD_LAZY)!
        let getLocation=unsafeBitCast(dlsym(library,"CGEventGetWindowLocation")!,to:WindowLocationGetter.self)
        expect(getLocation(scroll)==CGPoint(x:450,y:80))
        expect(CGEvent(source:nil)?.location==cursor)
        print("PASS targeted window IDs, click-state and screen coordinates without posting or moving the cursor")
    }
    static func sharedFrames() throws {
        var buffer:CVPixelBuffer?
        expect(CVPixelBufferCreate(kCFAllocatorDefault,8,4,kCVPixelFormatType_32BGRA,
                                  [kCVPixelBufferIOSurfacePropertiesKey:[:]] as CFDictionary,&buffer)==kCVReturnSuccess)
        let image=buffer!
        CVPixelBufferLockBaseAddress(image,[])
        let data=CVPixelBufferGetBaseAddress(image)!.assumingMemoryBound(to:UInt8.self)
        let stride=CVPixelBufferGetBytesPerRow(image)
        for y in 0..<4 { for x in 0..<8 { let p=y*stride+x*4; data[p]=255;data[p+1]=0;data[p+2]=0;data[p+3]=255 } }
        CVPixelBufferUnlockBaseAddress(image,[])
        var format:CMVideoFormatDescription?
        expect(CMVideoFormatDescriptionCreateForImageBuffer(allocator:kCFAllocatorDefault,imageBuffer:image,formatDescriptionOut:&format)==noErr)
        var sample:CMSampleBuffer?,timing=CMSampleTimingInfo(duration:.invalid,presentationTimeStamp:.zero,decodeTimeStamp:.invalid)
        expect(CMSampleBufferCreateReadyWithImageBuffer(allocator:kCFAllocatorDefault,imageBuffer:image,formatDescription:format!,sampleTiming:&timing,sampleBufferOut:&sample)==noErr)
        let attachments=CMSampleBufferGetSampleAttachmentsArray(sample!,createIfNecessary:true)! as NSArray
        let status=attachments[0] as! NSMutableDictionary
        let frames=SharedWindowFrames()
        status[SCStreamFrameInfo.status]=SCFrameStatus.complete.rawValue; frames.receive(sample!)
        let first=frames.revision(); expect(first==1)
        let png=NSBitmapImageRep(cgImage:try sharedFrameImage(frames.snapshot(after:0)!))
        expect(png.pixelsWide==8 && png.pixelsHigh==4)
        let color=png.colorAt(x:2,y:2)!.usingColorSpace(.deviceRGB)!
        expect(color.blueComponent>0.9 && color.redComponent<0.1)
        status[SCStreamFrameInfo.status]=SCFrameStatus.idle.rawValue; frames.receive(sample!)
        expect(frames.snapshot(after:first) != nil)
        status[SCStreamFrameInfo.status]=SCFrameStatus.suspended.rawValue; frames.receive(sample!)
        expect(frames.snapshot(after:0)==nil)
        status[SCStreamFrameInfo.status]=SCFrameStatus.complete.rawValue; frames.receive(sample!)
        frames.recordFailure(NSError(domain:SCStreamErrorDomain,code:-3805))
        expect(frames.snapshot(after:0)==nil && frames.stoppedError() != nil)
        frames.receive(sample!); expect(frames.snapshot(after:0)==nil)
        print("PASS shared-frame pixels, idle reuse, suspended-frame rejection and interruption invalidation without screen capture")
    }
    static func main() throws {
        let connection=sharingFailure(NSError(domain:SCStreamErrorDomain,code:-3805))
        expect(connection.code=="capture_stream_stopped")
        expect(connection.message.contains("-3805"))
        let userStop=sharingFailure(NSError(domain:SCStreamErrorDomain,code:-3817))
        expect(userStop.code=="user_interruption")
        expect(sharingFailure(NSError(domain:"OtherDomain",code:-3817)).code=="capture_stream_stopped")
        print("PASS system sharing failures preserve domain/code and are not mislabelled as user cancellation")
        popupLayers()
        dispatchErrors()
        let identity=processIdentity(ProcessInfo.processInfo.processIdentifier)
        expect(identity != nil)
        expect(identity==processIdentity(ProcessInfo.processInfo.processIdentifier))
        expect(processIdentity(-1)==nil)
        print("PASS kernel process identity without NSRunningApplication launchDate")

        let data=Data("{\"bounds\":{\"x\":-500,\"y\":20.5,\"width\":660,\"height\":540.25}}".utf8)
        let decoded=try JSONSerialization.jsonObject(with:data) as! [String:Any]
        expect(try bounds(decoded)==CGRect(x:-500,y:20.5,width:660,height:540.25))
        rejects { _ = try bounds(["bounds":["x":0,"y":0,"width":0,"height":10]]) }
        rejects { _ = try bounds(["bounds":["x":false,"y":0,"width":1,"height":10]]) }
        print("PASS mixed JSON integer/float bounds decoding")

        let size = try captureSize(CGRect(x: -500, y: 20, width: 660.25, height: 540.25))
        expect(size.0 == 1321 && size.1 == 1081)
        let limit = try captureSize(CGRect(x: 0, y: 0, width: 2000, height: 2000))
        expect(limit.0 * limit.1 == 16_000_000)
        for size in [CGSize(width: 0, height: 10), CGSize(width: CGFloat.infinity, height: 10),
                     CGSize(width: CGFloat.nan, height: 10), CGSize(width: 2001, height: 2000),
                     CGSize(width: CGFloat.greatestFiniteMagnitude, height: 100)] {
            rejects { _ = try captureSize(CGRect(origin: .zero, size: size)) }
        }
        print("PASS bounded capture dimensions before allocation")

        for value in [1.0, 2.0, 3.0] { expect(try boundedInteger(value, 1, 3) == Int(value)) }
        for value in [0.0, 4.0, 1.5, .infinity, -.infinity, .nan, .greatestFiniteMagnitude] {
            rejects { _ = try boundedInteger(value, 1, 3) }
        }
        expect(try boundedInteger(-20, -20, 20) == -20)
        expect(try boundedInteger(3000, 50, 3000) == 3000)
        rejects { _ = try boundedInteger(3001, 50, 3000) }
        print("PASS finite integer bounds without trapping conversions")

        try targetedEvents()
        try sharedFrames()
    }
}
