use anyhow::Result;
use std::time::Duration;

use crate::config::models::{AppConfig, TranscriptionProvider};
use crate::keychain;
use crate::transcription::providers::local_whisper::{LocalWhisperProvider, WhisperCtxCache};

use super::providers::gemini::GeminiProvider;
use super::providers::openai::OpenAIProvider;

pub async fn transcribe(
    config: &AppConfig,
    wav_bytes: Vec<u8>,
    whisper_ctx: WhisperCtxCache,
) -> Result<String> {
    match &config.provider {
        TranscriptionProvider::OpenAI => {
            let api_key = keychain::get("openai_api_key")?
                .ok_or_else(|| anyhow::anyhow!("OpenAI API key not set. Open Settings to configure."))?;
            let provider = OpenAIProvider::new(
                api_key,
                config.openai_api_url.clone(),
                config.openai_model.clone(),
            );
            transcribe_with_retry(|| provider.transcribe(wav_bytes.clone(), config.language.as_deref()))
                .await
        }
        TranscriptionProvider::Groq => {
            let api_key = keychain::get("groq_api_key")?
                .ok_or_else(|| anyhow::anyhow!("Groq API key not set. Open Settings to configure."))?;
            let provider = OpenAIProvider::new(
                api_key,
                config.groq_api_url.clone(),
                config.groq_model.clone(),
            );
            transcribe_with_retry(|| provider.transcribe(wav_bytes.clone(), config.language.as_deref()))
                .await
        }
        TranscriptionProvider::Gemini => {
            let api_key = keychain::get("gemini_api_key")?
                .ok_or_else(|| anyhow::anyhow!("Gemini API key not set. Open Settings to configure."))?;
            let provider = GeminiProvider::new(
                api_key,
                config.gemini_model.clone(),
            );
            transcribe_with_retry(|| provider.transcribe(wav_bytes.clone(), config.language.as_deref()))
                .await
        }
        TranscriptionProvider::LocalWhisper => {
            let model_path = config
                .local_whisper_model_path
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!(
                    "No model file selected. Choose a GGML .bin file in Settings → Local Whisper."
                ))?
                .clone();
            let provider = LocalWhisperProvider {
                model_path,
                ctx_cache: whisper_ctx,
                language: config.language.clone(),
            };
            provider.transcribe(wav_bytes).await
        }
    }
}

async fn transcribe_with_retry<F, Fut>(f: F) -> Result<String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<String>>,
{
    let mut last_err = None;
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt - 1))).await;
        }
        match f().await {
            Ok(text) => return Ok(text),
            Err(e) => {
                tracing::warn!("transcription attempt {} failed: {}", attempt + 1, e);
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap())
}
