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
}

impl OpenAIProvider {
    pub fn new(api_key: String, api_url: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            api_url,
            model,
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
            .context("OpenAI Whisper API request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "OpenAI Whisper API error {}: {}",
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
