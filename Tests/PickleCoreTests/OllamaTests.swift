import Foundation
import PickleCore

actor OllamaFixture: StreamingHTTPTransport {
    var requests: [URLRequest] = []
    let cloud: Bool, failure: Bool
    init(cloud: Bool = false, failure: Bool = false) { self.cloud = cloud; self.failure = failure }
    func send(_ request: URLRequest) async throws -> Data {
        requests.append(request)
        if failure { throw PickleError.message("Unavailable") }
        if request.url!.path == "/api/show" {
            return Data((cloud ? #"{"remote_host":"https://ollama.com","details":{"format":"gguf"},"capabilities":["completion"]}"# : #"{"details":{"format":"gguf"},"capabilities":["completion"]}"#).utf8)
        }
        if request.url!.path == "/api/tags" { return Data(#"{"models":[{"name":"llama3.2:1b"}]}"#.utf8) }
        return Data(#"{"message":{"content":"{\"bars\":[]}"},"done":true}"#.utf8)
    }
    func stream(_ request: URLRequest, line: @escaping @Sendable (String) async throws -> Void) async throws {
        requests.append(request)
        try await line(#"{"message":{"content":"A simple "},"done":false}"#)
        try await line(#"{"message":{"content":"answer."},"done":true,"done_reason":"stop","load_duration":2000000,"eval_duration":30000000,"eval_count":4}"#)
    }
}
final class OllamaTests: CheckSuite {
    let input = RequestInput(snapshot: .init(text: "The treatment may help.", appName: "Test", bundleID: "test"), action: .simplify)
    func testLocalRoutingStreamingAndNoCloudChecks() async throws {
        let transport = OllamaFixture(), jev = MockJev(), drafts = DraftRecorder()
        let provider = OllamaProvider(configuration: .init(address: "https://ross.example", token: "fixture"), transport: transport)
        let result = try await RequestPipeline(provider: provider, evaluator: jev).run(input, draft: { await drafts.add($0) })
        guard case .result(let answer) = result else { return fail("Expected result") }
        expectEqual(answer.text, "A simple answer."); expectEqual(answer.repairs, 0)
        let checks = await jev.count; expectEqual(checks, 0)
        let values = await drafts.values; expectEqual(values, ["A simple ", "A simple answer."])
        let requests = await transport.requests
        expectTrue(requests.allSatisfy { $0.url?.host == "ross.example" })
        let body = try JSONSerialization.jsonObject(with: requests.last!.httpBody!) as! [String: Any]
        expectEqual(body["keep_alive"] as? String, "2m")
        expectEqual((body["options"] as? [String: Any])?["num_ctx"] as? Int, 8192)
        expectEqual(requests.last?.value(forHTTPHeaderField: "Authorization"), "Bearer fixture")
        expectEqual(answer.timing?.loadSeconds, 0.002)
    }
    func testCloudAliasesAndMissingVisionFailClosed() async throws {
        for fixture in [OllamaFixture(cloud: true), OllamaFixture(failure: true)] {
            let provider = OllamaProvider(configuration: .init(address: "https://ross.example"), transport: fixture)
            do { _ = try await provider.generate(.init(system: "Explain", user: "Hello", structured: false)); fail("Expected failure") } catch {}
            let requests = await fixture.requests
            expectTrue(requests.allSatisfy { $0.url?.path != "/api/chat" })
        }
        let transport = OllamaFixture()
        let provider = OllamaProvider(configuration: .init(address: "http://127.0.0.1:11434", sameDevice: true), transport: transport)
        do { _ = try await provider.summarize(jpeg: Data([1,2]), selection: "Hello"); fail("Text model must reject images") } catch {}
        let calls = await transport.requests; expectEqual(calls.count, 1)
    }
    func testLocalOfflineDistinction() async throws {
        let transport = OllamaFixture()
        let remote = OllamaProvider(configuration: .init(address: "http://127.0.0.1:11436", sameDevice: false), transport: transport)
        do { _ = try await RequestPipeline(provider: remote, localOnly: true).run(input); fail("Tunnel is not same-device inference") } catch {}
        let calls = await transport.requests; expectTrue(calls.isEmpty)
        try LocalAIConfiguration(address: "http://127.0.0.1:11434", sameDevice: true).validate(offline: true)
        for config in [LocalAIConfiguration(address: "https://ross.example", sameDevice: true), .init(address: "https://ollama.com"), .init(address: "http://ross.example"), .init(address: "https://ross.example", model: "gpt-oss:cloud")] {
            do { try config.validate(); fail("Unsafe endpoint accepted") } catch {}
        }
    }
    func testOllamaNDJSONCompletionAndErrors() async throws {
        for line in ["data: [DONE]", "{", #"{"error":"secret source text"}"#, #"{"message":{"content":"Partial"},"done":true,"done_reason":"length"}"#] {
            do { try await OllamaStream().receive(line, draft: { _ in }); fail("Invalid chunk accepted") } catch { expectTrue(!error.localizedDescription.contains("secret source text")) }
        }
        let incomplete = OllamaStream()
        try await incomplete.receive(#"{"message":{"content":"Partial"},"done":false}"#, draft: { _ in })
        do { _ = try await incomplete.finish(model: "test"); fail("Truncated answer accepted") } catch {}
    }
    func testLocalContextPreservesSelectionAndBounds() throws {
        let selected = "Selected passage: treatment may help some people."
        let input = RequestInput(snapshot: .init(text: selected, appName: "Test", bundleID: "test"), action: .followUp, context: "Supplied context", question: "Why?", previousResult: String(repeating: "prior ", count: 1200), pageContext: String(repeating: "OCR ", count: 1400), reference: .init(url: "https://example.com", title: "Ref", text: String(repeating: "Reference ", count: 1100)))
        let prompt = try LocalPromptBudget.fit(.init(system: "Explain faithfully.", user: "", structured: false, localInput: input))
        let body = try JSONDecoder().decode([String:String].self, from: Data(prompt.user.utf8))
        expectEqual(body["source"], selected + "\n\nUser-supplied context:\nSupplied context")
        expectEqual(body["follow_up_question"], "Why?")
        expectTrue(prompt.system.utf8.count + prompt.user.utf8.count <= LocalPromptBudget.bytes)
        let large = RequestInput(snapshot: .init(text: String(repeating: "x", count: 8000), appName: "Test", bundleID: "test"), action: .simplify)
        do { _ = try LocalPromptBudget.fit(.init(system: "Explain", user: "", structured: false, localInput: large)); fail("Selected source must not truncate") } catch {}
    }
    func testLocalChartsUseSchemaAndKeepValidation() async throws {
        let transport = OllamaFixture()
        let provider = OllamaProvider(configuration: .init(address: "https://ross.example"), transport: transport)
        do {
            _ = try await RequestPipeline(provider: provider).run(.init(snapshot: .init(text: "Revenue was $10 million and $15 million.", appName: "Test", bundleID: "test"), action: .chart, chartChoice: .bar))
            fail("Invalid bars must fail")
        } catch {}
        let requests = await transport.requests
        let body = try JSONSerialization.jsonObject(with: requests.last!.httpBody!) as! [String: Any]
        expectTrue(body["format"] is [String: Any]); expectEqual(body["stream"] as? Bool, false)
        expectEqual(body["model"] as? String, "llama3.2:1b")
    }
}
