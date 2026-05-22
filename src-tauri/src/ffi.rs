//! C ABI exports for Swift callers (iOS AppIntent).
//!
//! The iOS AppIntent (`WhispIntent.swift`) runs in the host app's process
//! because `openAppWhenRun: true` is set, which means it can call symbols
//! statically linked into `whisp_rs_lib.a` via `@_silgen_name`. This module
//! exposes a synchronous local-Whisper transcription entry point so the
//! Action Button can run on-device inference instead of bailing out to a
//! cloud provider.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

use crate::transcription::providers::local_whisper::{LocalWhisperProvider, WhisperCtxCache};

/// Process-wide WhisperContext cache. The model is mmap'd once on the first
/// call and reused for every subsequent invocation in the same host-app
/// process.
fn ctx_cache() -> WhisperCtxCache {
    static CACHE: OnceLock<WhisperCtxCache> = OnceLock::new();
    CACHE
        .get_or_init(|| Arc::new(Mutex::new((None, None))))
        .clone()
}

/// Pre-warm the local Whisper model so the first user-visible AppIntent
/// invocation doesn't pay the ~1s mmap + Metal warmup cost. Safe to call
/// from any Tokio runtime; populates the same process-wide cache used by
/// `whisp_transcribe_local_wav`.
pub async fn warm_local_whisper(model_path: String, language: Option<String>) -> anyhow::Result<()> {
    let model_path = crate::commands::model_download::resolve_model_path(&model_path)?
        .to_string_lossy()
        .into_owned();
    let provider = LocalWhisperProvider {
        model_path,
        ctx_cache: ctx_cache(),
        language,
    };
    provider.ensure_loaded().await
}

/// Transcribes a 16 kHz mono 16-bit PCM WAV file with the local Whisper model
/// at `model_path`.
///
/// Returns a heap-allocated UTF-8 C string on success — the caller must free
/// it with `whisp_free_string`. On failure returns null and, when `err_out`
/// is non-null, writes a heap-allocated error message into `*err_out` that
/// the caller must also free with `whisp_free_string`.
///
/// `language` may be null; when non-null it is passed straight to whisper.cpp
/// (e.g. "en", "fr").
///
/// # Safety
/// All pointer arguments must point to valid NUL-terminated UTF-8 strings,
/// or be null where the signature allows. `err_out` (if non-null) must point
/// to a writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn whisp_transcribe_local_wav(
    wav_path: *const c_char,
    model_path: *const c_char,
    language: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        if wav_path.is_null() || model_path.is_null() {
            return Err("wav_path and model_path must not be null".to_string());
        }
        let wav_path = CStr::from_ptr(wav_path)
            .to_str()
            .map_err(|e| format!("wav_path utf-8: {e}"))?
            .to_string();
        let model_path_in = CStr::from_ptr(model_path)
            .to_str()
            .map_err(|e| format!("model_path utf-8: {e}"))?
            .to_string();
        let model_path = crate::commands::model_download::resolve_model_path(&model_path_in)
            .map_err(|e| format!("resolve model_path {model_path_in}: {e}"))?
            .to_string_lossy()
            .into_owned();
        let language = if language.is_null() {
            None
        } else {
            Some(
                CStr::from_ptr(language)
                    .to_str()
                    .map_err(|e| format!("language utf-8: {e}"))?
                    .to_string(),
            )
        };

        let wav_bytes =
            std::fs::read(&wav_path).map_err(|e| format!("read {wav_path}: {e}"))?;

        let provider = LocalWhisperProvider {
            model_path,
            ctx_cache: ctx_cache(),
            language,
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("tokio runtime: {e}"))?;
        rt.block_on(provider.transcribe(wav_bytes))
            .map_err(|e| format!("transcribe: {e}"))
    })();

    match result {
        Ok(text) => CString::new(text)
            .map(|c| c.into_raw())
            .unwrap_or(ptr::null_mut()),
        Err(msg) => {
            if !err_out.is_null() {
                if let Ok(c) = CString::new(msg) {
                    *err_out = c.into_raw();
                }
            }
            ptr::null_mut()
        }
    }
}

/// Frees a C string previously returned by `whisp_transcribe_local_wav`
/// (either the result or the err_out pointer).
///
/// # Safety
/// `s` must either be null or have been returned by a prior
/// `whisp_transcribe_local_wav` call and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn whisp_free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}
