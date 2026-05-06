import AVFAudio
import CoreMedia
import Foundation
import Speech

public typealias TategotoSpeechEventCallback = @convention(c) (
  UnsafePointer<CChar>?,
  UnsafeMutableRawPointer?
) -> Void

private struct TategotoSpeechConfig: Decodable {
  let localeIdentifier: String?

  enum CodingKeys: String, CodingKey {
    case localeIdentifier = "locale_identifier"
  }
}

private final class TategotoSpeechSession: @unchecked Sendable {
  private let callback: TategotoSpeechEventCallback
  private let userData: UnsafeMutableRawPointer?
  private let callbackQueue = DispatchQueue(label: "tech.s2n.tategoto.speech.callback")
  private let stateLock = NSLock()
  private let sessionID = UUID().uuidString
  private let inputFormat = AVAudioFormat(
    commonFormat: .pcmFormatInt16,
    sampleRate: 24_000,
    channels: 1,
    interleaved: false
  )!

  private var analyzer: SpeechAnalyzer?
  private var transcriber: SpeechTranscriber?
  private var inputContinuation: AsyncStream<AnalyzerInput>.Continuation?
  private var analyzerFormat: AVAudioFormat?
  private var converter: AVAudioConverter?
  private var resultTask: Task<Void, Never>?
  private var analyzerTask: Task<Void, Never>?
  private var sampleCursor: Int64 = 0
  private var resultIndex: Int = 0
  private var previousItemID: String?
  private var stopped = false
  private var bridgeRetain: Unmanaged<TategotoSpeechSession>?

  init(callback: @escaping TategotoSpeechEventCallback, userData: UnsafeMutableRawPointer?) {
    self.callback = callback
    self.userData = userData
  }

  func start(config: TategotoSpeechConfig) {
    Task {
      do {
        try await requestSpeechAuthorization()
        guard SpeechTranscriber.isAvailable else {
          throw TategotoSpeechError("SpeechTranscriber is not available on this Mac.")
        }

        let requestedLocale = config.localeIdentifier
          .flatMap { Locale(identifier: $0) }
          ?? Locale.current
        guard let locale = await SpeechTranscriber.supportedLocale(equivalentTo: requestedLocale)
        else {
          throw TategotoSpeechError(
            "SpeechTranscriber does not support locale: \(requestedLocale.identifier)"
          )
        }

        let transcriber = SpeechTranscriber(
          locale: locale,
          transcriptionOptions: [],
          reportingOptions: [],
          attributeOptions: [.audioTimeRange]
        )
        let analyzer = SpeechAnalyzer(
          modules: [transcriber],
          options: SpeechAnalyzer.Options(priority: .userInitiated, modelRetention: .whileInUse)
        )

        try await ensureAssets(for: [transcriber])
        guard let analyzerFormat = await SpeechAnalyzer.bestAvailableAudioFormat(
          compatibleWith: [transcriber],
          considering: inputFormat
        ) else {
          throw TategotoSpeechError("SpeechAnalyzer did not provide a compatible audio format.")
        }
        guard let converter = AVAudioConverter(from: inputFormat, to: analyzerFormat) else {
          throw TategotoSpeechError("Failed to create audio converter for SpeechAnalyzer.")
        }

        let (inputStream, continuation) = AsyncStream.makeStream(
          of: AnalyzerInput.self,
          bufferingPolicy: .bufferingNewest(64)
        )

        updateState {
          self.transcriber = transcriber
          self.analyzer = analyzer
          self.analyzerFormat = analyzerFormat
          self.converter = converter
          self.inputContinuation = continuation
        }

        resultTask = Task { [weak self, transcriber] in
          await self?.readResults(from: transcriber)
        }
        analyzerTask = Task { [weak self, analyzer, inputStream] in
          do {
            try await analyzer.start(inputSequence: inputStream)
          } catch {
            self?.emitError(error.localizedDescription)
          }
        }

        emit(["type": "ready"])
      } catch {
        emitError(error.localizedDescription)
        emitStopped()
      }
    }
  }

