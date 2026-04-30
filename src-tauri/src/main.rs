#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{mpsc, Arc, RwLock};
use std::sync::atomic::AtomicU64;
use tokio::sync::Mutex as TokioMutex;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Manager,
};

use whisp_rs_lib::{
    commands::{audio::*, config::*, dictionary::*, history::*, hud::*, model_download::*, permissions::*},
    config::persistence,
    history::store,
    hotkey::{event_tap, mode::HotkeyEvent},
    hud::panel,
    permissions,
    spawn_tasks, AppState,
};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("whisp_rs=debug".parse().unwrap()),
        )
        .init();

    // Load config
    let config = persistence::load().unwrap_or_default();

    // Determine database path
    let db_path = persistence::app_support_dir()
        .expect("cannot determine app support dir")
        .join("history.db");

    // Open SQLite database
    let db_options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);

    let db = SqlitePool::connect_with(db_options)
        .await
        .expect("failed to open SQLite database");

    store::create_schema(&db)
        .await
        .expect("failed to create history schema");

    let app_state = AppState {
        config: Arc::new(RwLock::new(config)),
        db: db.clone(),
        // Mask starts at 0; event_tap::install() sets the real value in setup.
        hotkey_mask: Arc::new(AtomicU64::new(0)),
        whisper_ctx: Arc::new(TokioMutex::new((None, None))),
        download_abort: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };

    // Channel for CGEventTap → hotkey task
    let (hotkey_tx, hotkey_rx) = mpsc::sync_channel::<HotkeyEvent>(64);
    let hotkey = app_state.config.read().unwrap().hotkey.clone();

    let state_arc = Arc::new(app_state.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            get_api_key,
            set_api_key,
            delete_api_key,
            get_history,
            delete_history_entry,
            clear_history,
            check_accessibility,
            open_accessibility_settings,
            check_microphone,
            request_microphone,
            open_microphone_settings,
            check_input_monitoring,
            request_input_monitoring,
            open_input_monitoring_settings,
            open_model_url,
            get_dictionary,
            add_dictionary_entry,
            remove_dictionary_entry,
            list_whisper_models,
            get_models_dir,
            get_downloaded_models,
            download_whisper_model,
            abort_model_download,
            list_audio_input_devices,
            hud_cancel_recording,
            hud_stop_recording,
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // Create tray icon
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&settings_item, &quit])?;

            let mut tray_builder = TrayIconBuilder::with_id("main");
            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }
            tray_builder
                .menu(&menu)
                .tooltip("Whisp2")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "settings" => {
                        if let Some(window) = app.get_webview_window("settings") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|_tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        // Left click: no-op for now
                    }
                })
                .build(app)?;

            // Create the floating HUD panel (must be on main thread)
            panel::create(app.handle());

            // Install CGEventTap (requires Accessibility permission).
            // Pass state_arc.hotkey_mask so the tap and set_config share the same Arc.
            if permissions::has_accessibility() {
                if !permissions::has_input_monitoring() {
                    tracing::warn!(
                        "Input Monitoring permission not granted — CGEventTap may be silently \
                         disabled by macOS. Open Settings → Privacy & Security → Input Monitoring."
                    );
                }
                if let Err(e) = event_tap::install(
                    hotkey,
                    hotkey_tx,
                    Arc::clone(&state_arc.hotkey_mask),
                ) {
                    tracing::error!("CGEventTap install failed: {}", e);
                }
            } else {
                tracing::warn!(
                    "Accessibility permission not granted — hotkey recording disabled. \
                     Open Settings → Privacy & Security → Accessibility to enable."
                );
            }

            // Spawn all async background tasks
            spawn_tasks(app_handle, state_arc.clone(), hotkey_rx);

            // Show settings on first launch if no API key / model is configured
            {
                use whisp_rs_lib::config::models::TranscriptionProvider;
                let config_snapshot = state_arc.config.read().unwrap().clone();
                let needs_setup = match &config_snapshot.provider {
                    TranscriptionProvider::OpenAI =>
                        matches!(whisp_rs_lib::keychain::get("openai_api_key"), Ok(None)),
                    TranscriptionProvider::Groq =>
                        matches!(whisp_rs_lib::keychain::get("groq_api_key"), Ok(None)),
                    TranscriptionProvider::Gemini =>
                        matches!(whisp_rs_lib::keychain::get("gemini_api_key"), Ok(None)),
                    TranscriptionProvider::LocalWhisper =>
                        config_snapshot.local_whisper_model_path.is_none(),
                };
                if needs_setup {
                    if let Some(window) = app.get_webview_window("settings") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running Whisp");
}
