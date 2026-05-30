use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

use crate::config::models::AppConfig;
use crate::hotkey::mode::RecordingCommand;

#[cfg(target_os = "macos")]
use crate::audio::capture;
#[cfg(target_os = "macos")]
use crate::config::models::RecordingMode;
#[cfg(target_os = "macos")]
use crate::hotkey::mode::{HotkeyEvent, RecordingState};
#[cfg(target_os = "macos")]
use crate::transcription::manager;

#[cfg(not(target_os = "macos"))]
use crate::audio::capture;
#[cfg(not(target_os = "macos"))]
use crate::transcription::manager;

pub mod audio;
pub mod commands;
pub mod config;
pub mod correction;
pub mod ffi;
pub mod history;
pub mod hotkey;
pub mod keychain;
pub mod transcription;

#[cfg(target_os = "macos")]
pub mod app_context;
#[cfg(target_os = "macos")]
pub mod injection;
pub mod permissions;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub db: sqlx::SqlitePool,
    pub hotkey_mask: Arc<AtomicU64>,
    pub whisper_ctx: crate::transcription::providers::local_whisper::WhisperCtxCache,
    pub download_abort: Arc<AtomicBool>,
    pub recording_cmd_tx: mpsc::Sender<RecordingCommand>,
}

/// Spawns all background async tasks. Called once inside Tauri's `setup` hook.
#[cfg(target_os = "macos")]
pub fn spawn_tasks(
    app_handle: tauri::AppHandle,
    state: Arc<AppState>,
    hotkey_rx: std::sync::mpsc::Receiver<HotkeyEvent>,
    cmd_rx: mpsc::Receiver<RecordingCommand>,
) {
    let rt = tokio::runtime::Handle::current();

    let mut cmd_rx = cmd_rx;
    let (text_tx, mut text_rx) = mpsc::channel::<(String, Option<String>)>(8);
    let (reset_tx, mut reset_rx) = mpsc::channel::<()>(8);
    // Carries a user-facing error message from the audio task into the FSM so
    // the menu bar icon turns red with the failure as its tooltip, instead of
    // silently resetting to Idle.
    let (error_tx, mut error_rx) = mpsc::channel::<String>(8);

    // Bridge std::sync::mpsc (CGEventTap) into tokio channel.
    let (async_hk_tx, mut async_hk_rx) = mpsc::channel::<HotkeyEvent>(64);
    std::thread::spawn(move || {
        while let Ok(event) = hotkey_rx.recv() {
            if async_hk_tx.blocking_send(event).is_err() {
                break;
            }
        }
    });

    // --- hotkey_task ---
    let cmd_tx_hk = state.recording_cmd_tx.clone();
    let state_hk = Arc::clone(&state);
    let ah_hk = app_handle.clone();
    let reset_tx_fsm = reset_tx.clone();
    rt.spawn(async move {
        let mut current = RecordingState::Idle;
        let mut tray_anim_abort: Option<tokio::task::AbortHandle> = None;

        loop {
            let new_state = tokio::select! {
                maybe_event = async_hk_rx.recv() => {
                    let event = match maybe_event {
                        Some(e) => e,
                        None => break,
                    };
                    let mode = state_hk.config.read().unwrap().recording_mode.clone();
                    match (&current, &event) {
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
                                RecordingMode::Toggle => current.clone(),
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
                    }
                }
                maybe_reset = reset_rx.recv() => {
                    if maybe_reset.is_none() { break; }
                    RecordingState::Idle
                }
                maybe_error = error_rx.recv() => {
                    match maybe_error {
                        Some(msg) => RecordingState::Error(msg),
                        None => break,
                    }
                }
            };

            if new_state != current {
                current = new_state.clone();
                update_tray_icon(&ah_hk, &current, &mut tray_anim_abort);

                // The Error state is transient: hold the red icon long enough to
                // be noticed, then fall back to Idle on its own.
                if matches!(current, RecordingState::Error(_)) {
                    let reset = reset_tx_fsm.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                        let _ = reset.send(()).await;
                    });
                }
            }
        }
    });

    // --- audio_task ---
    let text_tx_audio = text_tx.clone();
    let reset_tx_audio = reset_tx.clone();
    let error_tx_audio = error_tx.clone();
    let state_arc = Arc::clone(&state);
    rt.spawn(async move {
        let mut stop_tx: Option<mpsc::Sender<()>> = None;
        let mut pcm_rx: Option<mpsc::Receiver<Vec<f32>>> = None;
        let mut saved_vol: Option<f32> = None;
        let mut source_app: Option<String> = None;

        loop {
            match cmd_rx.recv().await {
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
                            let _ = error_tx_audio.send(format!("Couldn't start recording: {e}")).await;
                        }
                    }
                }
                Some(RecordingCommand::Stop) => {
                    if let Some(tx) = stop_tx.take() { drop(tx); }
                    if let Some(vol) = saved_vol.take() { audio::volume::restore(vol); }
                    if let Some(mut rx) = pcm_rx.take() {
                        let config = state_arc.config.read().unwrap().clone();
                        let db = state_arc.db.clone();
                        let text_tx = text_tx_audio.clone();
                        let reset_tx = reset_tx_audio.clone();
                        let error_tx = error_tx_audio.clone();
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
                                                    if config.save_history {
                                                        let provider_name = format!("{:?}", config.provider);
                                                        if let Err(e) = crate::history::store::insert(
                                                            &db, &text, app_id.as_deref(), &provider_name,
                                                        ).await {
                                                            tracing::warn!("history insert failed: {}", e);
                                                        } else if let Some(max) = config.max_history_entries {
                                                            if let Err(e) = crate::history::store::prune(&db, max).await {
                                                                tracing::warn!("history prune failed: {}", e);
                                                            }
                                                        }
                                                    }
                                                    let _ = text_tx.send((text, app_id)).await;
                                                    let _ = reset_tx.send(()).await;
                                                }
                                                Err(e) => {
                                                    tracing::error!("transcription failed: {}", e);
                                                    let _ = error_tx.send(e.to_string()).await;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("WAV encode failed: {}", e);
                                            let _ = error_tx.send(format!("Audio encoding failed: {e}")).await;
                                        }
                                    }
                                }
                                _ => {
                                    tracing::warn!("no audio captured");
                                    let _ = reset_tx.send(()).await;
                                }
                            }
                        });
                    }
                }
                Some(RecordingCommand::Cancel) | None => {
                    stop_tx.take();
                    pcm_rx.take();
                    source_app.take();
                    let _ = reset_tx_audio.send(()).await;
                }
            }
        }
    });

    // --- injection_task ---
    let ah_inj = app_handle.clone();
    let state_inj = Arc::clone(&state);
    rt.spawn(async move {
        while let Some((text, source_app)) = text_rx.recv().await {
            let play_sound = state_inj.config.read().unwrap().play_completion_sound;
            let _ = ah_inj.run_on_main_thread(move || {
                if let Err(e) = injection::text::type_text(&text, source_app.as_deref()) {
                    tracing::error!("text injection failed: {}", e);
                } else if play_sound {
                    std::thread::spawn(audio::sound::play);
                }
            });
        }
    });
}

