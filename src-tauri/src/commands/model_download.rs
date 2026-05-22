use std::sync::atomic::Ordering;
use std::sync::Arc;

use futures_util::StreamExt;
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

use crate::config::persistence;
use crate::AppState;

#[derive(serde::Serialize, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub filename: String,
    pub size_mb: u32,
    pub description: String,
}

fn model_catalog() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            name: "tiny.en".into(),
            filename: "ggml-tiny.en.bin".into(),
            size_mb: 75,
            description: "Fastest, English only".into(),
        },
        ModelInfo {
            name: "base.en".into(),
            filename: "ggml-base.en.bin".into(),
            size_mb: 142,
            description: "Recommended, English only".into(),
        },
        ModelInfo {
            name: "small.en".into(),
            filename: "ggml-small.en.bin".into(),
            size_mb: 466,
            description: "More accurate, English only".into(),
        },
        ModelInfo {
            name: "base".into(),
            filename: "ggml-base.bin".into(),
            size_mb: 142,
            description: "Multilingual base".into(),
        },
        ModelInfo {
            name: "large-v3-turbo-q5_0".into(),
            filename: "ggml-large-v3-turbo-q5_0.bin".into(),
            size_mb: 547,
            description: "High accuracy, quantized".into(),
        },
        ModelInfo {
            name: "large-v3-turbo".into(),
            filename: "ggml-large-v3-turbo.bin".into(),
            size_mb: 1600,
            description: "Best accuracy".into(),
        },
    ]
}

fn models_dir() -> anyhow::Result<std::path::PathBuf> {
    let dir = persistence::app_support_dir()?.join("models");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Resolve a stored model path to an absolute path on disk.
///
/// We persist just the filename (e.g., `"ggml-tiny.bin"`) so it survives the
/// iOS data-container UUID rotating on each Xcode reinstall. Legacy absolute
/// paths still resolve to themselves for back-compat.
pub fn resolve_model_path(stored: &str) -> anyhow::Result<std::path::PathBuf> {
    let p = std::path::PathBuf::from(stored);
    if p.is_absolute() {
        return Ok(p);
    }
    Ok(models_dir()?.join(stored))
}

/// Return the filename of the first `.bin` model found in `models_dir()`,
/// sorted alphabetically. Used on iOS to auto-pick a model after the data
/// container UUID rotates and the saved absolute path is stale.
pub fn scan_first_model_on_disk() -> anyhow::Result<Option<String>> {
    let dir = models_dir()?;
    let mut names: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("bin"))
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect();
    names.sort();
    Ok(names.into_iter().next())
}

#[derive(serde::Serialize, Clone)]
struct DownloadProgress {
    model_name: String,
    downloaded: u64,
    total: u64,
}

#[tauri::command]
pub fn list_whisper_models() -> Vec<ModelInfo> {
    model_catalog()
}

#[tauri::command]
pub fn get_models_dir() -> Result<String, String> {
    models_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_downloaded_models() -> Vec<String> {
    let Ok(dir) = models_dir() else {
        return vec![];
    };
    model_catalog()
        .into_iter()
        .filter(|m| dir.join(&m.filename).exists())
        .map(|m| m.name)
        .collect()
}

#[tauri::command]
pub async fn download_whisper_model(
    model_name: String,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let catalog = model_catalog();
    let info = catalog
        .iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| format!("Unknown model: {model_name}"))?;

    let dir = models_dir().map_err(|e| e.to_string())?;
    let dest = dir.join(&info.filename);

    if dest.exists() {
        return Ok(info.filename.clone());
    }

    // Reset abort flag for this download
    let abort = Arc::clone(&state.download_abort);
    abort.store(false, Ordering::Relaxed);

    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
        info.filename
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}: {}", resp.status(), info.filename));
    }

    let total = resp.content_length().unwrap_or(0);
    let dest_tmp = dest.with_extension("tmp");

    let mut file = tokio::fs::File::create(&dest_tmp)
        .await
        .map_err(|e| e.to_string())?;

    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;
    const EMIT_EVERY: u64 = 2 * 1024 * 1024; // 2 MB

    while let Some(chunk) = stream.next().await {
        if abort.load(Ordering::Relaxed) {
            drop(file);
            let _ = tokio::fs::remove_file(&dest_tmp).await;
            return Err("Download aborted".into());
        }
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;

        if total > 0 && (downloaded - last_emit >= EMIT_EVERY || downloaded == total) {
            last_emit = downloaded;
            let _ = app_handle.emit(
                "model_download_progress",
                DownloadProgress {
                    model_name: model_name.clone(),
                    downloaded,
                    total,
                },
            );
        }
    }

    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    tokio::fs::rename(&dest_tmp, &dest)
        .await
        .map_err(|e| e.to_string())?;

    Ok(info.filename.clone())
}

#[tauri::command]
pub fn abort_model_download(state: State<'_, AppState>) {
    state.download_abort.store(true, Ordering::Relaxed);
}
