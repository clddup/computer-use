import Foundation
import CoreGraphics

// Compile with bridge.swift. These checks allocate events but never post input or capture a screen.
@main
struct BridgeChecks {
    static func expect(_ value: Bool) { precondition(value) }

    static func rejects(_ operation: () throws -> Void) {
        do { try operation(); fatalError("Expected rejection") }
        catch { }
    }

    static func main() throws {
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

        for count in 1...3 {
            for type in [CGEventType.leftMouseDown, .leftMouseUp] {
                let event = try pointerEvent(type, .left, CGPoint(x: -50, y: 100), Int64(count))
                expect(event.getIntegerValueField(.mouseEventClickState) == count)
                expect(event.type == type)
                expect(event.location == CGPoint(x: -50, y: 100))
            }
        }
        print("PASS click-state metadata for native single/double/triple clicks (not posted)")
    }
}
