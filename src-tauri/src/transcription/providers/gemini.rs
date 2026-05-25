use anyhow::Result;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

pub struct GeminiProvider {
    api_key: String,
    model: String,
}

impl GeminiProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self { api_key, model }
    }

    pub async fn transcribe(&self, wav_bytes: Vec<u8>, language: Option<&str>) -> Result<String> {
        let client = reqwest::Client::new();
        // Use the x-goog-api-key header (not ?key= in the URL) so the key
        // never lands in URL-bearing logs, crash reports, or proxy traces.
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );

        let audio_b64 = B64.encode(&wav_bytes);

        let mut prompt = "Transcribe the audio accurately. Return only the transcription text, no commentary.".to_string();
        if let Some(lang) = language {
            prompt = format!("Transcribe the audio accurately in {}. Return only the transcription text, no commentary.", lang);
        }

        let body = serde_json::json!({
            "contents": [{
                "parts": [
                    {"text": prompt},
                    {
                        "inline_data": {
                            "mime_type": "audio/wav",
                            "data": audio_b64
                        }
                    }
                ]
            }]
        });

        let resp = client
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Gemini API error {}: {}", status, body));
        }

        let resp: serde_json::Value = resp.json().await?;

        let text = resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Gemini response missing text field"))?
            .trim()
            .to_string();

        Ok(text)
    }
}
