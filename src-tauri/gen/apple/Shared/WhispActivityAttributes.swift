#if os(iOS)
import ActivityKit
import Foundation

@available(iOS 16.2, *)
struct WhispActivityAttributes: ActivityAttributes {
    public struct ContentState: Codable, Hashable {
        var phase: Phase
        var levelDbfs: Float
        var transcriptPreview: String
        var errorMessage: String

        enum Phase: String, Codable, Hashable {
            case recording, processing, done, error
        }
    }

    var sessionId: String
    var startedAt: Date
}
#endif
