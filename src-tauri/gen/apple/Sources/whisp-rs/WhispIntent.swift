#if canImport(UIKit)
import ActivityKit
import AppIntents
import AudioToolbox
import AVFoundation
import Foundation
import SQLite3
import UIKit

// MARK: - Rust FFI declarations
//
// These symbols are statically linked into whisp_rs_lib.a, which is also
// linked into the host app. Because WhispRecordIntent sets openAppWhenRun
// = true, the AppIntent runs in the host-app process, so the symbols are
// reachable here. See src-tauri/src/ffi.rs for the Rust side.

@_silgen_name("whisp_transcribe_local_wav")
private func whisp_transcribe_local_wav(
    _ wavPath: UnsafePointer<CChar>,
    _ modelPath: UnsafePointer<CChar>,
    _ language: UnsafePointer<CChar>?,
    _ errOut: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("whisp_free_string")
private func whisp_free_string(_ s: UnsafeMutablePointer<CChar>?)

// MARK: - Provider config

private struct ProviderConfig {
    let apiKey: String
    let baseURL: String
    let model: String
    let provider: String
    let localModelPath: String?
    let language: String?
}

// Parsed-but-keyless snapshot of config.json. Cached across AppIntent
// invocations and invalidated on file mtime change. Keys live in keychain
// and are read fresh every call so user-side key rotation isn't masked.
private struct ConfigSnapshot {
    let provider: String
    let openaiURL: String
    let openaiModel: String
    let groqURL: String
    let groqModel: String
    let localModelPath: String?
    let language: String?
}

private final class ConfigCache {
    static let shared = ConfigCache()
    private let lock = NSLock()
    private var cached: ConfigSnapshot?
    private var cachedMtime: Date?
    private var cachedPath: String?

    func snapshot(at url: URL?) -> ConfigSnapshot? {
        guard let url else { return nil }
        let mtime = (try? FileManager.default.attributesOfItem(atPath: url.path)[.modificationDate]) as? Date

        lock.lock()
        defer { lock.unlock() }

        if let cached, let cachedMtime, let cachedPath,
           cachedPath == url.path, cachedMtime == mtime {
            return cached
        }

        guard let data = try? Data(contentsOf: url),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            // Don't poison the cache on parse failure — let caller fall back to defaults.
            return nil
        }

        let snap = ConfigSnapshot(
            provider:       json["provider"] as? String ?? "open_a_i",
            openaiURL:      json["openai_api_url"] as? String ?? "https://api.openai.com/v1/audio/transcriptions",
            openaiModel:    json["openai_model"] as? String ?? "whisper-1",
            groqURL:        json["groq_api_url"] as? String ?? "https://api.groq.com/openai/v1/audio/transcriptions",
            groqModel:      json["groq_model"] as? String ?? "whisper-large-v3-turbo",
            localModelPath: json["local_whisper_model_path"] as? String,
            language:       json["language"] as? String
        )
        cached = snap
        cachedMtime = mtime
        cachedPath = url.path
        return snap
    }
}

// MARK: - Record and Transcribe Intent

@available(iOS 16.0, *)
struct WhispRecordIntent: AppIntent {
    static var title: LocalizedStringResource = "Record & Transcribe"
    static var description = IntentDescription("Hold Action Button to record speech — result is copied to clipboard.")
    // true: brings Whisp to foreground. Required because AVAudioSession.setActive
    // fails with "Session activation failed" when the intent runs in background,
    // even with UIBackgroundModes: audio declared. iOS restricts cold-start audio
    // session activation from AppIntent extension context.
    static var openAppWhenRun: Bool = true

