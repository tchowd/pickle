import Foundation
import PickleCore

/// Pin the previously validated public IP while preserving the URL host for TLS and HTTP.
/// This prevents a second DNS lookup from rebinding a public hostname to a local address.
final class PublicPageDownload: @unchecked Sendable {
    private let lock = NSLock()
    private var process: Process?
    private var cancelled = false
    func cancel() {
        lock.lock(); cancelled = true
        if let process, process.isRunning { process.terminate() }
        lock.unlock()
    }
    func fetch(_ url: URL, address: String) async throws -> Data {
        try await withTaskCancellationHandler(operation: {
            try await withCheckedThrowingContinuation { continuation in
                DispatchQueue.global(qos: .userInitiated).async {
                    do { continuation.resume(returning: try self.download(url, address: address)) }
                    catch { continuation.resume(throwing: error) }
                }
            }
        }, onCancel: { self.cancel() })
    }
    private func download(_ url: URL, address: String) throws -> Data {
        let task = Process(), output = Pipe()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/curl")
        let port = url.port ?? (url.scheme == "https" ? 443 : 80)
        task.arguments = ["--disable", "--silent", "--show-error", "--globoff", "--noproxy", "*", "--ipv4",
                          "--proto", "=http,https", "--max-time", "4", "--max-filesize", "2000000",
                          "--resolve", "\(url.host!):\(port):\(address)", "--header", "Accept: text/html,application/xhtml+xml",
                          "--write-out", "\nPICKLE_HTTP_META\n%{http_code}\n%{content_type}", "--url", url.absoluteString]
        task.standardOutput = output; task.standardError = FileHandle.nullDevice; task.standardInput = FileHandle.nullDevice
        lock.lock()
        if cancelled { lock.unlock(); throw CancellationError() }
        process = task
        do { try task.run() } catch { lock.unlock(); throw error }
        lock.unlock()
        defer { lock.lock(); process = nil; lock.unlock() }
        var data = Data()
        while let chunk = try output.fileHandleForReading.read(upToCount: 65536), !chunk.isEmpty {
            data.append(chunk)
            if data.count > 2_001_024 { cancel(); throw PickleError.message("This page is too large.") }
        }
        task.waitUntilExit()
        lock.lock(); let wasCancelled = cancelled; lock.unlock()
        if wasCancelled { throw CancellationError() }
        guard task.terminationStatus == 0,
              let boundary = data.range(of: Data("\nPICKLE_HTTP_META\n".utf8), options: .backwards),
              boundary.lowerBound <= 2_000_000,
              let metadata = String(data: data[boundary.upperBound...], encoding: .utf8) else {
            throw PickleError.message("This page could not be fetched. Try the browser extension.")
        }
        let fields = metadata.components(separatedBy: "\n")
        guard fields.count == 2, fields[0] == "200", ["text/html", "application/xhtml+xml"].contains(fields[1].components(separatedBy: ";")[0].lowercased()) else {
            throw PickleError.message("This page needs browser access. Use the Pickle browser extension.")
        }
        return data.subdata(in: 0..<boundary.lowerBound)
    }
}
