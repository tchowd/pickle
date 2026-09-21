import Foundation
/// Owns task cancellation and the identity gate for late provider responses and progress events.
@MainActor public final class RequestRunner {
    private var task: Task<Void, Never>?
    private var generation = UUID()
    public init() {}
    public func cancel() { generation = UUID(); task?.cancel(); task = nil }
    public func start(pipeline: RequestPipeline, input: RequestInput, progress: @escaping @MainActor (String) -> Void, completion: @escaping @MainActor (Result<PipelineOutcome, Error>) -> Void) {
        cancel()
        let id = UUID(); generation = id
        task = Task { [weak self] in
            do {
                let outcome = try await pipeline.run(input) { [weak self] message in
                    await self?.deliverProgress(message, id: id, callback: progress)
                }
                try Task.checkCancellation()
                guard let self, self.generation == id else { return }
                self.task = nil; completion(.success(outcome))
            } catch {
                guard let self, self.generation == id, !Task.isCancelled else { return }
                self.task = nil; completion(.failure(error))
            }
        }
    }
    private func deliverProgress(_ message: String, id: UUID, callback: @MainActor (String) -> Void) {
        guard generation == id else { return }; callback(message)
    }
}
public actor ContentFreeMetrics {
    public private(set) var generationRequests = 0, evaluationRequests = 0, failures = 0, repairs = 0, cancellations = 0
    public private(set) var barRoutes = 0, flowRoutes = 0, undecidedRoutes = 0
    public private(set) var inputTokens = 0, outputTokens = 0
    public private(set) var latency: [Double] = []
    public init() {}
    public func record(_ event: MetricEvent) {
        switch event {
        case .generation(let usage): generationRequests += 1; add(usage)
        case .evaluation(let usage): evaluationRequests += 1; add(usage)
        case .repair: repairs += 1
        case .cancelled: cancellations += 1
        case .failure: failures += 1
        case .route(let kind):
            switch kind { case .bar: barRoutes += 1; case .flow: flowRoutes += 1; case nil: undecidedRoutes += 1 }
        case .completed(let seconds): latency.append(seconds); if latency.count > 100 { latency.removeFirst() }
        }
    }
    private func add(_ usage: Usage?) { inputTokens += usage?.input_tokens ?? 0; outputTokens += usage?.output_tokens ?? 0 }
    public func summary() -> String {
        "\(generationRequests) completed generation calls · \(evaluationRequests) completed checks · \(repairs) repairs · \(failures) failures · \(cancellations) cancellations\nRoutes: \(barRoutes) bar / \(flowRoutes) flow / \(undecidedRoutes) undecided. Reported usage: \(inputTokens) input / \(outputTokens) output tokens. Average time to result: \((latency.isEmpty ? 0 : latency.reduce(0, +) / Double(latency.count)).formatted(.number.precision(.fractionLength(2))))s. Costs are available in Cloudflare billing."
    }
}
