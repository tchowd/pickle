import AppKit
import AVFoundation
import ScreenCaptureKit
import Speech
import PickleCore

/// Explicit, bounded source-application recording. No background or retroactive capture.
@MainActor final class AudioContextService: NSObject, SCStreamOutput, SCStreamDelegate {
    private var stream: SCStream?
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var recognition: SFSpeechRecognitionTask?
    private var transcript = ""
    private var failure: Error?
    private var finalized = false
    private var stopped = false
    private let queue = DispatchQueue(label: "com.pickle.audio-context")
    func record(_ target: ScreenContextTarget) async throws -> String {
        defer { stop() }
        guard ScreenContextService.permitted else { throw PickleError.message("Allow screen and audio access in Settings first.") }
        let authorized = await withCheckedContinuation { continuation in
            SFSpeechRecognizer.requestAuthorization { continuation.resume(returning: $0 == .authorized) }
        }
        try Task.checkCancellation()
        guard authorized, let recognizer = SFSpeechRecognizer(), recognizer.isAvailable, recognizer.supportsOnDeviceRecognition else {
            throw PickleError.message("On-device speech recognition isn’t available. Use captions or paste a transcript instead.")
        }
        let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
        try Task.checkCancellation()
        guard let app = content.applications.first(where: { $0.processID == target.pid && $0.bundleIdentifier == target.bundleID }),
              let window = content.windows.first(where: { $0.windowID == target.windowID && $0.owningApplication?.processID == target.pid }),
              let display = content.displays.first(where: { $0.frame.intersects(window.frame) }) else {
            throw PickleError.message("Return to the video and invoke Pickle again.")
        }
        let filter = SCContentFilter(display: display, including: [app], exceptingWindows: [])
        let config = SCStreamConfiguration()
        config.capturesAudio = true; config.excludesCurrentProcessAudio = true
        config.sampleRate = 16000; config.channelCount = 1
        config.width = 2; config.height = 2; config.minimumFrameInterval = CMTime(value: 1, timescale: 1)
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.requiresOnDeviceRecognition = true; request.shouldReportPartialResults = true
        self.request = request
        recognition = recognizer.recognitionTask(with: request) { [weak self] result, error in
            Task { @MainActor in
                guard let self, !self.stopped else { return }
                if let result { self.transcript = result.bestTranscription.formattedString; self.finalized = result.isFinal }
                if let error { self.failure = error }
            }
        }
        let stream = SCStream(filter: filter, configuration: config, delegate: self)
        self.stream = stream
        try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: queue)
        return try await withTaskCancellationHandler(operation: {
            do {
                try await stream.startCapture()
                for _ in 0..<30 {
                    try await Task.sleep(for: .seconds(1))
                    if let failure { throw failure }
                }
                try await stream.stopCapture(); self.stream = nil
                request.endAudio()
                for _ in 0..<15 {
                    if finalized || failure != nil { break }
                    try await Task.sleep(for: .milliseconds(200))
                }
                try Task.checkCancellation()
                let result = PageContextLimits.bounded(transcript, bytes: WebReference.budget)
                stop()
                guard !result.isEmpty else { throw PickleError.message("No speech was heard. Play the video and try again.") }
                return result
            } catch { stop(); throw error }
        }, onCancel: { Task { @MainActor [weak self] in self?.stop() } })
    }
    func stop() {
        stopped = true; request?.endAudio(); request = nil
        recognition?.cancel(); recognition = nil
        let old = stream; stream = nil
        if let old { Task { try? await old.stopCapture() } }
    }
    nonisolated func stream(_ stream: SCStream, didStopWithError error: Error) {
        Task { @MainActor [weak self] in self?.failure = error }
    }
    nonisolated func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio, sampleBuffer.isValid,
              let description = sampleBuffer.formatDescription else { return }
        let format = AVAudioFormat(cmAudioFormatDescription: description)
        guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(sampleBuffer.numSamples)) else { return }
        buffer.frameLength = buffer.frameCapacity
        guard CMSampleBufferCopyPCMDataIntoAudioBufferList(sampleBuffer, at: 0, frameCount: Int32(sampleBuffer.numSamples), into: buffer.mutableAudioBufferList) == noErr else { return }
        Task { @MainActor [weak self] in
            guard let self, !self.stopped else { return }
            self.request?.append(buffer)
        }
    }
}
