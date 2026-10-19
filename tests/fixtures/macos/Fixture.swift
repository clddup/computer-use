import AppKit
import CoreGraphics

// Usage: Fixture /absolute/path/to/oracle.json "Unique Fixture Window Title"
// command.json beside the oracle accepts only window/focus fault injection, never input.
final class DragRegion: NSView {
    private(set) var mouseDown = false
    private(set) var downCount = 0
    private(set) var dragCount = 0
    private(set) var upCount = 0
    private(set) var clickCount = 0

    override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Fixture Drag Region")
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is unsupported") }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) {
        mouseDown = true
        downCount += 1
        clickCount = event.clickCount
        needsDisplay = true
    }

    override func mouseDragged(with event: NSEvent) {
        dragCount += 1
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        mouseDown = false
        upCount += 1
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.controlBackgroundColor.setFill()
        bounds.fill()
        NSColor.separatorColor.setStroke()
        NSBezierPath(rect: bounds.insetBy(dx: 1, dy: 1)).stroke()
        let text = "Fixture Drag Region\nDown: \(downCount)  Drag: \(dragCount)  Up: \(upCount)\nClick count: \(clickCount)"
        (text as NSString).draw(
            in: bounds.insetBy(dx: 12, dy: 12),
            withAttributes: [.font: NSFont.systemFont(ofSize: 14),
                             .foregroundColor: NSColor.labelColor]
        )
    }
}

final class FixtureScrollView: NSScrollView {
    private(set) var eventCount=0
    private(set) var lastDeltaY:CGFloat=0
    override func scrollWheel(with event:NSEvent) {
        eventCount += 1
        lastDeltaY=event.scrollingDeltaY
        super.scrollWheel(with:event)
    }
}

final class ScrollDocument: NSView {
    override var isFlipped: Bool { true }
}

final class Fixture: NSObject, NSApplicationDelegate, NSTextFieldDelegate {
    private let output: URL
    private let commandURL: URL
    private var commandID = 0
    private var sentinel: NSWindow?
    private let multiline = NSTextView()
    private let title: String
    private var window: NSWindow!
    private let input = NSTextField()
    private let countLabel = NSTextField(labelWithString: "Count: 0")
    private let scroll = FixtureScrollView()
    private let drag = DragRegion(frame: NSRect(x: 20, y: 20, width: 620, height: 100))
    private var count = 0
    private var text = ""
    private var ready = false
    private var timer: Timer?

