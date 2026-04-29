/// Recording state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingState {
    Idle,
    Recording,
    Processing,
}

/// Commands sent from hotkey task → audio task.
#[derive(Debug, Clone, Copy)]
pub enum RecordingCommand {
    Start,
    Stop,
    Cancel,
}

/// Events emitted by the CGEventTap callback.
#[derive(Debug, Clone, Copy)]
pub enum HotkeyEvent {
    KeyDown,
    KeyUp,
}
