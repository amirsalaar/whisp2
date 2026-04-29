#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{mpsc, Arc, RwLock};
use std::sync::atomic::AtomicU64;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Manager,
};

use whisp_rs_lib::{
    commands::{config::*, dictionary::*, history::*, permissions::*},
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
    };

    // Channel for CGEventTap → hotkey task
    let (hotkey_tx, hotkey_rx) = mpsc::sync_channel::<HotkeyEvent>(64);
    let hotkey = app_state.config.read().unwrap().hotkey.clone();

    let state_arc = Arc::new(app_state.clone());

    tauri::Builder::default()
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
            get_dictionary,
            add_dictionary_entry,
            remove_dictionary_entry,
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // Create tray icon
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&settings_item, &quit])?;

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().unwrap())
                .menu(&menu)
                .tooltip("Whisp")
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
            panel::create();

            // Install CGEventTap (requires Accessibility permission).
            // Pass state_arc.hotkey_mask so the tap and set_config share the same Arc.
            if permissions::has_accessibility() {
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

            // Show settings on first launch if no API key is configured
            {
                use whisp_rs_lib::config::models::TranscriptionProvider;
                let provider = state_arc.config.read().unwrap().provider.clone();
                let key_name = match provider {
                    TranscriptionProvider::OpenAI => "openai_api_key",
                    TranscriptionProvider::Groq => "groq_api_key",
                    TranscriptionProvider::Gemini => "gemini_api_key",
                };
                if matches!(whisp_rs_lib::keychain::get(key_name), Ok(None)) {
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
