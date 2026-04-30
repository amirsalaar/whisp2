use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, Mutex as TokioMutex};

use crate::audio::capture;
use crate::config::models::{AppConfig, RecordingMode};
use crate::hotkey::mode::{HotkeyEvent, RecordingCommand, RecordingState};
use crate::transcription::manager;

pub mod app_context;
pub mod audio;
pub mod commands;
pub mod config;
pub mod correction;
pub mod history;
pub mod hotkey;
pub mod hud;
pub mod injection;
pub mod keychain;
pub mod permissions;
pub mod transcription;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub db: sqlx::SqlitePool,
    /// Shared atomic holding the current CGEventTap device mask.
    /// Updating this at runtime changes the active hotkey without restarting.
    pub hotkey_mask: Arc<AtomicU64>,
    /// Cached local Whisper context: (loaded_model_path, context).
    /// Shared with manager::transcribe and commands::set_config.
    pub whisper_ctx: crate::transcription::providers::local_whisper::WhisperCtxCache,
    /// Set to true to abort an in-progress model download.
    pub download_abort: Arc<AtomicBool>,
}

/// Spawns all background async tasks. Called once inside Tauri's `setup` hook.
pub fn spawn_tasks(
    app_handle: tauri::AppHandle,
    state: Arc<AppState>,
    hotkey_rx: std::sync::mpsc::Receiver<HotkeyEvent>,
) {
    let rt = tokio::runtime::Handle::current();

    // Channels
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<RecordingCommand>(8);
    let (state_tx, mut state_rx) = mpsc::channel::<RecordingState>(8);
    // (text, source_app) — source_app used for app-aware injection delay
    let (text_tx, mut text_rx) = mpsc::channel::<(String, Option<String>)>(8);

    // Bridge the std::sync::mpsc receiver (from CGEventTap callback) into
    // a tokio channel so the hotkey_task can await events without blocking.
    let (async_hk_tx, mut async_hk_rx) = mpsc::channel::<HotkeyEvent>(64);
    std::thread::spawn(move || {
        while let Ok(event) = hotkey_rx.recv() {
            if async_hk_tx.blocking_send(event).is_err() {
                break;
            }
        }
    });

    // --- hotkey_task ---
    let cmd_tx_hk = cmd_tx.clone();
    let state_tx_hk = state_tx.clone();
    let state_hk = Arc::clone(&state);
    rt.spawn(async move {
        let mut current = RecordingState::Idle;

        loop {
            let event = match async_hk_rx.recv().await {
                Some(e) => e,
                None => break,
            };

            let mode = state_hk.config.read().unwrap().recording_mode.clone();

            let new_state = match (&current, &event) {
                (RecordingState::Idle, HotkeyEvent::KeyDown(bundle_id)) => {
                    let _ = cmd_tx_hk.send(RecordingCommand::Start(bundle_id.clone())).await;
                    RecordingState::Recording
                }
                (RecordingState::Recording, HotkeyEvent::KeyUp) => {
                    match mode {
                        RecordingMode::PressAndHold => {
                            let _ = cmd_tx_hk.send(RecordingCommand::Stop).await;
                            RecordingState::Processing
                        }
                        RecordingMode::Toggle => current.clone(), // KeyUp ignored in toggle mode
                    }
                }
                (RecordingState::Recording, HotkeyEvent::KeyDown(_)) => {
                    match mode {
                        RecordingMode::Toggle => {
                            let _ = cmd_tx_hk.send(RecordingCommand::Stop).await;
                            RecordingState::Processing
                        }
                        RecordingMode::PressAndHold => current.clone(),
                    }
                }
                _ => current.clone(),
            };

            if new_state != current {
                current = new_state.clone();
                let _ = state_tx_hk.send(current.clone()).await;
            }
        }
    });

    // --- audio_task ---
    let state_tx_audio = state_tx.clone();
    let text_tx_audio = text_tx.clone();
    let state_arc = Arc::clone(&state);
    rt.spawn(async move {
        let mut stop_tx: Option<mpsc::Sender<()>> = None;
        let mut pcm_rx: Option<mpsc::Receiver<Vec<f32>>> = None;
        let mut saved_vol: Option<f32> = None;
        let mut source_app: Option<String> = None;

        loop {
            let cmd = cmd_rx.recv().await;
            match cmd {
                Some(RecordingCommand::Start(bundle_id)) => {
                    source_app = bundle_id;
                    let input_device = state_arc.config.read().unwrap().input_device.clone();
                    match capture::start_recording(input_device) {
                        Ok((tx, rx)) => {
                            stop_tx = Some(tx);
                            pcm_rx = Some(rx);
                            saved_vol = audio::volume::boost();
                            tracing::info!("recording started");
                        }
                        Err(e) => {
                            tracing::error!("failed to start recording: {}", e);
                            let _ = state_tx_audio.send(RecordingState::Idle).await;
                        }
                    }
                }
                Some(RecordingCommand::Stop) => {
                    if let Some(tx) = stop_tx.take() {
                        drop(tx); // signal stop
                    }
                    if let Some(vol) = saved_vol.take() {
                        audio::volume::restore(vol);
                    }
                    if let Some(mut rx) = pcm_rx.take() {
                        let config = state_arc.config.read().unwrap().clone();
                        let db = state_arc.db.clone();
                        let text_tx = text_tx_audio.clone();
                        let state_tx = state_tx_audio.clone();
                        let app_id = source_app.take();
                        let whisper_ctx = Arc::clone(&state_arc.whisper_ctx);

                        tokio::spawn(async move {
                            let samples = rx.recv().await;
                            match samples {
                                Some(s) if !s.is_empty() => {
                                    match capture::encode_wav(&s) {
                                        Ok(wav) => {
                                            match manager::transcribe(&config, wav, whisper_ctx).await {
                                                Ok(text) => {
                                                    tracing::info!("transcribed: {}", text);
                                                    let text = crate::correction::dictionary::apply(text);
                                                    // Save to history with captured source app
                                                    if config.save_history {
                                                        let provider_name = format!("{:?}", config.provider);
                                                        if let Err(e) = crate::history::store::insert(
                                                            &db,
                                                            &text,
                                                            app_id.as_deref(),
                                                            &provider_name,
                                                        )
                                                        .await
                                                        {
                                                            tracing::warn!("history insert failed: {}", e);
                                                        } else if let Some(max) = config.max_history_entries {
                                                            if let Err(e) = crate::history::store::prune(&db, max).await {
                                                                tracing::warn!("history prune failed: {}", e);
                                                            }
                                                        }
                                                    }
                                                    let _ = text_tx.send((text, app_id)).await;
                                                    let _ = state_tx.send(RecordingState::Idle).await;
                                                }
                                                Err(e) => {
                                                    tracing::error!("transcription failed: {}", e);
                                                    let _ = state_tx.send(RecordingState::Error(e.to_string())).await;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("WAV encode failed: {}", e);
                                            let _ = state_tx.send(RecordingState::Error(e.to_string())).await;
                                        }
                                    }
                                }
                                _ => {
                                    tracing::warn!("no audio captured");
                                    let _ = state_tx.send(RecordingState::Idle).await;
                                }
                            }
                        });
                    }
                }
                Some(RecordingCommand::Cancel) | None => {
                    stop_tx.take();
                    pcm_rx.take();
                    source_app.take();
                }
            }
        }
    });

    // --- hud_task ---
    let ah_hud = app_handle.clone();
    let state_hud = Arc::clone(&state);
    let state_tx_hud = state_tx.clone();
    rt.spawn(async move {
        loop {
            match state_rx.recv().await {
                Some(s) => {
                    let show_hud = state_hud.config.read().unwrap().show_hud;
                    match &s {
                        RecordingState::Error(msg) => {
                            // Show error in HUD for 2s then auto-dismiss to Idle.
                            let label = format!("Error: {}", msg);
                            if show_hud {
                                let _ = ah_hud.run_on_main_thread({
                                    let label = label.clone();
                                    move || hud::panel::show(&label)
                                });
                            }
                            let state_tx = state_tx_hud.clone();
                            let ah = ah_hud.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                let _ = state_tx.send(RecordingState::Idle).await;
                                let _ = ah.run_on_main_thread(|| hud::panel::hide());
                            });
                        }
                        other => {
                            let label = match other {
                                RecordingState::Idle => "".to_string(),
                                RecordingState::Recording => "Recording...".to_string(),
                                RecordingState::Processing => "Processing...".to_string(),
                                RecordingState::Error(_) => unreachable!(),
                            };
                            let _ = ah_hud.run_on_main_thread(move || {
                                if label.is_empty() {
                                    hud::panel::hide();
                                } else if show_hud {
                                    hud::panel::show(&label);
                                }
                            });
                        }
                    }
                    update_tray_icon(&ah_hud, &s);
                }
                None => break,
            }
        }
    });

    // --- injection_task ---
    let ah_inj = app_handle.clone();
    let state_inj = Arc::clone(&state);
    rt.spawn(async move {
        loop {
            match text_rx.recv().await {
                Some((text, source_app)) => {
                    let play_sound = state_inj.config.read().unwrap().play_completion_sound;
                    let _ = ah_inj.run_on_main_thread(move || {
                        if let Err(e) = injection::text::type_text(&text, source_app.as_deref()) {
                            tracing::error!("text injection failed: {}", e);
                        } else if play_sound {
                            std::thread::spawn(|| audio::sound::play());
                        }
                    });
                }
                None => break,
            }
        }
    });
}

