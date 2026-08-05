# Floating Island (HUD) — Design

Date: 2026-08-04
Status: approved, implementing

## Problem

Whisp2's only feedback during dictation is the menu-bar tray icon. Users looking at
the app they're dictating *into* don't see the tray, so there's no in-view signal
that recording started, that audio is being heard, or that transcription is running.
Errors are worse: `RecordingState::Error(msg)` paints the tray red and puts the
message in a *tooltip*, which requires hovering the menu bar to read.

A floating on-screen island solves this. The repo already contains a half-built one.

## Current state: the HUD is orphaned dead code

Discovered while exploring, and the reason this is a revive rather than a greenfield build:

- `src-tauri/src/hud/panel.rs` exists (WebviewWindow + AppKit panel flags + proximity
  monitor) but **`pub mod hud;` is never declared in `lib.rs`**, so it doesn't compile
  into the binary.
- `panel::create()` is never called.
- `commands/hud.rs` defines `hud_stop_recording` / `hud_cancel_recording`, but they are
  **not registered** in the `invoke_handler`, and their bodies only log — they don't
  send any `RecordingCommand`.
- `hud.ts` listens for an `audio_level` event that **nothing in the Rust codebase emits**,
  so the waveform is pure synthetic animation, unrelated to the user's voice.
- `hud.css` styles `.hud-pill` / `.hud-title` / `.hud-btn`; `hud.ts` renders
  `.expanded-main-pill` / `.dots-pill` / `.hud-cancel` / `.hud-stop` /
  `.collapsed-idle-container`. **The two files do not intersect** — even if wired, the
  island would render unstyled.
- `AppConfig.show_hud` (default `true`) is never read.

## Reference: how anarlog does it

Anarlog is itself a Tauri app (confirmed: `__TAURI_INVOKE_KEY__`, `tauri_plugin_windows`
in the shipped binary) whose island is a **native Swift NSPanel** driven from Rust via a
`plugins/windows` crate. Extracted from `/Applications/Anarlog.app`:

- `FloatingBarState` payload: `amplitude, title, status, colorScheme, opacity,
  liveCaption*, transcriptBubbles`. `FloatingBarStatus` = `recording | error`.
- Swift types: `FloatingBarView/ViewModel/Manager`, `OverlayManager`,
  `FloatingPanelPositionController`, `FloatingBarSurfaceShape`, `FloatingBarDotPattern`,
  `FloatingBarHoverHandle`, `FloatingBarDragStart`.

Traits worth adopting: **live amplitude** on the bar, **draggable with position
persistence**, **explicit error status on the island itself**, hover-reveal affordance.

Traits deliberately *not* adopted:
- **Live caption / transcript bubbles.** Whisp transcribes *after* key release from one
  accumulated buffer — there is no incremental hypothesis to stream. Adding this would
  require a streaming provider. Out of scope.
- **Native Swift rendering.** Whisp2's island is already a transparent WebviewWindow.
  Rewriting it in Swift buys visual parity we can reach in CSS and costs a new build
  target. Keep the webview.
- **User-adjustable opacity.** No demand; skipped per YAGNI.

## Architecture

The island is a **second projection of the existing `RecordingState` FSM**, peer to the
tray icon. No new state machine. `hotkey_task` already owns the FSM and already calls
`update_tray_icon` on every transition; it gains a sibling `hud::panel::update` call.

```
CGEventTap ──> hotkey_task (RecordingState FSM)
                  ├──> update_tray_icon      (existing)
                  └──> hud::panel::update    (new)  ──emit "hud_state"──> hud.ts
cpal callback ──> amplitude tap ──emit "audio_level"──────────────────────> hud.ts
hud.ts buttons ──invoke──> commands::hud ──> RecordingCommand::{Stop,Cancel} ──> audio_task
```

Data flow is one-way for state (Rust → webview via events) and one-way for intent
(webview → Rust via commands). That mirrors the documented IPC boundary.

### State mapping

| `RecordingState` | `HudState` | Island |
|---|---|---|
| `Idle`, cursor away | `CollapsedIdle` | 50×18 handle |
| `Idle`, cursor near | `ExpandedIdle` | pill + hotkey hint |
| `Recording` | `RecordingControls` | live waveform + stop + cancel |
| `Processing` | `Processing` | "Transcribing…" |
| `Error(msg)` | `Error(msg)` | red pill + message, auto-clears via existing 4s FSM reset |

