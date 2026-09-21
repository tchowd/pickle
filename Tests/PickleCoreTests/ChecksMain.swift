import Foundation
import PickleCore
@main struct ChecksRunner {
    static func main() async {
        if let index = CommandLine.arguments.firstIndex(of: "--live-jev-pipeline"), CommandLine.arguments.count > index + 1 {
            let account = CommandLine.arguments[index + 1]
            let token = String(decoding: FileHandle.standardInput.readDataToEndOfFile(), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            do {
                let writer = CloudflareProvider(accountID: account, token: token, model: "@cf/meta/llama-3.2-1b-instruct")
                let checker = JevClient(accountID: account, token: token)
                let outcome = try await RequestPipeline(provider: writer, evaluator: checker).run(.init(snapshot: .init(text: "Rainwater is collected from rooftops and stored in a tank for watering gardens.", appName: "Public test fixture", bundleID: "fixture"), action: .simplify))
                guard case .result(let result) = outcome, result.evaluatorModel != nil else { throw PickleError.message("No evaluated result returned") }
                if case .notChecked = result.quality { throw PickleError.message("Jev checks did not complete") }
                print("LIVE_JEV_PIPELINE_PASS · \(result.evaluatorModel!) · \(result.quality.label) · \(result.elapsed.formatted())s\n\(result.text)")
            } catch { print("LIVE_JEV_PIPELINE_FAIL: \(error.localizedDescription)"); exit(1) }
            return
        }
        if let index = CommandLine.arguments.firstIndex(of: "--live-cloudflare-charts"), CommandLine.arguments.count > index + 1 {
            // Explicit opt-in: two billable calls, fixed public samples, credentials only on stdin.
            let account = CommandLine.arguments[index + 1]
            let token = String(decoding: FileHandle.standardInput.readDataToEndOfFile(), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            let modelIndex = CommandLine.arguments.firstIndex(of: "--writing-model")
            let model = modelIndex.flatMap { CommandLine.arguments.indices.contains($0 + 1) ? CommandLine.arguments[$0 + 1] : nil } ?? CloudflareProvider.defaultModel
            let provider = CloudflareProvider(accountID: account, token: token, model: model)
            var failures = 0
            for (source, kind) in [("Revenue was $10 million in 2024 and $15 million in 2025.", ChartKind.bar), ("The request is received, validated, and saved.", ChartKind.flow)] {
                if CommandLine.arguments.contains("--flow-only") && kind != .flow { continue }
                do {
                    let outcome = try await RequestPipeline(provider: provider).run(.init(snapshot: .init(text: source, appName: "CLI public fixture", bundleID: "fixture"), action: .chart, chartChoice: kind))
                    guard case .result(let result) = outcome, let chart = result.chart else { throw PickleError.message("No chart returned") }
                    print("LIVE_CHART_PASS \(kind.rawValue) · \(result.model) · \(result.elapsed.formatted())s\n\(chart.alternative)")
                } catch { failures += 1; print("LIVE_CHART_FAIL \(kind.rawValue): \(error.localizedDescription)") }
            }
            exit(failures == 0 ? 0 : 1)
        }
        if CommandLine.arguments.contains("--export-evaluators") {
            let banks = ["chart": DecisionEvaluators.chart, "context": DecisionEvaluators.context, "difficulty": DecisionEvaluators.difficulty, "fidelity": DecisionEvaluators.fidelity, "support": DecisionEvaluators.support]
            if let data = try? JSONEncoder().encode(banks) { print(String(decoding: data, as: UTF8.self)) }
            return
        }
        await run("testSimplifyUsesOnlyContextDifficultyAndFidelity") { try await PipelineTests().testSimplifyUsesOnlyContextDifficultyAndFidelity() }
        await run("testExpandUsesSupportNotFidelity") { try await PipelineTests().testExpandUsesSupportNotFidelity() }
        await run("testMissingContextStopsBeforeWriter") { try await PipelineTests().testMissingContextStopsBeforeWriter() }
        await run("testLimitedContextIsExplicitAndSameSource") { try await PipelineTests().testLimitedContextIsExplicitAndSameSource() }
        await run("testOneRepairAndRecheckAllDimensions") { try await PipelineTests().testOneRepairAndRecheckAllDimensions() }
        await run("testRepeatedConcernNeverLoopsOrGetsSuccessLabel") { try await PipelineTests().testRepeatedConcernNeverLoopsOrGetsSuccessLabel() }
        await run("testUncertainDoesNotAutomaticallyRepair") { try await PipelineTests().testUncertainDoesNotAutomaticallyRepair() }
        await run("testJevOutageStillGeneratesWithHonestLabel") { try await PipelineTests().testJevOutageStillGeneratesWithHonestLabel() }
        await run("testDisabledJev") { try await PipelineTests().testDisabledJev() }
        await run("testLocalOnlyMakesNoRemoteCalls") { try await PipelineTests().testLocalOnlyMakesNoRemoteCalls() }
        await run("testOversizeRejectedBeforeProviders") { try await PipelineTests().testOversizeRejectedBeforeProviders() }
        await run("testCancellationPropagates") { try await PipelineTests().testCancellationPropagates() }
        await run("testChartUnavailableAsksWithoutWriterCall") { try await PipelineTests().testChartUnavailableAsksWithoutWriterCall() }
        await run("testChartUnjustifiedDoesNotGenerate") { try await PipelineTests().testChartUnjustifiedDoesNotGenerate() }
        await run("testFollowUpStaysWithSourceAndUsesSupport") { try await PipelineTests().testFollowUpStaysWithSourceAndUsesSupport() }
        await run("testReadingPreferenceOverridesAutomaticDifficulty") { try await PipelineTests().testReadingPreferenceOverridesAutomaticDifficulty() }
        await run("testExplicitQuantitiesAndScale") { try ValidationTests().testExplicitQuantitiesAndScale() }
        await run("testDatesAndBareNumbersNotEvidence") { ValidationTests().testDatesAndBareNumbersNotEvidence() }
        await run("testPercentagesAndSignedValues") { ValidationTests().testPercentagesAndSignedValues() }
        await run("testBarUsesOnlyLocallyExtractedValues") { try ValidationTests().testBarUsesOnlyLocallyExtractedValues() }
        await run("testInventedOrDuplicateQuantitiesRejected") { ValidationTests().testInventedOrDuplicateQuantitiesRejected() }
        await run("testMixedUnitsRejected") { ValidationTests().testMixedUnitsRejected() }
        await run("testUnknownFieldsRejected") { ValidationTests().testUnknownFieldsRejected() }
        await run("testFlowValidatesSourceNodesAndEdges") { try ValidationTests().testFlowValidatesSourceNodesAndEdges() }
        await run("testOmittedFlowConditionRejected") { ValidationTests().testOmittedFlowConditionRejected() }
        await run("testInvalidJevProbabilitiesRejected") { ValidationTests().testInvalidJevProbabilitiesRejected() }
        await run("testChoiceDistributionMustBeCompleteAndConsistent") { ValidationTests().testChoiceDistributionMustBeCompleteAndConsistent() }
        await run("testDecisionThresholdBoundaries") { ValidationTests().testDecisionThresholdBoundaries() }
        await run("testJevCloudflareEnvelopeAndDirectModelPayload") { try ValidationTests().testJevCloudflareEnvelopeAndDirectModelPayload() }
        await run("testJevCompletedGatewayJob") { try ValidationTests().testJevCompletedGatewayJob() }
        await run("testJevIncompleteGatewayJobIsNotAnAnswer") { ValidationTests().testJevIncompleteGatewayJobIsNotAnAnswer() }
        await run("testCloudflareUsageFieldAliases") { try ValidationTests().testCloudflareUsageFieldAliases() }
        await run("testConversationLimitsAndUTF8Budgets") { ValidationTests().testConversationLimitsAndUTF8Budgets() }
        await run("testJevUsesCloudflareNativeEvaluationContract") { try await TransportTests().testJevUsesCloudflareNativeEvaluationContract() }
        await run("testLlamaNativeEnvelopeAndReportedModelUsage") { try await TransportTests().testLlamaNativeEnvelopeAndReportedModelUsage() }
        await run("testProviderInitializationDoesNotSendAnything") { try await TransportTests().testProviderInitializationDoesNotSendAnything() }
        await run("testInvalidAccountCannotAlterEndpointOrSend") { try await TransportTests().testInvalidAccountCannotAlterEndpointOrSend() }
        await run("testLateResponseCannotReplaceNewResult") { try await TransportTests().testLateResponseCannotReplaceNewResult() }
        await run("testCancelledRunnerDeliversNeitherResultNorError") { try await TransportTests().testCancelledRunnerDeliversNeitherResultNorError() }
        await run("testStructuredCloudflareResponseObjectAndSchema") { try await TransportTests().testStructuredCloudflareResponseObjectAndSchema() }
        await run("testMalformedNumbersNeverBecomePartialEvidence") { ValidationTests().testMalformedNumbersNeverBecomePartialEvidence() }
        await run("testExplicitProcessCannotDropSteps") { ValidationTests().testExplicitProcessCannotDropSteps() }
        await run("testTwoQuickOptionTaps") { InvocationTests().testTwoQuickTaps() }
        await run("testSmallWritingModelIsPreservedForProse") { try await TransportTests().testSmallWritingModelIsPreservedForProse() }
        await run("testSlowOptionTapsAndHoldsDoNotInvoke") { InvocationTests().testSlowTapsAndHoldsDoNotInvoke() }
        await run("testTypingOrModifiersInterruptOptionTaps") { InvocationTests().testTypingOrModifiersInterruptTapSequence() }
        print("\(Checks.tests) checks; \(Checks.failures) failures")
        exit(Checks.failures == 0 ? 0 : 1)
    }
    static func run(_ name: String, _ check: () async throws -> Void) async {
        let before = Checks.failures
        do { try await check() } catch { fail("Unexpected error in \(name): \(error)") }
        Checks.tests += 1
        print("[\(Checks.failures == before ? "PASS" : "FAIL")] \(name)")
    }
}
