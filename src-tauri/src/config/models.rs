use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionProvider {
    OpenAI,
    Groq,
    Gemini,
    LocalWhisper,
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
    pub groq_api_url: String,
    pub groq_model: String,
    pub gemini_model: String,
    pub play_completion_sound: bool,
    pub save_history: bool,
    pub show_hud: bool,
    pub language: Option<String>,
    /// Maximum number of history entries to keep. None = unlimited.
    pub max_history_entries: Option<usize>,
    /// Path to a GGML `.bin` model file for local on-device transcription.
    pub local_whisper_model_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: TranscriptionProvider::default(),
            recording_mode: RecordingMode::default(),
            hotkey: HotkeyTrigger::default(),
            openai_api_url: "https://api.openai.com/v1/audio/transcriptions".into(),
            openai_model: "whisper-1".into(),
            groq_api_url: "https://api.groq.com/openai/v1/audio/transcriptions".into(),
            groq_model: "whisper-large-v3-turbo".into(),
            gemini_model: "gemini-2.0-flash".into(),
            play_completion_sound: true,
            save_history: true,
            show_hud: true,
            language: None,
            max_history_entries: Some(500),
            local_whisper_model_path: None,
        }
    }
}
