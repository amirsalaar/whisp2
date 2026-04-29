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
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
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
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        let text = resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Gemini response missing text field"))?
            .trim()
            .to_string();

        Ok(text)
    }
}
