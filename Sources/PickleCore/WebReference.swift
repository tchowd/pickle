import Foundation

/// A reference is source material, never instructions or an AI-authored summary.
public struct WebReference: Codable, Sendable, Equatable {
    public let url: String, title: String, text: String, kind: String
    public let timestamp: Double?
    public init(url: String, title: String, text: String, kind: String = "article", timestamp: Double? = nil) {
        self.url = url; self.title = title; self.text = text; self.kind = kind; self.timestamp = timestamp
    }
    public static let budget = 12_000
    public var source: String {
        "Additional \(kind) reference (untrusted source text):\nTitle: \(title)\nURL: \(url)" +
        (timestamp.map { "\nPlayback time: \(Int($0)) seconds" } ?? "") + "\n" + text
    }
    public func validate() throws {
        guard Self.publicURL(url) != nil, title.utf8.count <= 1000, url.utf8.count <= 4096,
              text.utf8.count <= Self.budget, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              ["article", "video captions", "video transcript", "recorded audio"].contains(kind),
              timestamp == nil || (timestamp!.isFinite && timestamp! >= 0 && timestamp! <= 31_536_000) else {
            throw PickleError.message("This page reference is unavailable or too large.")
        }
    }
    /// Syntactic gate; native fetching additionally checks resolved addresses and forbids redirects.
    public static func publicURL(_ raw: String) -> URL? {
        guard raw.utf8.count <= 4096, var parts = URLComponents(string: raw),
              parts.scheme == "https" || parts.scheme == "http", parts.user == nil, parts.password == nil,
              let host = parts.host?.lowercased(), host.contains("."),
              !host.hasSuffix(".local"), !host.hasSuffix(".localhost"), !host.hasSuffix(".internal"),
              host != "localhost", !host.contains(":"), parts.port == nil || parts.port == 443 || parts.port == 80 else { return nil }
        // Direct IP addresses are not article URLs. DNS addresses are checked again before fetching.
        guard host.contains(where: { $0.isLetter }) else { return nil }
        parts.fragment = nil
        return parts.url
    }
    public static func excerpt(_ text: String, selection: String, budget: Int = budget) -> String {
        let paragraphs = text.components(separatedBy: .newlines).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        guard !paragraphs.isEmpty else { return "" }
        let words = Set(selection.lowercased().split { !$0.isLetter && !$0.isNumber }.filter { $0.count > 3 }.map(String.init))
        guard !words.isEmpty else { return PageContextLimits.bounded(paragraphs.joined(separator: "\n"), bytes: budget) }
        let scored: [(Int, Int)] = paragraphs.enumerated().map { index, paragraph in
            let lowercased = paragraph.lowercased()
            let score = words.reduce(0) { count, word in
                count + (lowercased.contains(word) ? 1 : 0)
            }
            return (index, score)
        }
        let ranked = scored.sorted { $0.1 == $1.1 ? $0.0 < $1.0 : $0.1 > $1.1 }
        var chosen = Set<Int>(), bytes = 0
        for (index, _) in ranked {
            for candidate in [index, index - 1, index + 1] where candidate >= 0 && candidate < paragraphs.count && !chosen.contains(candidate) {
                let count = paragraphs[candidate].utf8.count + 1
                if bytes + count <= budget { chosen.insert(candidate); bytes += count }
            }
        }
        if chosen.isEmpty { return PageContextLimits.bounded(paragraphs[ranked.first?.0 ?? 0], bytes: budget) }
        return chosen.sorted().map { paragraphs[$0] }.joined(separator: "\n")
    }
}