    func perform() async throws -> some IntentResult & ReturnsValue<String> {
        WhispLogger.log("WhispIntent", "perform() started")

        // Action Button re-press while a session is live: stop the current one
        // and exit. The previous perform()'s recorder will finish its own
        // transcription. This prevents stacked AVAudioRecorder instances.
        if let activeId = WhispRecorder.currentSessionId() {
            WhispLogger.log("WhispIntent", "active session \(activeId) — signalling stop and exiting")
            whispPostStopNotification(sessionId: activeId)
            return .result(value: "")
        }

        let recorder = WhispRecorder()
        let (text, provider): (String, String)
        do {
            (text, provider) = try await recorder.recordAndTranscribe()
        } catch {
            // Backstop: if anything threw past the recorder's own defer chain,
            // make absolutely sure the static session slot is cleared so the
            // next Action Button press doesn't get short-circuited at line 111.
            WhispRecorder.setActiveSession(nil)
            WhispLogger.error("WhispIntent", "recordAndTranscribe failed", error)
            throw error
        }
        WhispLogger.log("WhispIntent", "transcription complete: \(text.count) chars")

        // Haptic + sound work in any app state — these don't gate on .active.
        await MainActor.run {
            UINotificationFeedbackGenerator().notificationOccurred(.success)
            AudioServicesPlaySystemSound(1054) // brief "tock" — same family as ApplePay confirm
        }

        // Save to history.db so it appears in the app's History tab.
        saveToHistory(text: text, provider: provider)

        return .result(value: text)
    }
}

// MARK: - App Shortcuts (Action Button mapping)

@available(iOS 16.4, *)
struct WhispShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: WhispRecordIntent(),
            phrases: [
                "Record with \(.applicationName)",
                "Transcribe with \(.applicationName)",
                "Start \(.applicationName)",
            ],
            shortTitle: "Record & Transcribe",
            systemImageName: "mic.fill"
        )
    }
}

// MARK: - Recorder

@available(iOS 16.0, *)
private final class WhispRecorder: NSObject, AVAudioRecorderDelegate {
    private var recorder: AVAudioRecorder?
    private let lock = NSLock()
    private var continuation: CheckedContinuation<URL, Error>?

    // Tracks the in-flight recording session so a second Action Button press
    // can stop the first instead of starting a parallel AVAudioRecorder.
    // Also read by Darwin-notification observers to know which key to flip.
    private static let sessionLock = NSLock()
    private static var activeSessionId: String?

    static func currentSessionId() -> String? {
        sessionLock.lock(); defer { sessionLock.unlock() }
        return activeSessionId
    }

