import SwiftUI
import AppKit
import PickleCore

// Pickle's portal palette stays consistent across every window.
let pickleGreen = Color(red: 0.70, green: 0.96, blue: 0.25)
let pickleCyan = Color(red: 0.36, green: 0.86, blue: 0.78)
let picklePaper = Color(red: 0.045, green: 0.075, blue: 0.065)
let pickleSurface = Color(red: 0.085, green: 0.13, blue: 0.10)

struct PortalSurface: View {
    var body: some View {
        ZStack {
            DesktopGlass()
            picklePaper.opacity(0.12)
            RadialGradient(colors: [pickleGreen.opacity(0.045), .clear], center: .topTrailing, startRadius: 0, endRadius: 360)
        }.allowsHitTesting(false)
    }
}

// Blur the actual desktop behind the window, rather than only its own content.
// Native materials also honor macOS Reduce Transparency automatically.
private struct DesktopGlass: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = .hudWindow
        view.blendingMode = .behindWindow
        view.state = .active
        view.isEmphasized = false
        return view
    }
    func updateNSView(_ view: NSVisualEffectView, context: Context) {}
}

// Native vector artwork: a tiny pickle drifting through a dimensional portal.
struct PicklePortal: View {
    var body: some View {
        ZStack {
            Ellipse().fill(pickleGreen.opacity(0.06)).frame(width: 126, height: 76)
            ForEach(0..<4) { index in
                Ellipse().trim(from: 0.06, to: 0.93)
                    .stroke(index.isMultiple(of: 2) ? pickleGreen.opacity(0.8) : pickleCyan.opacity(0.5),
                            style: StrokeStyle(lineWidth: index == 0 ? 3 : 1.5, lineCap: .round, dash: index == 2 ? [8, 5] : []))
                    .frame(width: CGFloat(126 - index * 13), height: CGFloat(76 - index * 9))
                    .rotationEffect(.degrees(Double(index * 37) - 18))
            }
            Capsule().fill(LinearGradient(colors: [pickleGreen, Color(red: 0.24, green: 0.58, blue: 0.15)], startPoint: .leading, endPoint: .trailing))
                .frame(width: 29, height: 65).rotationEffect(.degrees(25))
                .overlay {
                    VStack(spacing: 7) {
                        HStack(spacing: 6) { Circle(); Circle() }
                        HStack(spacing: 6) { Circle(); Circle() }
                        HStack(spacing: 6) { Circle(); Circle() }
                    }.foregroundStyle(picklePaper.opacity(0.4)).frame(width: 12, height: 34).rotationEffect(.degrees(25))
                }
            Image(systemName: "sparkle").font(.system(size: 12)).foregroundStyle(pickleCyan).offset(x: 64, y: -28)
            Circle().fill(pickleGreen).frame(width: 4, height: 4).offset(x: -61, y: 26)
        }.frame(width: 150, height: 110).accessibilityHidden(true)
    }
}

extension ReadingAction {
    var title: String {
        switch self { case .simplify: return "Simplify"; case .expand: return "Explain more"; case .chart: return "Visualize"; case .followUp: return "Follow-up" }
    }
    var resultTitle: String {
        switch self { case .simplify: return "A little clearer."; case .expand: return "The bigger picture."; case .chart: return "See the connection."; case .followUp: return "Let’s go deeper." }
    }
}

struct PickleActionStyle: ButtonStyle {
    var selected = false
    func makeBody(configuration: Configuration) -> some View {
        ActionFace(configuration: configuration, selected: selected)
    }
    private struct ActionFace: View {
        let configuration: ButtonStyleConfiguration
        let selected: Bool
        @Environment(\.isEnabled) private var enabled
        @State private var hovered = false
        var body: some View {
            configuration.label.font(.system(size: 13, weight: .semibold, design: .rounded))
                .padding(.vertical, 12).padding(.horizontal, 10)
                .foregroundStyle(selected ? picklePaper : Color(red: 0.88, green: 0.94, blue: 0.85))
                .background(selected ? pickleGreen : (hovered ? pickleGreen.opacity(0.18) : Color.white.opacity(0.07)), in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(pickleGreen.opacity(selected ? 0.8 : hovered ? 0.5 : 0.10)))
                .shadow(color: selected ? pickleGreen.opacity(0.12) : .clear, radius: 8, y: 2)
                .opacity(!enabled ? 0.4 : configuration.isPressed ? 0.7 : 1)
                .onHover { hovered = $0 }
        }
    }
}

