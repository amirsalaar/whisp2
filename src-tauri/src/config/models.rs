use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionProvider {
    OpenAI,
    Gemini,
}

impl Default for TranscriptionProvider {
    fn default() -> Self {
        TranscriptionProvider::OpenAI
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingMode {
    PressAndHold,
    Toggle,
}

impl Default for RecordingMode {
    fn default() -> Self {
        RecordingMode::PressAndHold
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyTrigger {
    LeftOption,
    RightOption,
    LeftCommand,
    RightCommand,
    RightControl,
}

impl Default for HotkeyTrigger {
    fn default() -> Self {
        HotkeyTrigger::RightCommand
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub provider: TranscriptionProvider,
    pub recording_mode: RecordingMode,
    pub hotkey: HotkeyTrigger,
    pub openai_api_url: String,
    pub openai_model: String,
    pub play_completion_sound: bool,
    pub save_history: bool,
    pub show_hud: bool,
    pub language: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: TranscriptionProvider::default(),
            recording_mode: RecordingMode::default(),
            hotkey: HotkeyTrigger::default(),
            openai_api_url: "https://api.openai.com/v1/audio/transcriptions".into(),
            openai_model: "whisper-1".into(),
            play_completion_sound: true,
            save_history: true,
            show_hud: true,
            language: None,
        }
    }
}