#[cfg(target_os = "macos")]
fn render_waveform_idle(size: u32, r: u8, g: u8, b: u8, alpha: u8) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    // Mirrors the sidebar SVG: 5 bars bottom-aligned, bell-curve heights.
    // Heights (px in 22px canvas): short=5, mid=10, tall=16, mid=10, short=5.
    // Bar x-offsets: 4, 7, 10, 13, 16 — each 2px wide with 1px gap.
    let bar_xs: [u32; 5] = [4, 7, 10, 13, 16];
    let bar_heights: [u32; 5] = [5, 10, 16, 10, 5];
    let bar_w = 2u32;
    let base_y = size - 3; // bottom anchor

    for (i, &bx) in bar_xs.iter().enumerate() {
        let h = bar_heights[i];
        let top_y = base_y.saturating_sub(h);
        for px in bx..(bx + bar_w) {
            for py in top_y..base_y {
                if px < size && py < size {
                    let idx = ((py * size + px) * 4) as usize;
                    pixels[idx] = r;
                    pixels[idx + 1] = g;
                    pixels[idx + 2] = b;
                    pixels[idx + 3] = alpha;
                }
            }
        }
    }
    pixels
}

/// True if (fx, fy) falls inside the rounded-rect defined by the given bounds and corner radius.
#[cfg(target_os = "macos")]
fn in_rounded_rect(fx: f32, fy: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    if fx < x0 || fx > x1 || fy < y0 || fy > y1 { return false; }
    let cx = fx.max(x0 + r).min(x1 - r);
    let cy = fy.max(y0 + r).min(y1 - r);
    let dx = fx - cx;
    let dy = fy - cy;
    dx * dx + dy * dy <= r * r
}

