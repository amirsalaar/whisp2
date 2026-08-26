use anyhow::Result;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

/// Total request-size ceiling for inline audio on the Gemini API — 20 MB covering
/// the prompt and every attached file. Whisp captures 16 kHz mono 16-bit WAV
/// (32 KB/s), which base64 inflates to ~42.7 KB/s, so this trips at roughly eight
/// minutes of speech. Uploading via the Files API instead would buy more headroom
/// at the cost of a second round trip on every single dictation, which is the
/// wrong trade for clips that are normally seconds long.
const MAX_INLINE_B64_BYTES: usize = 20 * 1024 * 1024;

pub struct GeminiProvider {
    api_key: String,
    model: String,
}

impl GeminiProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self { api_key, model }
    }

    pub async fn transcribe(&self, wav_bytes: Vec<u8>, language: Option<&str>) -> Result<String> {
        if is_live_model(&self.model) {
            anyhow::bail!(
                "{} is a streaming model that only works over the Gemini Live API, which Whisp \
                 does not speak. Choose gemini-3.5-transcribe in Settings instead.",
                self.model
            );
        }

        let audio_b64 = B64.encode(&wav_bytes);
        if audio_b64.len() > MAX_INLINE_B64_BYTES {
            anyhow::bail!(
                "Recording is too long to send to Gemini in one request ({:.1} MB encoded, limit \
                 is 20 MB — about 8 minutes). Record a shorter clip, or switch to a local provider.",
                audio_b64.len() as f64 / (1024.0 * 1024.0)
            );
        }

        if uses_transcription_api(&self.model) {
            self.via_interactions(audio_b64, language).await
        } else {
            self.via_generate_content(audio_b64, language).await
        }
    }

    /// The dedicated `*-transcribe` speech-to-text models live only on the
    /// Interactions API, and they take their settings in a `transcription_config`
    /// block rather than a natural-language prompt.
    async fn via_interactions(&self, audio_b64: String, language: Option<&str>) -> Result<String> {
        let resp = reqwest::Client::new()
            .post("https://generativelanguage.googleapis.com/v1beta/interactions")
            // Use the x-goog-api-key header (not ?key= in the URL) so the key
            // never lands in URL-bearing logs, crash reports, or proxy traces.
            .header("x-goog-api-key", &self.api_key)
            .json(&interactions_body(&self.model, audio_b64, language))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Gemini API error {}: {}", status, body));
        }

        transcript_from_interaction(&resp.json().await?)
    }

    /// The general-purpose multimodal models (flash, pro) have no transcription
    /// config — you ask them to transcribe in the prompt.
    async fn via_generate_content(
        &self,
        audio_b64: String,
        language: Option<&str>,
    ) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );

        let prompt = match language {
            Some(lang) => format!(
                "Transcribe the audio accurately in {}. Return only the transcription text, no commentary.",
                lang
            ),
            None => "Transcribe the audio accurately. Return only the transcription text, no commentary."
                .to_string(),
        };

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

        let resp = reqwest::Client::new()
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

/// `gemini-3.5-transcribe` and friends are speech-to-text models served from
/// `/v1beta/interactions`; they reject `:generateContent`.
fn uses_transcription_api(model: &str) -> bool {
    model.contains("-transcribe")
}

/// The `-live` variants are WebSocket-only (Gemini Live API), so they can't be
/// driven by a one-shot HTTP POST no matter which endpoint we pick.
fn is_live_model(model: &str) -> bool {
    model.ends_with("-live")
}

fn interactions_body(model: &str, audio_b64: String, language: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "input": [{
            "type": "audio",
            "data": audio_b64,
            "mime_type": "audio/wav"
        }]
    });

    // Omitting language_codes entirely is what enables auto-detection across the
    // model's 85+ locales (including mid-sentence code-switching), so only send
    // the block when the user actually pinned a language. The field wants BCP-47,
    // and the bare ISO-639-1 codes the Settings field collects ("en", "fa") are
    // valid BCP-47 primary subtags, so pass the string through untouched.
    if let Some(lang) = language {
        body["generation_config"] = serde_json::json!({
            "transcription_config": { "language_codes": [lang] }
        });
    }

    body
}

