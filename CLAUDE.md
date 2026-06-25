# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Whisp2 is a macOS menu bar app + iOS app for voice-to-text. On macOS, hold a hotkey, speak, release — text is injected into the frontmost app via CGEvent Unicode posting (no clipboard). On iOS, the Action Button starts a recording driven by an AppIntent + Live Activity (Stop via the Live Activity button or app re-foregrounding). Built with Tauri 2 (Rust backend + React/TypeScript frontend, plus a Swift Live Activity extension), targeting macOS 13+ and iOS 17+.

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
make ui-install   # npm install in src-ui
```

iOS targets:

```sh
make ios-dev        # cargo tauri ios dev (connected, unlocked device)
make ios-build      # cargo tauri ios build
make ios-typecheck  # Swift typecheck of WhispIntent + Live Activity sources
make ios-regen      # Clean-regen whisp-rs.xcodeproj from project.yml
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

### Entry Points

- **`src-tauri/src/lib.rs::run()`** is the unified Tauri builder for both desktop and mobile (`#[cfg_attr(mobile, tauri::mobile_entry_point)]`). It builds the SQLite pool, constructs `AppState`, registers all Tauri commands, and on macOS installs the CGEventTap, builds the tray icon, and calls `spawn_tasks`. On iOS it spawns `spawn_mobile_audio_task` instead.
- **`src-tauri/src/main.rs`** is the desktop binary entry; it just calls `whisp_rs::run()`.
- **macOS `spawn_tasks`** runs three concurrent async tasks for the lifetime of the app:
  1. **hotkey_task** — reads `HotkeyEvent`s from the CGEventTap bridge, drives the `RecordingState` FSM (`Idle → Recording → Processing → Idle`, plus a transient `Error(String)` state that paints the tray red with the message as tooltip, then auto-resets to `Idle`), updates the tray icon.
  2. **audio_task** — receives `RecordingCommand` from hotkey_task; starts/stops `cpal` capture; when stopped, classifies the clip by RMS via `classify_rms` (`DeadMic` below `DEAD_MIC_RMS_THRESHOLD` → loud `Error` naming the device; `Silent` below `SILENCE_RMS_THRESHOLD` → skipped quietly to avoid hallucinations; `Speech` → transcribe), encodes PCM → WAV, calls `transcription::manager::transcribe`, applies dictionary corrections, saves to history, then sends the text to injection_task. Also warns loudly if the configured mic was unavailable and `capture::start_recording` fell back to the system default (`RecordingSession.fell_back`).
  3. **injection_task** — receives `(text, source_app)` pairs, calls `injection::text::type_text` on the main thread via `run_on_main_thread`.
- **iOS `spawn_mobile_audio_task`** — single async task driven by `commands::audio` from `WhispIntent`; emits `recording_state_changed` and `transcription_result` to the WebView. No hotkey, no injection.

### Rust Module Map (`src-tauri/src/`)

