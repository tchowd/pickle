import AVFoundation
import AppKit
import ScreenCaptureKit
import Speech

// No microphone input, no audio files, and no cloud recognizer fallback.
@MainActor private final class SourceAudio: NSObject, SCStreamOutput, SCStreamDelegate {
  private var stream: SCStream?
  private var request: SFSpeechAudioBufferRecognitionRequest?
  private var recognition: SFSpeechRecognitionTask?
  private var transcript = ""
  private var failure: Error?
  private var finalized = false
  private var stopped = false
  private let queue = DispatchQueue(label: "com.pickle.rune.source-audio")
  func record(windowID: UInt32, pid: Int32, bundleID: String) async throws -> String {
    defer { stop() }
    guard CGPreflightScreenCaptureAccess() else {
      throw AudioError.message("Allow screen and audio capture access in Settings first.")
    }
    let authorized = await withCheckedContinuation { continuation in
      SFSpeechRecognizer.requestAuthorization { continuation.resume(returning: $0 == .authorized) }
    }
    try Task.checkCancellation()
    guard authorized, let recognizer = SFSpeechRecognizer(), recognizer.isAvailable,
      recognizer.supportsOnDeviceRecognition
    else {
      throw AudioError.message(
        "On-device speech recognition is unavailable. Use captions or paste a transcript instead.")
    }
    let content = try await SCShareableContent.excludingDesktopWindows(
      true, onScreenWindowsOnly: true)
    try Task.checkCancellation()
    guard
      let app = content.applications.first(where: {
        $0.processID == pid && $0.bundleIdentifier == bundleID
      }),
      let window = content.windows.first(where: {
        $0.windowID == windowID && $0.owningApplication?.processID == pid
      }),
      let display = content.displays.first(where: { $0.frame.intersects(window.frame) })
    else {
      throw AudioError.message(
        "The source application is no longer available. Invoke Pickle from the video again.")
    }
    let filter = SCContentFilter(display: display, including: [app], exceptingWindows: [])
    let config = SCStreamConfiguration()
    config.capturesAudio = true
    config.excludesCurrentProcessAudio = true
    config.sampleRate = 16000
    config.channelCount = 1
    config.width = 2
    config.height = 2
    config.minimumFrameInterval = CMTime(value: 1, timescale: 1)
    let request = SFSpeechAudioBufferRecognitionRequest()
    request.requiresOnDeviceRecognition = true
    request.shouldReportPartialResults = true
    self.request = request
    recognition = recognizer.recognitionTask(with: request) { [weak self] result, error in
      Task { @MainActor in
        guard let self, !self.stopped else { return }
        if let result {
          self.transcript = result.bestTranscription.formattedString
          self.finalized = result.isFinal
        }
        if let error { self.failure = error }
      }
    }
    let stream = SCStream(filter: filter, configuration: config, delegate: self)
    self.stream = stream
    try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: queue)
    return try await withTaskCancellationHandler(
      operation: {
        try await stream.startCapture()
        for _ in 0..<30 {
          try await Task.sleep(for: .seconds(1))
          if let failure { throw failure }
        }
        try await stream.stopCapture()
        self.stream = nil
        request.endAudio()
        for _ in 0..<15 {
          if finalized || failure != nil { break }
          try await Task.sleep(for: .milliseconds(200))
        }
        try Task.checkCancellation()
        var text = ""
        for character in transcript {
          if text.utf8.count + String(character).utf8.count > 12000 { break }
          text.append(character)
        }
        guard !text.isEmpty else {
          throw AudioError.message("No speech was heard. Play the video and try again.")
        }
        return text
      }, onCancel: { Task { @MainActor [weak self] in self?.stop() } })
  }
  func stop() {
    stopped = true
    request?.endAudio()
    request = nil
    recognition?.cancel()
    recognition = nil
    let old = stream
    stream = nil
    if let old { Task { try? await old.stopCapture() } }
  }
  nonisolated func stream(_ stream: SCStream, didStopWithError error: Error) {
    Task { @MainActor [weak self] in self?.failure = error }
  }
  nonisolated func stream(
    _ stream: SCStream, didOutputSampleBuffer sample: CMSampleBuffer, of type: SCStreamOutputType
  ) {
    guard type == .audio, sample.isValid, let description = sample.formatDescription else { return }
    let format = AVAudioFormat(cmAudioFormatDescription: description)
    guard
      let buffer = AVAudioPCMBuffer(
        pcmFormat: format, frameCapacity: AVAudioFrameCount(sample.numSamples))
    else { return }
    buffer.frameLength = buffer.frameCapacity
    guard
      CMSampleBufferCopyPCMDataIntoAudioBufferList(
        sample, at: 0, frameCount: Int32(sample.numSamples), into: buffer.mutableAudioBufferList)
        == noErr
    else { return }
    Task { @MainActor [weak self] in
      guard let self, !self.stopped else { return }
      self.request?.append(buffer)
    }
  }
}
private enum AudioError: Error { case message(String) }
private final class AudioOperation {
  var task: Task<Void, Never>?
  deinit { task?.cancel() }
}
@_cdecl("pickle_audio_start") public func pickleAudioStart(
  _ input: UnsafePointer<CChar>, _ callback: @escaping PageCallback,
  _ context: UnsafeMutableRawPointer?
) -> UnsafeMutableRawPointer? {
  guard let data = String(cString: input).data(using: .utf8),
    let source = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
    let windowID = source["source_window"] as? UInt32, let pid = source["source_pid"] as? Int32,
    let bundle = source["app_id"] as? String
  else {
    callback(context, failure("This session has no source window for audio capture."))
    return nil
  }
  let operation = AudioOperation()
  operation.task = Task { @MainActor in
    let audio = SourceAudio()
    do {
      callback(
        context, response(try await audio.record(windowID: windowID, pid: pid, bundleID: bundle)))
    } catch is CancellationError {
      callback(context, failure("Listening was cancelled."))
    } catch AudioError.message(let message) { callback(context, failure(message)) } catch {
      callback(
        context,
        failure(
          "Source-app audio could not be transcribed locally. Use captions or paste a transcript."))
    }
  }
  return Unmanaged.passRetained(operation).toOpaque()
}
@_cdecl("pickle_audio_release") public func pickleAudioRelease(_ handle: UnsafeMutableRawPointer) {
  let operation = Unmanaged<AudioOperation>.fromOpaque(handle).takeRetainedValue()
  operation.task?.cancel()
}
