import Foundation

public typealias DraftSink = @Sendable (String) async -> Void
public protocol StreamingGenerativeProvider: GenerativeProvider {
    func generate(_ prompt: GenerationPrompt, draft: @escaping DraftSink) async throws -> Generation
}
public protocol StreamingHTTPTransport: HTTPTransport {
    func stream(_ request: URLRequest, line: @escaping @Sendable (String) async throws -> Void) async throws
}

/// A bounded SSE accumulator. A missing terminator is an interrupted answer, not success.
public actor ProseStream {
    private var text = "", done = false
    private var usage: Usage?
    private var reportedModel: String?
    public init() {}
    public func receive(_ line: String, draft: DraftSink) async throws {
        guard !done, line.hasPrefix("data:") else { return }
        let payload = line.dropFirst(5).trimmingCharacters(in: .whitespaces)
        if payload == "[DONE]" { done = true; return }
        struct Chunk: Decodable {
            struct Choice: Decodable { struct Delta: Decodable { let content: String? }; let delta: Delta? }
            let response: String?; let choices: [Choice]?; let usage: Usage?; let model: String?
            let error: String?
        }
        guard let data = payload.data(using: .utf8), let chunk = try? JSONDecoder().decode(Chunk.self, from: data), chunk.error == nil else {
            throw PickleError.message("The answer was interrupted. Please try again.")
        }
        let delta = chunk.response ?? chunk.choices?.first?.delta?.content ?? ""
        guard text.utf8.count + delta.utf8.count <= Limits.output else { throw PickleError.message("This answer is too long. Try a shorter passage.") }
        text += delta; usage = chunk.usage ?? usage; reportedModel = chunk.model ?? reportedModel
        if !delta.isEmpty { await draft(text) }
    }
    public func finish(model: String) throws -> Generation {
        guard done else { throw PickleError.message("The connection ended before the answer finished. Please try again.") }
        try ResultValidator.prose(text)
        return Generation(text: text, model: reportedModel ?? model, usage: usage)
    }
}

extension BoundedHTTPTransport: StreamingHTTPTransport {
    public func stream(_ request: URLRequest, line: @escaping @Sendable (String) async throws -> Void) async throws {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = Limits.requestSeconds
        config.timeoutIntervalForResource = Limits.requestSeconds
        config.urlCache = nil; config.httpCookieStorage = nil; config.httpShouldSetCookies = false
        let session = URLSession(configuration: config, delegate: self, delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        let (bytes, response) = try await session.bytes(for: request)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            throw PickleError.message("Couldn’t start the answer. Check your connection and account settings, then retry.")
        }
        guard http.value(forHTTPHeaderField: "Content-Type")?.contains("text/event-stream") == true else {
            throw PickleError.message("This model does not support streaming. Turn off streaming in Reading settings and retry.")
        }
        var buffer = Data(), count = 0
        for try await byte in bytes {
            try Task.checkCancellation()
            count += 1
            guard count <= Limits.responseBytes else { throw PickleError.message("Provider response exceeded the size limit.") }
            if byte == 10 {
                guard let decoded = String(data: buffer, encoding: .utf8) else { throw PickleError.message("The answer could not be read. Please retry.") }
                try await line(decoded.trimmingCharacters(in: .newlines)); buffer.removeAll(keepingCapacity: true)
            } else { buffer.append(byte) }
        }
        if !buffer.isEmpty {
            guard let decoded = String(data: buffer, encoding: .utf8) else { throw PickleError.message("The answer could not be read. Please retry.") }
            try await line(decoded.trimmingCharacters(in: .newlines))
        }
    }
}
