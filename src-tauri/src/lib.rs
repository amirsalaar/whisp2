use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

use crate::audio::capture;
use crate::config::models::AppConfig;
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
    let (text_tx, mut text_rx) = mpsc::channel::<String>(8);

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
    let ah_hk = app_handle.clone();
    rt.spawn(async move {
        let mut current = RecordingState::Idle;

        loop {
            let event = match async_hk_rx.recv().await {
                Some(e) => e,
                None => break,
            };

            let new_state = match (current, event) {
                (RecordingState::Idle, HotkeyEvent::KeyDown) => {
                    let _ = cmd_tx_hk.send(RecordingCommand::Start).await;
                    RecordingState::Recording
                }
                (RecordingState::Recording, HotkeyEvent::KeyUp) => {
                    let _ = cmd_tx_hk.send(RecordingCommand::Stop).await;
                    RecordingState::Processing
                }
                _ => current,
            };

            if new_state != current {
                current = new_state;
                let _ = state_tx_hk.send(current).await;
                update_tray_icon(&ah_hk, current);
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

        loop {
            let cmd = cmd_rx.recv().await;
            match cmd {
                Some(RecordingCommand::Start) => {
                    match capture::start_recording() {
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

                        tokio::spawn(async move {
                            let samples = rx.recv().await;
                            match samples {
                                Some(s) if !s.is_empty() => {
                                    match capture::encode_wav(&s) {
                                        Ok(wav) => {
                                            match manager::transcribe(&config, wav).await {
                                                Ok(text) => {
                                                    tracing::info!("transcribed: {}", text);
                                                    let text = crate::correction::dictionary::apply(text);
                                                    // Save to history
                                                    if config.save_history {
                                                        let provider_name = format!("{:?}", config.provider);
                                                        if let Err(e) = crate::history::store::insert(
                                                            &db,
                                                            &text,
                                                            None,
                                                            &provider_name,
                                                        )
                                                        .await
                                                        {
                                                            tracing::warn!("history insert failed: {}", e);
                                                        }
                                                    }
                                                    let _ = text_tx.send(text).await;
                                                }
                                                Err(e) => {
                                                    tracing::error!("transcription failed: {}", e);
                                                }
                                            }
                                        }
                                        Err(e) => tracing::error!("WAV encode failed: {}", e),
                                    }
                                }
                                _ => tracing::warn!("no audio captured"),
                            }
                            let _ = state_tx.send(RecordingState::Idle).await;
                        });
                    }
                }
                Some(RecordingCommand::Cancel) | None => {
                    stop_tx.take();
                    pcm_rx.take();
                }
            }
        }
    });

    // --- hud_task ---
    let ah_hud = app_handle.clone();
    let state_hud = Arc::clone(&state);
    rt.spawn(async move {
        loop {
            match state_rx.recv().await {
                Some(s) => {
                    let label = match s {
                        RecordingState::Idle => "",
                        RecordingState::Recording => "Recording...",
                        RecordingState::Processing => "Processing...",
                    };
                    let label = label.to_string();
                    let show_hud = state_hud.config.read().unwrap().show_hud;
                    let _ = ah_hud.run_on_main_thread(move || {
                        if label.is_empty() {
                            hud::panel::hide();
                        } else if show_hud {
                            hud::panel::show(&label);
                        }
                    });
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
                Some(text) => {
                    let play_sound = state_inj.config.read().unwrap().play_completion_sound;
                    let _ = ah_inj.run_on_main_thread(move || {
                        if let Err(e) = injection::text::type_text(&text) {
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

fn update_tray_icon(app: &tauri::AppHandle, state: RecordingState) {
    let tooltip = match state {
        RecordingState::Idle => "Whisp",
        RecordingState::Recording => "Whisp — Recording",
        RecordingState::Processing => "Whisp — Processing",
    };

    // 22x22 RGBA icon: different fill color per state
    // Idle: grey circle, Recording: red filled, Processing: yellow filled
    let (fg_r, fg_g, fg_b) = match state {
        RecordingState::Idle => (200u8, 200u8, 200u8),
        RecordingState::Recording => (230u8, 50u8, 50u8),
        RecordingState::Processing => (230u8, 180u8, 50u8),
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
