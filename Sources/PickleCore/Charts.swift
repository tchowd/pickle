import Foundation
public enum ChartKind: String, Codable, CaseIterable, Sendable { case bar = "bar_chart", flow = "flow_diagram" }
public struct NumericEvidence: Codable, Identifiable, Sendable, Equatable {
    public let id: String, value: Double, unit: String, quote: String, excerpt: String
}
public struct ChartSpec: Codable, Sendable {
    public struct Bar: Codable, Sendable { public let evidenceID: String }
    public struct Node: Codable, Identifiable, Sendable { public let id: String, label: String }
    public struct Edge: Codable, Sendable { public let from: String, to: String, evidence: String, condition: String }
    public let type: ChartKind
    public let bars: [Bar]?, nodes: [Node]?, edges: [Edge]?
}
public enum ValidatedChart: Sendable {
    case bar([NumericEvidence]), flow([ChartSpec.Node], [ChartSpec.Edge])
    public var alternative: String {
        switch self {
        case .bar(let bars): return "Quantities in your selection\n" + bars.map { "\($0.excerpt): \($0.value.formatted()) \($0.unit)" }.joined(separator: "\n")
        case .flow(let nodes, let edges):
            let labels = Dictionary(uniqueKeysWithValues: nodes.map { ($0.id, $0.label) })
            return "Relationships in your selection\n" + edges.map { "\(labels[$0.from] ?? "") → \(labels[$0.to] ?? "")\($0.condition.isEmpty ? "" : " [" + $0.condition + "]")\nSource: \($0.evidence)" }.joined(separator: "\n")
        }
    }
}
public enum ResultValidator {
    public static func prose(_ text: String) throws {
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, text.utf8.count <= Limits.output else { throw PickleError.message("The generated result was empty or too long. Try a shorter passage.") }
    }
    /// Conservative explicit-unit parser. Bare numbers, dates, conversions, and illustrative values are not accepted.
    public static func numericEvidence(in source: String) -> [NumericEvidence] {
        let pattern = #"(?<![\w.,])([$€£])?(-?(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?)(?:[ \t]+(thousand|million|billion))?[ \t]*(%|percent\b|USD\b|EUR\b|GBP\b|kg\b|km\b|ms\b|seconds\b|minutes\b|hours\b|users\b|requests\b|items\b)?(?![\w]|[.,]\d)"#
        guard let regex = try? NSRegularExpression(pattern: pattern, options: .caseInsensitive) else { return [] }
        let ns = source as NSString
        return regex.matches(in: source, range: NSRange(location: 0, length: ns.length)).compactMap { match in
            func group(_ n: Int) -> String { match.range(at: n).location == NSNotFound ? "" : ns.substring(with: match.range(at: n)) }
            let prefix = group(1), suffix = group(4).lowercased()
            guard !prefix.isEmpty || !suffix.isEmpty, let number = Double(group(2).replacingOccurrences(of: ",", with: "")), number.isFinite else { return nil }
            let currencies = ["$": "USD", "€": "EUR", "£": "GBP"]
            if !prefix.isEmpty && !suffix.isEmpty && currencies[prefix]?.lowercased() != suffix { return nil }
            let scale: Double = ["thousand": 1e3, "million": 1e6, "billion": 1e9][group(3).lowercased()] ?? 1
            let value = number * scale
            guard value.isFinite, abs(value) <= 1e15 else { return nil }
            let unit = currencies[prefix] ?? (["percent": "%", "usd": "USD", "eur": "EUR", "gbp": "GBP"][suffix] ?? suffix)
            let start = max(0, match.range.location - 45), end = min(ns.length, NSMaxRange(match.range) + 45)
            let excerpt = ns.substring(with: NSRange(location: start, length: end - start)).trimmingCharacters(in: .whitespacesAndNewlines)
            // Do not turn illustrative numeric examples into evidence-backed charts.
            if excerpt.range(of: #"(?i)hypothetical|illustrative|for example|suppose"#, options: .regularExpression) != nil { return nil }
            return NumericEvidence(id: "n\(match.range.location)", value: value, unit: unit, quote: ns.substring(with: match.range).trimmingCharacters(in: .whitespaces), excerpt: excerpt)
        }
    }
    public static func validate(_ text: String, kind: ChartKind, source: String) throws -> ValidatedChart {
        try prose(text)
        guard let data = text.data(using: .utf8) else { throw PickleError.message("Invalid chart encoding.") }
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw PickleError.message("Invalid chart JSON.") }
        let expected: Set<String> = kind == .bar ? ["type", "bars"] : ["type", "nodes", "edges"]
        guard Set(object.keys) == expected else { throw PickleError.message("Unexpected chart fields.") }
        func exactFields(_ name: String, _ fields: Set<String>) -> Bool {
            guard let rows = object[name] as? [[String: Any]] else { return false }
            return rows.allSatisfy { Set($0.keys) == fields }
        }
        guard kind == .bar ? exactFields("bars", ["evidenceID"]) : (exactFields("nodes", ["id", "label"]) && exactFields("edges", ["from", "to", "evidence", "condition"])) else { throw PickleError.message("Unexpected fields in chart entries.") }
        let spec: ChartSpec
        do { spec = try JSONDecoder().decode(ChartSpec.self, from: data) }
        catch { throw PickleError.message("The model returned an invalid chart schema. Try again or use Expand.") }
        guard spec.type == kind else { throw PickleError.message("The model returned the wrong chart type.") }
        switch kind {
        case .bar:
            guard let bars = spec.bars, (2...12).contains(bars.count), spec.nodes == nil, spec.edges == nil,
                  Set(bars.map(\.evidenceID)).count == bars.count else { throw PickleError.message("A bar chart needs 2–12 distinct source quantities.") }
            let evidence = Dictionary(uniqueKeysWithValues: numericEvidence(in: source).map { ($0.id, $0) })
            let values = try bars.map { bar -> NumericEvidence in
                guard let value = evidence[bar.evidenceID] else { throw PickleError.message("A chart quantity has no explicit source evidence.") }; return value
            }
            guard Set(values.map(\.unit)).count == 1 else { throw PickleError.message("Chart quantities use incompatible units.") }
            return .bar(values)
        case .flow:
            guard let nodes = spec.nodes, let edges = spec.edges, spec.bars == nil,
                  (2...10).contains(nodes.count), (1...16).contains(edges.count), Set(nodes.map(\.id)).count == nodes.count,
                  nodes.allSatisfy({ !$0.id.isEmpty && $0.id.count <= 40 && !$0.label.isEmpty && $0.label.count <= 180 && source.contains($0.label) })
            else { throw PickleError.message("The flow diagram has invalid or unsupported nodes.") }
            let labels = Dictionary(uniqueKeysWithValues: nodes.map { ($0.id, $0.label) })
            var seen = Set<String>(), connected = Set<String>()
            for edge in edges {
                guard let from = labels[edge.from], let to = labels[edge.to], edge.from != edge.to,
                      seen.insert(edge.from + "→" + edge.to).inserted,
                      !edge.evidence.isEmpty, edge.evidence.count <= 1200, source.contains(edge.evidence),
                      edge.evidence.contains(from), edge.evidence.contains(to),
                      edge.condition.count <= 180, edge.condition.isEmpty || edge.evidence.contains(edge.condition)
                else { throw PickleError.message("A flow connection lacks valid source evidence.") }
                let conditional = edge.evidence.range(of: #"(?i)\b(if|unless|except|only when|provided that)\b"#, options: .regularExpression) != nil
                guard !conditional || !edge.condition.isEmpty else { throw PickleError.message("The diagram omitted a source condition.") }
                connected.insert(edge.from); connected.insert(edge.to)
            }
            let steps = explicitOrderedSteps(source)
            if !steps.isEmpty {
                guard Set(steps).isSubset(of: Set(nodes.map(\.label))), zip(steps, steps.dropFirst()).allSatisfy({ from, to in edges.contains { labels[$0.from] == from && labels[$0.to] == to } }) else { throw PickleError.message("The diagram omitted an explicitly stated process step or connection.") }
            }
            guard connected == Set(nodes.map(\.id)) else { throw PickleError.message("The diagram has disconnected nodes.") }
            return .flow(nodes, edges)
        }
    }
    /// A deliberately narrow English sequence recognizer. Keeps simple explicit operation lists complete.
    public static func explicitOrderedSteps(_ source: String) -> [String] {
        guard let regex = try? NSRegularExpression(pattern: #"\b(?:is|are)\s+([a-z]+ed(?:,\s*[a-z]+ed)*(?:,?\s+and\s+[a-z]+ed))\b"#, options: .caseInsensitive),
              let match = regex.firstMatch(in: source, range: NSRange(source.startIndex..., in: source)), let range = Range(match.range(at: 1), in: source) else { return [] }
        return String(source[range]).replacingOccurrences(of: #"\band\b"#, with: ",", options: .regularExpression).split(separator: ",").map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
    }
    public static func flowEvidence(_ source: String) -> [String] {
        if source.count <= 1200 { return [source] }
        var quotes: [String] = []
        source.enumerateSubstrings(in: source.startIndex..<source.endIndex, options: .bySentences) { sentence, _, _, _ in
            if let sentence, sentence.count <= 1200 { quotes.append(sentence.trimmingCharacters(in: .whitespacesAndNewlines)) }
        }
        return quotes
    }
    public static func jsonSchema(_ kind: ChartKind, source: String) throws -> String {
        func object(_ properties: [String: Any]) -> [String: Any] { ["type": "object", "properties": properties, "required": Array(properties.keys).sorted(), "additionalProperties": false] }
        var properties: [String: Any] = ["type": ["type": "string", "enum": [kind.rawValue]]]
        if kind == .bar {
            properties["bars"] = ["type": "array", "minItems": 2, "maxItems": 12, "items": object(["evidenceID": ["type": "string", "enum": numericEvidence(in: source).map(\.id)]])]
        } else {
            let steps = explicitOrderedSteps(source)
            let label: [String: Any] = steps.isEmpty ? ["type": "string", "maxLength": 180] : ["type": "string", "enum": steps]
            let condition: [String: Any] = ["type": "string", "maxLength": 180]
            let id: [String: Any] = ["type": "string", "maxLength": 40]
            properties["nodes"] = ["type": "array", "minItems": max(2, steps.count), "maxItems": steps.isEmpty ? 10 : steps.count, "items": object(["id": id, "label": label])]
            properties["edges"] = ["type": "array", "minItems": 1, "maxItems": 16, "items": object(["from": id, "to": id, "evidence": ["type": "string", "enum": flowEvidence(source)], "condition": condition])]
        }
        return String(decoding: try JSONSerialization.data(withJSONObject: object(properties), options: .sortedKeys), as: UTF8.self)
    }
    public static func schemaPrompt(_ kind: ChartKind, source: String) -> String {
        if kind == .bar {
            let candidates = (try? String(data: JSONEncoder().encode(numericEvidence(in: source)), encoding: .utf8)) ?? "[]"
            return "Return only JSON: {\"type\":\"bar_chart\",\"bars\":[{\"evidenceID\":\"ID\"}]}. Select 2–12 comparable candidates with the same unit measuring the same thing. Do not add values or labels. App code supplies those. Candidates: " + candidates
        }
        return "Return only JSON: {\"type\":\"flow_diagram\",\"nodes\":[{\"id\":\"a\",\"label\":\"exact source substring\"}],\"edges\":[{\"from\":\"a\",\"to\":\"b\",\"evidence\":\"exact source quote containing both node labels and the relationship\",\"condition\":\"exact source condition or empty string\"}]}. Use 2–10 nodes and 1–16 supported directed edges. Include every explicit process step and connect the complete stated sequence; do not stop after the first step. Use operation phrases as process nodes, not generic nouns. Preserve all conditions. No inferred relationships. All labels must be verbatim substrings of source. Select edge evidence from the source quotes allowed by the schema; each quote must contain both connected node labels."
    }
}
