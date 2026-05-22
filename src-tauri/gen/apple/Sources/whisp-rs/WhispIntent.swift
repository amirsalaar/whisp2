#if canImport(UIKit)
import AppIntents
import AudioToolbox
import AVFoundation
import Foundation
import SQLite3
import UIKit

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
        NSLog("[WhispIntent] perform() started")
        let recorder = WhispRecorder()
        let (text, provider) = try await recorder.recordAndTranscribe()
        NSLog("[WhispIntent] transcription complete: %d chars", text.count)

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
    private let fileURL: URL = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString)
        .appendingPathExtension("m4a")

    func recordAndTranscribe() async throws -> (text: String, provider: String) {
        let audioURL = try await record()
        defer {
            try? FileManager.default.removeItem(at: audioURL)
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        }
        return try await transcribe(audioURL: audioURL)
    }

    private func record() async throws -> URL {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.record, mode: .default, options: .allowBluetooth)
            try session.setActive(true)
        } catch {
            NSLog("[WhispIntent] AVAudioSession activation failed: %@", error.localizedDescription)
            throw error
        }

        let settings: [String: Any] = [
            AVFormatIDKey: Int(kAudioFormatMPEG4AAC),
            AVSampleRateKey: 16000,
            AVNumberOfChannelsKey: 1,
            AVEncoderAudioQualityKey: AVAudioQuality.medium.rawValue,
        ]

        recorder = try AVAudioRecorder(url: fileURL, settings: settings)
        recorder?.isMeteringEnabled = true
        recorder?.delegate = self

        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                lock.lock()
                self.continuation = continuation
                lock.unlock()
                // Hard cap; silence detection will stop it sooner.
                recorder?.record(forDuration: 8)
                // Poll for silence: stop ~1.5s after speech ends.
                Task { await self.stopOnSilence() }
            }
        } onCancel: {
            self.recorder?.stop()
        }
    }

    // -40 dBFS threshold paired with averagePower (RMS). Peak power was too noisy
    // — instantaneous spikes from background noise kept resetting the trail timer.
    private let speechThreshold: Float = -40
    // Stop 1.0s after last speech detected (gives time for natural pause between words).
    private let silenceTrailSeconds: TimeInterval = 1.0

    private func stopOnSilence() async {
        var speechDetected = false
        var lastSpeechTime = Date()
        let pollInterval: TimeInterval = 0.1

        while recorder?.isRecording == true {
            try? await Task.sleep(nanoseconds: UInt64(pollInterval * 1_000_000_000))
            guard recorder?.isRecording == true else { break }

            recorder?.updateMeters()
            let power = recorder?.averagePower(forChannel: 0) ?? -160
            NSLog("[WhispIntent] dBFS=%.1f speechDetected=%@", power, speechDetected ? "true" : "false")

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
        lock.lock()
        let c = continuation
        continuation = nil
        lock.unlock()
        if flag {
            c?.resume(returning: recorder.url)
        } else {
            c?.resume(throwing: WhispError.recordingFailed)
        }
    }

    private func transcribe(audioURL: URL) async throws -> (text: String, provider: String) {
        let (apiKey, baseURL, model, provider) = try readProviderConfig()
        NSLog("[WhispIntent] transcribe: provider=%@ baseURL=%@ model=%@", provider, baseURL, model)

        guard let endpointURL = URL(string: baseURL) else {
            throw WhispError.apiFailed("Invalid API URL: \(baseURL)")
        }

        let audioData = try Data(contentsOf: audioURL)
        NSLog("[WhispIntent] audio file size: %d bytes", audioData.count)

        var request = URLRequest(url: endpointURL)
        request.httpMethod = "POST"
        request.timeoutInterval = 60

        let boundary = UUID().uuidString
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")

        var body = Data()
        body.appendField("model", value: model, boundary: boundary)
        body.appendFile("file", filename: "audio.m4a", mimeType: "audio/m4a", data: audioData, boundary: boundary)
        body.appendFinalBoundary(boundary: boundary)
        request.httpBody = body

        NSLog("[WhispIntent] sending request to %@", baseURL)
        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await URLSession.shared.data(for: request)
        } catch {
            NSLog("[WhispIntent] URLSession error: %@", error.localizedDescription)
            throw WhispError.apiFailed("Network error: \(error.localizedDescription)")
        }
        let statusCode = (response as? HTTPURLResponse)?.statusCode ?? -1
        NSLog("[WhispIntent] response status: %d, data: %d bytes", statusCode, data.count)

        guard statusCode == 200 else {
            let body = String(data: data, encoding: .utf8) ?? "(non-utf8)"
            throw WhispError.apiFailed("HTTP \(statusCode): \(body)")
        }

        let decoded = try JSONDecoder().decode(TranscriptionResponse.self, from: data)
        NSLog("[WhispIntent] decoded text: %d chars", decoded.text.count)
        return (text: decoded.text, provider: provider)
    }

    // Read provider, URL, model, and key from config.json + keychain.
    // Mirrors what Rust does in transcription/manager.rs.
    private func readProviderConfig() throws -> (apiKey: String, baseURL: String, model: String, provider: String) {
        let defaultOpenAI = ("https://api.openai.com/v1/audio/transcriptions", "whisper-1")
        let defaultGroq   = ("https://api.groq.com/openai/v1/audio/transcriptions", "whisper-large-v3-turbo")

        let provider: String
        let openaiURL: String
        let openaiModel: String
        let groqURL: String
        let groqModel: String

        let configURL = FileManager.default
            .urls(for: .documentDirectory, in: .userDomainMask)
            .first?
            .appendingPathComponent("config.json")

        NSLog("[WhispIntent] config path: %@", configURL?.path ?? "nil")

        if let configURL,
           let data = try? Data(contentsOf: configURL),
           let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            NSLog("[WhispIntent] config loaded, provider=%@", json["provider"] as? String ?? "nil")
            provider    = json["provider"] as? String ?? "open_a_i"
            openaiURL   = json["openai_api_url"] as? String ?? defaultOpenAI.0
            openaiModel = json["openai_model"] as? String ?? defaultOpenAI.1
            groqURL     = json["groq_api_url"] as? String ?? defaultGroq.0
            groqModel   = json["groq_model"] as? String ?? defaultGroq.1
        } else {
            NSLog("[WhispIntent] config not found or parse failed — using OpenAI defaults")
            provider    = "open_a_i"
            openaiURL   = defaultOpenAI.0
            openaiModel = defaultOpenAI.1
            groqURL     = defaultGroq.0
            groqModel   = defaultGroq.1
        }

        NSLog("[WhispIntent] using provider=%@", provider)

        switch provider {
        case "groq":
            guard let key = readKeychainKey("groq_api_key") else { throw WhispError.noApiKey }
            return (key, groqURL, groqModel, "groq")
        case "gemini":
            guard let key = readKeychainKey("gemini_api_key") else { throw WhispError.noApiKey }
            return (key, openaiURL, openaiModel, "gemini")
        case "local_whisper":
            // Local Whisper can't run from the Action Button — fall back to OpenAI if available.
            guard let key = readKeychainKey("openai_api_key") else { throw WhispError.localWhisperUnsupported }
            return (key, openaiURL, openaiModel, "open_a_i")
        default: // "open_a_i"
            guard let key = readKeychainKey("openai_api_key") else { throw WhispError.noApiKey }
            return (key, openaiURL, openaiModel, "open_a_i")
        }
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
    case localWhisperUnsupported
    case apiFailed(String)

    var errorDescription: String? {
        switch self {
        case .recordingFailed: return "Recording failed."
        case .noApiKey: return "No API key found. Open Whisp, add your key in Settings, then try again."
        case .localWhisperUnsupported: return "Local Whisper can't run from the Action Button. Switch to OpenAI or Groq in Settings."
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
