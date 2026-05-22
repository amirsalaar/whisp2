#if os(iOS)
import ActivityKit
import AppIntents
import SwiftUI
import WidgetKit

@available(iOS 17.0, *)
struct WhispLiveActivityWidget: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: WhispActivityAttributes.self) { context in
            LockScreenView(state: context.state, sessionId: context.attributes.sessionId)
                .padding(16)
                .activityBackgroundTint(.black)
                .activitySystemActionForegroundColor(.white)
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Image(systemName: phaseIcon(context.state.phase))
                        .foregroundStyle(phaseTint(context.state.phase))
                        .font(.title3)
                }
                DynamicIslandExpandedRegion(.trailing) {
                    if context.state.phase == .recording {
                        StopButton(sessionId: context.attributes.sessionId)
                    } else {
                        LevelMeter(level: context.state.levelDbfs,
                                   tint: phaseTint(context.state.phase))
                            .frame(width: 80, height: 14)
                    }
                }
                DynamicIslandExpandedRegion(.center) {
                    if context.state.phase == .recording {
                        LevelMeter(level: context.state.levelDbfs,
                                   tint: phaseTint(context.state.phase))
                            .frame(height: 14)
                    }
                }
                DynamicIslandExpandedRegion(.bottom) {
                    Text(phaseLabel(context.state))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            } compactLeading: {
                Image(systemName: phaseIcon(context.state.phase))
                    .foregroundStyle(phaseTint(context.state.phase))
            } compactTrailing: {
                LevelMeter(level: context.state.levelDbfs,
                           tint: phaseTint(context.state.phase))
                    .frame(width: 28, height: 8)
            } minimal: {
                Image(systemName: phaseIcon(context.state.phase))
                    .foregroundStyle(phaseTint(context.state.phase))
            }
        }
    }
}

@available(iOS 17.0, *)
private struct LockScreenView: View {
    let state: WhispActivityAttributes.ContentState
    let sessionId: String

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: phaseIcon(state.phase))
                .foregroundStyle(phaseTint(state.phase))
                .font(.title2)
                .frame(width: 32)

            VStack(alignment: .leading, spacing: 4) {
                Text("Whisp")
                    .font(.headline)
                    .foregroundStyle(.white)
                Text(phaseLabel(state))
                    .font(.caption)
                    .foregroundStyle(.white.opacity(0.7))
                    .lineLimit(2)
            }

            Spacer()

            if state.phase == .recording {
                LevelMeter(level: state.levelDbfs, tint: phaseTint(state.phase))
                    .frame(width: 48, height: 10)
                StopButton(sessionId: sessionId)
            }
        }
    }
}

@available(iOS 17.0, *)
private struct StopButton: View {
    let sessionId: String

    var body: some View {
        Button(intent: WhispStopIntent(sessionId: sessionId)) {
            Image(systemName: "stop.fill")
                .font(.system(size: 14, weight: .bold))
                .foregroundStyle(.white)
                .frame(width: 32, height: 32)
                .background(Circle().fill(.red))
        }
        .buttonStyle(.plain)
    }
}

@available(iOS 17.0, *)
private struct LevelMeter: View {
    let level: Float
    let tint: Color

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 3)
                    .fill(.white.opacity(0.15))
                RoundedRectangle(cornerRadius: 3)
                    .fill(tint)
                    .frame(width: geo.size.width * CGFloat(normalized))
                    .animation(.easeOut(duration: 0.15), value: normalized)
            }
        }
    }

    private var normalized: Float {
        let clamped = max(-60, min(0, level))
        return (clamped + 60) / 60
    }
}

@available(iOS 17.0, *)
private func phaseIcon(_ p: WhispActivityAttributes.ContentState.Phase) -> String {
    switch p {
    case .recording:  return "mic.fill"
    case .processing: return "waveform.path.ecg"
    case .done:       return "checkmark.circle.fill"
    case .error:      return "exclamationmark.circle.fill"
    }
}

@available(iOS 17.0, *)
private func phaseTint(_ p: WhispActivityAttributes.ContentState.Phase) -> Color {
    switch p {
    case .recording:  return .red
    case .processing: return .orange
    case .done:       return .green
    case .error:      return .red
    }
}

@available(iOS 17.0, *)
private func phaseLabel(_ s: WhispActivityAttributes.ContentState) -> String {
    switch s.phase {
    case .recording:  return "Listening…"
    case .processing: return "Transcribing…"
    case .done:       return s.transcriptPreview.isEmpty ? "Done" : s.transcriptPreview
    case .error:      return s.errorMessage.isEmpty ? "Error" : s.errorMessage
    }
}
#endif
