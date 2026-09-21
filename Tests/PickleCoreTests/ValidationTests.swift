import Foundation
import PickleCore
final class ValidationTests: CheckSuite {
    func testExplicitQuantitiesAndScale() throws {
        let evidence = ResultValidator.numericEvidence(in: "Revenue was $10 million in 2024 and $15 million in 2025.")
        expectEqual(evidence.count, 2); expectEqual(evidence.map(\.value), [10_000_000, 15_000_000]); expectEqual(Set(evidence.map(\.unit)), ["USD"])
    }
    func testMalformedNumbersNeverBecomePartialEvidence() {
        expectTrue(ResultValidator.numericEvidence(in: "$10,5 and $1e6").isEmpty)
        expectEqual(ResultValidator.numericEvidence(in: "The price is $10.").map(\.value), [10])
    }
    func testDatesAndBareNumbersNotEvidence() { expectTrue(ResultValidator.numericEvidence(in: "2024 had 12 and 2025 had 16.").isEmpty) }
    func testPercentagesAndSignedValues() { expectEqual(ResultValidator.numericEvidence(in: "The change was -12% and then 5 percent.").map(\.value), [-12, 5]) }
    func testBarUsesOnlyLocallyExtractedValues() throws {
        let source = "Revenue was $10 million in 2024 and $15 million in 2025."
        let values = ResultValidator.numericEvidence(in: source)
        let json = "{\"type\":\"bar_chart\",\"bars\":[{\"evidenceID\":\"\(values[0].id)\"},{\"evidenceID\":\"\(values[1].id)\"}]}"
        guard case .bar(let result) = try ResultValidator.validate(json, kind: .bar, source: source) else { return fail() }
        expectEqual(result, values)
    }
    func testInventedOrDuplicateQuantitiesRejected() {
        for ids in [["n0", "fake"], ["n0", "n0"]] {
            let json = "{\"type\":\"bar_chart\",\"bars\":[{\"evidenceID\":\"\(ids[0])\"},{\"evidenceID\":\"\(ids[1])\"}]}"
            expectThrows(try ResultValidator.validate(json, kind: .bar, source: "$10 and $15"))
        }
    }
    func testMixedUnitsRejected() {
        let source = "$10 and 15%", values = ResultValidator.numericEvidence(in: "$10 and 15%")
        let json = "{\"type\":\"bar_chart\",\"bars\":[{\"evidenceID\":\"\(values[0].id)\"},{\"evidenceID\":\"\(values[1].id)\"}]}"
        expectThrows(try ResultValidator.validate(json, kind: .bar, source: source))
    }
    func testUnknownFieldsRejected() {
        expectThrows(try ResultValidator.validate(#"{"type":"bar_chart","bars":[],"html":"<script>alert(1)</script>"}"#, kind: .bar, source: "$10 and $20"))
    }
    func testFlowValidatesSourceNodesAndEdges() throws {
        let json = #"{"type":"flow_diagram","nodes":[{"id":"a","label":"received"},{"id":"b","label":"validated"}],"edges":[{"from":"a","to":"b","evidence":"The request is received then validated.","condition":""}]}"#
        _ = try ResultValidator.validate(json, kind: .flow, source: "The request is received then validated.")
        expectThrows(try ResultValidator.validate(json.replacingOccurrences(of: #""to":"b""#, with: #""to":"c""#), kind: .flow, source: "The request is received then validated."))
        expectThrows(try ResultValidator.validate(json, kind: .flow, source: "An unrelated passage."))
    }
    func testExplicitProcessCannotDropSteps() {
        expectEqual(ResultValidator.explicitOrderedSteps("The request is received, validated, and saved."), ["received", "validated", "saved"])
        let incomplete = #"{"type":"flow_diagram","nodes":[{"id":"a","label":"request"},{"id":"b","label":"received"}],"edges":[{"from":"a","to":"b","evidence":"The request is received, validated, and saved.","condition":""}]}"#
        expectThrows(try ResultValidator.validate(incomplete, kind: .flow, source: "The request is received, validated, and saved."))
    }
    func testOmittedFlowConditionRejected() {
        let json = #"{"type":"flow_diagram","nodes":[{"id":"a","label":"validated"},{"id":"b","label":"saved"}],"edges":[{"from":"a","to":"b","evidence":"If validated, it is saved.","condition":""}]}"#
        expectThrows(try ResultValidator.validate(json, kind: .flow, source: "If validated, it is saved."))
    }
    func testInvalidJevProbabilitiesRejected() {
        for p in [-0.1, 1.1, Double.nan, Double.infinity] { expectThrows(try DecisionResponse(model: "test", answers: ["q": .init(noul: p)]).validate(for: ["q": .init("Question?")])) }
        expectThrows(try DecisionResponse(model: "test", answers: [:]).validate(for: ["q": .init("Question?")]))
    }
    func testChoiceDistributionMustBeCompleteAndConsistent() {
        let q = ["difficulty": DecisionQuestion(choice: "Difficulty?", criteria: ["easy": "Easy", "hard": "Hard"])]
        let response = DecisionResponse(model: "test", answers: ["difficulty": .init(choice: "easy", confidence: 0.9, probabilities: ["easy": 0.1, "hard": 0.9])])
        expectThrows(try response.validate(for: q))
    }
    func testDecisionThresholdBoundaries() {
        let policy = DecisionPolicy()
        expectEqual(policy.quality(.init(model: "x", answers: ["x": .init(noul: 0.2)]), keys: ["x"]), .checked)
        expectEqual(policy.quality(.init(model: "x", answers: ["x": .init(noul: 0.8)]), keys: ["x"]), .concerns(["x"]))
        expectEqual(policy.quality(.init(model: "x", answers: [:]), keys: ["x"]), .uncertain)
    }
    func testJevCloudflareEnvelopeAndDirectModelPayload() throws {
        let payload = #"{"model":"jev-1.13.0","answers":{"q":{"type":"noul","noul":0.9}},"usage":{"input_tokens":10,"output_tokens":1}}"#
        let a = try JevClient.decode(Data(payload.utf8))
        let b = try JevClient.decode(Data("{\"success\":true,\"result\":\(payload)}".utf8))
        expectEqual(a.answers["q"]?.noul, b.answers["q"]?.noul)
        expectThrows(try JevClient.decode(Data(#"{"success":false,"result":null}"#.utf8)))
    }
    func testCloudflareUsageFieldAliases() throws {
        let usage = try JSONDecoder().decode(Usage.self, from: Data(#"{"prompt_tokens":67,"completion_tokens":25}"#.utf8))
        expectEqual(usage.input_tokens, 67); expectEqual(usage.output_tokens, 25)
    }
    func testJevCompletedGatewayJob() throws {
        let data = Data(#"{"success":true,"result":{"state":"Completed","result":{"model":"jev-1.13.0","answers":{"sample":{"type":"noul","noul":1}},"usage":{"input_tokens":283,"output_tokens":20}},"gatewayMetadata":{"keySource":"Unified"}}}"#.utf8)
        let result = try JevClient.decode(data)
        try result.validate(for: ["sample": DecisionQuestion("Does source mention the sky?")])
        expectEqual(result.model, "jev-1.13.0")
        expectEqual(result.answers["sample"]?.noul, 1)
        expectEqual(result.usage?.input_tokens, 283)
    }
    func testJevIncompleteGatewayJobIsNotAnAnswer() {
        for body in [
            #"{"success":true,"result":{"state":"Running","result":null}}"#,
            #"{"success":true,"result":{"state":"Completed","result":null}}"#,
            #"{"success":false,"result":{"state":"Completed","result":{"model":"jev","answers":{}}}}"#
        ] { expectThrows(try JevClient.decode(Data(body.utf8))) }
    }
    func testConversationLimitsAndUTF8Budgets() {
        let snapshot = SelectionSnapshot(text: "Test", appName: "Test", bundleID: "test")
        expectThrows(try RequestInput(snapshot: snapshot, action: .followUp, conversation: Array(repeating: ConversationTurn(question: "a", answer: "b", quality: .checked), count: 7)).validate())
        expectThrows(try RequestInput(snapshot: snapshot, action: .expand, context: String(repeating: "🫛", count: 2000)).validate())
    }
}
