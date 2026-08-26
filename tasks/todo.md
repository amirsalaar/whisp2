# whisp-rs todos

## Done: floating island (anarlog-inspired) (2026-08-04)

Revived and upgraded the existing pill HUD instead of rebuilding it: fixed the
CSS/TS mismatch, fed it real audio levels, made it draggable with a remembered
position, and added the recording controls + processing/status states.

- [x] `hud/position.rs` — pure placement + offscreen clamping, unit-tested.
- [x] Audio level tap → `audio_level` event; waveform tracks the live mic.
- [x] Wired into the FSM in `lib.rs` (proximity + recording state → `HudState`).
- [x] Frontend: state renderers, drag-to-move, error pill.
- [x] `hud_position` in config + persistence, preserved across settings saves.
- [x] `capabilities/hud.json` — without it the island's webview is inert.
- [x] Two real defects found and fixed during QA (see `lessons.md`):
      a synchronous Keychain command blocking the main thread and freezing the
      whole UI, and the island's first state being dropped when it painted before
      the webview was listening.
- [x] Quality gates: 101 Rust tests, clippy `-D warnings`, ui-build all green.
- [x] Disposable QA harness deleted; its drag/relaunch check promoted into
      `hud::position`'s tests.
- [x] Isolated code review (two reviewers with no access to my reasoning) +
      `/receiving-code-review`. Both independently found the boot-race lost update
      I'd already fixed. Nine further defects verified against the source and fixed:
      the error caption overflowing onto the desktop, `overflow: hidden` dropped
      from `.hud-pill` by this diff, a mid-drag pause being saved as the drop, a
      plain click permanently pinning a never-dragged island, Reduce Motion being
      ignored by the JS-driven shimmer, an always-on rAF loop at a hardcoded 60Hz,
      back-to-back errors showing the stale message, Discard leaving the mic gain
      boosted, and the multi-monitor coordinate flip. Pushed back on five backend
      findings the reviewer had already walked back itself, plus one pre-existing
      stale-form issue outside this scope.
- [ ] Hand-check the items in `tasks/test-commands/validate-floating-island.sh`
      that need Accessibility (stop/cancel buttons, error pill, show_hud toggle,
      full-screen). `run-dev.sh` re-signs ad-hoc, which revokes the grant — so
      re-grant in System Settings and relaunch before testing.

## Active: shareable logs + 30-day retention (2026-06-22)

Goal: users can access/copy logs to share in a bug report; errors are captured;
logs auto-pruned to 30 days so the dir never outgrows its size.

- [x] init_logging: prune `whisp.log*` files older than 30 days at startup (by mtime);
      `prune_old_logs` unit-tested (keeps recent + non-log files).
- [x] diagnostics.rs (macos): `read_recent_logs()` — newest daily-log content,
      size-capped (256KB) so it's copy/paste-friendly for an issue.
- [x] diagnostics.rs (macos): `open_log_dir()` — reveal logs folder in Finder.
- [x] Registered both commands in lib.rs invoke_handler (stubbed off-macOS).
- [x] App.tsx: desktop Diagnostics → Application logs card (View / Copy / Open Folder),
      mirroring the iOS on-device-log card.
- [x] make check / lint-rs / test (41/41) / ui-build / build all green.



## Active: fix system-default mic silently capturing silence (2026-06-22)

### Root cause
- macOS system-default input often isn't the user's real mic (laptop mic / a
  virtual "Microsoft Teams Audio"-style driver that returns silence).
- cpal binds a concrete device id identically for None vs explicit selection
  (cpal device.rs:235), so "System Default" just follows the wrong/silent OS default.
- When the selected device is absent, `capture::record_until_stop` silently falls
  back to the OS default (capture.rs:56) and lib.rs silently drops the silent clip
  as "below silence threshold" — user gets no feedback and must re-select each time.

### Plan
- [ ] capture.rs: resolve device synchronously in `start_recording`; return a
      `RecordingSession` carrying stop_tx, pcm_rx, the actual device name, and a
      `fell_back` flag so the caller knows if a substitute device was used.
- [ ] lib.rs macOS audio_task: on fallback, surface a LOUD warning (red tray +
      tooltip naming the substitute) instead of switching silently.
- [ ] lib.rs: separate a dead/silent mic (rms≈0) from a genuinely quiet clip.
      Dead mic → loud Error naming the device + "check Microphone permission";
      keep quiet-clip silence-skip for anti-hallucination (commits 9e46cb7/b5ad291).
- [x] Mirror RecordingSession use in mobile task (log only; no tray).
- [x] Tests + `make check`, `make lint-rs`, `make test` (40/40), `make build` (.app+.dmg).
- [x] QA (Rust-adapted; no web URL), squash-merged to main locally, pushed via
      personal SSH key with [release] trailer → Release workflow run 27978990934 in_progress.

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

## Follow-on: detected-language feedback for Parakeet (2026-07-03)

- [ ] Parakeet auto-detects language but nothing tells the user WHICH language was
      detected, so a wrong result is ambiguous (model error vs unsupported language).
      Surface the detected language in the HUD or a log line to close the loop.
      Surfaced during the Parakeet multi-language UX fix; deferred as separate scope.

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
- [x] GitHub repo — live at github.com/amirsalaar/whisp2
- [ ] Universal binary build (`--target universal-apple-darwin`) — release.yml ships aarch64-only DMG today; signing/notarization/DMG already wired, universal arch still pending
- [x] GitHub Actions CI — `.github/workflows/ci.yml` (check/test/clippy + frontend lint on push/PR to main) and `release.yml` (tag / workflow_dispatch / `[release]` trailer → build + sign + publish, version synced back to main)

## Done: Gemini 3.5 Transcribe provider (2026-08-26)

Google's dedicated speech-to-text model, added as the default for the Gemini
provider. Not a model-string swap: it lives on a different API surface.

- [x] Verified the wire format against Google's docs *and* the Interaction
      resource reference. `gemini-3.5-transcribe` is served from
      `POST /v1beta/interactions` with `model` + `input[]`, inline audio as
      `data`, and options under `generation_config.transcription_config` — not
      `:generateContent`.
- [x] `providers/gemini.rs` routes by model: `*-transcribe` → Interactions API,
      flash/pro → the existing `generateContent` path (untouched).
- [x] Response parsing walks `steps[] → model_output → content[]`. Deliberately
      does **not** read `output_text`: that field is SDK-synthesized and absent
      from the REST payload (see `lessons.md`).
- [x] Language: omit `language_codes` for the model's own 85+ locale
      auto-detection; pass the Settings string through as BCP-47 when pinned.
- [x] `-live` variants rejected with a message pointing at the non-live model
      (they need the WebSocket Live API).
- [x] 20 MB inline-request ceiling checked up front (~8 min of 16 kHz mono),
      with a message naming the actual size instead of an opaque API error.
- [x] Found and fixed a latent break: *all three* Gemini models Whisp offered
      (2.0-flash, 1.5-flash, 1.5-pro) have been shut down by Google, so the
      provider failed on every attempt. Picker refreshed to current models;
      `RETIRED_GEMINI_MODELS` swaps a stale config onto the default at load time
      (in memory — no surprise writes to the user's file).
- [x] Quality gates: 123 Rust tests (12 new, all on pure builders/parsers),
      clippy `-D warnings`, ui-build, ui-lint (one pre-existing warning), fmt.
- [ ] **Not verified against the live API** — no Gemini key was used, so the
      request shape is doc-derived. One real dictation with a key confirms it.