    static func setActiveSession(_ id: String?) {
        sessionLock.lock(); defer { sessionLock.unlock() }
        activeSessionId = id
    }
    // 16 kHz mono 16-bit PCM WAV — what the local whisper.cpp path expects,
    // and what the cloud endpoints (OpenAI/Groq) also accept.
    private let fileURL: URL = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString)
        .appendingPathExtension("wav")

    // In-memory stop flag flipped by the Darwin-notification observer or the
    // foreground re-entry observer. Polled by meterAndWaitForStop. Local to
    // this recorder instance so the value is always immediate (UserDefaults
    // synchronization across processes is too slow).
    private let stopRequested = AtomicFlag()

    func recordAndTranscribe() async throws -> (text: String, provider: String) {
        let activity: Any?
        let sessionId: String
        if #available(iOS 17.0, *) {
            let started = (try? Self.startActivity()) ?? nil
            activity = started?.activity
            sessionId = started?.sessionId ?? UUID().uuidString
        } else {
            activity = nil
            sessionId = UUID().uuidString
        }

        Self.setActiveSession(sessionId)
        // Reset the stop flag for this session before recording starts.
        Self.appGroupDefaults?.set(false, forKey: Self.stopKey(sessionId))

        // Cross-process stop signal from WhispStopIntent (Live Activity button
        // running in the extension process). Darwin notifications cross
        // process boundaries immediately, unlike UserDefaults reads.
        let darwinObserver = installDarwinStopObserver(sessionId: sessionId)

        // Foreground re-entry stops the recorder. Debounce 1.5s to ignore the
        // initial activation that the AppIntent itself triggers.
        let foregroundObserver = await Self.installForegroundStopObserver { [weak self] in
            self?.stopRequested.set()
        }
        defer {
            removeDarwinStopObserver(darwinObserver)
            if let foregroundObserver {
                NotificationCenter.default.removeObserver(foregroundObserver)
            }
            Self.appGroupDefaults?.removeObject(forKey: Self.stopKey(sessionId))
            Self.setActiveSession(nil)
        }

        let audioURL: URL
        do {
            audioURL = try await record(activity: activity, sessionId: sessionId)
        } catch {
            if #available(iOS 17.0, *) {
                await Self.endActivity(activity, phase: .error,
                                       preview: "",
                                       errorMessage: error.localizedDescription)
            }
            throw error
        }
        defer {
            try? FileManager.default.removeItem(at: audioURL)
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        }

        if #available(iOS 17.0, *) {
            await Self.updateActivity(activity, phase: .processing, level: 0)
        }

        do {
            let result = try await transcribe(audioURL: audioURL)
            if #available(iOS 17.0, *) {
                await Self.endActivity(activity, phase: .done,
                                       preview: result.text, errorMessage: "")
            }
            return result
        } catch {
            if #available(iOS 17.0, *) {
                await Self.endActivity(activity, phase: .error,
                                       preview: "",
                                       errorMessage: error.localizedDescription)
            }
            throw error
        }
    }

    private func record(activity: Any? = nil, sessionId: String) async throws -> URL {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.record, mode: .default, options: .allowBluetooth)
            try session.setActive(true)
        } catch {
            WhispLogger.error("WhispIntent", "AVAudioSession activation failed", error)
            throw error
        }

        let settings: [String: Any] = [
            AVFormatIDKey: Int(kAudioFormatLinearPCM),
            AVSampleRateKey: 16000,
            AVNumberOfChannelsKey: 1,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsFloatKey: false,
            AVLinearPCMIsBigEndianKey: false,
        ]

        do {
            recorder = try AVAudioRecorder(url: fileURL, settings: settings)
        } catch {
            WhispLogger.error("WhispIntent", "AVAudioRecorder init failed", error)
            throw error
        }
        recorder?.isMeteringEnabled = true
        recorder?.delegate = self

        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                lock.lock()
                self.continuation = continuation
                lock.unlock()
                // Unbounded recording — only explicit stop signals end this.
                let started = recorder?.record() ?? false
                if !started {
                    WhispLogger.error("WhispIntent", "recorder.record() returned false (silent start failure)")
                    self.resumeOnce(throwing: WhispError.recordingFailed)
                    return
                }
                Task { [activity] in
                    // Verify the recorder actually entered the recording state
                    // before we trust it. AVFoundation occasionally returns
                    // true from record() but never transitions to isRecording,
                    // which leaves the continuation pinned forever and traps
                    // activeSessionId in the static slot — the user has to
                    // close+reopen the app to escape.
                    if !(await self.confirmRecordingStarted()) {
                        WhispLogger.error("WhispIntent", "recorder failed to start within 500ms — aborting session")
                        self.recorder?.stop()
                        self.resumeOnce(throwing: WhispError.recordingFailed)
                        return
                    }
                    await self.meterAndWaitForStop(activity: activity, sessionId: sessionId)
                    // Watchdog: if AVAudioRecorder.stop() was called but the
                    // delegate callback never fires, the continuation would
                    // hang forever. After 3s, force-resume with whatever's on
                    // disk (or fail) so the session can wind down.
                    Task {
                        try? await Task.sleep(nanoseconds: 3 * 1_000_000_000)
                        if self.continuationStillPending() {
                            WhispLogger.error("WhispIntent", "delegate never fired post-stop — force-resuming")
                            if let url = self.recorder?.url,
                               FileManager.default.fileExists(atPath: url.path) {
                                self.resumeOnce(returning: url)
                            } else {
                                self.resumeOnce(throwing: WhispError.recordingFailed)
                            }
                        }
                    }
                }
            }
        } onCancel: {
            self.recorder?.stop()
        }
    }

    // Poll until recorder.isRecording is true or 500ms elapses. Returns true
    // if the recorder reached the recording state.
    private func confirmRecordingStarted() async -> Bool {
        for _ in 0..<10 {
            if recorder?.isRecording == true { return true }
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
        return recorder?.isRecording == true
    }

    private func resumeOnce(returning url: URL) {
        lock.lock()
        let c = continuation
        continuation = nil
        lock.unlock()
        c?.resume(returning: url)
    }

    private func resumeOnce(throwing error: Error) {
        lock.lock()
        let c = continuation
        continuation = nil
        lock.unlock()
        c?.resume(throwing: error)
    }

    private func continuationStillPending() -> Bool {
        lock.lock(); defer { lock.unlock() }
        return continuation != nil
    }

    // -40 dBFS averagePower threshold; 1.5s trailing silence after speech is
    // detected ends the recording naturally. Long enough to ride out
    // mid-sentence breaths and thinking pauses but short enough that users
    // don't have to reach for the Stop button on every utterance.
    private let speechThreshold: Float = -40
    private let silenceTrailSeconds: TimeInterval = 1.5

    // Poll loop: pushes audio level to the Live Activity at ~5 Hz, watches
    // the cross-process stop signal (WhispStopIntent / foreground re-entry),
    // and runs silence detection. Stops on whichever fires first.
    private func meterAndWaitForStop(activity: Any? = nil, sessionId: String) async {
        let pollInterval: TimeInterval = 0.1
        var pollCount = 0
        var speechDetected = false
        var lastSpeechTime = Date()

        while recorder?.isRecording == true {
            try? await Task.sleep(nanoseconds: UInt64(pollInterval * 1_000_000_000))
            guard recorder?.isRecording == true else { break }

            if stopRequested.isSet {
                NSLog("[WhispIntent] stop requested — stopping recorder")
                recorder?.stop()
                break
            }

            recorder?.updateMeters()
            let power = recorder?.averagePower(forChannel: 0) ?? -160

            pollCount += 1
            if #available(iOS 17.0, *), pollCount % 2 == 0 {
                await WhispRecorder.updateActivity(activity, phase: .recording, level: power)
            }

            if power > speechThreshold {
                speechDetected = true
                lastSpeechTime = Date()
            } else if speechDetected && Date().timeIntervalSince(lastSpeechTime) >= silenceTrailSeconds {
                NSLog("[WhispIntent] silence detected after speech — stopping recorder")
                recorder?.stop()
                break
            }
        }
    }

    // AVAudioRecorderDelegate — called on an arbitrary thread by AVFoundation
    func audioRecorderDidFinishRecording(_ recorder: AVAudioRecorder, successfully flag: Bool) {
        if flag {
            resumeOnce(returning: recorder.url)
        } else {
            WhispLogger.error("WhispIntent", "audioRecorderDidFinishRecording successfully=false")
            resumeOnce(throwing: WhispError.recordingFailed)
        }
    }

    func audioRecorderEncodeErrorDidOccur(_ recorder: AVAudioRecorder, error: Error?) {
        WhispLogger.error("WhispIntent", "audioRecorderEncodeErrorDidOccur", error)
        resumeOnce(throwing: error ?? WhispError.recordingFailed)
    }

    private func transcribe(audioURL: URL) async throws -> (text: String, provider: String) {
        let cfg = try readProviderConfig()
        WhispLogger.log("WhispIntent",
            "transcribe: provider=\(cfg.provider) baseURL=\(cfg.baseURL) model=\(cfg.model)")

        if cfg.provider == "local_whisper", let modelPath = cfg.localModelPath {
            return try runLocalWhisper(wavURL: audioURL, modelPath: modelPath, language: cfg.language)
        }

        guard let endpointURL = URL(string: cfg.baseURL) else {
            throw WhispError.apiFailed("Invalid API URL: \(cfg.baseURL)")
        }

        let audioData = try Data(contentsOf: audioURL)
        NSLog("[WhispIntent] audio file size: %d bytes", audioData.count)

        var request = URLRequest(url: endpointURL)
        request.httpMethod = "POST"
        request.timeoutInterval = 60

        let boundary = UUID().uuidString
        request.setValue("Bearer \(cfg.apiKey)", forHTTPHeaderField: "Authorization")
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")

        var body = Data()
        body.appendField("model", value: cfg.model, boundary: boundary)
        body.appendFile("file", filename: "audio.wav", mimeType: "audio/wav", data: audioData, boundary: boundary)
        body.appendFinalBoundary(boundary: boundary)
        request.httpBody = body

        NSLog("[WhispIntent] sending request to %@", cfg.baseURL)
        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await URLSession.shared.data(for: request)
        } catch {
            WhispLogger.error("WhispIntent", "URLSession error", error)
            throw WhispError.apiFailed("Network error: \(error.localizedDescription)")
        }
        let statusCode = (response as? HTTPURLResponse)?.statusCode ?? -1
        NSLog("[WhispIntent] response status: %d, data: %d bytes", statusCode, data.count)

        guard statusCode == 200 else {
            let body = String(data: data, encoding: .utf8) ?? "(non-utf8)"
            WhispLogger.error("WhispIntent", "HTTP \(statusCode): \(body)")
            throw WhispError.apiFailed("HTTP \(statusCode): \(body)")
        }

        let decoded = try JSONDecoder().decode(TranscriptionResponse.self, from: data)
        NSLog("[WhispIntent] decoded text: %d chars", decoded.text.count)
        return (text: decoded.text, provider: cfg.provider)
    }

    // Run on-device Whisper via the Rust FFI. Synchronous from Swift's POV
    // (the Rust side spins a current-thread tokio runtime) but the AppIntent
    // is already on a background async context so this doesn't block the
    // main thread.
    private func runLocalWhisper(wavURL: URL, modelPath: String, language: String?)
        throws -> (text: String, provider: String) {
        NSLog("[WhispIntent] local_whisper: wav=%@ model=%@", wavURL.path, modelPath)
        var errPtr: UnsafeMutablePointer<CChar>? = nil
        let resultPtr: UnsafeMutablePointer<CChar>? = wavURL.path.withCString { wav in
            modelPath.withCString { model in
                if let lang = language, !lang.isEmpty {
                    return lang.withCString { langPtr in
                        whisp_transcribe_local_wav(wav, model, langPtr, &errPtr)
                    }
                } else {
                    return whisp_transcribe_local_wav(wav, model, nil, &errPtr)
                }
            }
        }

        if let resultPtr {
            let text = String(cString: resultPtr)
            whisp_free_string(resultPtr)
            NSLog("[WhispIntent] local_whisper success: %d chars", text.count)
            return (text: text, provider: "local_whisper")
        }

        let msg: String
        if let errPtr {
            msg = String(cString: errPtr)
            whisp_free_string(errPtr)
        } else {
            msg = "unknown local Whisper error"
        }
        WhispLogger.error("WhispIntent", "local_whisper failed: \(msg)")
        throw WhispError.apiFailed("Local Whisper: \(msg)")
    }

    // Read provider, URL, model, key, and (for local_whisper) model path from
    // config.json + keychain. Mirrors what Rust does in transcription/manager.rs.
    // The parsed config is cached across AppIntent invocations and invalidated
    // by config.json mtime; keychain is read fresh so key rotation takes effect
    // immediately.
    private func readProviderConfig() throws -> ProviderConfig {
        let configURL = FileManager.default
            .urls(for: .documentDirectory, in: .userDomainMask)
            .first?
            .appendingPathComponent("config.json")

        let snap = ConfigCache.shared.snapshot(at: configURL) ?? ConfigSnapshot(
            provider:       "open_a_i",
            openaiURL:      "https://api.openai.com/v1/audio/transcriptions",
            openaiModel:    "whisper-1",
            groqURL:        "https://api.groq.com/openai/v1/audio/transcriptions",
            groqModel:      "whisper-large-v3-turbo",
            localModelPath: nil,
            language:       nil
        )
        NSLog("[WhispIntent] using provider=%@", snap.provider)

        switch snap.provider {
        case "groq":
            guard let key = readKeychainKey("groq_api_key") else { throw WhispError.noApiKey }
            return ProviderConfig(apiKey: key, baseURL: snap.groqURL, model: snap.groqModel,
                                  provider: "groq", localModelPath: nil, language: snap.language)
        case "gemini":
            // Gemini uses a different API shape (generateContent, not multipart)
            // and a different auth header (x-goog-api-key, not Bearer). The
            // iOS transcribe path only speaks the OpenAI multipart shape, so
            // routing Gemini through it would send the Google key as a Bearer
            // token to OpenAI's endpoint. Reject cleanly until iOS gets a real
            // Gemini implementation.
            throw WhispError.apiFailed("Gemini is not yet supported on iOS. Pick OpenAI, Groq, or Local Whisper in Settings.")
        case "local_whisper":
            guard let stored = snap.localModelPath, !stored.isEmpty else {
                throw WhispError.localWhisperModelMissing
            }
            // Config stores a filename ("ggml-tiny.bin") to survive iOS data
            // container UUID rotation across Xcode reinstalls. Resolve to an
            // absolute path under Documents/models for the existence check
            // and so Rust's mmap call gets a usable path. Legacy absolute
            // paths from older builds pass through unchanged.
            let resolved = Self.resolveModelPath(stored)
            guard FileManager.default.fileExists(atPath: resolved) else {
                WhispLogger.error("WhispIntent", "local model not found at resolved path: \(resolved) (stored=\(stored))")
                throw WhispError.localWhisperModelMissing
            }
            return ProviderConfig(apiKey: "", baseURL: "", model: "",
                                  provider: "local_whisper", localModelPath: resolved,
                                  language: snap.language)
        default: // "open_a_i"
            guard let key = readKeychainKey("openai_api_key") else { throw WhispError.noApiKey }
            return ProviderConfig(apiKey: key, baseURL: snap.openaiURL, model: snap.openaiModel,
                                  provider: "open_a_i", localModelPath: nil, language: snap.language)
        }
    }

    // Mirror of Rust's commands::model_download::resolve_model_path.
    // Absolute path → return as-is. Relative (filename) → resolve under
    // Documents/models which is where the Rust download path writes.
    static func resolveModelPath(_ stored: String) -> String {
        if stored.hasPrefix("/") { return stored }
        let docs = FileManager.default
            .urls(for: .documentDirectory, in: .userDomainMask)
            .first
        guard let docs else { return stored }
        return docs.appendingPathComponent("models").appendingPathComponent(stored).path
    }

    private func readKeychainKey(_ account: String) -> String? {
        // Standard [String: Any] pattern; kSecReturnData requires kCFBooleanTrue not Swift Bool.
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "com.whisp2.app",
            kSecAttrAccount as String: account,
            kSecReturnData as String: kCFBooleanTrue as Any,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        // status 0=success, -25300=not found, -34018=missing entitlement
        NSLog("[WhispIntent] keychain '%@': status=%d", account, status)
        guard status == errSecSuccess, let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }
}

