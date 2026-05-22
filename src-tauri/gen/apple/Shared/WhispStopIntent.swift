#if os(iOS)
import AppIntents
import Foundation

// Shared between the host app target and the WhispLiveActivity extension.
// The Stop button on the Live Activity invokes this intent in the extension
// process. UserDefaults alone can lag across processes, so we also post a
// Darwin notification — the host's CFNotificationCenter observer flips an
// in-memory flag the recorder polls.

public let whispAppGroupSuite = "group.com.whisp2.app"
public let whispStopDarwinNotification = "com.whisp2.app.stop"

public func whispStopKey(_ sessionId: String) -> String { "whisp.stop.\(sessionId)" }

public func whispPostStopNotification(sessionId: String) {
    UserDefaults(suiteName: whispAppGroupSuite)?
        .set(true, forKey: whispStopKey(sessionId))
    let center = CFNotificationCenterGetDarwinNotifyCenter()
    CFNotificationCenterPostNotification(
        center,
        CFNotificationName(whispStopDarwinNotification as CFString),
        nil, nil, true
    )
}

@available(iOS 17.0, *)
public struct WhispStopIntent: AppIntent {
    public static var title: LocalizedStringResource = "Stop Whisp Recording"
    // Runs in the LiveActivity extension process — must not foreground host app.
    public static var openAppWhenRun: Bool = false

    @Parameter(title: "Session ID")
    public var sessionId: String

    public init() {}
    public init(sessionId: String) { self.sessionId = sessionId }

    public func perform() async throws -> some IntentResult {
        whispPostStopNotification(sessionId: sessionId)
        return .result()
    }
}
#endif