// A dedicated native drag surface keeps text selection, scrolling and buttons intact.
struct WindowDragHandle: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView { DragView() }
    func updateNSView(_ nsView: NSView, context: Context) {}
    private final class DragView: NSView {
        override init(frame: NSRect) { super.init(frame: frame); toolTip = "Drag to move"; setAccessibilityLabel("Drag to move window") }
        required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
        override var mouseDownCanMoveWindow: Bool { true }
        override func mouseDown(with event: NSEvent) { window?.performDrag(with: event) }
        override func resetCursorRects() { addCursorRect(bounds, cursor: .openHand) }
    }
}

struct DragGrip: View {
    var body: some View {
        Capsule().fill(pickleGreen.opacity(0.6)).frame(width: 30, height: 3)
            .frame(maxWidth: .infinity).frame(height: 18)
            .overlay(WindowDragHandle())
    }
}

@MainActor func pickleStatusIcon() -> NSImage {
    let image = NSImage(size: NSSize(width: 18, height: 20), flipped: false) { _ in
        NSColor.black.setStroke()
        let outline = NSBezierPath(roundedRect: NSRect(x: 4, y: 1, width: 10, height: 18), xRadius: 5, yRadius: 5)
        outline.lineWidth = 1.6
        outline.stroke()
        NSColor.black.setFill()
        for point in [NSPoint(x: 7, y: 5), NSPoint(x: 10, y: 9), NSPoint(x: 7, y: 13)] {
            NSBezierPath(ovalIn: NSRect(origin: point, size: NSSize(width: 2, height: 2))).fill()
        }
        return true
    }
    image.isTemplate = true
    image.accessibilityDescription = "Pickle reading assistant"
    return image
}

// Shared inset treatment for cards and fields, over the window's desktop glass.
struct GlassInset: ViewModifier {
    var radius: CGFloat = 12
    var focused = false
    func body(content: Content) -> some View {
        content
            .background(.white.opacity(0.045), in: RoundedRectangle(cornerRadius: radius))
            .background(picklePaper.opacity(0.20), in: RoundedRectangle(cornerRadius: radius))
            .overlay(RoundedRectangle(cornerRadius: radius).strokeBorder(focused ? pickleGreen.opacity(0.7) : .white.opacity(0.13)))
    }
}
extension View {
    func glassInset(radius: CGFloat = 12, focused: Bool = false) -> some View {
        modifier(GlassInset(radius: radius, focused: focused))
    }
}

struct GlassSection<Content: View>: View {
    private let title: String?
    private let content: Content
    init(_ title: String? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            if let title { Text(title).font(.system(size: 14, weight: .semibold, design: .rounded)).foregroundStyle(pickleCyan).accessibilityAddTraits(.isHeader) }
            content
        }.frame(maxWidth: .infinity, alignment: .leading).padding(18).glassInset(radius: 16)
    }
}

struct GlassFieldStyle: TextFieldStyle {
    func _body(configuration: TextField<Self._Label>) -> some View {
        configuration.textFieldStyle(.plain).padding(10).glassInset(radius: 8)
    }
}

struct GlassToggleStyle: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 16) {
            configuration.label
            Spacer(minLength: 12)
            Toggle(isOn: configuration.$isOn) { configuration.label }
                .labelsHidden().toggleStyle(.switch)
        }.frame(maxWidth: .infinity)
    }
}
