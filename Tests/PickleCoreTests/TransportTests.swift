import Foundation
import PickleCore
actor MockTransport: HTTPTransport {
    var requests: [URLRequest] = []
    let response: Data
    init(_ json: String) { response = Data(json.utf8) }
    func send(_ request: URLRequest) async throws -> Data { requests.append(request); return response }
}
final class TransportTests: CheckSuite {
    func testJevUsesCloudflareNativeEvaluationContract() async throws {
        let t = MockTransport(#"{"result":{"model":"jev-1.13.0","answers":{"flag":{"type":"noul","noul":0.9}},"usage":{"input_tokens":10,"output_tokens":1}},"success":true}"#)
        let j = JevClient(accountID: String(repeating: "a", count: 32), token: "test-token", transport: t)
        _ = try await j.evaluate(state: ["source": "Sample"], questions: ["flag": DecisionQuestion("Question?")])
        let request = await t.requests.first!
        expectEqual(request.url?.host, "api.cloudflare.com"); expectTrue(request.url!.path.hasSuffix("/ai/run")); expectEqual(request.httpMethod, "POST")
        expectEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer test-token")
        let body = try JSONSerialization.jsonObject(with: request.httpBody!) as! [String: Any]
        expectEqual(body["model"] as? String, "typesafe/jev"); expectNil(body["messages"])
        let input = body["input"] as! [String: Any]; expectTrue(input["state"] != nil); expectTrue(input["questions"] != nil)
    }
    func testLlamaNativeEnvelopeAndReportedModelUsage() async throws {
        let t = MockTransport(#"{"success":true,"result":{"response":"Sample response","model":"resolved-model","usage":{"prompt_tokens":67,"completion_tokens":25}}}"#)
        let w = CloudflareProvider(accountID: String(repeating: "a", count: 32), token: "test-token", transport: t)
        let input = RequestInput(snapshot: .init(text: "Sample", appName: "Test", bundleID: "test"), action: .simplify)
        let outcome = try await RequestPipeline(provider: w).run(input)
        guard case .result(let result) = outcome else { return fail() }
        expectEqual(result.model, "resolved-model"); expectEqual(result.text, "Sample response")
        let request = await t.requests.first!; expectTrue(request.url!.path.hasSuffix(CloudflareProvider.defaultModel))
        let body = try JSONSerialization.jsonObject(with: request.httpBody!) as! [String: Any]
        expectEqual(body["stream"] as? Bool, false); expectEqual(body["max_tokens"] as? Int, 1800)
    }
    func testStructuredCloudflareResponseObjectAndSchema() async throws {
        let source = "$10 and $15"
        let e = ResultValidator.numericEvidence(in: source)
        let payload = "{\"success\":true,\"result\":{\"response\":{\"type\":\"bar_chart\",\"bars\":[{\"evidenceID\":\"\(e[0].id)\"},{\"evidenceID\":\"\(e[1].id)\"}]}}}"
        let t = MockTransport(payload)
        let writer = CloudflareProvider(accountID: String(repeating: "a", count: 32), token: "test-token", model: "@cf/meta/llama-3.2-1b-instruct", transport: t)
        let outcome = try await RequestPipeline(provider: writer).run(.init(snapshot: .init(text: source, appName: "Test", bundleID: "test"), action: .chart, chartChoice: .bar))
        guard case .result(let result) = outcome, case .bar(let bars) = result.chart else { return fail("Expected parsed chart") }
        expectEqual(bars.map(\.value), [10, 15])
        let request = await t.requests.first!
        expectTrue(request.url!.path.hasSuffix(CloudflareProvider.diagramModel))
        expectEqual(result.model, CloudflareProvider.diagramModel)
        let body = try JSONSerialization.jsonObject(with: request.httpBody!) as! [String: Any]
        let format = body["response_format"] as! [String: Any]
        expectEqual(format["type"] as? String, "json_schema"); expectTrue(format["json_schema"] != nil)
    }
    func testProviderInitializationDoesNotSendAnything() async throws {
        let t = MockTransport("{}")
        _ = CloudflareProvider(accountID: String(repeating: "a", count: 32), token: "test", transport: t)
        _ = JevClient(accountID: String(repeating: "a", count: 32), token: "test", transport: t)
        let requests = await t.requests; expectEqual(requests.count, 0)
    }
    func testSmallWritingModelIsPreservedForProse() async throws {
        let t = MockTransport(#"{"success":true,"result":{"response":"A short explanation."}}"#)
        let model = "@cf/meta/llama-3.2-1b-instruct"
        let writer = CloudflareProvider(accountID: String(repeating: "a", count: 32), token: "test-token", model: model, transport: t)
        let outcome = try await RequestPipeline(provider: writer).run(.init(snapshot: .init(text: "Rainwater is collected in a tank.", appName: "Test", bundleID: "test"), action: .simplify))
        guard case .result(let result) = outcome else { return fail("Expected prose") }
        let request = await t.requests.first!
        expectTrue(request.url!.path.hasSuffix(model))
        expectEqual(result.model, model)
        let body = try JSONSerialization.jsonObject(with: request.httpBody!) as! [String: Any]
        expectNil(body["response_format"])
    }
    func testInvalidAccountCannotAlterEndpointOrSend() async throws {
        let t = MockTransport("{}"), j = JevClient(accountID: "../elsewhere", token: "test", transport: MockTransport("{}"))
        do { _ = try await j.evaluate(state: [:], questions: [:]); fail("Must reject invalid account") } catch {}
        let w = CloudflareProvider(accountID: "../elsewhere", token: "test", transport: t)
        do { _ = try await RequestPipeline(provider: w).run(RequestInput(snapshot: .init(text: "Sample", appName: "Test", bundleID: "test"), action: .simplify)); fail() } catch {}
        let requests = await t.requests; expectEqual(requests.count, 0)
    }
    @MainActor func testLateResponseCannotReplaceNewResult() async throws {
        let runner = RequestRunner()
        let input = RequestInput(snapshot: .init(text: "Sample", appName: "Test", bundleID: "test"), action: .simplify)
        var delivered: [String] = []
        runner.start(pipeline: .init(provider: UncooperativeWriter()), input: input, progress: { _ in }) { result in if case .success(.result(let r)) = result { delivered.append(r.text) } }
        try await Task.sleep(nanoseconds: 5_000_000)
        runner.start(pipeline: .init(provider: MockWriter(["New response"])), input: input, progress: { _ in }) { result in if case .success(.result(let r)) = result { delivered.append(r.text) } }
        try await Task.sleep(nanoseconds: 50_000_000)
        expectEqual(delivered, ["New response"])
    }
    @MainActor func testCancelledRunnerDeliversNeitherResultNorError() async throws {
        let runner = RequestRunner()
        var deliveries = 0
        runner.start(pipeline: .init(provider: UncooperativeWriter()), input: RequestInput(snapshot: .init(text: "Sample", appName: "Test", bundleID: "test"), action: .simplify), progress: { _ in }) { _ in deliveries += 1 }
        try await Task.sleep(nanoseconds: 5_000_000); runner.cancel()
        try await Task.sleep(nanoseconds: 50_000_000); expectEqual(deliveries, 0)
    }
}
struct UncooperativeWriter: GenerativeProvider {
    let isRemote = false
    func generate(_ prompt: GenerationPrompt) async throws -> Generation {
        try? await Task.sleep(nanoseconds: 500_000_000) // Simulate a provider ignoring cancellation.
        return Generation(text: "Old response", model: "mock")
    }
}
