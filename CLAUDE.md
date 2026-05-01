# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Whisp2 is a macOS menu bar app for voice-to-text. Hold a hotkey, speak, release — transcribed text is injected into the frontmost app via CGEvent Unicode posting (no clipboard). Built with Tauri 2 (Rust backend + React/TypeScript frontend), targeting macOS 13+.

## Commands

All common tasks are in the `Makefile`:

```sh
make dev          # Start full Tauri dev session (Vite + Rust hot-reload)
make build        # Production .app bundle → src-tauri/target/release/bundle/
make test         # Run Rust unit tests
make check        # Fast type-check (cargo check, no linking)
make lint-rs      # Clippy with -D warnings
make ui-lint      # ESLint on src-ui
make fmt          # rustfmt
make clean        # Remove all build artifacts
```

Run a single Rust test:
```sh
~/.cargo/bin/cargo test --package whisp-rs <test_name>
```

Frontend only (no Rust):
```sh
make ui-dev       # Vite dev server at localhost:1420
make ui-build     # tsc + vite build
```

## Architecture

### Two Entry Points

- **`src-tauri/src/main.rs`** — async Tokio main; builds the SQLite pool, constructs `AppState`, installs the CGEventTap, registers all Tauri commands, builds the tray icon, then calls `spawn_tasks`.
- **`src-tauri/src/lib.rs`** — defines `AppState` and `spawn_tasks`. Contains three concurrent async tasks that run for the lifetime of the app:
  1. **hotkey_task** — reads `HotkeyEvent`s from the CGEventTap bridge, drives the `RecordingState` FSM (`Idle → Recording → Processing → Idle`), updates the tray icon.
  2. **audio_task** — receives `RecordingCommand` from hotkey_task; starts/stops `cpal` capture; when stopped, encodes PCM → WAV, calls `transcription::manager::transcribe`, applies dictionary corrections, saves to history, then sends the text to injection_task.
  3. **injection_task** — receives `(text, source_app)` pairs, calls `injection::text::type_text` on the main thread via `run_on_main_thread`.

### Rust Module Map (`src-tauri/src/`)

| Module | Responsibility |
|---|---|
| `hotkey/event_tap.rs` | CGEventTap install; bridges std `mpsc` → tokio via a thread |
| `hotkey/mode.rs` | `RecordingState`, `RecordingCommand`, `HotkeyEvent` enums |
| `audio/capture.rs` | cpal recording; returns `(stop_tx, pcm_rx)` — drop `stop_tx` to stop |
| `audio/volume.rs` | Temporarily boost mic input gain during recording |
| `audio/sound.rs` | Play completion chime |
| `transcription/manager.rs` | Routes to the right provider; 3-attempt exponential-backoff retry |
| `transcription/providers/` | `openai.rs` (also used for Groq), `gemini.rs`, `local_whisper.rs` |
| `injection/text.rs` | CGEvent Unicode posting in 20-char UTF-16 chunks; terminal apps get longer delays |
| `hud/panel.rs` | NSPanel-based floating HUD window (separate from the settings WebView) |
| `config/models.rs` | `AppConfig`, `TranscriptionProvider`, `RecordingMode`, `HotkeyTrigger` |
| `config/persistence.rs` | JSON config at `~/Library/Application Support/com.whisp.whisp-rs/config.json` |
| `history/store.rs` | SQLite via sqlx; `history.db` in the app support dir |
| `correction/dictionary.rs` | Whole-word substitutions applied post-transcription |
| `keychain.rs` | macOS Keychain read/write for API keys (`openai_api_key`, `groq_api_key`, `gemini_api_key`) |
| `permissions/` | `has_accessibility()`, `has_input_monitoring()`, `check_microphone()` |
| `commands/` | Tauri `#[command]` handlers — thin wrappers that call into the modules above |

### Frontend (`src-ui/src/`)

- **`App.tsx`** — single settings window; tabs: Settings, History, Dictionary, Permissions. All state is local React state; calls backend via `invoke`. No router, no state management library.
- **`Onboarding.tsx`** — shown once on first launch (gated by `localStorage.whisp_onboarding_done`).
- **`hud.ts`** — pure DOM script loaded in the HUD WebView (not in `App.tsx`). Listens for `hud_state` and `audio_level` Tauri events; renders the floating HUD states imperatively.

### IPC Boundary

Tauri commands are the only IPC. Frontend calls `invoke("command_name", { ...args })`. The backend emits events (`hud_state`, `audio_level`, `model_download_progress`) with `app_handle.emit(...)`. There are no REST endpoints or sockets.

### Serde Naming

`AppConfig` and all enums use `#[serde(rename_all = "snake_case")]`, so the frontend receives e.g. `"open_a_i"`, `"press_and_hold"`. Match these exactly when adding new variants.

## macOS-Specific Constraints

- **Three permissions required**: Accessibility (CGEventTap), Input Monitoring (keyboard events in other apps), Microphone. Accessibility must be granted manually in System Settings — there is no programmatic request API.
- **CGEventTap is installed on startup** only if Accessibility is already granted. If the user grants it later, they must restart the app.
- **Text injection runs on the main thread** — `run_on_main_thread` is mandatory; calling CGEvent APIs from an async task will silently fail.
- **`macos-private-api = true`** in `tauri.conf.json` is required for the NSPanel HUD.

## Data Locations (macOS)

- Config JSON: `~/Library/Application Support/com.whisp.whisp-rs/config.json`
- History DB: `~/Library/Application Support/com.whisp.whisp-rs/history.db`
- Downloaded Whisper models: `~/Library/Application Support/com.whisp.whisp-rs/models/`
- API keys: macOS Keychain (service `com.whisp.whisp-rs`)
