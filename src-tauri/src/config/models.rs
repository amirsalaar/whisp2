use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyTrigger {
    LeftOption,
    RightOption,
    LeftCommand,
    RightCommand,
    RightControl,
    /// Globe / Fn key — CGEventFlags::maskSecondaryFn
    Fn,
}

impl Default for HotkeyTrigger {
    fn default() -> Self {
        HotkeyTrigger::RightCommand
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Name of the cpal input device to use. None = OS default.
    pub input_device: Option<String>,
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
            input_device: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_is_openai() {
        assert_eq!(TranscriptionProvider::default(), TranscriptionProvider::OpenAI);
    }

    #[test]
    fn default_recording_mode_is_press_and_hold() {
        assert_eq!(RecordingMode::default(), RecordingMode::PressAndHold);
    }

    #[test]
    fn app_config_serde_roundtrip() {
        let original = AppConfig::default();
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: AppConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }
}
