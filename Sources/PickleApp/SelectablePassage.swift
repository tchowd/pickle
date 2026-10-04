import SwiftUI
import AppKit
import PickleCore

/// Select any word or phrase, then use its contextual menu to explain it in place.
/// Answers render their light Markdown; source passages always stay literal.
struct SelectablePassage: NSViewRepresentable {
    let text: String
    let size: CGFloat
    var markdown = false
    var explain: ((String) -> Void)?
    func makeNSView(context: Context) -> PassageTextView {
        // TextKit 1 supports the code block backgrounds and the sizing below.
        let view = PassageTextView(usingTextLayoutManager: false)
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
        let source = PassageTextView.Source(text: text, size: size, markdown: markdown)
        guard view.source != source else { return }
        view.source = source
        let typography = AnswerTypography(size: size)
        view.textStorage?.setAttributedString(markdown ? typography.render(text) : typography.literal(text))
    }
    func sizeThatFits(_ proposal: ProposedViewSize, nsView: PassageTextView, context: Context) -> CGSize? {
        let width = max(1, proposal.width ?? 440)
        nsView.textContainer?.containerSize = NSSize(width: width, height: .greatestFiniteMagnitude)
        guard let layout = nsView.layoutManager, let container = nsView.textContainer else { return nil }
        layout.ensureLayout(for: container)
        return CGSize(width: width, height: ceil(layout.usedRect(for: container).height) + 2)
    }
    final class PassageTextView: NSTextView {
        struct Source: Equatable { let text: String, size: CGFloat, markdown: Bool }
        var source: Source?
        var explain: ((String) -> Void)?
        override func menu(for event: NSEvent) -> NSMenu? {
            let menu = super.menu(for: event) ?? NSMenu()
            let selected = (string as NSString).substring(with: selectedRange()).trimmingCharacters(in: .whitespacesAndNewlines)
            if explain != nil && !selected.isEmpty && selected.count <= 150 {
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

/// Reading typography: generous lines, compact list rhythm, hanging indents and quiet accents.
struct AnswerTypography {
    let size: CGFloat
    private var lineSpacing: CGFloat { (size * 0.38).rounded() }
    private var blockGap: CGFloat { (size * 0.75).rounded() }
    private var itemGap: CGFloat { (size * 0.4).rounded() }
    private var indentStep: CGFloat { (size * 1.3).rounded() }

    func literal(_ text: String) -> NSAttributedString {
        let paragraph = NSMutableParagraphStyle(); paragraph.lineSpacing = lineSpacing
        return NSAttributedString(string: text, attributes: [.font: font(), .foregroundColor: NSColor.labelColor, .paragraphStyle: paragraph])
    }

    func render(_ text: String) -> NSAttributedString {
        let blocks = AnswerMarkup.blocks(text)
        let markerFont = font(weight: .semibold).withMonospacedDigits
        // One marker column per answer keeps "9." and "10." items aligned.
        let numberWidth = blocks.compactMap { block -> CGFloat? in
            guard case .numbered(let number) = block.kind else { return nil }
            return (number as NSString).size(withAttributes: [.font: markerFont]).width
        }.max() ?? 0
        let numberColumn = max(size * 1.5, ceil(numberWidth + size * 0.5))
        let output = NSMutableAttributedString()
        for (index, block) in blocks.enumerated() {
            let next = blocks.indices.contains(index + 1) ? blocks[index + 1] : nil
            let paragraph = NSMutableParagraphStyle()
            paragraph.lineSpacing = lineSpacing
            paragraph.paragraphSpacing = next == nil ? 0 : block.isListItem && next!.isListItem ? itemGap : blockGap
            var color = NSColor.labelColor, base = font(), strong = font(weight: .semibold), italic = false
            switch block.kind {
            case .heading(let level):
                base = font(scale: level == 1 ? 1.25 : level == 2 ? 1.15 : 1.05, weight: .bold); strong = base
                paragraph.paragraphSpacingBefore = index == 0 ? 0 : itemGap
                paragraph.paragraphSpacing = next == nil ? 0 : itemGap
            case .quote:
                color = .secondaryLabelColor; italic = true
                paragraph.firstLineHeadIndent = indentStep * 0.75; paragraph.headIndent = indentStep * 0.75
            case .code:
                let box = NSTextBlock()
                box.backgroundColor = NSColor.white.withAlphaComponent(0.06)
                box.setWidth(10, type: .absoluteValueType, for: .padding)
                paragraph.textBlocks = [box]; paragraph.lineSpacing = 3
            case .bullet, .numbered:
                let lead = CGFloat(block.depth) * indentStep
                let column: CGFloat = block.marker == "•" || block.marker == "◦" ? (size * 1.1).rounded() : numberColumn
                paragraph.firstLineHeadIndent = lead; paragraph.headIndent = lead + column
                paragraph.tabStops = [NSTextTab(textAlignment: .left, location: lead + column)]
            case .paragraph: break
            }
            let start = output.length
            if let marker = block.marker {
                output.append(NSAttributedString(string: marker + "\t", attributes: [.font: markerFont, .foregroundColor: NSColor(pickleGreen)]))
            }
            for run in block.runs {
                var runFont = run.code ? NSFont.monospacedSystemFont(ofSize: size * 0.88, weight: .regular)
                    : run.strong ? strong : base
                var attributes: [NSAttributedString.Key: Any] = [.foregroundColor: run.code && block.kind != .code ? NSColor(pickleCyan) : color]
                if run.emphasis || italic {
                    // SF Rounded has no italic face; slant it rather than switching typefaces.
                    let slanted = runFont.italic
                    if slanted.fontDescriptor.symbolicTraits.contains(.italic) { runFont = slanted } else { attributes[.obliqueness] = 0.14 }
                }
                attributes[.font] = runFont
                // Keep wrapped lines in the same paragraph so spacing and hanging indents hold.
                output.append(NSAttributedString(string: run.text.replacingOccurrences(of: "\n", with: "\u{2028}"), attributes: attributes))
            }
            if next != nil { output.append(NSAttributedString(string: "\n", attributes: [.font: base])) }
            output.addAttribute(.paragraphStyle, value: paragraph, range: NSRange(location: start, length: output.length - start))
        }
        return output
    }

    private func font(scale: CGFloat = 1, weight: NSFont.Weight = .regular) -> NSFont {
        let system = NSFont.systemFont(ofSize: size * scale, weight: weight)
        return NSFont(descriptor: system.fontDescriptor.withDesign(.rounded) ?? system.fontDescriptor, size: size * scale) ?? system
    }
}

private extension NSFont {
    var italic: NSFont {
        NSFont(descriptor: fontDescriptor.withSymbolicTraits(fontDescriptor.symbolicTraits.union(.italic)), size: pointSize) ?? self
    }
    var withMonospacedDigits: NSFont {
        let settings: [[NSFontDescriptor.FeatureKey: Int]] = [[.typeIdentifier: kNumberSpacingType, .selectorIdentifier: kMonospacedNumbersSelector]]
        return NSFont(descriptor: fontDescriptor.addingAttributes([.featureSettings: settings]), size: pointSize) ?? self
    }
}