`HudState::ShortcutListening` is **removed**: nothing can reach it, and dead variants
violate the repo's no-placeholder rule.

`HudState::Error` is added because the FSM's `Error` variant currently only surfaces as a
tray tooltip. Putting the message on the island is the main user-visible win beyond
"you can see it's recording".

### Components

- **`hud/panel.rs`** (Rust, macOS-only) — owns the window: creation, AppKit panel flags,
  position resolution/clamping, `HudState` → event emission, cursor-event toggling.
  Consumers call `create` / `update` / `update_with_label`; they never touch AppKit.
- **`hud/position.rs`** (Rust, new, platform-agnostic) — pure geometry:
  `resolve_position(saved, visible_frame, screen_height, size) -> (f64, f64)`. Clamping a
  saved position back onto the visible screen is the only nontrivial logic in the feature,
  so it lives in a pure function with unit tests and no AppKit dependency.
- **`audio/level.rs`** (Rust, new) — amplitude tap: RMS over each cpal callback buffer,
  smoothed and rate-limited, emitted as `audio_level`. Pure `f32` math, unit-tested;
  knows nothing about Tauri (takes a callback).
- **`commands/hud.rs`** — thin command wrappers that forward to `RecordingCommand`.
- **`hud.ts` / `hud.css`** — the renderer. Class names reconciled; drag wiring added.

### Position persistence

- New `AppConfig.hud_position: Option<(f64, f64)>`, `None` = default bottom-center.
  Serde snake_case per repo convention.
- Drag uses Tauri's `startDragging()` on the pill body (not on buttons), so no custom
  mouse-delta math.
- Persisted on `Moved` window event, debounced.
- **On launch the saved position is clamped to the current visible frame.** A position
  saved on a since-disconnected external display would otherwise strand the island
  offscreen with no way to recover but editing config.json. This is the tested edge case.

### Proximity

The existing global `NSEvent` mouse-moved monitor computes its threshold as a hardcoded
"88px above the screen bottom", which is only correct for a bottom-anchored island. Once
draggable, the threshold must derive from the window's **actual current frame**. The
monitor keeps `try_send` (drop-on-full) so the main thread is never blocked by an event
that fires hundreds of times per second.

## Error handling

- **Window creation failure** — already logged and non-fatal; the app runs tray-only.
  Preserved: the island is an enhancement, never a hard dependency.
- **FSM errors** — surfaced *on* the island (new `Error` state) rather than swallowed,
  per the repo's error-propagation rule.
- **Amplitude emission failure** — dropped silently and deliberately. This fires ~20×/sec;
  logging a failure per frame would flood the log, and a missing frame is invisible.
  This is the one place a dropped error is correct, and it's commented as such.
- **`show_hud = false`** — no window is created and no events are emitted, rather than
  creating a hidden window.

## Testing

Rust unit tests (pure functions, no AppKit / no window):
- `resolve_position`: default when `None`; saved position honored when on-screen; clamped
  when off right/bottom edge; clamped when the saved display is gone (position far
  outside the frame); never returns a negative coordinate.
- amplitude: RMS of silence ≈ 0; RMS of full-scale ≈ 1; smoothing moves toward the target
  monotonically; output always in `0..=1`.
- `HudState::as_str` round-trip for every variant, so the Rust payload strings and the
  TypeScript `HudState` union cannot silently diverge.

Manual QA (documented as a script in `tasks/test-commands/`):
1. Island appears bottom-center at launch; expands on cursor approach.
2. Hold hotkey → waveform reacts to actual voice (goes flat when silent).
3. Stop button ends recording and text is injected; cancel discards.
4. Drag to a new position, quit, relaunch → position restored.
5. `show_hud = false` → no island, tray still works.
6. Trigger a mic error → red island with a readable message, auto-clears.

## Out of scope

Live transcript bubbles, native Swift rendering, adjustable opacity, iOS (the island is
macOS-only, matching `hud/panel.rs`'s existing `cfg`).