// MARK: - Live Activity helpers

@available(iOS 17.0, *)
extension WhispRecorder {
    struct StartedActivity {
        let activity: Activity<WhispActivityAttributes>
        let sessionId: String
    }

    static func startActivity() throws -> StartedActivity? {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else {
            NSLog("[WhispIntent] Live Activities disabled by user — skipping")
            return nil
        }
        let sessionId = UUID().uuidString
        let attrs = WhispActivityAttributes(
            sessionId: sessionId,
            startedAt: Date()
        )
        let initial = WhispActivityAttributes.ContentState(
            phase: .recording,
            levelDbfs: -160,
            transcriptPreview: "",
            errorMessage: ""
        )
        let a = try Activity.request(
            attributes: attrs,
            content: .init(state: initial, staleDate: Date().addingTimeInterval(60)),
            pushType: nil
        )
        return StartedActivity(activity: a, sessionId: sessionId)
    }

    static func updateActivity(
        _ erased: Any?,
        phase: WhispActivityAttributes.ContentState.Phase,
        level: Float,
        errorMessage: String = ""
    ) async {
        guard let a = erased as? Activity<WhispActivityAttributes> else { return }
        let s = WhispActivityAttributes.ContentState(
            phase: phase,
            levelDbfs: level,
            transcriptPreview: "",
            errorMessage: errorMessage
        )
        await a.update(.init(state: s, staleDate: Date().addingTimeInterval(60)))
    }