| Module | Responsibility |
|---|---|
| `app_context/mod.rs` | Shared platform-agnostic context handle (paths, logging) |
| `ffi.rs` | iOS-only FFI shims (Swift ↔ Rust bridges for the AVFoundation pipeline) |
| `hotkey/` | macOS-only (`#[cfg(target_os = "macos")]`). `event_tap.rs` installs CGEventTap; `mode.rs` defines `RecordingState`, `RecordingCommand`, `HotkeyEvent` |
| `audio/capture.rs` | cpal recording; `start_recording` resolves the device synchronously and returns a `RecordingSession { stop_tx, pcm_rx, device_name, fell_back }` — drop `stop_tx` to stop. `device_name`/`fell_back` let the caller warn when the chosen mic was absent and a substitute was used. "System Default" (and the named-device fallback) route through `resolve_system_default`, which on macOS queries CoreAudio `kAudioDevicePropertyTransportType` and diverts away from silent virtual/Continuity defaults (e.g. Teams/Zoom loopback, idle iPhone mic) to a real physical mic. Pure selection logic is `choose_system_default`/`is_reliable_transport`; CoreAudio enumeration is the macOS-only `macos_transport` submodule |
| `audio/volume.rs` | Temporarily boost mic input gain during recording |
| `audio/sound.rs` | Play completion chime |
| `transcription/manager.rs` | Routes to the right provider; 3-attempt exponential-backoff retry |
| `transcription/providers/` | `openai.rs` (also used for Groq), `gemini.rs`, `local_whisper.rs` |
| `injection/text.rs` | macOS-only (`#[cfg(target_os = "macos")]`). CGEvent Unicode posting in 20-char UTF-16 chunks; terminal apps get longer delays |
| `hud/panel.rs` | NSPanel-based floating HUD window (separate from the settings WebView) |
| `config/models.rs` | `AppConfig`, `TranscriptionProvider`, `RecordingMode`, `HotkeyTrigger` |
| `config/persistence.rs` | JSON config at `~/Library/Application Support/com.whisp2.app/config.json` |
| `history/store.rs` | SQLite via sqlx; `history.db` in the app support dir |
| `history/models.rs` | `HistoryEntry` struct + sqlx row mapping |
| `correction/dictionary.rs` | Whole-word substitutions applied post-transcription |
| `correction/semantic.rs` | LLM-based post-transcription correction (opt-in) |
| `keychain.rs` | macOS Keychain read/write for API keys (`openai_api_key`, `groq_api_key`, `gemini_api_key`) |
| `permissions/` | `has_accessibility()`, `has_input_monitoring()`, `check_microphone()` |
| `commands/audio.rs` | iOS recording start/stop commands invoked by `WhispIntent` |
| `commands/hud.rs` | Show/hide the macOS HUD panel |
| `commands/shortcut.rs` | Hotkey capture for the settings UI |
| `commands/model_download.rs` | Download Whisper models; emits `model_download_progress`. `resolve_model_path` joins the stored filename with `app_support_dir()/models/` |
| `commands/diagnostics.rs` | Log access for bug reports. iOS: `read_ios_log`/`clear_ios_log` (FFI to Swift `WhispLogger`). macOS: `read_recent_logs` (newest daily files first, capped at 256 KB for copy/paste) and `open_log_dir` (reveal logs in Finder); both stubbed off-macOS so the shared `invoke_handler` stays uniform |
| `commands/` | Tauri `#[command]` handlers — thin wrappers that call into the modules above |

### Frontend (`src-ui/src/`)

- **`App.tsx`** — single settings window; tabs: Settings, History, Dictionary, Permissions. All state is local React state; calls backend via `invoke`. No router, no state management library.
- **`Onboarding.tsx`** — shown once on first launch (gated by `localStorage.whisp_onboarding_done`).
- **`hud.ts`** — pure DOM script loaded in the HUD WebView (not in `App.tsx`). Listens for `hud_state` Tauri events; renders the floating HUD states imperatively. (Also subscribes to `audio_level`, but no current backend code path emits it — legacy listener.)

### iOS Architecture

- **`src-tauri/gen/apple/Sources/whisp-rs/WhispIntent.swift`** — Action Button AppIntent. Foregrounds the host app, starts a Live Activity, runs `WhispRecorder` with no hard duration cap; stops on a cross-process flag.
- **`Shared/WhispActivityAttributes.swift`** + **`Shared/WhispStopIntent.swift`** — types shared between the host app and the Live Activity widget extension.
- **`WhispLiveActivity/`** — widget extension target rendering the Lock Screen + Dynamic Island UI; the Stop button is an interactive `Button(intent: WhispStopIntent(...))` (iOS 17+).
- **Cross-process IPC**: stop signal travels via the App Group `group.com.whisp2.app` UserDefaults key `whisp.stop.<sessionId>`; the recorder polls every 100 ms.
- **App lifecycle**: a `UIApplication.didBecomeActiveNotification` observer (1.5 s debounce) also stops recording when the user re-foregrounds the app.
- **Project regen**: `make ios-regen` clean-regenerates `whisp-rs.xcodeproj` from `gen/apple/project.yml` — in-place merges have stale-state bugs, so always clean-regen after editing `project.yml`.

