import Foundation

/// Writers often return light Markdown even when asked for plain text. This keeps only
/// presentation structure: links render as their label, and inline HTML stays literal text.
public enum AnswerMarkup {
    public struct Run: Equatable, Sendable {
        public let text: String
        public var strong = false, emphasis = false, code = false
        public init(_ text: String, strong: Bool = false, emphasis: Bool = false, code: Bool = false) {
            self.text = text; self.strong = strong; self.emphasis = emphasis; self.code = code
        }
    }
    public enum Kind: Equatable, Sendable { case paragraph, heading(Int), bullet, numbered(String), quote, code }
    public struct Block: Equatable, Sendable {
        public let kind: Kind
        public let depth: Int
        /// Line breaks inside a block are "\n".
        public let runs: [Run]
        public var text: String { runs.map(\.text).joined() }
        public var marker: String? {
            switch kind {
            case .bullet: return depth == 0 ? "•" : "◦"
            case .numbered(let number): return number
            default: return nil
            }
        }
        public var isListItem: Bool { marker != nil }
    }

    public static func blocks(_ source: String) -> [Block] {
        var blocks: [Block] = []
        var lines: [String] = [], kind = Kind.paragraph, depth = 0
        var fence: [String]?
        var listIndents: [Int] = []
        func flush() {
            guard !lines.isEmpty else { return }
            blocks.append(Block(kind: kind, depth: depth, runs: inline(lines.joined(separator: "\n"))))
            lines = []
        }
        func closeFence() {
            guard let code = fence else { return }
            blocks.append(Block(kind: .code, depth: 0, runs: [Run(code.joined(separator: "\n"), code: true)]))
            fence = nil
        }
        for raw in source.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n") {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if fence != nil {
                if line.hasPrefix("```") { closeFence() } else { fence?.append(raw) }
                continue
            }
            if line.hasPrefix("```") { flush(); listIndents = []; fence = []; continue }
            if line.isEmpty { flush(); continue }
            let indent = raw.prefix { $0 == " " || $0 == "\t" }.reduce(0) { $0 + ($1 == "\t" ? 4 : 1) }
            if let (level, title) = heading(line) {
                flush(); listIndents = []
                blocks.append(Block(kind: .heading(level), depth: 0, runs: inline(title)))
            } else if isRule(line) {
                flush(); listIndents = []
            } else if let (itemKind, content) = listItem(line) {
                flush()
                while let last = listIndents.last, last > indent { listIndents.removeLast() }
                if listIndents.last != indent { listIndents.append(indent) }
                kind = itemKind; depth = min(listIndents.count - 1, 3); lines = [content]
            } else if line.hasPrefix(">") {
                if kind != .quote { flush(); listIndents = [] }
                kind = .quote; depth = 0
                lines.append(String(line.dropFirst()).trimmingCharacters(in: .whitespaces))
            } else if !lines.isEmpty && (kind == .paragraph || (indent > 0 && kind != .quote)) {
                // Plain lines continue a paragraph; indented lines continue a list item.
                lines.append(line)
            } else {
                flush(); listIndents = []
                kind = .paragraph; depth = 0; lines = [line]
            }
        }
        // An unclosed fence is still shown as code while an answer streams in.
        closeFence(); flush()
        return blocks
    }

    /// Readable text without Markdown syntax, for copying and matching selections.
    public static func plainText(_ source: String) -> String {
        let blocks = blocks(source)
        var result = ""
        for (index, block) in blocks.enumerated() {
            if index > 0 { result += block.isListItem && blocks[index - 1].isListItem ? "\n" : "\n\n" }
            if let marker = block.marker { result += String(repeating: "  ", count: block.depth) + marker + " " }
            result += block.text
        }
        return result
    }

    private static func heading(_ line: String) -> (Int, String)? {
        let level = line.prefix { $0 == "#" }.count
        guard (1...6).contains(level), line.dropFirst(level).first == " " else { return nil }
        var title = line.dropFirst(level).trimmingCharacters(in: .whitespaces)
        while title.hasSuffix("#") { title.removeLast() }
        title = title.trimmingCharacters(in: .whitespaces)
        return title.isEmpty ? nil : (level, title)
    }

    private static func isRule(_ line: String) -> Bool {
        let marks = line.filter { $0 != " " }
        guard marks.count >= 3, let first = marks.first, "-*_".contains(first) else { return false }
        return marks.allSatisfy { $0 == first }
    }

    private static func listItem(_ line: String) -> (Kind, String)? {
        if let first = line.first, "-*+•–".contains(first), line.dropFirst().first == " " {
            return (.bullet, line.dropFirst(2).trimmingCharacters(in: .whitespaces))
        }
        let digits = line.prefix { $0.isASCII && $0.isNumber }
        guard (1...3).contains(digits.count) else { return nil }
        let rest = line.dropFirst(digits.count)
        guard let delimiter = rest.first, delimiter == "." || delimiter == ")", rest.dropFirst().first == " " else { return nil }
        return (.numbered(digits + "."), rest.dropFirst(2).trimmingCharacters(in: .whitespaces))
    }

    private static func inline(_ text: String) -> [Run] {
        let options = AttributedString.MarkdownParsingOptions(allowsExtendedAttributes: false, interpretedSyntax: .inlineOnlyPreservingWhitespace, failurePolicy: .returnPartiallyParsedIfPossible)
        guard let parsed = try? AttributedString(markdown: text, options: options) else { return [Run(text)] }
        var runs: [Run] = []
        for part in parsed.runs {
            let intent = part.inlinePresentationIntent ?? []
            let run = Run(String(parsed[part.range].characters), strong: intent.contains(.stronglyEmphasized),
                          emphasis: intent.contains(.emphasized), code: intent.contains(.code))
            if let last = runs.last, last.strong == run.strong, last.emphasis == run.emphasis, last.code == run.code {
                runs[runs.count - 1] = Run(last.text + run.text, strong: run.strong, emphasis: run.emphasis, code: run.code)
            } else { runs.append(run) }
        }
        return runs
    }
}
