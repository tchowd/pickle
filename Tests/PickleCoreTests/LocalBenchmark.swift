import Foundation
import PickleCore

enum LocalBenchmark {
    static func cancellation(address: String) async -> Int32 {
        let token = String(data: FileHandle.standardInput.readDataToEndOfFile(), encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let provider = OllamaProvider(configuration: .init(address: address, token: token))
        let drafts = DraftRecorder()
        let task = Task {
            do {
                _ = try await provider.generate(.init(system: "Follow the request.", user: "Count from 1 to 1000, writing every number on its own line. Do not stop early.", structured: false), draft: { await drafts.add($0) })
                return false
            } catch { return Task.isCancelled }
        }
        for _ in 0..<250 {
            if await !drafts.values.isEmpty { break }
            try? await Task.sleep(for: .milliseconds(20))
        }
        let hadDraft = await !drafts.values.isEmpty
        let start = Date(); task.cancel()
        let cancelled = await task.value
        let cancellationSeconds = Date().timeIntervalSince(start)
        try? await Task.sleep(for: .milliseconds(200))
        let probeStart = Date()
        do {
            let probe = try await provider.generate(.init(system: "Be brief.", user: "Say ready.", structured: false))
            print("LOCAL_CANCELLATION: active_stream=\(hadDraft), cancelled=\(cancelled), client_stop_seconds=\(cancellationSeconds), next_request_seconds=\(Date().timeIntervalSince(probeStart)), next_answer=\(probe.text)")
            return hadDraft && cancelled ? 0 : 1
        } catch { print("LOCAL_CANCELLATION_PROBE_FAILED: \(error.localizedDescription)"); return 1 }
    }
    static func run(address: String) async -> Int32 {
        let token = String(data: FileHandle.standardInput.readDataToEndOfFile(), encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let provider = OllamaProvider(configuration: .init(address: address, token: token))
        let short = "The treatment may reduce symptoms in some patients, but the evidence remains limited."
        let source = "The community library will extend its weekday opening hours from 6 PM to 8 PM for a three-month trial. Weekend hours will not change. The trial is funded by a temporary grant, and permanent hours will depend on attendance and future funding."
        let snapshot = SelectionSnapshot(text: source, appName: "Fixed benchmark", bundleID: "fixture")
        let inputs: [(String, RequestInput)] = [
            ("cold_simplify", .init(snapshot: .init(text: short, appName: "Fixed benchmark", bundleID: "fixture"), action: .simplify)),
            ("warm_context", .init(snapshot: snapshot, action: .simplify, context: "The proposal follows a survey of residents who work during the day. Staff schedules will be adjusted; no additional permanent positions have been promised.", pageContext: "The council will review the results at the end of the trial. The grant is temporary.")),
            ("follow_up", .init(snapshot: snapshot, action: .followUp, question: "Are the longer hours permanent?", previousResult: "The library is testing longer weekday hours for three months.")),
            ("bar_chart", .init(snapshot: .init(text: "Revenue was $10 million in 2024 and $15 million in 2025.", appName: "Fixed benchmark", bundleID: "fixture"), action: .chart, chartChoice: .bar)),
            ("flow_chart", .init(snapshot: .init(text: "The request is received, validated, and saved.", appName: "Fixed benchmark", bundleID: "fixture"), action: .chart, chartChoice: .flow))]
        var failures = 0
        for (name, input) in inputs {
            let start = Date()
            do {
                let outcome = try await RequestPipeline(provider: provider).run(input, draft: { _ in })
                guard case .result(let result) = outcome else { throw PickleError.message("No result") }
                var record: [String: Any] = ["fixture":name,"success":true,"seconds":Date().timeIntervalSince(start),"answer":result.text,"chart_validated":result.chart != nil]
                if let timing = result.timing {
                    record["load_seconds"] = timing.loadSeconds; record["ttft_seconds"] = timing.firstTokenSeconds
                    record["generation_seconds"] = timing.generationSeconds; record["output_tokens"] = timing.outputTokens
                    record["tokens_per_second"] = Double(timing.outputTokens)/max(timing.generationSeconds,0.000001)
                }
                print(String(decoding: try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys]), as: UTF8.self))
            } catch {
                failures += 1
                let record: [String: Any] = ["fixture":name,"success":false,"seconds":Date().timeIntervalSince(start),"error":error.localizedDescription]
                print(String(decoding: (try? JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])) ?? Data(), as: UTF8.self))
            }
        }
        return failures == 0 ? 0 : 1
    }
}