/// Draws a colored pill background into `pixels` (22×22 canvas, full-width capsule).
#[cfg(target_os = "macos")]
fn draw_pill(pixels: &mut [u8], size: u32, r: u8, g: u8, b: u8) {
    let s = size as f32;
    // Full-width capsule: x 1..s-1, y 4..s-4, radius = half height = (s-8)/2.
    let (x0, y0, x1, y1) = (1.0f32, 4.0, s - 1.0, s - 4.0);
    let radius = (y1 - y0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            if in_rounded_rect(fx, fy, x0, y0, x1, y1, radius) {
                let idx = ((y * size + x) * 4) as usize;
                pixels[idx]     = r;
                pixels[idx + 1] = g;
                pixels[idx + 2] = b;
                pixels[idx + 3] = 240;
            }
        }
    }
}

/// Animated waveform equalizer — colored pill bg with white bars on top.
/// `r/g/b` sets the pill color; bars are always white.
#[cfg(target_os = "macos")]
fn render_equalizer_frame(size: u32, t: f32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    draw_pill(&mut pixels, size, r, g, b);

    // Bars scaled to fit inside the pill interior (base heights reduced ~60%).
    let base_heights = [3.0f32, 6.0, 10.0, 6.0, 3.0];
    let amplitude = 2.0f32;
    let phases = [0.0f32, 1.1, 2.3, 0.6, 1.8];
    let speeds = [6.5f32, 8.0, 7.0, 9.0, 5.5];
    let bar_xs: [u32; 5] = [4, 7, 10, 13, 16];
    let bar_w = 2u32;
    let base_y = size - 5; // 1 px above pill bottom edge

    for (i, &bx) in bar_xs.iter().enumerate() {
        let osc = (t * speeds[i] + phases[i]).sin();
        let h = (base_heights[i] + osc * amplitude).round().max(1.0) as u32;
        for px in bx..(bx + bar_w) {
            for py in (base_y - h)..base_y {
                if px < size && py < size {
                    let idx = ((py * size + px) * 4) as usize;
                    pixels[idx]     = 255;
                    pixels[idx + 1] = 255;
                    pixels[idx + 2] = 255;
                    pixels[idx + 3] = 230;
                }
            }
        }
    }
    pixels
}

/// Spinner arc — colored pill bg with a white partial-circle arc on top.
#[cfg(target_os = "macos")]
fn render_spinner_icon(size: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    draw_pill(&mut pixels, size, r, g, b);

    let cx = size as f32 / 2.0;
    let radius = 4.5f32; // smaller, fits inside the pill
    let thickness = 2.0f32;
    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5 - cx;
            let fy = y as f32 + 0.5 - cx;
            let dist = (fx * fx + fy * fy).sqrt();
            if (dist - radius).abs() <= thickness / 2.0 {
                let angle = fy.atan2(fx);
                let skip = angle > -std::f32::consts::FRAC_PI_2 && angle < 0.3;
                if !skip {
                    let idx = ((y * size + x) * 4) as usize;
                    pixels[idx]     = 255;
                    pixels[idx + 1] = 255;
                    pixels[idx + 2] = 255;
                    pixels[idx + 3] = 220;
                }
            }
        }
    }
    pixels
}

/// Static waveform on a colored pill — used for the Error state.
#[cfg(target_os = "macos")]
fn render_waveform_pill(size: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    // Reuse idle waveform (white) then overlay a pill background first.
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    draw_pill(&mut pixels, size, r, g, b);

    let bar_xs: [u32; 5] = [4, 7, 10, 13, 16];
    let bar_heights: [u32; 5] = [3, 6, 10, 6, 3];
    let bar_w = 2u32;
    let base_y = size - 5;

    for (i, &bx) in bar_xs.iter().enumerate() {
        let h = bar_heights[i];
        let top_y = base_y.saturating_sub(h);
        for px in bx..(bx + bar_w) {
            for py in top_y..base_y {
                if px < size && py < size {
                    let idx = ((py * size + px) * 4) as usize;
                    pixels[idx]     = 255;
                    pixels[idx + 1] = 255;
                    pixels[idx + 2] = 255;
                    pixels[idx + 3] = 230;
                }
            }
        }
    }
    pixels
}