/// Pull the transcript out of an Interactions response.
///
/// There is deliberately no `output_text` read here: that field is a convenience
/// synthesized by Google's own SDKs and is absent from the REST payload. The text
/// has to be gathered from the `model_output` step's text content parts, which is
/// also what the SDKs concatenate.
fn transcript_from_interaction(resp: &serde_json::Value) -> Result<String> {
    if let Some(err) = resp["errors"].as_array().and_then(|e| e.first()) {
        anyhow::bail!("Gemini transcription failed: {}", err);
    }

    let steps = resp["steps"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Gemini response has no steps array"))?;

    let text: String = steps
        .iter()
        .filter(|step| step["type"] == "model_output")
        .filter_map(|step| step["content"].as_array())
        .flatten()
        .filter(|part| part["type"] == "text")
        .filter_map(|part| part["text"].as_str())
        .collect();

    if text.trim().is_empty() {
        anyhow::bail!(
            "Gemini returned no transcript (status: {})",
            resp["status"].as_str().unwrap_or("unknown")
        );
    }

    Ok(text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcribe_models_route_to_the_interactions_api() {
        assert!(uses_transcription_api("gemini-3.5-transcribe"));
        assert!(uses_transcription_api("gemini-3.5-transcribe-live"));
    }

    #[test]
    fn chat_models_stay_on_generate_content() {
        assert!(!uses_transcription_api("gemini-3.7-flash"));
        assert!(!uses_transcription_api("gemini-3.5-flash-lite"));
    }

    #[test]
    fn only_the_live_variants_are_rejected() {
        assert!(is_live_model("gemini-3.5-transcribe-live"));
        assert!(!is_live_model("gemini-3.5-transcribe"));
    }

    #[test]
    fn body_omits_language_codes_when_auto_detecting() {
        let body = interactions_body("gemini-3.5-transcribe", "AAAA".into(), None);

        assert_eq!(body["model"], "gemini-3.5-transcribe");
        assert_eq!(body["input"][0]["type"], "audio");
        assert_eq!(body["input"][0]["data"], "AAAA");
        assert_eq!(body["input"][0]["mime_type"], "audio/wav");
        assert!(
            body.get("generation_config").is_none(),
            "an empty language must leave auto-detection on, not pin a locale"
        );
    }

    #[test]
    fn body_pins_the_language_the_user_chose() {
        let body = interactions_body("gemini-3.5-transcribe", "AAAA".into(), Some("fa"));

        assert_eq!(
            body["generation_config"]["transcription_config"]["language_codes"],
            serde_json::json!(["fa"])
        );
    }

    #[test]
    fn transcript_comes_from_the_model_output_step() {
        // Shaped after the documented Interaction resource: no `output_text` on the
        // wire, and the user's own turn is echoed back as a `user_input` step that
        // must not leak into the transcript.
        let resp = serde_json::json!({
            "id": "v1_abc",
            "object": "interaction",
            "status": "completed",
            "model": "gemini-3.5-transcribe",
            "steps": [
                {"type": "user_input", "content": [{"type": "text", "text": "NOT THE TRANSCRIPT"}]},
                {"type": "model_output", "content": [{"type": "text", "text": "  hello world  "}]}
            ]
        });

        assert_eq!(transcript_from_interaction(&resp).unwrap(), "hello world");
    }

    #[test]
    fn transcript_joins_multiple_text_parts() {
        let resp = serde_json::json!({
            "status": "completed",
            "steps": [{
                "type": "model_output",
                "content": [
                    {"type": "text", "text": "first half "},
                    {"type": "text", "text": "second half"}
                ]
            }]
        });

        assert_eq!(
            transcript_from_interaction(&resp).unwrap(),
            "first half second half"
        );
    }

    #[test]
    fn word_annotations_do_not_become_the_transcript() {
        let resp = serde_json::json!({
            "status": "completed",
            "steps": [{
                "type": "model_output",
                "content": [
                    {"type": "text", "text": "hi", "annotations": [
                        {"type": "word_info", "text": "hi", "start_offset": "0.100s"}
                    ]},
                    {"type": "audio", "data": "ignored"}
                ]
            }]
        });

        assert_eq!(transcript_from_interaction(&resp).unwrap(), "hi");
    }

    #[test]
    fn reported_errors_surface_instead_of_an_empty_transcript() {
        let resp = serde_json::json!({
            "status": "failed",
            "errors": [{"message": "audio too long"}],
            "steps": []
        });

        let err = transcript_from_interaction(&resp).unwrap_err().to_string();
        assert!(err.contains("audio too long"), "unexpected error: {err}");
    }

    #[test]
    fn an_empty_transcript_is_an_error_not_an_empty_string() {
        let resp = serde_json::json!({
            "status": "requires_action",
            "steps": [{"type": "model_output", "content": [{"type": "text", "text": "   "}]}]
        });

        let err = transcript_from_interaction(&resp).unwrap_err().to_string();
        assert!(
            err.contains("requires_action"),
            "the status is the only clue the user gets: {err}"
        );
    }

    #[test]
    fn a_malformed_response_is_an_error() {
        let resp = serde_json::json!({"status": "completed"});

        assert!(transcript_from_interaction(&resp).is_err());
    }
}
