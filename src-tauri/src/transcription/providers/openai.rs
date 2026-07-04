use anyhow::{Context, Result};
use reqwest::{multipart, Client};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct WhisperResponse {
    text: String,
}

pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    api_url: String,
    model: String,
    /// Human-readable provider name for logs/errors. This struct backs both the
    /// OpenAI and Groq providers (same wire format), so the label is what tells
    /// them apart in a "… API error 401" message.
    label: &'static str,
}

impl OpenAIProvider {
    pub fn new(api_key: String, api_url: String, model: String) -> Self {
        Self::with_label("OpenAI", api_key, api_url, model)
    }

    pub fn with_label(
        label: &'static str,
        api_key: String,
        api_url: String,
        model: String,
    ) -> Self {
        Self {
            client: Client::new(),
            api_key,
            api_url,
            model,
            label,
        }
    }

    pub async fn transcribe(&self, wav_bytes: Vec<u8>, language: Option<&str>) -> Result<String> {
        let file_part = multipart::Part::bytes(wav_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let mut form = multipart::Form::new()
            .part("file", file_part)
            .text("model", self.model.clone());

        if let Some(lang) = language {
            form = form.text("language", lang.to_string());
        }

        let response = self
            .client
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .with_context(|| format!("{} transcription API request failed", self.label))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "{} transcription API error {}: {}",
                self.label,
                status,
                body
            ));
        }

        let result: WhisperResponse = response
            .json()
            .await
            .context("failed to parse Whisper API response")?;

        Ok(result.text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Groq reuses this provider, so its errors used to say "OpenAI" — which is
    /// exactly what made a Groq 401 look like an OpenAI bug in the logs. The
    /// label must name the real provider in the failure message.
    #[tokio::test]
    async fn error_message_uses_provider_label() {
        // Unroutable address → request fails fast; we only assert the label,
        // not network success.
        let provider = OpenAIProvider::with_label(
            "Groq",
            "gsk_fake".into(),
            "http://127.0.0.1:1/v1/audio/transcriptions".into(),
            "whisper-large-v3-turbo".into(),
        );
        let err = provider
            .transcribe(vec![0u8; 16], None)
            .await
            .expect_err("request to port 1 must fail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("Groq"),
            "error should name the Groq provider, got: {msg}"
        );
        assert!(
            !msg.contains("OpenAI"),
            "Groq error must not mention OpenAI, got: {msg}"
        );
    }
}
