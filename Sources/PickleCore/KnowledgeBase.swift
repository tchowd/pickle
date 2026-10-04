import Foundation
import Combine

/// One completed answer, as the user asked for it. Only a pointer to an attached reference is kept;
/// screenshots, OCR, visual summaries, audio, reference bodies, prompts and credentials never are.
public struct KnowledgeEntry: Codable, Identifiable, Sendable, Equatable {
    public enum Action: String, Codable, Sendable {
        case simplify, expand, chart, followUp = "follow-up", adjustment, explain
    }
    public struct Reference: Codable, Sendable, Equatable {
        public let url: String, title: String, kind: String
        public init(url: String, title: String, kind: String) { self.url = url; self.title = title; self.kind = kind }
    }
    public var id = UUID()
    public let recordedAt: Date
    public let appName: String, bundleID: String
    public let action: Action
    /// The user's own words: a typed question, an adjustment's button label, or the term to explain.
    public let question: String?
    public let passage: String, answer: String
    public let chart: ChartKind?
    public let reference: Reference?

    public init(input: RequestInput, result: ReadingResult, action: Action, question: String?, recordedAt: Date = Date()) {
        self.recordedAt = recordedAt
        appName = input.snapshot.appName; bundleID = input.snapshot.bundleID
        self.action = action
        self.question = question.flatMap { $0.isEmpty ? nil : $0 }
        passage = input.snapshot.text
        // Charts keep their text alternative; the chart itself is not re-rendered from the log.
        answer = result.text
        chart = result.chart?.kind
        reference = input.reference.map { Reference(url: $0.url, title: $0.title, kind: $0.kind) }
    }
    func duplicates(_ other: KnowledgeEntry) -> Bool {
        passage == other.passage && action == other.action && question == other.question && answer == other.answer
    }
}

/// Opt-in, on-this-Mac log of completed answers. It is never read back into a request.
@MainActor public final class KnowledgeBase: ObservableObject {
    /// Five times the bookmark library: one reading session can produce several entries.
    public static let capacity = 1_000
    nonisolated public static var defaultFile: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Pickle/KnowledgeBase/entries.json")
    }
    @Published public private(set) var entries: [KnowledgeEntry] = []
    @Published public var error: String?
    public let file: URL
    private var unreadable = false
    private struct Archive: Codable { var version = 1; var entries: [KnowledgeEntry] }
    private static let unreadableMessage = "The knowledge base could not be opened. Its file has not been changed, and nothing new is recorded."

    public init(file: URL = KnowledgeBase.defaultFile) {
        self.file = file
        guard FileManager.default.fileExists(atPath: file.path) else { return }
        do {
            let archive = try Self.decoder.decode(Archive.self, from: Data(contentsOf: file))
            guard archive.version == 1 else { throw PickleError.message("Unsupported version") }
            entries = archive.entries
        } catch { unreadable = true; self.error = Self.unreadableMessage }
    }

    public func append(_ entry: KnowledgeEntry) {
        guard !unreadable else { error = Self.unreadableMessage; return }
        guard !entries.contains(where: entry.duplicates) else { return }
        guard entries.count < Self.capacity else { error = "Your knowledge base is full (\(Self.capacity.formatted()) entries). Delete entries to keep recording."; return }
        persist([entry] + entries)
    }
    public func remove(_ id: UUID) {
        guard !unreadable else { error = Self.unreadableMessage; return }
        persist(entries.filter { $0.id != id })
    }
    public func removeAll() {
        guard !unreadable else { error = Self.unreadableMessage; return }
        persist([])
    }
    public func search(_ query: String) -> [KnowledgeEntry] {
        let query = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return entries }
        return entries.filter { entry in
            [entry.passage, entry.question ?? "", entry.answer, entry.reference?.title ?? "", entry.reference?.url ?? ""]
                .contains { $0.localizedCaseInsensitiveContains(query) }
        }
    }

    private func persist(_ updated: [KnowledgeEntry]) {
        do {
            let directory = file.deletingLastPathComponent()
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            try Self.encoder.encode(Archive(entries: updated)).write(to: file, options: .atomic)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
            entries = updated; error = nil
        } catch { self.error = "Couldn’t update the knowledge base. Please try again." }
    }
    private static var encoder: JSONEncoder {
        let encoder = JSONEncoder(); encoder.dateEncodingStrategy = .iso8601; encoder.outputFormatting = [.sortedKeys]
        return encoder
    }
    private static var decoder: JSONDecoder {
        let decoder = JSONDecoder(); decoder.dateDecodingStrategy = .iso8601
        return decoder
    }
}