/// Sets the tray icon to the idle waveform immediately. Call this right after the
/// tray is built in `main.rs` so the waveform replaces the default `.png` icon at launch.
#[cfg(target_os = "macos")]
pub fn set_idle_tray_icon(app: &tauri::AppHandle) {
    set_tray(app, "Whisp", render_waveform_idle(22, 255, 255, 255, 200), 22);
}

#[cfg(target_os = "macos")]
fn set_tray(app: &tauri::AppHandle, tooltip: &str, pixels: Vec<u8>, size: u32) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tooltip));
        let img = tauri::image::Image::new_owned(pixels, size, size);
        let _ = tray.set_icon(Some(img));
    }
}

#[cfg(target_os = "macos")]
fn update_tray_icon(
    app: &tauri::AppHandle,
    state: &RecordingState,
    anim_abort: &mut Option<tokio::task::AbortHandle>,
) {
    if let Some(h) = anim_abort.take() { h.abort(); }

    match state {
        RecordingState::Recording => {
            // Red pill (#c0392b) + animated white waveform bars
            let app = app.clone();
            let handle = tokio::spawn(async move {
                let mut t = 0.0f32;
                let mut interval = tokio::time::interval(
                    std::time::Duration::from_millis(1000 / 12)
                );
                loop {
                    interval.tick().await;
                    set_tray(&app, "Whisp — Recording", render_equalizer_frame(22, t, 192, 57, 43), 22);
                    t += 1.0 / 12.0;
                }
            });
            *anim_abort = Some(handle.abort_handle());
        }
        RecordingState::Idle => {
            // White with slight transparency — matches macOS menu bar icon convention
            set_tray(app, "Whisp", render_waveform_idle(22, 255, 255, 255, 200), 22);
        }
        RecordingState::Processing => {
            // Amber pill + white spinner arc
            set_tray(app, "Whisp — Processing", render_spinner_icon(22, 232, 169, 40), 22);
        }
        RecordingState::Error(msg) => {
            // Red pill + white static waveform; the failure shows as the tooltip.
            set_tray(app, &format!("Whisp — {msg}"), render_waveform_pill(22, 192, 57, 43), 22);
        }
    }
}

