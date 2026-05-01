use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

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
pub mod injection;
pub mod keychain;
pub mod permissions;
pub mod transcription;

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
            };

            if new_state != current {
                current = new_state.clone();
                update_tray_icon(&ah_hk, &current, &mut tray_anim_abort);
            }
        }
    });

    // --- audio_task ---
    let text_tx_audio = text_tx.clone();
    let reset_tx_audio = reset_tx.clone();
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
                            let _ = reset_tx_audio.send(()).await;
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
                                                    let _ = reset_tx.send(()).await;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("WAV encode failed: {}", e);
                                            let _ = reset_tx.send(()).await;
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
                    std::thread::spawn(|| audio::sound::play());
                }
            });
        }
    });
}

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
fn in_rounded_rect(fx: f32, fy: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    if fx < x0 || fx > x1 || fy < y0 || fy > y1 { return false; }
    let cx = fx.max(x0 + r).min(x1 - r);
    let cy = fy.max(y0 + r).min(y1 - r);
    let dx = fx - cx;
    let dy = fy - cy;
    dx * dx + dy * dy <= r * r
}

/// Draws a colored pill background into `pixels` (22×22 canvas, full-width capsule).
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
pub fn set_idle_tray_icon(app: &tauri::AppHandle) {
    set_tray(app, "Whisp", render_waveform_idle(22, 255, 255, 255, 200), 22);
}

fn set_tray(app: &tauri::AppHandle, tooltip: &str, pixels: Vec<u8>, size: u32) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tooltip));
        let img = tauri::image::Image::new_owned(pixels, size, size);
        let _ = tray.set_icon(Some(img));
    }
}

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
        RecordingState::Error(_) => {
            // Red pill + white static waveform
            set_tray(app, "Whisp — Error", render_waveform_pill(22, 192, 57, 43), 22);
        }
    }
}

#[cfg(test)]
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
