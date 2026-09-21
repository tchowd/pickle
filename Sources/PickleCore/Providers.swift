import Foundation

/// Ephemeral, bounded transport. Never log raw provider errors; they may echo source content.
public protocol HTTPTransport: Sendable { func send(_ request: URLRequest) async throws -> Data }
public final class BoundedHTTPTransport: NSObject, HTTPTransport, URLSessionTaskDelegate, @unchecked Sendable {
    public override init() { super.init() }
    public func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) { completionHandler(nil) }
    public func send(_ request: URLRequest) async throws -> Data {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = Limits.requestSeconds
        config.timeoutIntervalForResource = Limits.requestSeconds
        config.urlCache = nil; config.httpCookieStorage = nil; config.httpShouldSetCookies = false
        let session = URLSession(configuration: config, delegate: self, delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        let (bytes, response) = try await session.bytes(for: request)
        guard let http = response as? HTTPURLResponse else { throw PickleError.message("Invalid provider response.") }
        guard (200...299).contains(http.statusCode) else {
            // Cloudflare also returns 403 for unsupported structured output. Inspect only
            // bounded numeric error codes; never surface provider text that may echo input.
            if http.statusCode == 403 {
                var errorData = Data()
                for try await byte in bytes {
                    try Task.checkCancellation()
                    guard errorData.count < 16_384 else { break }
                    errorData.append(byte)
                }
                struct ErrorEnvelope: Decodable {
                    struct Entry: Decodable { let code: Int? }
                    let errors: [Entry]
                }
                if let envelope = try? JSONDecoder().decode(ErrorEnvelope.self, from: errorData), envelope.errors.contains(where: { $0.code == 5025 }) {
                    throw PickleError.message("This Cloudflare model does not support the structured output needed for diagrams. Use a model with JSON Schema support.")
                }
            }
            switch http.statusCode {
            case 401, 403: throw PickleError.message("Provider access denied. Check the API key and account permissions in Settings.")
            case 402: throw PickleError.message("Cloudflare requires a billing setup for this model. Check your AI billing settings; generation can continue with Jev disabled.")
            case 429, 529: throw PickleError.message("Provider is busy or rate limited. Wait before retrying.")
            default: throw PickleError.message("Provider request failed (HTTP \(http.statusCode)). Try again later.")
            }
        }
        var data = Data()
        for try await byte in bytes {
            try Task.checkCancellation()
            guard data.count < Limits.responseBytes else { throw PickleError.message("Provider response exceeded the size limit.") }
            data.append(byte)
        }
        return data
    }
}
private func post<T: Encodable>(_ url: URL, token: String, body: T) throws -> URLRequest {
    var request = URLRequest(url: url, timeoutInterval: Limits.requestSeconds)
    request.httpMethod = "POST"; request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.httpBody = try JSONEncoder().encode(body)
    return request
}
public struct CloudflareProvider: StreamingGenerativeProvider {
    public static let defaultModel = "@cf/meta/llama-3.3-70b-instruct-fp8-fast"
    public static let diagramModel = defaultModel
    public let isRemote = true
    private let accountID: String, token: String, model: String, transport: any HTTPTransport
    public init(accountID: String, token: String, model: String = Self.defaultModel, transport: any HTTPTransport = BoundedHTTPTransport()) {
        self.accountID = accountID; self.token = token; self.model = model; self.transport = transport
    }
    public func generate(_ prompt: GenerationPrompt, draft: @escaping DraftSink) async throws -> Generation {
        guard !prompt.structured, let streaming = transport as? any StreamingHTTPTransport else { return try await generate(prompt) }
        guard accountID.range(of: "^[a-fA-F0-9]{32}$", options: .regularExpression) != nil, !token.isEmpty,
              model.range(of: "^@cf/[a-zA-Z0-9_-]+/[a-zA-Z0-9_.-]+$", options: .regularExpression) != nil else {
            throw PickleError.message("Connect your account in Settings first.")
        }
        struct Message: Encodable { let role: String, content: String }
        struct Body: Encodable { let messages: [Message]; let max_tokens = 1800; let temperature = 0.2; let stream = true }
        let url = URL(string: "https://api.cloudflare.com/client/v4/accounts/\(accountID)/ai/run/\(model)")!
        let body = Body(messages: [.init(role: "system", content: prompt.system), .init(role: "user", content: prompt.user)])
        var request = try post(url, token: token, body: body)
        request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
        let accumulator = ProseStream()
        try await streaming.stream(request) { line in try await accumulator.receive(line, draft: draft) }
        try Task.checkCancellation()
        return try await accumulator.finish(model: model)
    }
    public func generate(_ prompt: GenerationPrompt) async throws -> Generation {
        guard accountID.range(of: "^[a-fA-F0-9]{32}$", options: .regularExpression) != nil, !token.isEmpty,
              model.range(of: "^@cf/[a-zA-Z0-9_-]+/[a-zA-Z0-9_.-]+$", options: .regularExpression) != nil
        else { throw PickleError.message("Configure a valid Cloudflare account ID, model, and API token in Settings.") }
        struct Message: Encodable { let role: String, content: String }
        struct Format: Encodable { let type: String; let json_schema: JSONValue? }
        struct Body: Encodable { let messages: [Message]; let max_tokens = 1800; let temperature = 0.2; let stream = false; let response_format: Format? }
        struct Envelope: Decodable { let success: Bool; let result: Output? }
        struct Output: Decodable { let response: JSONValue; let usage: Usage?; let model: String? }
        // Diagram schemas need a supported model; the selected writing model can be
        // optimized for prose without breaking charts (e.g. Llama 3.2 1B).
        let requestModel = prompt.structured ? Self.diagramModel : model
        let url = URL(string: "https://api.cloudflare.com/client/v4/accounts/\(accountID)/ai/run/\(requestModel)")!
        let schema = try prompt.schemaJSON.map { try JSONDecoder().decode(JSONValue.self, from: Data($0.utf8)) }
        let format: Format? = prompt.structured ? Format(type: schema == nil ? "json_object" : "json_schema", json_schema: schema) : nil
        let body = Body(messages: [.init(role: "system", content: prompt.system), .init(role: "user", content: prompt.user)], response_format: format)
        let data = try await transport.send(post(url, token: token, body: body))
        let envelope = try JSONDecoder().decode(Envelope.self, from: data)
        guard envelope.success, let result = envelope.result else { throw PickleError.message("Cloudflare could not generate a response.") }
        let responseText: String
        switch result.response {
        case .string(let text): responseText = text
        case .object where prompt.structured: responseText = String(decoding: try JSONEncoder().encode(result.response), as: UTF8.self)
        default: throw PickleError.message("Cloudflare returned an unsupported response format.")
        }
        try ResultValidator.prose(responseText)
        return Generation(text: responseText, model: result.model ?? requestModel, usage: result.usage)
    }
}
public struct JevClient: DecisionClient {
    public static let defaultModel = "typesafe/jev"
    private let accountID: String, token: String, transport: any HTTPTransport
    public init(accountID: String, token: String, transport: any HTTPTransport = BoundedHTTPTransport()) { self.accountID = accountID; self.token = token; self.transport = transport }
    public func evaluate(state: [String: String], questions: [String: DecisionQuestion]) async throws -> DecisionResponse {
        guard !token.isEmpty, accountID.range(of: "^[a-fA-F0-9]{32}$", options: .regularExpression) != nil else { throw PickleError.message("Configure Cloudflare credentials for Jev in Settings.") }
        struct Input: Encodable { let state: [String: String], questions: [String: DecisionQuestion] }
        struct Body: Encodable { let model: String; let input: Input }
        let request = try post(URL(string: "https://api.cloudflare.com/client/v4/accounts/\(accountID)/ai/run")!, token: token, body: Body(model: Self.defaultModel, input: Input(state: state, questions: questions)))
        let data = try await transport.send(request)
        let result = try Self.decode(data)
        try result.validate(for: questions)
        return result
    }
    public static func decode(_ data: Data) throws -> DecisionResponse {
        // Third-party models can return a completed job inside the REST envelope.
        struct Job: Decodable { let state: String; let result: DecisionResponse? }
        struct JobEnvelope: Decodable { let success: Bool; let result: Job }
        if let envelope = try? JSONDecoder().decode(JobEnvelope.self, from: data) {
            guard envelope.success, envelope.result.state == "Completed", let result = envelope.result.result else {
                throw PickleError.message("Cloudflare has not completed the Jev evaluation. Try again shortly.")
            }
            return result
        }
        // Also accept the direct model payload and the standard REST result envelope.
        struct Envelope: Decodable { let success: Bool; let result: DecisionResponse? }
        if let envelope = try? JSONDecoder().decode(Envelope.self, from: data) {
            guard envelope.success, let result = envelope.result else { throw PickleError.message("Cloudflare could not evaluate this passage with Jev.") }
            return result
        }
        return try JSONDecoder().decode(DecisionResponse.self, from: data)
    }
    /// Explicit, tiny inference test on fixed public sample text. May incur Cloudflare charges.
    public func checkConnection() async throws -> String {
        let response = try await evaluate(state: ["source": "The sky is blue."], questions: ["sample": DecisionQuestion("Does source mention the sky?")])
        return response.model
    }
}
/// A real offline mode for UI exploration, with fixed sample output, not an AI model.
public struct SampleProvider: GenerativeProvider {
    public let isRemote = false
    public init() {}
    public func generate(_ prompt: GenerationPrompt) async throws -> Generation {
        try Task.checkCancellation()
        return Generation(text: "The treatment might help some people, but there isn’t much evidence yet.\n\nThis is a sample explanation. Connect your account in Settings to explore your own passages.", model: "Offline sample · fixed response")
    }
}

// Cloudflare JSON Mode returns an object in result.response; prose mode returns a string.
private indirect enum JSONValue: Codable {
    case string(String), number(Double), boolean(Bool), object([String: JSONValue]), array([JSONValue]), null
    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let value = try? c.decode(Bool.self) { self = .boolean(value) }
        else if let value = try? c.decode(String.self) { self = .string(value) }
        else if let value = try? c.decode(Double.self) { self = .number(value) }
        else if let value = try? c.decode([String: JSONValue].self) { self = .object(value) }
        else { self = .array(try c.decode([JSONValue].self)) }
    }
    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .string(let value): try c.encode(value)
        case .number(let value): try c.encode(value)
        case .boolean(let value): try c.encode(value)
        case .object(let value): try c.encode(value)
        case .array(let value): try c.encode(value)
        case .null: try c.encodeNil()
        }
    }
}
