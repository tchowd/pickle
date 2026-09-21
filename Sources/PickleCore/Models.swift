import Foundation

public enum Limits {
    // UTF-8 budgets also bound worst-case token consumption. Reject; never silently truncate sources.
    public static let selection = 12_000, context = 6_000, question = 2_000, output = 12_000
    public static let conversationBytes = 16_000, turns = 6, responseBytes = 256_000
    public static let requestSeconds: TimeInterval = 40
}
public enum PickleError: Error, LocalizedError, Equatable {
    case message(String)
    public var errorDescription: String? { if case .message(let message) = self { return message }; return nil }
}
public enum ReadingAction: String, CaseIterable, Codable, Sendable { case simplify = "Simplify", expand = "Expand", chart = "Generate chart", followUp = "Follow-up" }
public enum ReadingLevel: String, CaseIterable, Codable, Sendable { case automatic = "Automatic", plain = "Plain language", balanced = "Balanced", technical = "Keep technical detail" }
public struct SelectionBounds: Codable, Sendable, Equatable {
    public let x: Double, y: Double, width: Double, height: Double
    public init(x: Double, y: Double, width: Double, height: Double) { self.x = x; self.y = y; self.width = width; self.height = height }
}
public struct SelectionSnapshot: Identifiable, Codable, Sendable, Equatable {
    public let id: UUID, text: String, appName: String, bundleID: String, capturedAt: Date
    public let bounds: SelectionBounds?, method: String, limitations: [String]
    public let sourceTitle: String?, sourceURL: String?
    public init(text: String, appName: String, bundleID: String, bounds: SelectionBounds? = nil, method: String = "Accessibility", limitations: [String] = [], sourceTitle: String? = nil, sourceURL: String? = nil) {
        id = UUID(); self.text = text; self.appName = appName; self.bundleID = bundleID; capturedAt = Date(); self.bounds = bounds; self.method = method; self.limitations = limitations; self.sourceTitle = sourceTitle; self.sourceURL = sourceURL
    }
}
public struct ConversationTurn: Identifiable, Sendable {
    public let id = UUID()
    public let question: String, answer: String, quality: QualityStatus
    public init(question: String, answer: String, quality: QualityStatus) { self.question = question; self.answer = answer; self.quality = quality }
}
public enum QualityStatus: Equatable, Sendable {
    case checked, concerns([String]), uncertain, notChecked(String), chartValidated
    public var label: String {
        switch self {
        case .checked: return "Checked against your selection"
        case .concerns: return "Check found concerns — review the source"
        case .uncertain: return "Check inconclusive — review the source"
        case .notChecked(let reason): return "Semantic checks not performed · \(reason)"
        case .chartValidated: return "Chart structure and source evidence checked"
        }
    }
    public var details: String {
        switch self {
        case .concerns(let flags): return flags.joined(separator: ", ").replacingOccurrences(of: "_", with: " ")
        default: return "These checks assess support in the supplied text. They do not establish whether the source is true. Model judgments can be wrong."
        }
    }
}
public struct ReadingResult: Identifiable, Sendable {
    public let id = UUID()
    public let action: ReadingAction, text: String, chart: ValidatedChart?, quality: QualityStatus
    public let model: String, evaluatorModel: String?, elapsed: Double, repairs: Int
    public let notes: [String]
    public init(action: ReadingAction, text: String, chart: ValidatedChart? = nil, quality: QualityStatus, model: String, evaluatorModel: String? = nil, elapsed: Double = 0, repairs: Int = 0, notes: [String] = []) {
        self.action = action; self.text = text; self.chart = chart; self.quality = quality; self.model = model; self.evaluatorModel = evaluatorModel; self.elapsed = elapsed; self.repairs = repairs; self.notes = notes
    }
}
public struct RequestInput: Sendable {
    public let snapshot: SelectionSnapshot, action: ReadingAction, context: String, level: ReadingLevel
    public let limited: Bool, chartChoice: ChartKind?, question: String, previousResult: String, conversation: [ConversationTurn]
    public init(snapshot: SelectionSnapshot, action: ReadingAction, context: String = "", level: ReadingLevel = .automatic, limited: Bool = false, chartChoice: ChartKind? = nil, question: String = "", previousResult: String = "", conversation: [ConversationTurn] = []) {
        self.snapshot = snapshot; self.action = action; self.context = context; self.level = level; self.limited = limited; self.chartChoice = chartChoice; self.question = question; self.previousResult = previousResult; self.conversation = conversation
    }
    public var source: String { snapshot.text + (context.isEmpty ? "" : "\n\nUser-supplied context:\n" + context) }
    public func validate() throws {
        guard !snapshot.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { throw PickleError.message("Select or paste some text first.") }
        guard snapshot.text.utf8.count <= Limits.selection, context.utf8.count <= Limits.context, question.utf8.count <= Limits.question else { throw PickleError.message("This selection, context, or question is too large. Use a shorter passage.") }
        guard previousResult.utf8.count <= Limits.output, conversation.count <= Limits.turns,
              conversation.reduce(0, { $0 + $1.question.utf8.count + $1.answer.utf8.count }) <= Limits.conversationBytes else { throw PickleError.message("This conversation is full. Clear follow-ups to continue with the same selection.") }
    }
}
public struct Usage: Codable, Sendable {
    public let input_tokens: Int?, output_tokens: Int?
    public init(input_tokens: Int? = nil, output_tokens: Int? = nil) { self.input_tokens = input_tokens; self.output_tokens = output_tokens }
    enum CodingKeys: String, CodingKey { case input_tokens, output_tokens, prompt_tokens, completion_tokens }
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        input_tokens = try c.decodeIfPresent(Int.self, forKey: .input_tokens) ?? c.decodeIfPresent(Int.self, forKey: .prompt_tokens)
        output_tokens = try c.decodeIfPresent(Int.self, forKey: .output_tokens) ?? c.decodeIfPresent(Int.self, forKey: .completion_tokens)
    }
    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encodeIfPresent(input_tokens, forKey: .input_tokens); try c.encodeIfPresent(output_tokens, forKey: .output_tokens)
    }
}
public struct Generation: Sendable {
    public let text: String, model: String, usage: Usage?
    public init(text: String, model: String, usage: Usage? = nil) { self.text = text; self.model = model; self.usage = usage }
}
public struct GenerationPrompt: Sendable {
    public let system: String, user: String, structured: Bool
    public let schemaJSON: String?
    public init(system: String, user: String, structured: Bool, schemaJSON: String? = nil) { self.system = system; self.user = user; self.structured = structured; self.schemaJSON = schemaJSON }
}
public protocol GenerativeProvider: Sendable {
    var isRemote: Bool { get }
    func generate(_ prompt: GenerationPrompt) async throws -> Generation
}
public enum MetricEvent: Sendable {
    case generation(Usage?), evaluation(Usage?), repair, cancelled, failure, route(ChartKind?), completed(Double)
}
public typealias MetricSink = @Sendable (MetricEvent) -> Void