    static func endActivity(
        _ erased: Any?,
        phase: WhispActivityAttributes.ContentState.Phase,
        preview: String,
        errorMessage: String = ""
    ) async {
        guard let a = erased as? Activity<WhispActivityAttributes> else { return }
        let s = WhispActivityAttributes.ContentState(
            phase: phase,
            levelDbfs: 0,
            transcriptPreview: String(preview.prefix(80)),
            errorMessage: errorMessage
        )
        // Linger 4s on Lock Screen so user sees the result, then auto-dismiss.
        await a.end(
            .init(state: s, staleDate: nil),
            dismissalPolicy: .after(Date().addingTimeInterval(4))
        )
    }
}

// MARK: - Stop signal plumbing (Darwin notification + foreground observer)

// Thread-safe flag for the recorder polling loop. Set from observer callbacks
// that may run on arbitrary threads (Darwin notification queue, main queue).
private final class AtomicFlag {
    private let lock = NSLock()
    private var value = false
    var isSet: Bool {
        lock.lock(); defer { lock.unlock() }
        return value
    }
    func set() {
        lock.lock(); defer { lock.unlock() }
        value = true
    }
}

@available(iOS 16.0, *)
extension WhispRecorder {
    static var appGroupDefaults: UserDefaults? {
        UserDefaults(suiteName: whispAppGroupSuite)
    }
    static func stopKey(_ sessionId: String) -> String { whispStopKey(sessionId) }