/// Mobile audio pipeline: capture → transcribe → emit result event + copy to clipboard.
/// No volume boost, no CGEvent injection — output goes to the frontend via event.
#[cfg(not(target_os = "macos"))]
async fn spawn_mobile_audio_task(
    app_handle: tauri::AppHandle,
    state: Arc<AppState>,
    mut cmd_rx: mpsc::Receiver<RecordingCommand>,
) {
    use tauri::Emitter;

    let mut stop_tx: Option<mpsc::Sender<()>> = None;
    let mut pcm_rx: Option<mpsc::Receiver<Vec<f32>>> = None;

    loop {
        match cmd_rx.recv().await {
            Some(RecordingCommand::Start(_)) => {
                let input_device = state.config.read().unwrap().input_device.clone();
                match capture::start_recording(input_device) {
                    Ok((tx, rx)) => {
                        stop_tx = Some(tx);
                        pcm_rx = Some(rx);
                        let _ = app_handle.emit("recording_state_changed", "recording");
                        tracing::info!("mobile recording started");
                    }
                    Err(e) => {
                        tracing::error!("failed to start recording: {}", e);
                        let _ = app_handle.emit("recording_state_changed", "idle");
                    }
                }
            }
            Some(RecordingCommand::Stop) => {
                drop(stop_tx.take());
                let _ = app_handle.emit("recording_state_changed", "processing");

                if let Some(mut rx) = pcm_rx.take() {
                    let config = state.config.read().unwrap().clone();
                    let db = state.db.clone();
                    let whisper_ctx = Arc::clone(&state.whisper_ctx);
                    let ah = app_handle.clone();

                    tokio::spawn(async move {
                        let samples = rx.recv().await;
                        match samples {
                            Some(s) if !s.is_empty() => {
                                match capture::encode_wav(&s) {
                                    Ok(wav) => {
                                        match manager::transcribe(&config, wav, whisper_ctx).await {
                                            Ok(text) => {
                                                tracing::info!("mobile transcribed: {}", text);
                                                let text = crate::correction::dictionary::apply(text);
                                                if config.save_history {
                                                    let provider_name = format!("{:?}", config.provider);
                                                    if let Err(e) = crate::history::store::insert(
                                                        &db, &text, None, &provider_name,
                                                    ).await {
                                                        tracing::warn!("history insert failed: {}", e);
                                                    } else if let Some(max) = config.max_history_entries {
                                                        if let Err(e) = crate::history::store::prune(&db, max).await {
                                                            tracing::warn!("history prune failed: {}", e);
                                                        }
                                                    }
                                                }
                                                let _ = ah.emit("transcription_result", &text);
                                                let _ = ah.emit("recording_state_changed", "idle");
                                            }
                                            Err(e) => {
                                                tracing::error!("transcription failed: {}", e);
                                                let _ = ah.emit("recording_state_changed", "idle");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("WAV encode failed: {}", e);
                                        let _ = ah.emit("recording_state_changed", "idle");
                                    }
                                }
                            }
                            _ => {
                                tracing::warn!("no audio captured");
                                let _ = ah.emit("recording_state_changed", "idle");
                            }
                        }
                    });
                } else {
                    let _ = app_handle.emit("recording_state_changed", "idle");
                }
            }
            Some(RecordingCommand::Cancel) | None => {
                drop(stop_tx.take());
                drop(pcm_rx.take());
                let _ = app_handle.emit("recording_state_changed", "idle");
                if cmd_rx.is_closed() {
                    break;
                }
            }
        }
    }
}

/// App entry point called by both `main.rs` (desktop) and the iOS/Android mobile_entry_point.
/// All Tauri builder setup lives here so the iOS linker can find the required runtime symbols.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Initialize tracing once at startup. Without this, every `tracing::*` call in
/// the app is silently dropped — which left us blind while debugging. Writes to
/// stdout and to a daily-rotated file under the app support dir so failures on a
/// user's machine can be inspected after the fact.
fn init_logging() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,whisp_rs_lib=debug"));

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stdout));

    if let Ok(dir) = config::persistence::app_support_dir() {
        let log_dir = dir.join("logs");
        if std::fs::create_dir_all(&log_dir).is_ok() {
            let appender = tracing_appender::rolling::daily(&log_dir, "whisp.log");
            // Non-blocking writer needs its guard kept alive for the process
            // lifetime; leak it intentionally.
            let (writer, guard) = tracing_appender::non_blocking(appender);
            std::mem::forget(guard);
            let _ = registry
                .with(fmt::layer().with_ansi(false).with_writer(writer))
                .try_init();
            return;
        }
    }

    let _ = registry.try_init();
}

