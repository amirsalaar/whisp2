# whisp-rs todos

## Done

- [x] Tauri v2 project scaffold — LSUIElement, no sandbox, entitlements.plist
- [x] Menu bar tray icon — Quit and Settings... menu items
- [x] tokio runtime in setup, AppState (config + db)
- [x] Keychain module — get/set/delete via security-framework
- [x] CGEventTap — L/R modifier bitmask detection, std→tokio mpsc bridge
- [x] HUD NSPanel — non-activating, floating, CanJoinAllSpaces
- [x] cpal audio capture — rubato 16kHz mono resampling, hound WAV encode
- [x] OpenAI Whisper API — multipart POST, 3-attempt exponential backoff
- [x] SQLite history schema — sqlx runtime query_as, create_schema on launch
- [x] CGEvent Unicode text injection — 20-chunk UTF-16, 2ms inter-chunk delay
- [x] React settings UI — provider, API key (keychain), hotkey, mode, history tab
- [x] Tauri IPC commands — get/set config, get/set/delete API key, history CRUD, permissions
- [x] Groq Whisper provider — reuses OpenAIProvider, `groq_api_key` in keychain, model selector (whisper-large-v3-turbo default)
- [x] Vite port fixed to 1420 (was defaulting to 5173)
- [x] Info.plist with LSUIElement=true wired into tauri.conf.json bundle

---

## Bugs / gaps found during QA

- [ ] **Hotkey change requires restart** — `set_config` saves new hotkey but the running CGEventTap still listens on the original key. Fix: store a `Sender<HotkeyTrigger>` in AppState so `set_config` can signal the tap to reinstall itself without restarting the app.

- [ ] **`show_hud` config ignored** — `panel::show()` is always called. In `lib.rs` hud_task, read `state.config` and skip `panel::show`/`hide` when `show_hud` is false.

- [ ] **No error feedback on transcription failure** — errors are only logged. User sees HUD disappear with no indication something went wrong. Add an `Error(String)` variant to `RecordingState`, send it from the audio task on failure, display briefly in the HUD before hiding.

- [ ] **source_app always None in history** — `app_context::frontmost_bundle_id()` exists but is never called. Capture it at `RecordingCommand::Start` time (before focus shifts to the HUD) and thread it through to `store::insert`.

- [ ] **No first-launch onboarding** — if no API key is set, transcription silently fails. On first launch (config file absent or no key in keychain), auto-show the settings window so user is prompted to configure.

- [ ] **Tray icon image doesn't change state** — `update_tray_icon` only sets tooltip. Plan requires distinct icon images for idle / recording / processing. Need icon assets and `tray.set_icon()` calls per state.

- [ ] **Microphone permission hardcoded** — `has_microphone()` returns `true` unconditionally. Implement real `AVCaptureDevice.authorizationStatus` check via objc2 (or prompt user on first recording attempt if denied).

- [ ] **Daemon module stubs unused** — `daemon/process.rs` + `daemon/rpc.rs` exist as empty stubs. Either implement for local Whisper subprocess, or delete if whisper-rs covers the use case.

---

## Remaining features

- [ ] Toggle recording mode (currently only press-and-hold)
- [ ] Gemini transcription provider
- [ ] Local Whisper via whisper-rs (whisper.cpp, static link, no API call)
- [ ] WhisperKit provider (Apple Neural Engine, macOS 14+)
- [ ] Parakeet / Whisper-MLX provider (Apple Silicon MLX)
- [ ] Gemma provider
- [ ] Semantic correction post-processing (`correction/semantic.rs`)
- [ ] Personal dictionary (`correction/dictionary.rs`)
- [ ] History search and retention settings UI
- [ ] CoreAudio input volume boost (`audio/volume.rs`)
- [ ] App-aware injection delay (5ms for terminal emulators vs 2ms default, uses `app_context::frontmost_bundle_id`)
- [ ] CGEventTap health-check timer (re-enable tap every 5s if macOS silently disables it)
- [ ] Completion sound (`play_completion_sound` config wired, no audio playback yet)
- [ ] GitHub repo — create at github.com/amirsalaar/whisp-rs when ready to share
- [ ] Universal binary build + notarization + DMG packaging (`cargo tauri build --target universal-apple-darwin`)
- [ ] GitHub Actions CI — build on push to main, release on version tag
