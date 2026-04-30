use std::io::Cursor;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Shared cache: (loaded_model_path, context).
/// Wrapped in Arc<Mutex<...>> so it can be shared across async tasks and AppState.
/// WhisperContext is stored behind an Arc so it can be cloned out of the lock before
/// passing into `spawn_blocking` without holding the lock across the blocking call.
pub type WhisperCtxCache = Arc<Mutex<(Option<String>, Option<Arc<WhisperContext>>)>>;

pub struct LocalWhisperProvider {
    pub model_path: String,
    pub ctx_cache: WhisperCtxCache,
    pub language: Option<String>,
}

impl LocalWhisperProvider {
    pub async fn transcribe(&self, wav_bytes: Vec<u8>) -> Result<String> {
        // 1. Decode WAV (16-bit PCM, 16 kHz, mono) → f32 samples.
        //    encode_wav scales f32 → i16, so we reverse: i16 / i16::MAX → f32.
        let samples: Vec<f32> = {
            let cursor = Cursor::new(&wav_bytes);
            let mut reader = hound::WavReader::new(cursor)?;
            reader
                .samples::<i16>()
                .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
                .collect::<std::result::Result<Vec<f32>, _>>()?
        };

        // 2. Ensure context is loaded for the current model path.
        //    Clone Arc<WhisperContext> out of the lock so the lock is not held
        //    across spawn_blocking (which would block the async runtime).
        let ctx: Arc<WhisperContext> = {
            let mut guard = self.ctx_cache.lock().await;
            if guard.0.as_deref() != Some(&self.model_path) {
                let path = self.model_path.clone();
                let new_ctx = tokio::task::spawn_blocking(move || {
                    WhisperContext::new_with_params(&path, WhisperContextParameters::default())
                })
                .await?
                .map_err(|e| anyhow::anyhow!("failed to load Whisper model: {e}"))?;
                *guard = (Some(self.model_path.clone()), Some(Arc::new(new_ctx)));
                tracing::info!("Whisper model loaded: {}", self.model_path);
            }
            Arc::clone(guard.1.as_ref().unwrap())
        };

        // 3. Run inference inside spawn_blocking.
        //    WhisperState is !Send, so it must be created and consumed in the same closure.
        let language = self.language.clone();
        let text = tokio::task::spawn_blocking(move || -> Result<String> {
            let mut state = ctx
                .create_state()
                .map_err(|e| anyhow::anyhow!("failed to create Whisper state: {e}"))?;

            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            if let Some(lang) = language.as_deref() {
                params.set_language(Some(lang));
            }

            state
                .full(params, &samples)
                .map_err(|e| anyhow::anyhow!("Whisper inference failed: {e}"))?;

            // Collect segment text. full_n_segments() returns c_int directly (no Result).
            let n = state.full_n_segments();
            let mut parts = Vec::with_capacity(n as usize);
            for i in 0..n {
                if let Some(seg) = state.get_segment(i) {
                    let text = seg
                        .to_str()
                        .map_err(|e| anyhow::anyhow!("failed to get segment {i} text: {e}"))?;
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                }
            }

            Ok(parts.join(" "))
        })
        .await??;

        Ok(text)
    }
}