fn update_tray_icon(app: &tauri::AppHandle, state: &RecordingState) {
    let tooltip = match state {
        RecordingState::Idle => "Whisp",
        RecordingState::Recording => "Whisp — Recording",
        RecordingState::Processing => "Whisp — Processing",
        RecordingState::Error(_) => "Whisp — Error",
    };

    // 22x22 RGBA icon: different fill color per state
    // Idle: grey, Recording: red, Processing: yellow, Error: orange
    let (fg_r, fg_g, fg_b) = match state {
        RecordingState::Idle => (200u8, 200u8, 200u8),
        RecordingState::Recording => (230u8, 50u8, 50u8),
        RecordingState::Processing => (230u8, 180u8, 50u8),
        RecordingState::Error(_) => (230u8, 100u8, 30u8),
    };

    let size: u32 = 22;
    let cx = size as f32 / 2.0;
    let radius = (size as f32 / 2.0) - 1.5;
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx + 0.5;
            let dy = y as f32 - cx + 0.5;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * size + x) * 4) as usize;
            if dist <= radius {
                pixels[idx] = fg_r;
                pixels[idx + 1] = fg_g;
                pixels[idx + 2] = fg_b;
                pixels[idx + 3] = 220;
            } else {
                pixels[idx + 3] = 0; // transparent
            }
        }
    }

    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tooltip));
        let img = tauri::image::Image::new_owned(pixels, size, size);
        let _ = tray.set_icon(Some(img));
    }
}
