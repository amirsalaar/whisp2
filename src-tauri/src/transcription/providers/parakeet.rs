use std::io::Cursor;
use std::sync::{Arc, Mutex as StdMutex};

use anyhow::Result;
use parakeet_rs::{ParakeetTDT, TimestampMode, Transcriber};
use tokio::sync::Mutex;

/// Shared cache: (loaded_model_dir, model).
///
/// Mirrors `WhisperCtxCache`. The outer `tokio::sync::Mutex` guards the cache
/// slot across async tasks; the inner `std::sync::Mutex` is required because
/// `ParakeetTDT::transcribe_samples` takes `&mut self` (it mutates an internal
/// feature cache), so a shared model must be locked for the duration of a call.
/// `ParakeetTDT` is `Send` (its `ort::Session`s and FFT plan are `Send + Sync`),
/// so the `Arc` can be cloned out of the lock and moved into `spawn_blocking`.
pub type ParakeetCtxCache = Arc<Mutex<(Option<String>, Option<Arc<StdMutex<ParakeetTDT>>>)>>;

pub struct ParakeetProvider {
    /// Absolute path to the directory holding the ONNX weights + `vocab.txt`.
    pub model_dir: String,
    pub ctx_cache: ParakeetCtxCache,
}

impl ParakeetProvider {
    pub async fn transcribe(&self, wav_bytes: Vec<u8>) -> Result<String> {
        // 1. Decode WAV (16-bit PCM, 16 kHz, mono) → f32 samples.
        //    encode_wav scales f32 → i16, so reverse: i16 / i16::MAX → f32.
        let samples: Vec<f32> = {
            let cursor = Cursor::new(&wav_bytes);
            let mut reader = hound::WavReader::new(cursor)?;
            reader
                .samples::<i16>()
                .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
                .collect::<std::result::Result<Vec<f32>, _>>()?
        };

        // 2. Ensure the model is loaded for the current directory. Clone the
        //    Arc out of the lock so the async lock is not held across the
        //    blocking inference call.
        let model: Arc<StdMutex<ParakeetTDT>> = {
            let mut guard = self.ctx_cache.lock().await;
            if guard.0.as_deref() != Some(&self.model_dir) {
                let dir = self.model_dir.clone();
                let loaded =
                    tokio::task::spawn_blocking(move || ParakeetTDT::from_pretrained(&dir, None))
                        .await?
                        .map_err(|e| anyhow::anyhow!("failed to load Parakeet model: {e}"))?;
                *guard = (
                    Some(self.model_dir.clone()),
                    Some(Arc::new(StdMutex::new(loaded))),
                );
                tracing::info!("Parakeet model loaded: {}", self.model_dir);
            }
            Arc::clone(guard.1.as_ref().unwrap())
        };

        // 3. Run inference inside spawn_blocking. The whisp capture pipeline
        //    always produces 16 kHz mono, matching Parakeet's expected input.
        let text = tokio::task::spawn_blocking(move || -> Result<String> {
            let mut model = model
                .lock()
                .map_err(|_| anyhow::anyhow!("Parakeet model lock poisoned"))?;
            let result = model
                .transcribe_samples(samples, 16_000, 1, Some(TimestampMode::Sentences))
                .map_err(|e| anyhow::anyhow!("Parakeet transcription failed: {e}"))?;
            Ok(result.text.trim().to_string())
        })
        .await??;

        Ok(text)
    }
}
