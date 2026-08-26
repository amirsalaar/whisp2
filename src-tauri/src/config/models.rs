use serde::{Deserialize, Serialize};

/// Google's dedicated speech-to-text model. Preferred over the general-purpose
/// multimodal models for dictation: it is purpose-built for transcription rather
/// than prompted into it.
pub const DEFAULT_GEMINI_MODEL: &str = "gemini-3.5-transcribe";

/// Gemini models Google has shut down. A config written by an older Whisp can
/// still name one, and requests against them fail outright, so `config::load`
/// swaps them for [`DEFAULT_GEMINI_MODEL`].
pub const RETIRED_GEMINI_MODELS: &[&str] = &[
    "gemini-2.0-flash",
    "gemini-2.0-flash-lite",
    "gemini-1.5-flash",
    "gemini-1.5-pro",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionProvider {
    #[default]
    OpenAI,
    Groq,
    Gemini,
    LocalWhisper,
    Parakeet,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingMode {
    #[default]
    PressAndHold,
    Toggle,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyTrigger {
    LeftOption,
    RightOption,
    LeftCommand,
    #[default]
    RightCommand,
    RightControl,
    /// Globe / Fn key — CGEventFlags::maskSecondaryFn
    Fn,
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
    /// Directory name (under `models/`) holding the Parakeet ONNX weights
    /// (encoder, decoder_joint, vocab.txt) for local on-device transcription.
    pub parakeet_model_path: Option<String>,
    /// Name of the cpal input device to use. None = OS default.
    pub input_device: Option<String>,
    /// Where the user dragged the floating island, as a logical top-left position in
    /// Tauri screen coordinates. None = the default bottom-center placement. Always
    /// re-clamped onto the current screen at launch (see `hud::position`), so a
    /// position saved on a since-disconnected display can't strand the window.
    #[serde(default)]
    pub hud_position: Option<(f64, f64)>,
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
            gemini_model: DEFAULT_GEMINI_MODEL.into(),
            play_completion_sound: true,
            save_history: true,
            show_hud: true,
            language: None,
            max_history_entries: Some(500),
            local_whisper_model_path: None,
            parakeet_model_path: None,
            input_device: None,
            hud_position: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_is_openai() {
        assert_eq!(
            TranscriptionProvider::default(),
            TranscriptionProvider::OpenAI
        );
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
