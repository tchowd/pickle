import Foundation
import PickleCore

actor MockWriter: GenerativeProvider {
    nonisolated let isRemote: Bool
    var prompts: [GenerationPrompt] = []
    let responses: [String]
    var delay: UInt64
    init(_ responses: [String] = ["A faithful explanation."], remote: Bool = false, delay: UInt64 = 0) { self.responses = responses; self.isRemote = remote; self.delay = delay }
    func generate(_ prompt: GenerationPrompt) async throws -> Generation {
        prompts.append(prompt)
        if delay > 0 { try await Task.sleep(nanoseconds: delay) }
        return Generation(text: responses[min(prompts.count - 1, responses.count - 1)], model: "mock")
    }
    var count: Int { prompts.count }
}
actor MockJev: DecisionClient {
    var batches: [[String: DecisionQuestion]] = []
    var states: [[String: String]] = []
    let fail: Bool
    let values: [String: Double]
    let repairSucceeds: Bool
    init(values: [String: Double] = [:], fail: Bool = false, repairSucceeds: Bool = false) { self.values = values; self.fail = fail; self.repairSucceeds = repairSucceeds }
    func evaluate(state: [String: String], questions: [String: DecisionQuestion]) async throws -> DecisionResponse {
        batches.append(questions); states.append(state)
        if fail { throw PickleError.message("Unavailable") }
        let answers = questions.mapValues { question -> DecisionAnswer in
            if question.type == "choice" { return DecisionAnswer(choice: "specialist", confidence: 0.95, probabilities: ["everyday": 0, "professional": 0, "specialist": 1, "dense": 0]) }
            return DecisionAnswer(noul: 0)
        }
        var result = answers
        for key in questions.keys where values[key] != nil { result[key] = DecisionAnswer(noul: repairSucceeds && batches.count >= 3 ? 0.01 : values[key]!) }
        return DecisionResponse(model: "mock-jev", answers: result)
    }
    var count: Int { batches.count }
}
final class PipelineTests: CheckSuite {
    let source = SelectionSnapshot(text: "The treatment may help some patients, but evidence is limited.", appName: "Test", bundleID: "test")
    func input(_ action: ReadingAction = .simplify, limited: Bool = false) -> RequestInput { RequestInput(snapshot: source, action: action, limited: limited) }
    func result(_ outcome: PipelineOutcome) throws -> ReadingResult {
        guard case .result(let r) = outcome else { throw PickleError.message("Expected result") }; return r
    }
    func testSimplifyUsesOnlyContextDifficultyAndFidelity() async throws {
        let w = MockWriter(), j = MockJev()
        let r = try result(await RequestPipeline(provider: w, evaluator: j).run(input()))
        expectEqual(r.quality, .checked)
        let batches = await j.batches
        expectEqual(batches.count, 2)
        expectEqual(Set(batches[0].keys), Set(DecisionEvaluators.context.keys).union(DecisionEvaluators.difficulty.keys))
        expectEqual(Set(batches[1].keys), Set(DecisionEvaluators.fidelity.keys))
    }
    func testExpandUsesSupportNotFidelity() async throws {
        let j = MockJev(); _ = try await RequestPipeline(provider: MockWriter(), evaluator: j).run(input(.expand))
        let batches = await j.batches; expectEqual(Set(batches.last!.keys), Set(DecisionEvaluators.support.keys))
    }
    func testMissingContextStopsBeforeWriter() async throws {
        let w = MockWriter(), j = MockJev(values: ["missing_referent": 0.95])
        let outcome = try await RequestPipeline(provider: w, evaluator: j).run(input())
        guard case .needsContext(let flags) = outcome else { return fail("Should request context") }
        expectEqual(flags, ["missing_referent"]); let count = await w.count; expectEqual(count, 0)
    }
    func testLimitedContextIsExplicitAndSameSource() async throws {
        let j = MockJev(values: ["missing_referent": 0.95]), w = MockWriter()
        let r = try result(await RequestPipeline(provider: w, evaluator: j).run(input(limited: true)))
        expectTrue(r.notes.contains { $0.contains("Limited explanation") })
        let prompts = await w.prompts; expectTrue(prompts[0].system.contains("Context may be missing")); expectTrue(prompts[0].user.contains(source.text))
    }
    func testOneRepairAndRecheckAllDimensions() async throws {
        let j = MockJev(values: ["lost_uncertainty": 0.99], repairSucceeds: true), w = MockWriter(["Bad", "Repaired"])
        let r = try result(await RequestPipeline(provider: w, evaluator: j).run(input()))
        expectEqual(r.repairs, 1); expectEqual(r.text, "Repaired"); expectEqual(r.quality, .checked)
        let wc = await w.count, jc = await j.count; expectEqual(wc, 2); expectEqual(jc, 3)
        let prompts = await w.prompts; expectTrue(prompts[1].system.contains("Restore all uncertainty"))
    }
    func testRepeatedConcernNeverLoopsOrGetsSuccessLabel() async throws {
        let j = MockJev(values: ["lost_uncertainty": 0.99]), w = MockWriter()
        let r = try result(await RequestPipeline(provider: w, evaluator: j).run(input()))
        expectEqual(r.repairs, 1); expectEqual(r.quality, .concerns(["lost_uncertainty"]))
        let wc = await w.count; expectEqual(wc, 2)
    }
    func testUncertainDoesNotAutomaticallyRepair() async throws {
        let w = MockWriter(), j = MockJev(values: ["lost_uncertainty": 0.55])
        let r = try result(await RequestPipeline(provider: w, evaluator: j).run(input()))
        expectEqual(r.quality, .uncertain); expectEqual(r.repairs, 0)
    }
    func testJevOutageStillGeneratesWithHonestLabel() async throws {
        let j = MockJev(fail: true)
        let r = try result(await RequestPipeline(provider: MockWriter(), evaluator: j).run(input()))
        expectEqual(r.quality, .notChecked("Jev unavailable")); let count = await j.count; expectEqual(count, 1)
    }
    func testDisabledJev() async throws {
        let r = try result(await RequestPipeline(provider: MockWriter()).run(input()))
        expectEqual(r.quality, .notChecked("Jev disabled"))
    }
    func testLocalOnlyMakesNoRemoteCalls() async throws {
        let w = MockWriter(remote: true), j = MockJev()
        do { _ = try await RequestPipeline(provider: w, evaluator: j, localOnly: true).run(input()); fail("Must block remote writer") } catch {}
        let wc = await w.count, jc = await j.count; expectEqual(wc, 0); expectEqual(jc, 0)
        _ = try await RequestPipeline(provider: MockWriter(), evaluator: j, localOnly: true).run(input())
        let still = await j.count; expectEqual(still, 0)
    }
    func testOversizeRejectedBeforeProviders() async throws {
        let w = MockWriter(), j = MockJev()
        let large = RequestInput(snapshot: .init(text: String(repeating: "a", count: Limits.selection + 1), appName: "Test", bundleID: "test"), action: .simplify)
        do { _ = try await RequestPipeline(provider: w, evaluator: j).run(large); fail("Must reject") } catch {}
        let wc = await w.count, jc = await j.count; expectEqual(wc, 0); expectEqual(jc, 0)
    }
    func testCancellationPropagates() async throws {
        let w = MockWriter(delay: 2_000_000_000), j = MockJev()
        let task = Task { try await RequestPipeline(provider: w, evaluator: j).run(input()) }
        try await Task.sleep(nanoseconds: 10_000_000); task.cancel()
        do { _ = try await task.value; fail("Cancelled result must not finish") } catch is CancellationError {} catch { fail("Unexpected error") }
    }
    func testChartUnavailableAsksWithoutWriterCall() async throws {
        let w = MockWriter()
        let outcome = try await RequestPipeline(provider: w).run(input(.chart))
        guard case .chooseChart = outcome else { return fail("Must ask for format") }
        let count = await w.count; expectEqual(count, 0)
    }
    func testChartUnjustifiedDoesNotGenerate() async throws {
        let w = MockWriter(), j = MockJev()
        let r = try result(await RequestPipeline(provider: w, evaluator: j).run(input(.chart)))
        expectTrue(r.text.contains("does not clearly support")); let wc = await w.count; expectEqual(wc, 0)
        let batches = await j.batches; expectEqual(batches.count, 1); expectEqual(Set(batches[0].keys), Set(DecisionEvaluators.chart.keys))
    }
    func testFollowUpStaysWithSourceAndUsesSupport() async throws {
        let j = MockJev(), w = MockWriter()
        let request = RequestInput(snapshot: source, action: .followUp, question: "Why?", previousResult: "Prior explanation", conversation: [.init(question: "What?", answer: "Earlier response", quality: .uncertain)])
        _ = try await RequestPipeline(provider: w, evaluator: j).run(request)
        let states = await j.states; expectEqual(states[0]["source"], source.text); expectNil(states[1]["prior_explanation"])
        let batches = await j.batches; expectEqual(Set(batches[0].keys), Set(DecisionEvaluators.context.keys)); expectEqual(Set(batches[1].keys), Set(DecisionEvaluators.support.keys))
    }
    func testReadingPreferenceOverridesAutomaticDifficulty() async throws {
        let w = MockWriter(), j = MockJev()
        _ = try await RequestPipeline(provider: w, evaluator: j).run(RequestInput(snapshot: source, action: .simplify, level: .plain))
        let prompts = await w.prompts; expectTrue(prompts[0].system.contains("overrides automatic difficulty: Plain language"))
    }
}