    @MainActor
    static func installForegroundStopObserver(_ onStop: @escaping () -> Void) -> NSObjectProtocol? {
        let started = Date()
        // Ignore the activation that the AppIntent itself triggers when bringing
        // the app forward. Anything after the debounce window is a user re-entry.
        let debounce: TimeInterval = 1.5
        return NotificationCenter.default.addObserver(
            forName: UIApplication.didBecomeActiveNotification,
            object: nil,
            queue: .main
        ) { _ in
            guard Date().timeIntervalSince(started) >= debounce else {
                NSLog("[WhispIntent] foreground activation within debounce — ignoring")
                return
            }
            NSLog("[WhispIntent] foreground re-entry — stop signal")
            onStop()
        }
    }

    func installDarwinStopObserver(sessionId: String) -> UnsafeMutableRawPointer {
        // We register the recorder's stopRequested flag as the observer's
        // context. CFNotificationCenterAddObserver uses an Unmanaged opaque
        // pointer; we keep the flag alive via the recorder's own lifetime.
        let ctx = Unmanaged.passUnretained(stopRequested).toOpaque()
        let center = CFNotificationCenterGetDarwinNotifyCenter()
        CFNotificationCenterAddObserver(
            center,
            ctx,
            { _, ctx, _, _, _ in
                guard let ctx else { return }
                let flag = Unmanaged<AtomicFlag>.fromOpaque(ctx).takeUnretainedValue()
                NSLog("[WhispIntent] darwin stop notification — setting flag")
                flag.set()
            },
            whispStopDarwinNotification as CFString,
            nil,
            .deliverImmediately
        )
        return ctx
    }

