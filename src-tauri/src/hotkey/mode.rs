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
#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    KeyDown(Option<String>), // frontmost bundle ID at press time
    KeyUp,
}