### IPC Boundary

Tauri commands are the only IPC. Frontend calls `invoke("command_name", { ...args })`. The backend emits events (`hud_state`, `model_download_progress`, `recording_state_changed`, `transcription_result`) with `app_handle.emit(...)`. `recording_state_changed` carries `"recording" | "processing" | "idle"`; `transcription_result` carries the final text. There are no REST endpoints or sockets.

### Serde Naming

`AppConfig` and all enums use `#[serde(rename_all = "snake_case")]`, so the frontend receives e.g. `"open_a_i"`, `"press_and_hold"`. Match these exactly when adding new variants.

## macOS-Specific Constraints

- **Three permissions required**: Accessibility (CGEventTap), Input Monitoring (keyboard events in other apps), Microphone. Accessibility must be granted manually in System Settings — there is no programmatic request API.
- **CGEventTap is installed on startup** only if Accessibility is already granted. If the user grants it later, they must restart the app.
- **Text injection runs on the main thread** — `run_on_main_thread` is mandatory; calling CGEvent APIs from an async task will silently fail.
- **`macos-private-api = true`** in `tauri.conf.json` is required for the NSPanel HUD.
- **Settings window hide-on-close**: `lib.rs::run()` intercepts `WindowEvent::CloseRequested` for the `settings` label, calls `api.prevent_close()` and `window.hide()`. The app keeps running in the menu bar — teardown logic must not depend on the settings window closing.

## Reset + Stored Paths

- **`reset_app_data` Tauri command** wipes downloaded `*.bin` models, the config file, the SQLite history table, the in-memory config + cached `WhisperContext`, and the three Keychain entries. Surfaced in the UI's Settings → Danger Zone.
- **`local_whisper_model_path` is stored as a filename**, not absolute. `commands::model_download::resolve_model_path` joins it with `app_support_dir()/models/`. New code must not assume the field is a full path.

## Data Locations (macOS)

- Config JSON: `~/Library/Application Support/com.whisp2.app/config.json`
- History DB: `~/Library/Application Support/com.whisp2.app/history.db`
- Downloaded Whisper models: `~/Library/Application Support/com.whisp2.app/models/`
- Logs: `~/Library/Application Support/com.whisp2.app/logs/whisp.log.YYYY-MM-DD` (daily-rolling, written by `init_logging` in `lib.rs`; `prune_old_logs` deletes files older than `LOG_RETENTION_DAYS` (30) at startup)
- API keys: macOS Keychain (service `com.whisp2.app`)

## Versioning & Release

- **`src-tauri/tauri.conf.json` `version` is the source of truth** for the app version. Tauri stamps it into the bundle's `CFBundleShortVersionString`/`CFBundleVersion`, which is what Get Info and the About box show. `src-tauri/Cargo.toml` (and `Cargo.lock`) mirror it. Keep them in sync when bumping. Leaving these at the `0.1.0` placeholder makes every local `make build` report `0.1.0` even when releases are correct.
- **`.github/workflows/release.yml` is the release authority.** It bumps `tauri.conf.json` + `Cargo.toml` to the resolved version on the runner before building, then (after a successful publish) commits that version back to `main` in a `[skip ci]` commit so local/dev builds match what shipped. Three entry points: pushing a `vX.Y.Z` tag, `workflow_dispatch` (pick a bump or explicit version), or a push to `main` whose HEAD commit message contains `[release]` / `[release:minor]` / `[release:major]`.
- **CHANGELOG.md follows Keep a Changelog + SemVer.** Add a dated section per release; `CHANGELOG.md`, `tauri.conf.json`, and the git tag should all agree on the version.

## Repo Skills (`.claude/skills/`)

- **`falsify-with-disposable-harness`** — a debugging skill: when a bug resists multiple fixes or its logs/repro look contaminated, build a throwaway single-process harness that runs the suspect paths side-by-side to falsify the leading hypothesis, verify the fix at the built-artifact boundary, then delete the harness and promote its check into a real test. `SKILL.md` + `evals/evals.json`.
