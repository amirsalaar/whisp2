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

/// A downloadable Parakeet model. Unlike Whisper's single `.bin`, a Parakeet
/// model is a set of ONNX files that must live together in one directory.
struct ParakeetModelInfo {
    /// Display name and the subdirectory (under `models/`) the files land in.
    name: &'static str,
    dir_name: &'static str,
    size_mb: u32,
    description: &'static str,
    /// HuggingFace repo the files are pulled from (resolve/main/<file>).
    hf_repo: &'static str,
    /// The files to download. `from_pretrained` auto-selects the int8 weights,
    /// so we ship only those to keep the download small (~670 MB vs ~2.5 GB).
    files: &'static [&'static str],
}

fn parakeet_catalog() -> Vec<ParakeetModelInfo> {
    vec![ParakeetModelInfo {
        name: "parakeet-tdt-0.6b-v3",
        dir_name: "parakeet-tdt-0.6b-v3",
        size_mb: 685,
        description: "NVIDIA Parakeet, 25 languages, quantized (int8)",
        hf_repo: "istupakov/parakeet-tdt-0.6b-v3-onnx",
        files: &[
            "encoder-model.int8.onnx",
            "decoder_joint-model.int8.onnx",
            "vocab.txt",
        ],
    }]
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

#[tauri::command]
pub fn list_parakeet_models() -> Vec<ModelInfo> {
    parakeet_catalog()
        .into_iter()
        // `ModelInfo` is shared with the Whisper catalog, where `filename` is a
        // single `.bin`. For Parakeet there is no single file, so `filename`
        // carries the model *directory* name — which is exactly what gets stored
        // in `parakeet_model_path` and resolved by `resolve_model_path`.
        .map(|m| ModelInfo {
            name: m.name.to_string(),
            filename: m.dir_name.to_string(),
            size_mb: m.size_mb,
            description: m.description.to_string(),
        })
        .collect()
}

/// Names of Parakeet models whose weight directory holds every required file
/// (a partial/aborted download leaves some files missing, so it reads as "not
/// downloaded").
#[tauri::command]
pub fn get_downloaded_parakeet_models() -> Vec<String> {
    let Ok(dir) = models_dir() else {
        return vec![];
    };
    parakeet_catalog()
        .into_iter()
        .filter(|m| {
            let model_dir = dir.join(m.dir_name);
            m.files
                .iter()
                .all(|f| model_dir.join(f).exists())
        })
        .map(|m| m.name.to_string())
        .collect()
}

/// Whether the given stored Parakeet model dir name holds a complete set of
/// weight files. A bare directory (e.g. from an aborted download) is NOT
/// complete, so this must be used instead of a plain `path.exists()` check.
#[cfg(target_os = "macos")]
pub fn parakeet_model_is_complete(stored: &str) -> bool {
    let Ok(model_dir) = resolve_model_path(stored) else {
        return false;
    };
    // A stored name that matches a catalog entry uses that entry's file list;
    // otherwise fall back to requiring the two ONNX files + vocab.
    if let Some(info) = parakeet_catalog().iter().find(|m| m.dir_name == stored) {
        info.files.iter().all(|f| model_dir.join(f).exists())
    } else {
        model_dir.join("vocab.txt").exists()
            && std::fs::read_dir(&model_dir)
                .map(|rd| {
                    rd.filter_map(|e| e.ok()).any(|e| {
                        e.path()
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n.starts_with("encoder") && n.ends_with(".onnx"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
    }
}

/// Download all ONNX files for a Parakeet model into `models/<dir_name>/`.
/// Emits `model_download_progress` with aggregate byte counts across files.
/// Returns the directory name to store in `parakeet_model_path`.
#[tauri::command]
pub async fn download_parakeet_model(
    model_name: String,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let catalog = parakeet_catalog();
    let info = catalog
        .iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| format!("Unknown Parakeet model: {model_name}"))?;

    let model_dir = models_dir().map_err(|e| e.to_string())?.join(info.dir_name);
    std::fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;

    // Already complete?
    if info.files.iter().all(|f| model_dir.join(f).exists()) {
        return Ok(info.dir_name.to_string());
    }

    // Clean up any `.tmp` files orphaned by a previously killed download (a
    // graceful abort removes its own tmp, but a hard process kill leaves one
    // behind, and a stale tmp would never be reused since we rename atomically).
    for file in info.files {
        let tmp = model_dir.join(file).with_extension("tmp");
        if tmp.exists() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    let abort = Arc::clone(&state.download_abort);
    abort.store(false, Ordering::Relaxed);

    let client = reqwest::Client::new();

    // Fetch each file's size first so progress reflects the whole model, not
    // per-file resets.
    let mut file_sizes: Vec<u64> = Vec::with_capacity(info.files.len());
    for file in info.files {
        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            info.hf_repo, file
        );
        let resp = client
            .head(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}: {}", resp.status(), file));
        }
        file_sizes.push(resp.content_length().unwrap_or(0));
    }
    let total: u64 = file_sizes.iter().sum();

    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;
    const EMIT_EVERY: u64 = 2 * 1024 * 1024; // 2 MB

    for (i, file) in info.files.iter().enumerate() {
        let dest = model_dir.join(file);
        if dest.exists() {
            // Count against the HEAD-reported size (not on-disk bytes) so the
            // running total stays consistent with `total` and never exceeds it.
            downloaded += file_sizes[i];
            continue;
        }

        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            info.hf_repo, file
        );
        let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}: {}", resp.status(), file));
        }

        let dest_tmp = dest.with_extension("tmp");
        let mut out = tokio::fs::File::create(&dest_tmp)
            .await
            .map_err(|e| e.to_string())?;

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if abort.load(Ordering::Relaxed) {
                drop(out);
                let _ = tokio::fs::remove_file(&dest_tmp).await;
                return Err("Download aborted".into());
            }
            let chunk = chunk.map_err(|e| e.to_string())?;
            out.write_all(&chunk).await.map_err(|e| e.to_string())?;
            downloaded += chunk.len() as u64;

            if total > 0 && (downloaded - last_emit >= EMIT_EVERY || downloaded >= total) {
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

        out.flush().await.map_err(|e| e.to_string())?;
        drop(out);
        tokio::fs::rename(&dest_tmp, &dest)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(info.dir_name.to_string())
}
