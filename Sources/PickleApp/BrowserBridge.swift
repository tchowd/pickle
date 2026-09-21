import AppKit
import Network
import PickleCore

struct BrowserPayload: Decodable {
    let id: UUID, capturedAt: Double, selection: String, reference: WebReference, bundleID: String
}
@MainActor final class BrowserBridge {
    private var listener: NWListener?
    private var connections: [UUID: NWConnection] = [:]
    private let token = UUID().uuidString + UUID().uuidString
    private var seen = Set<UUID>()
    static let directory = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/Pickle/BrowserBridge", isDirectory: true)
    private let configurationDirectory: URL
    let receive: (BrowserPayload) throws -> Void
    init(directory: URL? = nil, receive: @escaping (BrowserPayload) throws -> Void) { self.configurationDirectory = directory ?? Self.directory; self.receive = receive }
    static func installExtension(id: String, browser: String) throws {
        guard id.range(of: "^[a-p]{32}$", options: .regularExpression) != nil else { throw PickleError.message("Paste the extension ID shown by your browser.") }
        let choices = ["Chrome": ("Google/Chrome", "com.google.Chrome"), "Edge": ("Microsoft Edge", "com.microsoft.edgemac"), "Brave": ("BraveSoftware/Brave-Browser", "com.brave.Browser"), "Arc": ("Arc/User Data", "company.thebrowser.Browser")]
        guard let choice = choices[browser], let resources = Bundle.main.resourceURL,
              FileManager.default.fileExists(atPath: resources.appendingPathComponent("browser-host.py").path),
              FileManager.default.isExecutableFile(atPath: "/usr/bin/python3") else { throw PickleError.message("Use the packaged Pickle app. Browser setup also requires Python 3 on this Mac.") }
        func quote(_ value: String) -> String { "'" + value.replacingOccurrences(of: "'", with: "'\"'\"'") + "'" }
        let support = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support")
        let directory = support.appendingPathComponent("Pickle/NativeHost")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let launcher = directory.appendingPathComponent("host-" + choice.1)
        let script = "#!/bin/sh\nexport PICKLE_BROWSER_BUNDLE=" + quote(choice.1) + "\nexport PICKLE_APP=" + quote(Bundle.main.bundleURL.path) + "\nexec /usr/bin/python3 " + quote(resources.appendingPathComponent("browser-host.py").path) + " \"$@\"\n"
        try script.write(to: launcher, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: launcher.path)
        let manifest = support.appendingPathComponent(choice.0).appendingPathComponent("NativeMessagingHosts/com.pickle.reader.json")
        try FileManager.default.createDirectory(at: manifest.deletingLastPathComponent(), withIntermediateDirectories: true)
        let data = try JSONSerialization.data(withJSONObject: ["name": "com.pickle.reader", "description": "Pickle page context", "path": launcher.path, "type": "stdio", "allowed_origins": ["chrome-extension://" + id + "/"]], options: .prettyPrinted)
        try data.write(to: manifest, options: .atomic)
    }
    func start() throws {
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: .any)
        let listener = try NWListener(using: parameters)
        self.listener = listener
        listener.stateUpdateHandler = { [weak self] state in
            Task { @MainActor in
                guard let self, case .ready = state, let port = self.listener?.port else { return }
                do {
                    try FileManager.default.createDirectory(at: self.configurationDirectory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
                    try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: self.configurationDirectory.path)
                    let data = try JSONSerialization.data(withJSONObject: ["port": Int(port.rawValue), "token": self.token])
                    let path = self.configurationDirectory.appendingPathComponent("connection.json")
                    try data.write(to: path, options: .atomic)
                    try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: path.path)
                } catch { self.stop() }
            }
        }
        listener.newConnectionHandler = { [weak self] connection in Task { @MainActor in self?.accept(connection) } }
        listener.start(queue: .main)
    }
    func stop() {
        listener?.cancel(); listener = nil
        connections.values.forEach { $0.cancel() }; connections = [:]
        let path = configurationDirectory.appendingPathComponent("connection.json")
        if let data = try? Data(contentsOf: path), let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any], object["token"] as? String == token { try? FileManager.default.removeItem(at: path) }
    }
    private func accept(_ connection: NWConnection) {
        guard connections.count < 4 else { connection.cancel(); return }
        let id = UUID(); connections[id] = connection
        connection.start(queue: .main)
        read(connection, id: id, bytes: Data())
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(5))
            self?.connections.removeValue(forKey: id)?.cancel()
        }
    }
    private func read(_ connection: NWConnection, id: UUID, bytes: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] data, _, complete, error in
            Task { @MainActor in
                guard let self, self.connections[id] != nil else { return }
                var bytes = bytes; if let data { bytes.append(data) }
                guard bytes.count <= 100_000, error == nil else { self.connections.removeValue(forKey: id)?.cancel(); return }
                if let end = bytes.firstIndex(of: 10) {
                    var reply = ["ok": "true"]
                    do {
                        guard let object = try JSONSerialization.jsonObject(with: bytes[..<end]) as? [String: Any], object["token"] as? String == self.token,
                              let payload = object["payload"] else { throw PickleError.message("Browser connection rejected.") }
                        let message = try JSONDecoder().decode(BrowserPayload.self, from: JSONSerialization.data(withJSONObject: payload))
                        guard message.capturedAt.isFinite, abs(Date().timeIntervalSince1970 - message.capturedAt) < 60,
                              !self.seen.contains(message.id), SelectionService.browsers.contains(message.bundleID) else { throw PickleError.message("This page capture expired. Try again.") }
                        try message.reference.validate()
                        try self.receive(message)
                        if self.seen.count > 100 { self.seen.removeAll() }; self.seen.insert(message.id)
                    } catch { reply = ["error": (error as? PickleError)?.localizedDescription ?? "Could not use this browser context."] }
                    let response = (try? JSONSerialization.data(withJSONObject: reply)) ?? Data()
                    connection.send(content: response, completion: .contentProcessed { _ in
                        Task { @MainActor [weak self] in self?.connections.removeValue(forKey: id)?.cancel() }
                    })
                } else if complete { self.connections.removeValue(forKey: id)?.cancel() }
                else { self.read(connection, id: id, bytes: bytes) }
            }
        }
    }
}
