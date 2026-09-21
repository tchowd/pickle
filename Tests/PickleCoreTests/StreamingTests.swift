import Foundation
import PickleCore

actor DraftRecorder {
    var values: [String] = []
    func add(_ value: String) { values.append(value) }
}
actor SSEFixture: StreamingHTTPTransport {
    var requests: [URLRequest] = []
    func send(_ request: URLRequest) async throws -> Data { throw PickleError.message("Unexpected buffered call") }
    func stream(_ request: URLRequest, line: @escaping @Sendable (String) async throws -> Void) async throws {
        requests.append(request)
        for event in [": keepalive", #"data: {"response":"Hello "}"#, #"data: {"response":"world 🌍","usage":{"completion_tokens":3}}"#, "data: [DONE]"] {
            try await line(event)
        }
    }
}
struct LateStreamingWriter: StreamingGenerativeProvider {
    let isRemote = false
    func generate(_ prompt: GenerationPrompt) async throws -> Generation { Generation(text: "old", model: "test") }
    func generate(_ prompt: GenerationPrompt, draft: @escaping DraftSink) async throws -> Generation {
        await draft("initial")
        // Deliberately ignore cancellation, exercising RequestRunner's identity gate.
        await withCheckedContinuation { continuation in
            DispatchQueue.global().asyncAfter(deadline: .now() + 0.04) { continuation.resume() }
        }
        await draft("stale draft")
        return Generation(text: "stale answer", model: "test")
    }
}
actor RepairStreamingWriter: StreamingGenerativeProvider {
    nonisolated let isRemote = false
    let failRepair: Bool
    var calls = 0
    init(failRepair: Bool) { self.failRepair = failRepair }
    func generate(_ prompt: GenerationPrompt, draft: @escaping DraftSink) async throws -> Generation {
        calls += 1
        await draft("Initial answer")
        return Generation(text: "Initial answer", model: "test")
    }
    func generate(_ prompt: GenerationPrompt) async throws -> Generation {
        calls += 1
        if failRepair { throw PickleError.message("Repair unavailable") }
        return Generation(text: "Revised answer", model: "test")
    }
}
final class StreamingTests: CheckSuite {
    func testStreamingProviderAndPipeline() async throws {
        let transport = SSEFixture(), drafts = DraftRecorder()
        let provider = CloudflareProvider(accountID: String(repeating: "a", count: 32), token: "fixture", transport: transport)
        let outcome = try await RequestPipeline(provider: provider).run(.init(snapshot: .init(text: "Hello world", appName: "Test", bundleID: "test"), action: .simplify), draft: { await drafts.add($0) })
        guard case .result(let result) = outcome else { return fail("Expected result") }
        expectEqual(result.text, "Hello world 🌍")
        let received = await drafts.values
        expectEqual(received, ["Hello ", "Hello world 🌍"])
        let request = await transport.requests.first!
        let body = try JSONSerialization.jsonObject(with: request.httpBody!) as! [String: Any]
        expectEqual(body["stream"] as? Bool, true)
        expectEqual(request.value(forHTTPHeaderField: "Accept"), "text/event-stream")
    }
    func testDraftRemainsVisibleThroughoutRepair() async throws {
        for fails in [false, true] {
            let writer = RepairStreamingWriter(failRepair: fails)
            let checker = MockJev(values: ["lost_uncertainty": 0.99], repairSucceeds: true)
            let events = DraftRecorder()
            let input = RequestInput(snapshot: .init(text: "Treatment may help.", appName: "Test", bundleID: "test"), action: .simplify)
            let outcome = try await RequestPipeline(provider: writer, evaluator: checker).run(input,
                draft: { await events.add("draft:" + $0) }, progress: { await events.add("status:" + $0) })
            guard case .result(let result) = outcome else { return fail("Expected completed result") }
            let recorded = await events.values
            expectEqual(recorded.filter { $0.hasPrefix("draft:") }, ["draft:Initial answer"])
            let review = recorded.firstIndex(of: "status:Reviewing…")!
            let refine = recorded.firstIndex(of: "status:Refining…")!
            expectTrue(recorded.firstIndex(of: "draft:Initial answer")! < review && review < refine)
            expectEqual(result.text, fails ? "Initial answer" : "Revised answer")
            expectEqual(result.repairs, 1)
            let calls = await writer.calls
            expectEqual(calls, 2)
        }
    }
    func testIncompleteAndInvalidStreamsFail() async throws {
        let incomplete = ProseStream()
        try await incomplete.receive(#"data: {"response":"Partial"}"#, draft: { _ in })
        do { _ = try await incomplete.finish(model: "test"); fail("Truncated stream must fail") } catch {}
        let invalid = ProseStream()
        do { try await invalid.receive("data: not-json", draft: { _ in }); fail("Malformed stream must fail") } catch {}
        let empty = ProseStream()
        try await empty.receive("data: [DONE]", draft: { _ in })
        do { _ = try await empty.finish(model: "test"); fail("Empty answer must fail") } catch {}
    }
    func testStreamOutputLimitAndOpenAIChunks() async throws {
        let oversized = ProseStream()
        let payload = String(decoding: try JSONEncoder().encode(["response": String(repeating: "x", count: Limits.output + 1)]), as: UTF8.self)
        do { try await oversized.receive("data: " + payload, draft: { _ in }); fail("Oversized answer must fail") } catch {}
        let stream = ProseStream()
        try await stream.receive(#"data: {"choices":[{"delta":{"content":"An answer."}}]}"#, draft: { _ in })
        try await stream.receive("data: [DONE]", draft: { _ in })
        let result = try await stream.finish(model: "test")
        expectEqual(result.text, "An answer.")
    }
    @MainActor func testLateDraftCannotEnterNewSession() async throws {
        let runner = RequestRunner()
        let input = RequestInput(snapshot: .init(text: "Sample", appName: "Test", bundleID: "test"), action: .simplify)
        var drafts: [String] = [], results: [String] = []
        runner.start(pipeline: .init(provider: LateStreamingWriter()), input: input, draft: { drafts.append($0) }, progress: { _ in }) { _ in results.append("old") }
        try await Task.sleep(for: .milliseconds(10))
        runner.start(pipeline: .init(provider: SampleProvider()), input: input, draft: { drafts.append($0) }, progress: { _ in }) { _ in results.append("new") }
        try await Task.sleep(for: .milliseconds(100))
        expectEqual(drafts, ["initial"])
        expectEqual(results, ["new"])
    }
}