    func removeDarwinStopObserver(_ ctx: UnsafeMutableRawPointer) {
        let center = CFNotificationCenterGetDarwinNotifyCenter()
        CFNotificationCenterRemoveEveryObserver(center, ctx)
    }
}

// MARK: - History persistence

@available(iOS 16.0, *)
private func saveToHistory(text: String, provider: String) {
    guard let dbURL = FileManager.default
        .urls(for: .documentDirectory, in: .userDomainMask)
        .first?
        .appendingPathComponent("history.db") else { return }

    var db: OpaquePointer?
    guard sqlite3_open(dbURL.path, &db) == SQLITE_OK else {
        NSLog("[WhispIntent] history.db open failed")
        return
    }
    defer { sqlite3_close(db) }

    let createTable = """
        CREATE TABLE IF NOT EXISTS history (
            id TEXT PRIMARY KEY,
            text TEXT NOT NULL,
            source_app TEXT,
            provider TEXT NOT NULL,
            word_count INTEGER NOT NULL DEFAULT 0,
            char_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        )
        """
    sqlite3_exec(db, createTable, nil, nil, nil)

    let id = UUID().uuidString
    let now = ISO8601DateFormatter().string(from: Date())
    let wordCount = Int32(text.split(whereSeparator: \.isWhitespace).count)
    let charCount = Int32(text.count)

    let sql = "INSERT INTO history (id, text, source_app, provider, word_count, char_count, created_at) VALUES (?, ?, NULL, ?, ?, ?, ?)"
    var stmt: OpaquePointer?
    guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else {
        NSLog("[WhispIntent] history insert prepare failed")
        return
    }
    defer { sqlite3_finalize(stmt) }

    (id as NSString).utf8String.map { sqlite3_bind_text(stmt, 1, $0, -1, nil) }
    (text as NSString).utf8String.map { sqlite3_bind_text(stmt, 2, $0, -1, nil) }
    (provider as NSString).utf8String.map { sqlite3_bind_text(stmt, 3, $0, -1, nil) }
    sqlite3_bind_int(stmt, 4, wordCount)
    sqlite3_bind_int(stmt, 5, charCount)
    (now as NSString).utf8String.map { sqlite3_bind_text(stmt, 6, $0, -1, nil) }

    if sqlite3_step(stmt) == SQLITE_DONE {
        NSLog("[WhispIntent] history saved: %d chars", charCount)
    } else {
        NSLog("[WhispIntent] history step failed: %s", sqlite3_errmsg(db))
    }
}