  func retainForBridge(_ retained: Unmanaged<TategotoSpeechSession>) {
    updateState {
      bridgeRetain = retained
    }
  }

  func appendPCM16(_ bytes: UnsafePointer<UInt8>, length: Int) {
    stateLock.lock()
    let stopped = self.stopped
    let continuation = inputContinuation
    let converter = self.converter
    let analyzerFormat = self.analyzerFormat
    let startCursor = sampleCursor
    stateLock.unlock()

    guard !stopped, let continuation, let converter, let analyzerFormat, length >= 2 else {
      return
    }
    let frameCount = length / MemoryLayout<Int16>.size
    guard let inputBuffer = AVAudioPCMBuffer(
      pcmFormat: inputFormat,
      frameCapacity: AVAudioFrameCount(frameCount)
    ) else {
      emitError("Failed to allocate input audio buffer.")
      return
    }

    inputBuffer.frameLength = AVAudioFrameCount(frameCount)
    let destination = inputBuffer.int16ChannelData![0]
    bytes.withMemoryRebound(to: Int16.self, capacity: frameCount) { source in
      destination.update(from: source, count: frameCount)
    }

    guard let converted = convert(inputBuffer, with: converter, to: analyzerFormat) else {
      return
    }

    let startTime = CMTime(value: startCursor, timescale: Int32(analyzerFormat.sampleRate))
    stateLock.lock()
    sampleCursor += Int64(converted.frameLength)
    stateLock.unlock()
    continuation.yield(AnalyzerInput(buffer: converted, bufferStartTime: startTime))
  }

  func stop() {
    stateLock.lock()
    if stopped {
      stateLock.unlock()
      return
    }
    stopped = true
    let continuation = inputContinuation
    let analyzer = self.analyzer
    stateLock.unlock()

    continuation?.finish()
    Task { [self, analyzer] in
      do {
        try await analyzer?.finalizeAndFinishThroughEndOfInput()
      } catch {
        emitError(error.localizedDescription)
      }
      resultTask?.cancel()
      analyzerTask?.cancel()
      emitStopped()
      releaseBridgeRetain()
    }
  }

  private func readResults(from transcriber: SpeechTranscriber) async {
    do {
      for try await result in transcriber.results {
        guard result.isFinal else {
          continue
        }
        let text = String(result.text.characters).trimmingCharacters(in: .whitespacesAndNewlines)
        resultIndex += 1
        let itemID = "apple_\(resultIndex)"
        emit([
          "type": "segment",
          "session_id": sessionID,
          "item_id": itemID,
          "previous_item_id": previousItemID ?? NSNull(),
          "text": text,
          "start_offset_secs": result.range.start.seconds,
          "end_offset_secs": result.range.end.seconds,
        ])
        previousItemID = itemID
      }
    } catch {
      emitError(error.localizedDescription)
    }
  }

  private func convert(
    _ inputBuffer: AVAudioPCMBuffer,
    with converter: AVAudioConverter,
    to format: AVAudioFormat
  ) -> AVAudioPCMBuffer? {
    let ratio = format.sampleRate / inputBuffer.format.sampleRate
    let capacity = AVAudioFrameCount(Double(inputBuffer.frameLength) * ratio) + 8
    guard let outputBuffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: capacity) else {
      emitError("Failed to allocate converted audio buffer.")
      return nil
    }

    var didProvideInput = false
    var conversionError: NSError?
    converter.reset()
    converter.convert(to: outputBuffer, error: &conversionError) { _, status in
      if didProvideInput {
        status.pointee = .noDataNow
        return nil
      }
      didProvideInput = true
      status.pointee = .haveData
      return inputBuffer
    }

