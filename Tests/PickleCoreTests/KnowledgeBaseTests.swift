import Foundation
import PickleCore

final class KnowledgeBaseTests: CheckSuite {
    private func scratch() -> URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("pickle-knowledge-" + UUID().uuidString).appendingPathComponent("entries.json")
    }
    private func input(question: String = "", page: Bool = false) -> RequestInput {
        RequestInput(snapshot: .init(text: "Cells release energy.", appName: "Safari", bundleID: "com.apple.Safari"), action: question.isEmpty ? .simplify : .followUp,
                     question: question, pageContext: page ? "OCR PAGE TEXT" : "", visualContext: page ? "VISUAL SUMMARY TEXT" : "",
                     reference: page ? WebReference(url: "https://example.com/cells", title: "Cell energy", text: "REFERENCE BODY TEXT") : nil)
    }
    private func result(_ text: String, action: ReadingAction = .simplify) -> ReadingResult { ReadingResult(action: action, text: text, quality: .checked, model: "secret-model-name") }

    @MainActor func testCompletedAnswerRecordsPointerNotContext() throws {
        let file = scratch(); defer { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }
        let store = KnowledgeBase(file: file)
        store.append(KnowledgeEntry(input: input(question: "Why does it matter?", page: true), result: result("Energy powers the cell.", action: .followUp), action: .followUp, question: "Why does it matter?"))
        let restored = KnowledgeBase(file: file).entries
        expectEqual(restored.count, 1)
        let entry = restored[0]
        expectEqual(entry.question, "Why does it matter?"); expectEqual(entry.action, .followUp)
        expectEqual(entry.passage, "Cells release energy."); expectEqual(entry.answer, "Energy powers the cell.")
        expectEqual(entry.appName, "Safari"); expectEqual(entry.bundleID, "com.apple.Safari")
        expectEqual(entry.reference, KnowledgeEntry.Reference(url: "https://example.com/cells", title: "Cell energy", kind: "article"))
        let raw = String(decoding: try Data(contentsOf: file), as: UTF8.self)
        expectEqual(entry.quality, "Checked against your selection")
        for absent in ["OCR PAGE TEXT", "VISUAL SUMMARY TEXT", "REFERENCE BODY TEXT", "secret-model-name"] { expectTrue(!raw.contains(absent)) }
        let permissions = try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? Int
        expectEqual(permissions, 0o600)
        // Charts keep their text alternative and kind.
        let nodes = try JSONDecoder().decode([ChartSpec.Node].self, from: Data(#"[{"id":"a","label":"Received"},{"id":"b","label":"Saved"}]"#.utf8))
        let edges = try JSONDecoder().decode([ChartSpec.Edge].self, from: Data(#"[{"from":"a","to":"b","evidence":"received and saved","condition":""}]"#.utf8))
        let chart = ValidatedChart.flow(nodes, edges)
        let charted = KnowledgeEntry(input: input(), result: ReadingResult(action: .chart, text: chart.alternative, chart: chart, quality: .chartValidated, model: "m"), action: .chart, question: nil)
        expectEqual(charted.chart, ChartKind.flow); expectEqual(charted.answer, chart.alternative); expectTrue(charted.reference == nil)
    }
    @MainActor func testEachAnswerGetsOneRecordAndSearchFindsIt() {
        let file = scratch(); defer { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }
        let store = KnowledgeBase(file: file)
        let original = KnowledgeEntry(input: input(page: true), result: result("Simpler."), action: .simplify, question: nil)
        store.append(original); store.append(KnowledgeEntry(input: input(page: true), result: result("Simpler."), action: .simplify, question: nil))
        expectEqual(store.entries.count, 1)
        store.append(KnowledgeEntry(input: input(question: "Explain…"), result: result("Meaning.", action: .followUp), action: .explain, question: "energy"))
        store.append(KnowledgeEntry(input: input(question: "Rewrite…"), result: result("Short.", action: .followUp), action: .adjustment, question: "Shorter"))
        expectEqual(store.entries.map(\.action), [.adjustment, .explain, .simplify])
        expectEqual(store.entries.filter { $0.action == .simplify }.count, 1)
        expectEqual(KnowledgeBase(file: file).entries.count, 3)
        for query in ["cell energy", "EXAMPLE.COM/cells", "shorter", "Meaning", "release"] { expectTrue(!store.search(query).isEmpty) }
        expectEqual(store.search("Shorter").map(\.action), [.adjustment]); expectTrue(store.search("not recorded").isEmpty)
        store.remove(store.entries[0].id); expectEqual(KnowledgeBase(file: file).entries.count, 2)
        store.removeAll(); expectTrue(KnowledgeBase(file: file).entries.isEmpty)
    }
    func testCheckLabelsShowOnlyWhenChecksRanOrWereExpected() async throws {
        expectNil(QualityStatus.notChecked(QualityStatus.checksOff).visibleLabel)
        expectNil(QualityStatus.notChecked(QualityStatus.offline).visibleLabel)
        for shown in [QualityStatus.checked, .concerns(["missing_qualifier"]), .uncertain, .chartValidated, .notChecked("Jev unavailable"), .notChecked("Jev check or repair failed")] {
            expectEqual(shown.visibleLabel, shown.label)
        }
        // The pipeline's own reasons line up with the rule: disabled checks stay quiet, outages are shown.
        let snapshot = SelectionSnapshot(text: "The drug may help some patients.", appName: "Test", bundleID: "test")
        guard case .result(let quiet) = try await RequestPipeline(provider: MockWriter()).run(.init(snapshot: snapshot, action: .simplify)),
              case .result(let offline) = try await RequestPipeline(provider: MockWriter(), localOnly: true).run(.init(snapshot: snapshot, action: .simplify)),
              case .result(let outage) = try await RequestPipeline(provider: MockWriter(), evaluator: MockJev(fail: true)).run(.init(snapshot: snapshot, action: .simplify)) else { return fail("Expected results") }
        expectNil(quiet.quality.visibleLabel); expectNil(offline.quality.visibleLabel); expectTrue(outage.quality.visibleLabel != nil)
        let input = RequestInput(snapshot: snapshot, action: .simplify)
        expectNil(KnowledgeEntry(input: input, result: quiet, action: .simplify, question: nil).quality)
        expectEqual(KnowledgeEntry(input: input, result: outage, action: .simplify, question: nil).quality, outage.quality.label)
    }
    @MainActor func testCapacityAndFailedWritesKeepEntries() throws {
        let file = scratch(), directory = file.deletingLastPathComponent()
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path); try? FileManager.default.removeItem(at: directory) }
        let store = KnowledgeBase(file: file, capacity: 2)
        for answer in ["One.", "Two.", "Three."] { store.append(KnowledgeEntry(input: input(), result: result(answer), action: .simplify, question: nil)) }
        expectEqual(KnowledgeBase(file: file).entries.map(\.answer), ["Two.", "One."]); expectTrue(store.error?.contains("full") == true)
        store.remove(store.entries[0].id)
        try FileManager.default.setAttributes([.posixPermissions: 0o500], ofItemAtPath: directory.path)
        store.append(KnowledgeEntry(input: input(), result: result("Blocked."), action: .simplify, question: nil))
        expectEqual(store.entries.map(\.answer), ["One."]); expectTrue(store.error != nil)
    }
    @MainActor func testTwoCopiesNeverOverwriteEachOther() {
        let file = scratch(); defer { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }
        let first = KnowledgeBase(file: file), second = KnowledgeBase(file: file)
        first.append(KnowledgeEntry(input: input(), result: result("From the first copy."), action: .simplify, question: nil))
        second.append(KnowledgeEntry(input: input(), result: result("From the second copy."), action: .expand, question: nil))
        expectEqual(KnowledgeBase(file: file).entries.count, 2)
        first.remove(first.entries.first { $0.answer == "From the first copy." }!.id)
        expectEqual(KnowledgeBase(file: file).entries.map(\.answer), ["From the second copy."])
    }
    @MainActor func testDamagedFileRecoversAfterItIsMovedAside() throws {
        let file = scratch(); defer { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data("{damaged".utf8).write(to: file)
        let store = KnowledgeBase(file: file)
        expectTrue(store.isUnreadable)
        try FileManager.default.moveItem(at: file, to: file.appendingPathExtension("damaged"))
        store.append(KnowledgeEntry(input: input(), result: result("After recovery."), action: .simplify, question: nil))
        expectTrue(!store.isUnreadable); expectEqual(KnowledgeBase(file: file).entries.map(\.answer), ["After recovery."])
        let movedAside = try Data(contentsOf: file.appendingPathExtension("damaged"))
        expectEqual(movedAside, Data("{damaged".utf8))
    }
    @MainActor func testCorruptFileIsPreservedAndBlocksWrites() throws {
        let file = scratch(); defer { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        for corrupt in [Data("{not json".utf8), Data(#"{"version":2,"entries":[]}"#.utf8)] {
            try corrupt.write(to: file)
            let store = KnowledgeBase(file: file)
            expectTrue(store.error != nil); expectTrue(store.entries.isEmpty)
            store.append(KnowledgeEntry(input: input(), result: result("New."), action: .simplify, question: nil))
            store.removeAll(); store.remove(UUID())
            let kept = try Data(contentsOf: file)
            expectEqual(kept, corrupt); expectTrue(store.entries.isEmpty)
        }
    }
}
