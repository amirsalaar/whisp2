/// Recording state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingState {
    Idle,
    Recording,
    Processing,
    Error(String),
}

/// Commands sent from hotkey task → audio task.
#[derive(Debug, Clone)]
pub enum RecordingCommand {
    Start(Option<String>), // source_app bundle ID captured at KeyDown
    Stop,
    Cancel,
}

/// Events emitted by the CGEventTap callback.
#[derive(Debug, Clone, PartialEq)]
pub enum HotkeyEvent {
    KeyDown(Option<String>), // frontmost bundle ID at press time
    KeyUp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recording_state_eq() {
        assert_eq!(RecordingState::Idle, RecordingState::Idle);
        assert_ne!(RecordingState::Idle, RecordingState::Recording);
    }

    #[test]
    fn test_recording_state_clone() {
        let s = RecordingState::Error("oops".into());
        assert_eq!(s.clone(), RecordingState::Error("oops".into()));
    }

    #[test]
    fn test_hotkey_event_clone() {
        let e = HotkeyEvent::KeyDown(Some("com.apple.Safari".into()));
        assert_eq!(e.clone(), HotkeyEvent::KeyDown(Some("com.apple.Safari".into())));
    }
}
