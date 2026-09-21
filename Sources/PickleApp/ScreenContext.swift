import AppKit
import ScreenCaptureKit
import Vision
import PickleCore

struct ScreenContextTarget: Sendable {
    let windowID: CGWindowID
    let pid: pid_t
    let bundleID: String
}
struct CapturedPage: Sendable {
    let id = UUID()
    let jpeg: Data
    let text: String
    let capturedAt: Date
    let truncated: Bool
}

@MainActor enum ScreenContextService {
    static var permitted: Bool { CGPreflightScreenCaptureAccess() }
    static func requestPermission() { CGRequestScreenCaptureAccess() }
    /// Resolve the source before the panel becomes key. Never fall back to a display capture.
    static func target(for snapshot: SelectionSnapshot) -> ScreenContextTarget? {
        guard let app = NSWorkspace.shared.frontmostApplication,
              app.bundleIdentifier == snapshot.bundleID,
              app.processIdentifier != ProcessInfo.processInfo.processIdentifier,
              let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]],
              let source = windows.first(where: {
                  ($0[kCGWindowOwnerPID as String] as? Int32) == app.processIdentifier &&
                  ($0[kCGWindowLayer as String] as? Int) == 0 &&
                  ($0[kCGWindowAlpha as String] as? Double ?? 1) > 0
              }), let id = source[kCGWindowNumber as String] as? UInt32 else { return nil }
        return ScreenContextTarget(windowID: id, pid: app.processIdentifier, bundleID: snapshot.bundleID)
    }
    static func capture(_ target: ScreenContextTarget) async throws -> CapturedPage {
        guard permitted else { throw PickleError.message("Allow Screen Recording in Settings to include page context.") }
        let available = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
        try Task.checkCancellation()
        guard let window = available.windows.first(where: { $0.windowID == target.windowID && $0.owningApplication?.processID == target.pid && $0.owningApplication?.bundleIdentifier == target.bundleID }),
              window.frame.width > 0, window.frame.height > 0 else {
            throw PickleError.message("The source window is no longer available. Select the passage again to refresh its context.")
        }
        let filter = SCContentFilter(desktopIndependentWindow: window)
        let config = SCStreamConfiguration()
        let scale = min(2, 2000 / max(window.frame.width, window.frame.height))
        config.width = max(1, Int(window.frame.width * scale)); config.height = max(1, Int(window.frame.height * scale))
        config.showsCursor = false
        let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
        try Task.checkCancellation()
        // Vision and image compression stay off the main thread. Only compressed data is retained.
        let worker = Task.detached(priority: .userInitiated) { try process(image) }
        return try await withTaskCancellationHandler(operation: { try await worker.value }, onCancel: { worker.cancel() })
    }
    nonisolated static func process(_ image: CGImage) throws -> CapturedPage {
        try Task.checkCancellation()
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate; request.usesLanguageCorrection = true
        request.automaticallyDetectsLanguage = true
        try VNImageRequestHandler(cgImage: image).perform([request])
        try Task.checkCancellation()
        let text = (request.results ?? []).compactMap { observation -> String? in
            guard let best = observation.topCandidates(1).first, best.confidence >= 0.35 else { return nil }
            return best.string
        }.joined(separator: "\n")
        let factor = min(1, 1440 / Double(max(image.width, image.height)))
        let width = max(1, Int(Double(image.width) * factor)), height = max(1, Int(Double(image.height) * factor))
        guard let canvas = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                                     space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else {
            throw PickleError.message("Couldn’t prepare this screenshot.")
        }
        canvas.setFillColor(CGColor(gray: 1, alpha: 1)); canvas.fill(CGRect(x: 0, y: 0, width: width, height: height))
        canvas.interpolationQuality = .high; canvas.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        guard let resized = canvas.makeImage() else { throw PickleError.message("Couldn’t prepare this screenshot.") }
        let bitmap = NSBitmapImageRep(cgImage: resized)
        for quality in [0.7, 0.5, 0.3] {
            if let data = bitmap.representation(using: .jpeg, properties: [.compressionFactor: quality]), data.count <= PageContextLimits.imageBytes {
                return CapturedPage(jpeg: data, text: PageContextLimits.bounded(text), capturedAt: Date(), truncated: text.utf8.count > PageContextLimits.textBytes)
            }
        }
        throw PickleError.message("This screenshot is too detailed to keep. You can still use the selected passage.")
    }
}