pub fn run() {
    use std::sync::atomic::AtomicBool;
    use sqlx::sqlite::SqliteConnectOptions;
    use sqlx::SqlitePool;
    use tokio::sync::Mutex as TokioMutex;

    init_logging();

    let rt = tokio::runtime::Runtime::new().expect("failed to create Tokio runtime");
    rt.block_on(async {
        let config = config::persistence::load().unwrap_or_default();

        let db_path = config::persistence::app_support_dir()
            .expect("cannot determine app support dir")
            .join("history.db");

        let db_options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true);

        let db = SqlitePool::connect_with(db_options)
            .await
            .expect("failed to open SQLite database");

        history::store::create_schema(&db)
            .await
            .expect("failed to create history schema");

        let (cmd_tx, cmd_rx) = mpsc::channel::<RecordingCommand>(8);

        let app_state = AppState {
            config: Arc::new(RwLock::new(config)),
            db,
            hotkey_mask: Arc::new(AtomicU64::new(0)),
            whisper_ctx: Arc::new(TokioMutex::new((None, None))),
            download_abort: Arc::new(AtomicBool::new(false)),
            recording_cmd_tx: cmd_tx,
        };

        #[cfg(target_os = "macos")]
        let hotkey = app_state.config.read().unwrap().hotkey.clone();
        #[cfg(target_os = "macos")]
        let (hotkey_tx, hotkey_rx) = std::sync::mpsc::sync_channel::<HotkeyEvent>(64);

        let state_arc = Arc::new(app_state.clone());

        let mut builder = tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .manage(app_state)
            .on_window_event(|_window, _event| {
                // Closing the settings window must not quit the app — we live in
                // the menu bar. Hide instead and prevent the default close.
                #[cfg(target_os = "macos")]
                if _window.label() == "settings" {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = _event {
                        api.prevent_close();
                        let _ = _window.hide();
                    }
                }
            })
            .invoke_handler(tauri::generate_handler![
                commands::config::get_config,
                commands::config::set_config,
                commands::config::get_api_key,
                commands::config::set_api_key,
                commands::config::delete_api_key,
                commands::history::get_history,
                commands::history::delete_history_entry,
                commands::history::clear_history,
                commands::permissions::check_accessibility,
                commands::permissions::open_accessibility_settings,
                commands::permissions::check_microphone,
                commands::permissions::request_microphone,
                commands::permissions::open_microphone_settings,
                commands::permissions::check_input_monitoring,
                commands::permissions::request_input_monitoring,
                commands::permissions::open_input_monitoring_settings,
                commands::config::open_model_url,
                commands::dictionary::get_dictionary,
                commands::dictionary::add_dictionary_entry,
                commands::dictionary::remove_dictionary_entry,
                commands::model_download::list_whisper_models,
                commands::model_download::get_models_dir,
                commands::model_download::get_downloaded_models,
                commands::model_download::download_whisper_model,
                commands::model_download::abort_model_download,
                commands::audio::list_audio_input_devices,
                commands::audio::start_recording_mobile,
                commands::audio::stop_recording_mobile,
                commands::config::get_platform,
                commands::config::reset_app_data,
                commands::shortcut::install_shortcut,
                commands::diagnostics::read_ios_log,
                commands::diagnostics::clear_ios_log,
            ]);

        #[cfg(target_os = "macos")]
        {
            use tauri::{
                menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
                tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
                Manager,
            };
            builder = builder.setup(move |app| {
                let app_handle = app.handle().clone();

                let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
                let settings_item = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&settings_item, &quit])?;

                // App-wide menu bar so Cmd+, opens Settings when a Whisp window
                // has focus. LSUIElement=true keeps us out of the Dock, but we
                // still get a menu bar when the user activates the app.
                let app_settings = MenuItem::with_id(
                    app, "settings", "Settings...", true, Some("Cmd+,"),
                )?;
                let app_quit = MenuItem::with_id(
                    app, "quit", "Quit Whisp", true, Some("Cmd+Q"),
                )?;
                let separator = PredefinedMenuItem::separator(app)?;
                let app_submenu = Submenu::with_items(
                    app, "Whisp", true,
                    &[&app_settings, &separator, &app_quit],
                )?;
                let app_menu = Menu::with_items(app, &[&app_submenu])?;
                app.set_menu(app_menu)?;
                app.on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "settings" => {
                        if let Some(w) = app.get_webview_window("settings") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    _ => {}
                });

                let mut tray_builder = TrayIconBuilder::with_id("main");
                if let Some(icon) = app.default_window_icon().cloned() {
                    tray_builder = tray_builder.icon(icon);
                }
                tray_builder
                    .menu(&menu)
                    .tooltip("Whisp2")
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "quit" => app.exit(0),
                        "settings" => {
                            if let Some(w) = app.get_webview_window("settings") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|_tray, event| {
                        if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {}
                    })
                    .build(app)?;

                set_idle_tray_icon(&app_handle);

                if permissions::has_accessibility() {
                    if !permissions::has_input_monitoring() {
                        tracing::warn!("Input Monitoring not granted — CGEventTap may be disabled");
                    }
                    if let Err(e) = hotkey::event_tap::install(
                        hotkey, hotkey_tx, Arc::clone(&state_arc.hotkey_mask),
                    ) {
                        tracing::error!("CGEventTap install failed: {}", e);
                    }
                } else {
                    tracing::warn!("Accessibility not granted — hotkey recording disabled");
                }

                spawn_tasks(app_handle, state_arc.clone(), hotkey_rx, cmd_rx);

                {
                    use config::models::TranscriptionProvider;
                    let cfg = state_arc.config.read().unwrap().clone();
                    let needs_setup = match &cfg.provider {
                        TranscriptionProvider::OpenAI =>
                            matches!(keychain::get("openai_api_key"), Ok(None)),
                        TranscriptionProvider::Groq =>
                            matches!(keychain::get("groq_api_key"), Ok(None)),
                        TranscriptionProvider::Gemini =>
                            matches!(keychain::get("gemini_api_key"), Ok(None)),
                        TranscriptionProvider::LocalWhisper => match cfg.local_whisper_model_path.as_deref() {
                            None => true,
                            Some(name) => !commands::model_download::resolve_model_path(name)
                                .map(|p| p.exists())
                                .unwrap_or(false),
                        },
                    };
                    if needs_setup {
                        if let Some(w) = app.get_webview_window("settings") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                }
                Ok(())
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            builder = builder.setup(move |app| {
                let app_handle = app.handle().clone();
                tokio::spawn(spawn_mobile_audio_task(app_handle, state_arc.clone(), cmd_rx));

                // iOS: auto-pick a model if the saved path is missing or stale
                // (the data-container UUID rotates on every Xcode reinstall, which
                // invalidates legacy absolute paths), then pre-warm the model so
                // the first Action Button press doesn't pay the ~1s mmap + Metal
                // warmup cost. Gated on provider == LocalWhisper to avoid pinning
                // ~75 MB of resident memory for users on a cloud provider.
                #[cfg(target_os = "ios")]
                {
                    use config::models::TranscriptionProvider;
                    let cfg_now = state_arc.config.read().unwrap().clone();
                    if matches!(cfg_now.provider, TranscriptionProvider::LocalWhisper) {
                        let saved_ok = cfg_now
                            .local_whisper_model_path
                            .as_deref()
                            .and_then(|s| commands::model_download::resolve_model_path(s).ok())
                            .map(|p| p.exists())
                            .unwrap_or(false);

                        let resolved_name = if saved_ok {
                            cfg_now.local_whisper_model_path.clone()
                        } else {
                            match commands::model_download::scan_first_model_on_disk() {
                                Ok(Some(name)) => {
                                    let mut new_cfg = cfg_now.clone();
                                    new_cfg.local_whisper_model_path = Some(name.clone());
                                    {
                                        let mut w = state_arc.config.write().unwrap();
                                        *w = new_cfg.clone();
                                    }
                                    if let Err(e) = config::persistence::save(&new_cfg) {
                                        tracing::warn!("auto-pick save failed: {e}");
                                    } else {
                                        tracing::info!("auto-picked Whisper model: {name}");
                                    }
                                    Some(name)
                                }
                                Ok(None) => None,
                                Err(e) => {
                                    tracing::warn!("scan_first_model_on_disk failed: {e}");
                                    None
                                }
                            }
                        };

                        if let Some(name) = resolved_name {
                            let language = cfg_now.language.clone();
                            tokio::spawn(async move {
                                if let Err(e) = crate::ffi::warm_local_whisper(name, language).await {
                                    tracing::warn!("Whisper pre-warm failed: {e}");
                                }
                            });
                        }
                    }
                }

                Ok(())
            });
        }

        builder
            .run(tauri::generate_context!())
            .expect("error running Whisp");
    });
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn test_equalizer_buffer_size() {
        let buf = render_equalizer_frame(22, 0.0, 255, 255, 255);
        assert_eq!(buf.len(), 22 * 22 * 4);
    }

    #[test]
    fn test_equalizer_animates() {
        let frame_t0 = render_equalizer_frame(22, 0.0, 255, 255, 255);
        let frame_t1 = render_equalizer_frame(22, 1.0, 255, 255, 255);
        assert_ne!(frame_t0, frame_t1);
    }

    #[test]
    fn test_spinner_buffer_size() {
        let buf = render_spinner_icon(22, 255, 255, 255);
        assert_eq!(buf.len(), 22 * 22 * 4);
    }

    #[test]
    fn test_spinner_has_pixels() {
        let buf = render_spinner_icon(22, 255, 255, 255);
        assert!(buf.chunks(4).any(|p| p[3] != 0));
    }

    #[test]
    fn test_waveform_idle_buffer_size() {
        let buf = render_waveform_idle(22, 255, 255, 255, 200);
        assert_eq!(buf.len(), 22 * 22 * 4);
    }

    #[test]
    fn test_waveform_idle_has_pixels() {
        let buf = render_waveform_idle(22, 255, 255, 255, 200);
        assert!(buf.chunks(4).any(|p| p[3] != 0));
    }

    #[test]
    fn test_waveform_idle_is_static() {
        // render_waveform_idle has no time parameter — calling twice gives identical output
        let a = render_waveform_idle(22, 100, 100, 100, 200);
        let b = render_waveform_idle(22, 100, 100, 100, 200);
        assert_eq!(a, b);
    }
}
