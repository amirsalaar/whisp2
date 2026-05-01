#if canImport(UIKit)
import AppIntents
import AVFoundation
import Foundation
import UIKit

// MARK: - Record and Transcribe Intent

@available(iOS 16.0, *)
struct WhispRecordIntent: AppIntent {
    static var title: LocalizedStringResource = "Record & Transcribe"
    static var description = IntentDescription("Hold Action Button to record speech — result is copied to clipboard.")
    static var openAppWhenRun: Bool = false

    func perform() async throws -> some IntentResult {
        let recorder = WhispRecorder()
        let text = try await recorder.recordAndTranscribe()
        UIPasteboard.general.string = text
        return .result()
    }
}

// MARK: - App Shortcuts (Action Button mapping)

@available(iOS 16.4, *)
struct WhispShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: WhispRecordIntent(),
            phrases: ["Whisp", "Record with Whisp", "Transcribe with Whisp"],
            shortTitle: "Record & Transcribe",
            systemImageName: "mic.fill"
        )
    }
}

// MARK: - Recorder

@available(iOS 16.0, *)
private final class WhispRecorder: NSObject, AVAudioRecorderDelegate {
    private var recorder: AVAudioRecorder?
    private var continuation: CheckedContinuation<URL, Error>?
    private let fileURL: URL = {
        FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("m4a")
    }()

    func recordAndTranscribe() async throws -> String {
        let audioURL = try await record()
        return try await transcribe(audioURL: audioURL)
    }

    private func record() async throws -> URL {
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.record, mode: .default)
        try session.setActive(true)

        let settings: [String: Any] = [
            AVFormatIDKey: Int(kAudioFormatMPEG4AAC),
            AVSampleRateKey: 16000,
            AVNumberOfChannelsKey: 1,
            AVEncoderAudioQualityKey: AVAudioQuality.medium.rawValue,
        ]

        recorder = try AVAudioRecorder(url: fileURL, settings: settings)
        recorder?.delegate = self

        return try await withCheckedThrowingContinuation { continuation in
            self.continuation = continuation
            recorder?.record(forDuration: 30)
        }
    }

    // AVAudioRecorderDelegate — called when recording ends (duration elapsed or stopped)
    nonisolated func audioRecorderDidFinishRecording(_ recorder: AVAudioRecorder, successfully flag: Bool) {
        if flag {
            continuation?.resume(returning: recorder.url)
        } else {
            continuation?.resume(throwing: WhispError.recordingFailed)
        }
        continuation = nil
    }

    private func transcribe(audioURL: URL) async throws -> String {
        let apiKey = readApiKey()
        guard let apiKey else { throw WhispError.noApiKey }

        let audioData = try Data(contentsOf: audioURL)

        var request = URLRequest(url: URL(string: "https://api.openai.com/v1/audio/transcriptions")!)
        request.httpMethod = "POST"

        let boundary = UUID().uuidString
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")

        var body = Data()
        body.appendField("model", value: "whisper-1", boundary: boundary)
        body.appendFile("file", filename: "audio.m4a", mimeType: "audio/m4a", data: audioData, boundary: boundary)
        body.appendFinalBoundary(boundary: boundary)
        request.httpBody = body

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw WhispError.apiFailed(String(data: data, encoding: .utf8) ?? "unknown error")
        }

        let decoded = try JSONDecoder().decode(TranscriptionResponse.self, from: data)
        return decoded.text
    }

    private func readApiKey() -> String? {
        let query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: "com.whisp2.app",
            kSecAttrAccount: "openai_api_key",
            kSecReturnData: true,
            kSecMatchLimit: kSecMatchLimitOne,
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess, let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }
}

// MARK: - Supporting types

private struct TranscriptionResponse: Decodable {
    let text: String
}

private enum WhispError: LocalizedError {
    case recordingFailed
    case noApiKey
    case apiFailed(String)

    var errorDescription: String? {
        switch self {
        case .recordingFailed: return "Recording failed."
        case .noApiKey: return "No OpenAI API key found. Open Whisp and add your key."
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
#endif