    if let conversionError {
      emitError(conversionError.localizedDescription)
      return nil
    }
    return outputBuffer
  }

  private func ensureAssets(for modules: [any SpeechModule]) async throws {
    let status = await AssetInventory.status(forModules: modules)
    switch status {
    case .installed:
      return
    case .supported, .downloading:
      if let request = try await AssetInventory.assetInstallationRequest(supporting: modules) {
        try await request.downloadAndInstall()
      }
    case .unsupported:
      throw TategotoSpeechError("Required SpeechTranscriber assets are unsupported.")
    @unknown default:
      throw TategotoSpeechError("Unknown SpeechTranscriber asset status.")
    }
  }

  private func requestSpeechAuthorization() async throws {
    let status = SFSpeechRecognizer.authorizationStatus()
    if status == .authorized {
      return
    }

    guard isRunningFromAppBundle else {
      throw TategotoSpeechError(
        "Speech recognition permission cannot be requested from `pnpm tauri dev`. macOS requires a bundled .app with NSSpeechRecognitionUsageDescription. Run the bundled Tategoto.app once to grant Speech Recognition permission."
      )
    }

    let requested = await withCheckedContinuation { continuation in
      SFSpeechRecognizer.requestAuthorization { status in
        continuation.resume(returning: status)
      }
    }

    guard requested == .authorized else {
      throw TategotoSpeechError("Speech recognition permission was not granted.")
    }
  }

  private var isRunningFromAppBundle: Bool {
    Bundle.main.bundleURL.pathExtension == "app"
  }

  private func emitError(_ message: String) {
    emit(["type": "error", "message": message])
  }

  private func emitStopped() {
    emit(["type": "stopped"])
  }

  private func emit(_ payload: [String: Any]) {
    callbackQueue.async { [callback, userData] in
      guard JSONSerialization.isValidJSONObject(payload),
        let data = try? JSONSerialization.data(withJSONObject: payload),
        let message = String(data: data, encoding: .utf8)
      else {
        return
      }
      message.withCString { pointer in
        callback(pointer, userData)
      }
    }
  }

  private func updateState(_ operation: () -> Void) {
    stateLock.lock()
    operation()
    stateLock.unlock()
  }

  private func releaseBridgeRetain() {
    stateLock.lock()
    let retained = bridgeRetain
    bridgeRetain = nil
    stateLock.unlock()
    retained?.release()
  }
}

private struct TategotoSpeechError: LocalizedError {
  let message: String

  init(_ message: String) {
    self.message = message
  }

  var errorDescription: String? {
    message
  }
}

@_cdecl("tategoto_speech_start")
public func tategoto_speech_start(
  _ configJSON: UnsafePointer<CChar>,
  _ callback: TategotoSpeechEventCallback?,
  _ userData: UnsafeMutableRawPointer?
) -> UnsafeMutableRawPointer? {
  guard let callback else {
    return nil
  }
  let configData = Data(String(cString: configJSON).utf8)
  guard let config = try? JSONDecoder().decode(TategotoSpeechConfig.self, from: configData) else {
    return nil
  }
  let session = TategotoSpeechSession(callback: callback, userData: userData)
  let retained = Unmanaged.passRetained(session)
  session.retainForBridge(retained)
  session.start(config: config)
  return retained.toOpaque()
}

@_cdecl("tategoto_speech_append_pcm16")
public func tategoto_speech_append_pcm16(
  _ handle: UnsafeMutableRawPointer,
  _ pcm: UnsafePointer<UInt8>,
  _ length: Int
) {
  let session = Unmanaged<TategotoSpeechSession>.fromOpaque(handle).takeUnretainedValue()
  session.appendPCM16(pcm, length: length)
}

@_cdecl("tategoto_speech_stop")
public func tategoto_speech_stop(_ handle: UnsafeMutableRawPointer) {
  let session = Unmanaged<TategotoSpeechSession>.fromOpaque(handle).takeUnretainedValue()
  session.stop()
}
