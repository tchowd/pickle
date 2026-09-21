import SwiftUI
import AppKit

/// Select any word or phrase, then use its contextual menu to explain it in place.
struct SelectablePassage: NSViewRepresentable {
    let text: String
    let size: CGFloat
    let explain: (String) -> Void
    func makeNSView(context: Context) -> PassageTextView {
        let view = PassageTextView()
        view.isEditable = false; view.isSelectable = true; view.drawsBackground = false
        view.textContainerInset = .zero
        view.textContainer?.lineFragmentPadding = 0
        view.isHorizontallyResizable = false
        view.isVerticallyResizable = true
        view.textContainer?.widthTracksTextView = true
        view.setAccessibilityLabel("Passage. Select a word and use Explain selection from the context menu.")
        return view
    }
    func updateNSView(_ view: PassageTextView, context: Context) {
        view.explain = explain
        let paragraph = NSMutableParagraphStyle(); paragraph.lineSpacing = 7
        let font = NSFont.systemFont(ofSize: size)
        let rounded = NSFont(descriptor: font.fontDescriptor.withDesign(.rounded) ?? font.fontDescriptor, size: size) ?? font
        let value = NSAttributedString(string: text, attributes: [.font: rounded, .foregroundColor: NSColor.labelColor, .paragraphStyle: paragraph])
        if view.attributedString() != value { view.textStorage?.setAttributedString(value) }
    }
    func sizeThatFits(_ proposal: ProposedViewSize, nsView: PassageTextView, context: Context) -> CGSize? {
        let width = max(1, proposal.width ?? 440)
        nsView.textContainer?.containerSize = NSSize(width: width, height: .greatestFiniteMagnitude)
        guard let layout = nsView.layoutManager, let container = nsView.textContainer else { return nil }
        layout.ensureLayout(for: container)
        return CGSize(width: width, height: ceil(layout.usedRect(for: container).height) + 2)
    }
    final class PassageTextView: NSTextView {
        var explain: ((String) -> Void)?
        override func menu(for event: NSEvent) -> NSMenu? {
            let menu = super.menu(for: event) ?? NSMenu()
            let selected = (string as NSString).substring(with: selectedRange()).trimmingCharacters(in: .whitespacesAndNewlines)
            if !selected.isEmpty && selected.count <= 150 {
                menu.insertItem(.separator(), at: 0)
                let item = NSMenuItem(title: "Explain selection in context", action: #selector(explainSelection), keyEquivalent: "")
                item.target = self; menu.insertItem(item, at: 0)
            }
            return menu
        }
        @objc private func explainSelection() {
            explain?((string as NSString).substring(with: selectedRange()))
        }
    }
}
