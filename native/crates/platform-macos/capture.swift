import AppKit
import CoreImage
import ScreenCaptureKit

final class SharedWindowFrames:NSObject,SCStreamOutput,SCStreamDelegate {
    private let lock=NSLock()
    private var failure:Error?
    private var image:CVPixelBuffer?
    private var sequence:UInt64=0
    private var received=0
    private var lastStatus:Int?

    func stoppedError() -> Error? { lock.lock(); defer {lock.unlock()}; return failure }
    func revision() -> UInt64 { lock.lock(); defer {lock.unlock()}; return sequence }
    func diagnostics() -> String {
        lock.lock(); defer {lock.unlock()}
        return "received=\(received), status=\(lastStatus.map(String.init) ?? "none"), revision=\(sequence)"
    }
    func snapshot(after revision:UInt64) -> CVPixelBuffer? {
        lock.lock(); defer {lock.unlock()}
        return sequence>revision ? image:nil
    }
    func stream(_ stream:SCStream,didStopWithError error:Error) {
        recordFailure(error)
    }
    func recordFailure(_ error:Error) {
        lock.lock(); failure=error; image=nil; lock.unlock()
        let cause=sharingFailure(error)
        fputs("macOS sharing: \(cause.code): \(cause.message)\n",stderr)
    }
    func stream(_ stream:SCStream,didOutputSampleBuffer sampleBuffer:CMSampleBuffer,of type:SCStreamOutputType) {
        if type == .screen { receive(sampleBuffer) }
    }
    func receive(_ sampleBuffer:CMSampleBuffer) {
        lock.lock(); received+=1; lock.unlock()
        guard let attachments=CMSampleBufferGetSampleAttachmentsArray(sampleBuffer,createIfNecessary:false) as? [[SCStreamFrameInfo:Any]],
              let raw=attachments.first?[.status] as? Int,let status=SCFrameStatus(rawValue:raw) else {
            lock.lock(); image=nil; lastStatus=nil; lock.unlock(); return
        }
        lock.lock(); defer {lock.unlock()}
        lastStatus=raw
        guard failure==nil else { return }
        if status == .complete,sampleBuffer.isValid,let buffer=sampleBuffer.imageBuffer {
            image=buffer; sequence+=1
        } else if status == .idle,image != nil {
            sequence+=1
        } else {
            image=nil
        }
    }
}

private let frameImageContext=CIContext(options:[.cacheIntermediates:false])

func sharedFrameImage(_ buffer:CVPixelBuffer) throws -> CGImage {
    let image=CIImage(cvPixelBuffer:buffer)
    guard image.extent.width*image.extent.height<=16_000_000,
          let result=frameImageContext.createCGImage(image,from:image.extent) else {
        throw failure("capture_limit","Shared-window frame exceeds image limits")
    }
    return result
}

private func sharedImage(_ frames:SharedWindowFrames,cancelled:()->Bool) throws -> CGImage {
    let ready=Date().addingTimeInterval(1.0/15),deadline=Date().addingTimeInterval(3)
    while Date()<deadline {
        if cancelled() { throw failure("cancelled","Shared-window capture cancelled before returning an image") }
        if let error=frames.stoppedError() { throw sharingFailure(error) }
        if Date()>=ready,let buffer=frames.snapshot(after:0) {
            return try sharedFrameImage(buffer)
        }
        RunLoop.current.run(mode:.default,before:Date().addingTimeInterval(0.01))
        pumpPresentationEvents()
    }
    throw failure("capture_timeout","No current frame from the selected window sharing stream (\(frames.diagnostics())). Do not fall back to desktop capture")
}

func captureSharedWindow(_ window:[String:Any],cancelled:()->Bool={false}) throws -> [String:Any] {
    if cancelled() { throw failure("cancelled","Shared-window capture cancelled before starting") }
    let frames=try sharedWindowFrames(window)
    let image=try sharedImage(frames,cancelled:cancelled)
    try validate(window)
    if cancelled() { throw failure("cancelled","Shared-window capture cancelled before returning an image") }
    guard let data=NSBitmapImageRep(cgImage:image).representation(using:.png,properties:[:]),data.count<=9_000_000 else {
        throw failure("capture_limit","Shared-window PNG exceeds size limit")
    }
    if cancelled() { throw failure("cancelled","Shared-window capture cancelled before returning an image") }
    if let error=presentationInterruptionError() { throw error }
    try validate(window)
    return ["id":UUID().uuidString,"mime_type":"image/png","data":data.base64EncodedString(),
            "width":image.width,"height":image.height,"screen_bounds":try rect(bounds(window)),
            "capture_method":"screencapturekit-stream"]
}