    init(output: URL, title: String) {
        self.output = output
        self.commandURL = output.deletingLastPathComponent().appendingPathComponent("command.json")
        self.title = title
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 660, height: 540),
            styleMask: [.titled, .closable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.title = title
        window.isReleasedWhenClosed = false
        let content = window.contentView!

        let description = NSTextField(labelWithString: "Local AppKit fixture — synthetic data only")
        description.frame = NSRect(x: 20, y: 500, width: 620, height: 24)
        content.addSubview(description)

        let increment = NSButton(title: "Fixture Increment", target: self, action: #selector(incrementCount))
        increment.frame = NSRect(x: 20, y: 450, width: 180, height: 32)
        increment.bezelStyle = .rounded
        increment.setAccessibilityLabel("Fixture Increment")
        content.addSubview(increment)
        countLabel.frame = NSRect(x: 220, y: 454, width: 200, height: 24)
        countLabel.setAccessibilityLabel("Fixture Count")
        content.addSubview(countLabel)

        let inputLabel = NSTextField(labelWithString: "Fixture Input")
        inputLabel.frame = NSRect(x: 20, y: 408, width: 150, height: 24)
        content.addSubview(inputLabel)
        input.frame = NSRect(x: 180, y: 406, width: 460, height: 28)
        input.setAccessibilityLabel("Fixture Input")
        input.delegate = self
        content.addSubview(input)

        let passwordLabel = NSTextField(labelWithString: "Fixture Password")
        passwordLabel.frame = NSRect(x: 20, y: 366, width: 150, height: 24)
        content.addSubview(passwordLabel)
        let password = NSSecureTextField(frame: NSRect(x: 180, y: 364, width: 460, height: 28))
        password.setAccessibilityLabel("Fixture Password")
        password.stringValue = "FIXTURE-PASSWORD-DO-NOT-EXPOSE"
        content.addSubview(password)

        let multilineScroll = NSScrollView(frame: NSRect(x: 340, y: 140, width: 300, height: 200))
        multilineScroll.borderType = .bezelBorder
        multilineScroll.hasVerticalScroller = true
        multiline.frame = NSRect(x: 0, y: 0, width: 280, height: 196)
        multiline.isRichText = false
        multiline.isHorizontallyResizable = false
        multiline.isVerticallyResizable = true
        multiline.autoresizingMask = [.width]
        multiline.textContainer?.widthTracksTextView = true
        multiline.textContainer?.containerSize = NSSize(width: 280, height: CGFloat.greatestFiniteMagnitude)
        multiline.font = NSFont.systemFont(ofSize: 14)
        multiline.setAccessibilityLabel("Fixture Multiline Input")
        multilineScroll.documentView = multiline
        content.addSubview(multilineScroll)

        scroll.frame = NSRect(x: 20, y: 140, width: 300, height: 200)
        scroll.borderType = .bezelBorder
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = false
        scroll.autohidesScrollers = false
        scroll.setAccessibilityLabel("Fixture Scroll Region")
        let document = ScrollDocument(frame: NSRect(x: 0, y: 0, width: 270, height: 1200))
        for index in 0..<24 {
            let row = NSTextField(labelWithString: "Fixture scroll row \(index + 1)")
            row.frame = NSRect(x: 12, y: 12 + index * 48, width: 240, height: 24)
            document.addSubview(row)
        }
        scroll.documentView = document
        content.addSubview(scroll)
        scroll.contentView.scroll(to: .zero)
        scroll.reflectScrolledClipView(scroll.contentView)
        content.addSubview(drag)

        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        window.makeFirstResponder(input)
        ready = true
        saveOracle()

        let timer = Timer(timeInterval: 0.05, repeats: true) { [weak self] _ in
            self?.saveOracle()
        }
        self.timer = timer
        // Keep sampling while AppKit is tracking a mouse press or scrollbar drag.
        RunLoop.main.add(timer, forMode: .common)
    }

    @objc private func incrementCount() {
        count += 1
        countLabel.stringValue = "Count: \(count)"
    }

    func controlTextDidChange(_ notification: Notification) {
        guard let field = notification.object as? NSTextField, field === input else { return }
        // Read the field editor's current value, not just a value committed on blur.
        text = input.currentEditor()?.string ?? input.stringValue
    }

    private func consumeCommand() {
        guard let data = try? Data(contentsOf: commandURL),
              let command = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let number = command["id"] as? NSNumber,
              CFGetTypeID(number) != CFBooleanGetTypeID(),
              number.doubleValue.isFinite,
              number.doubleValue.rounded() == number.doubleValue,
              number.doubleValue < Double(Int.max),
              number.doubleValue > Double(commandID),
              let kind = command["kind"] as? String else { return }
        switch kind {
        case "move":
            var origin = window.frame.origin
            origin.x += 20
            window.setFrameOrigin(origin)
        case "focus_other":
            if sentinel == nil {
                let other = NSWindow(
                    contentRect: NSRect(x: 0, y: 0, width: 320, height: 120),
                    styleMask: [.titled, .closable], backing: .buffered, defer: false
                )
                other.title = title + " — Sentinel"
                other.isReleasedWhenClosed = false
                let label = NSTextField(labelWithString: "Fixture focus sentinel — synthetic data only")
                label.frame = NSRect(x: 12, y: 40, width: 296, height: 40)
                other.contentView?.addSubview(label)
                other.center()
                sentinel = other
            }
            sentinel?.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
        case "focus_main":
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
        default:
            return
        }
        commandID = number.intValue
    }

    private func saveOracle() {
        if ready { consumeCommand() }
        let flags = CGEventSource.flagsState(.combinedSessionState)
        let modifiers: [(String, CGEventFlags)] = [
            ("command", .maskCommand), ("shift", .maskShift),
            ("control", .maskControl), ("alt", .maskAlternate),
            ("caps_lock", .maskAlphaShift), ("fn", .maskSecondaryFn)
        ]
        let frame = window.frame
        // AppKit is bottom-left based; CG global coordinates start at the main display's top-left.
        let primaryTop = CGDisplayBounds(CGMainDisplayID()).height
        let snapshot: [String: Any] = [
            "pid": ProcessInfo.processInfo.processIdentifier,
            "window_id": String(window.windowNumber),
            "count": count,
            "text": text,
            "multiline_text": multiline.string,
            "command_id": commandID,
            "foreground_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0,
            "pointer_down": CGEventSource.buttonState(.combinedSessionState, button: .left),
            "mouse_down": drag.mouseDown,
            "down_count": drag.downCount,
            "drag_count": drag.dragCount,
            "up_count": drag.upCount,
            "click_count": drag.clickCount,
            "scroll_y": scroll.contentView.bounds.origin.y,
            "scroll_events": scroll.eventCount,
            "scroll_delta_y": scroll.lastDeltaY,
            "held_modifiers": modifiers.compactMap { flags.contains($0.1) ? $0.0 : nil },
            "window_bounds": ["x": frame.minX, "y": primaryTop - frame.maxY,
                              "width": frame.width, "height": frame.height],
            "ready": ready && window.isVisible
        ]
        do {
            let data = try JSONSerialization.data(withJSONObject: snapshot, options: [.sortedKeys])
            try data.write(to: output, options: .atomic)
        } catch {
            fputs("Fixture oracle write failed: \(error)\n", stderr)
            exit(1)
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    func applicationWillTerminate(_ notification: Notification) {
        timer?.invalidate()
        ready = false
        saveOracle()
    }
}

guard CommandLine.arguments.count == 3, !CommandLine.arguments[2].isEmpty else {
    fputs("Usage: Fixture <oracle.json> <unique-window-title>\n", stderr)
    exit(2)
}
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let fixture = Fixture(
    output: URL(fileURLWithPath: CommandLine.arguments[1]),
    title: CommandLine.arguments[2]
)
app.delegate = fixture
app.run()