// MARK: - Supporting types

private struct TranscriptionResponse: Decodable {
    let text: String
}

private enum WhispError: LocalizedError {
    case recordingFailed
    case noApiKey
    case localWhisperModelMissing
    case apiFailed(String)

    var errorDescription: String? {
        switch self {
        case .recordingFailed: return "Recording failed."
        case .noApiKey: return "No API key found. Open Whisp, add your key in Settings, then try again."
        case .localWhisperModelMissing: return "No local Whisper model selected. Open Whisp, go to Settings → Local (on-device), and download a model."
        case .apiFailed(let msg): return "Transcription failed: \(msg)"
        }
    }
}

// MARK: - Multipart helpers

private extension Data {
    mutating func appendField(_ name: String, value: String, boundary: String) {
        append("--\(boundary)\r\n".data(using: .utf8)!)
        append("Content-Disposition: form-data; name=\"\(name)\"\r\n\r\n".data(using: .utf8)!)
        append("\(value)\r\n".data(using: .utf8)!)
    }

    mutating func appendFile(_ name: String, filename: String, mimeType: String, data fileData: Data, boundary: String) {
        append("--\(boundary)\r\n".data(using: .utf8)!)
        append("Content-Disposition: form-data; name=\"\(name)\"; filename=\"\(filename)\"\r\n".data(using: .utf8)!)
        append("Content-Type: \(mimeType)\r\n\r\n".data(using: .utf8)!)
        append(fileData)
        append("\r\n".data(using: .utf8)!)
    }

    mutating func appendFinalBoundary(boundary: String) {
        append("--\(boundary)--\r\n".data(using: .utf8)!)
    }
}

// MARK: - Shortcut installer (UIDocumentInteractionController)

// UIApplication.openURL on a file:// URL inside the app bundle is sandbox-blocked
// cross-app, so Shortcuts can't read the bundled .shortcut directly. The supported
// path is to copy the file to a readable location (Documents/) and present a
// UIDocumentInteractionController "Open in…" sheet anchored on the key window.
@available(iOS 16.0, *)
private final class WhispShortcutInstaller: NSObject, UIDocumentInteractionControllerDelegate {
    static let shared = WhispShortcutInstaller()

    private var interaction: UIDocumentInteractionController?

    func present() -> Bool {
        guard let bundleURL = Bundle.main.url(forResource: "RecordAndTranscribe", withExtension: "shortcut", subdirectory: "assets")
            ?? Bundle.main.url(forResource: "RecordAndTranscribe", withExtension: "shortcut") else {
            NSLog("[WhispShortcutInstaller] bundled .shortcut not found")
            return false
        }

        let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
        let dest = docs.appendingPathComponent("RecordAndTranscribe.shortcut")
        do {
            if FileManager.default.fileExists(atPath: dest.path) {
                try FileManager.default.removeItem(at: dest)
            }
            try FileManager.default.copyItem(at: bundleURL, to: dest)
        } catch {
            NSLog("[WhispShortcutInstaller] copy failed: %@", error.localizedDescription)
            return false
        }

        guard let scene = UIApplication.shared.connectedScenes
            .first(where: { $0.activationState == .foregroundActive }) as? UIWindowScene,
            let window = scene.windows.first(where: { $0.isKeyWindow }) ?? scene.windows.first,
            let rootVC = window.rootViewController else {
            NSLog("[WhispShortcutInstaller] no foreground window")
            return false
        }

        let dic = UIDocumentInteractionController(url: dest)
        dic.uti = "com.apple.shortcut"
        dic.delegate = self
        self.interaction = dic

        let presented = dic.presentOpenInMenu(from: rootVC.view.bounds, in: rootVC.view, animated: true)
        if !presented {
            NSLog("[WhispShortcutInstaller] presentOpenInMenu returned false (no apps registered for .shortcut)")
            self.interaction = nil
            return false
        }
        return true
    }

    func documentInteractionControllerDidDismissOpenInMenu(_ controller: UIDocumentInteractionController) {
        self.interaction = nil
    }
}

@available(iOS 16.0, *)
@_cdecl("whisp_present_shortcut_installer")
public func whisp_present_shortcut_installer() -> Bool {
    return WhispShortcutInstaller.shared.present()
}
#endif
