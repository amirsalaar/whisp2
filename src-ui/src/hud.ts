// The floating island: an always-on-top pill that shows dictation state next to
// whatever app the user is dictating into.
//
// Runs in its own transparent WebviewWindow (`hud.html`), not in the React
// settings app — so this is plain imperative DOM, deliberately dependency-free.
//
// Backend contract (`src-tauri/src/hud/panel.rs`):
//   - event `hud_state`   → "<state>" or "<state>:<label>"
//   - event `audio_level` → number in 0..1, ~20x/sec while recording
//   - command `hud_stop_recording` / `hud_cancel_recording` / `hud_save_position`

import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

// Must stay in lockstep with `HudState::as_str` in panel.rs.
type HudState =
  | 'collapsed-idle'
  | 'expanded-idle'
  | 'recording-controls'
  | 'processing'
  | 'error'
  | 'hidden';

const ALL_STATES: readonly HudState[] = [
  'collapsed-idle', 'expanded-idle', 'recording-controls',
  'processing', 'error', 'hidden',
];

function isHudState(value: string): value is HudState {
  return (ALL_STATES as readonly string[]).includes(value);
}

// ─── Waveform ────────────────────────────────────────────────────────────────

// Per-bar amplitude weights, so the bars don't all move as one block. Sampled
// once per bar index, wrapping for waveforms with more bars than entries.
const BAR_PATTERN = [0.24, 0.42, 0.68, 0.92, 0.78, 0.54, 0.36, 0.62, 0.86, 0.58];

interface WaveformConfig {
  barCount: number;
  barWidth: number;    // px
  spacing: number;     // px gap between bars
  frameHeight: number; // px, the waveform's fixed box
  minBarHeight: number;
  /** How much of the bar's travel comes from the idle shimmer, in px. */
  animLift: number;
  /** How much comes from the live mic level, in px at full scale. */
  voiceLiftScale: number;
}

const WAVEFORM_RECORDING: WaveformConfig = {
  barCount: 12, barWidth: 3, spacing: 3,
  frameHeight: 24, minBarHeight: 4, animLift: 3, voiceLiftScale: 16,
};

let currentLevel = 0;
let animClock = 0;    // seconds
let activeCfg: WaveformConfig | null = null;
let currentState: HudState = 'collapsed-idle';

/**
 * Whether the user has asked for reduced motion.
 *
 * The idle shimmer is driven from JS, not CSS, so the `prefers-reduced-motion`
 * block in hud.css can't reach it — dropping the bars' CSS transition there
 * actually made the shimmer *steppier* rather than calmer. Honouring the query
 * here drops the shimmer entirely, leaving the bars driven purely by the real
 * mic level (which is information, not decoration).
 */
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');

function barHeight(index: number, cfg: WaveformConfig): number {
  const level = Math.max(0, Math.min(currentLevel, 1));
  // Sine shimmer, phase-shifted per bar so the row ripples instead of pulsing.
  const phase = (Math.sin(animClock * 7 + index * 0.8) + 1) / 2;
  const shimmer = reducedMotion.matches ? 0 : phase * cfg.animLift;
  const voice = level * (BAR_PATTERN[index % BAR_PATTERN.length] ?? 0.5) * cfg.voiceLiftScale;
  return Math.min(cfg.minBarHeight + shimmer + voice, cfg.frameHeight);
}

/** The live bars, captured at build time so the loop needn't re-query the DOM. */
let activeBars: HTMLElement[] = [];

function makeWaveform(cfg: WaveformConfig): HTMLElement {
  const wrap = document.createElement('div');
  wrap.className = 'waveform';
  wrap.style.height = `${cfg.frameHeight}px`;
  wrap.style.gap = `${cfg.spacing}px`;
  const bars: HTMLElement[] = [];
  for (let i = 0; i < cfg.barCount; i++) {
    const bar = document.createElement('div');
    bar.className = 'waveform-bar';
    bar.style.width = `${cfg.barWidth}px`;
    bar.style.height = `${cfg.minBarHeight}px`;
    wrap.appendChild(bar);
    bars.push(bar);
  }
  activeBars = bars;
  return wrap;
}

/**
 * The in-flight `requestAnimationFrame` handle, or `null` while idle.
 *
 * The loop only runs while a waveform is on screen — which is only during
 * recording. An always-on rAF would keep the compositor awake for the entire
 * session (the island's resting state is a motionless nub), costing battery to
 * animate nothing.
 */
let rafHandle: number | null = null;
/** Timestamp of the previous frame, for a real elapsed-time delta. */
let lastFrameMs: number | null = null;

function tickWaveform(nowMs: number): void {
  const cfg = activeCfg;
  if (!cfg || activeBars.length === 0) {
    rafHandle = null;
    lastFrameMs = null;
    return;
  }
  // Advance by measured elapsed time, not an assumed 1/60s: on a 120Hz ProMotion
  // display a fixed per-frame step runs the shimmer at double speed.
  if (lastFrameMs !== null) {
    animClock += (nowMs - lastFrameMs) / 1000;
  }
  lastFrameMs = nowMs;
  for (let i = 0; i < activeBars.length; i++) {
    activeBars[i]!.style.height = `${barHeight(i, cfg)}px`;
  }
  rafHandle = requestAnimationFrame(tickWaveform);
}

/** Starts the waveform loop if a waveform is showing and it isn't already running. */
function syncWaveformLoop(): void {
  if (activeCfg && activeBars.length > 0) {
    if (rafHandle === null) rafHandle = requestAnimationFrame(tickWaveform);
  } else if (rafHandle !== null) {
    cancelAnimationFrame(rafHandle);
    rafHandle = null;
    lastFrameMs = null;
  }
}

// ─── Drag ────────────────────────────────────────────────────────────────────

const appWindow = getCurrentWindow();

/** Persist where the user dropped the island, in logical (CSS-pixel) coordinates. */
async function savePosition(): Promise<void> {
  const [physical, scale] = await Promise.all([
    appWindow.outerPosition(),
    appWindow.scaleFactor(),
  ]);
  const { x, y } = physical.toLogical(scale);
  await invoke('hud_save_position', { x, y });
}

/**
 * Makes `el` a drag handle for the whole island window.
 *
 * `startDragging` hands the gesture to the OS, so we never see a mouseup — the
 * drop is detected by [`watchForDrops`] instead, which debounces the window's own
 * move events.
 */
function makeDraggable(el: HTMLElement): void {
  el.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    // Buttons inside the pill must stay clickable, not start a drag.
    if (e.target instanceof Element && e.target.closest('button')) return;
    e.preventDefault();
    userIsDragging = true;
    appWindow.startDragging().catch((err) => console.error('island drag failed:', err));
  });
}

/**
 * Whether the move events now arriving belong to a drag the user started.
 *
 * The island can also be moved by macOS — a resolution change or a display being
 * unplugged repositions windows — and those moves must not be persisted, or the
 * island would adopt a spot the user never chose and stop recentering afterwards.
 * Set on mousedown, cleared once the resulting drop has been saved.
 */
let userIsDragging = false;

/**
 * How long the window must hold still before a move is treated as a drop.
 *
 * Only needs to outlast the gap between move events within one gesture, so a
 * user who pauses mid-drag isn't recorded as having dropped it there.
 */
const DROP_SETTLE_MS = 450;

/**
 * Persists the island's position once it stops moving.
 *
 * Deliberately driven by the window's `Moved` events rather than by the drag
 * gesture: `startDragging` hands the gesture to the OS, so there is no mouseup to
 * hook, and anything that infers the drop from the *gesture* gets it wrong in two
 * ways. Timing out on stillness saves a mid-drag pause and then never corrects it
 * (no second mousedown follows). Saving unconditionally on mousedown persists a
 * position for a plain click that never moved anything — which for a
 * never-dragged island means writing its auto-computed spot into config.json,
 * pinning it there so it stops recentering when the resolution, Dock, or display
 * arrangement changes.
 *
 * Watching moves avoids both: a click that doesn't move the window emits nothing,
 * and a pause emits nothing either, so the debounce resolves on the real drop.
 */
async function watchForDrops(): Promise<UnlistenFn> {
  let settle: ReturnType<typeof setTimeout> | undefined;
  return appWindow.onMoved(() => {
    if (!userIsDragging) return;
    clearTimeout(settle);
    settle = setTimeout(() => {
      userIsDragging = false;
      savePosition().catch((err) => console.error('saving island position failed:', err));
    }, DROP_SETTLE_MS);
  });
}

// ─── State renderers ─────────────────────────────────────────────────────────

/** The caption shown in `expanded-idle`; the backend can override it per-event. */
let idleLabel = 'Hold your hotkey to dictate';
/** The failure text shown in `error`, from the FSM's `RecordingState::Error`. */
let errorLabel = 'Something went wrong';

function pill(state: HudState): HTMLElement {
  const el = document.createElement('div');
  el.className = `hud-pill ${state}`;
  return el;
}

function renderCollapsedIdle(root: HTMLElement): void {
  activeCfg = null;
  // A bare handle: small, dim, and click-through (the window ignores the cursor
  // in this state), purely a "Whisp is alive and lives here" marker.
  root.appendChild(pill('collapsed-idle'));
}

function renderExpandedIdle(root: HTMLElement): void {
  activeCfg = null;
  const el = pill('expanded-idle');

  const title = document.createElement('div');
  title.className = 'hud-title';
  title.textContent = 'Whisp';

  const subtitle = document.createElement('div');
  subtitle.className = 'hud-subtitle';
  subtitle.textContent = idleLabel;

  el.appendChild(title);
  el.appendChild(subtitle);
  makeDraggable(el);
  root.appendChild(el);
}

function renderRecordingControls(root: HTMLElement): void {
  activeCfg = WAVEFORM_RECORDING;
  const el = pill('recording-controls');

  const cancel = document.createElement('button');
  cancel.className = 'hud-btn';
  cancel.title = 'Discard';
  cancel.setAttribute('aria-label', 'Discard recording');
  cancel.textContent = '✕';
  cancel.addEventListener('click', () => {
    invoke('hud_cancel_recording').catch(console.error);
  });

  const stop = document.createElement('button');
  stop.className = 'hud-btn stop';
  stop.title = 'Stop and transcribe';
  stop.setAttribute('aria-label', 'Stop and transcribe');
  const square = document.createElement('div');
  square.className = 'stop-square';
  stop.appendChild(square);
  stop.addEventListener('click', () => {
    invoke('hud_stop_recording').catch(console.error);
  });

  el.appendChild(cancel);
  el.appendChild(makeWaveform(WAVEFORM_RECORDING));
  el.appendChild(stop);
  makeDraggable(el);
  root.appendChild(el);
}

function renderProcessing(root: HTMLElement): void {
  activeCfg = null;
  const el = pill('processing');
  const spinner = document.createElement('div');
  spinner.className = 'hud-spinner';
  const status = document.createElement('span');
  status.className = 'hud-status';
  status.textContent = 'Transcribing…';
  el.appendChild(spinner);
  el.appendChild(status);
  root.appendChild(el);
}

function renderError(root: HTMLElement): void {
  activeCfg = null;
  const el = pill('error');
  // The pill is click-through and auto-dismisses after a few seconds, so a
  // screen-reader user gets no other chance to hear the failure.
  el.setAttribute('role', 'alert');
  const icon = document.createElement('span');
  icon.className = 'hud-error-icon';
  icon.textContent = '!';
  const status = document.createElement('span');
  status.className = 'hud-status';
  status.textContent = errorLabel;
  el.appendChild(icon);
  el.appendChild(status);
  root.appendChild(el);
}

// ─── Main render ─────────────────────────────────────────────────────────────

function renderState(state: HudState): void {
  const root = document.getElementById('hud-root');
  if (!root) return;
  while (root.firstChild) root.removeChild(root.firstChild);
  // The old bars just left the DOM; a renderer that draws a waveform repopulates
  // this, and the loop stops for any state that doesn't.
  activeBars = [];

  switch (state) {
    case 'collapsed-idle':     renderCollapsedIdle(root); break;
    case 'expanded-idle':      renderExpandedIdle(root); break;
    case 'recording-controls': renderRecordingControls(root); break;
    case 'processing':         renderProcessing(root); break;
    case 'error':              renderError(root); break;
    case 'hidden':             activeCfg = null; break;
  }
  syncWaveformLoop();
}

function setState(state: HudState, label?: string): void {
  if (label !== undefined) {
    if (state === 'error') errorLabel = label;
    else if (state === 'expanded-idle') idleLabel = label;
  }
  // Re-render on an unchanged state only when its caption actually changed —
  // otherwise a repeated event would restart the CSS enter animation.
  if (state === currentState && label === undefined) return;
  currentState = state;
  // The mic level is per-recording; a stale peak would make the next session's
  // waveform open mid-swing.
  if (state !== 'recording-controls') currentLevel = 0;
  renderState(state);
}

/** Parses a `hud_state` payload: `"<state>"` or `"<state>:<label>"`. */
export function parseHudStatePayload(raw: string): { state: HudState; label?: string } | null {
  const colon = raw.indexOf(':');
  if (colon === -1) {
    return isHudState(raw) ? { state: raw } : null;
  }
  const state = raw.slice(0, colon);
  if (!isHudState(state)) return null;
  // Labels can themselves contain colons (URLs, "error: 401"), so only split once.
  return { state, label: raw.slice(colon + 1) };
}

// ─── Boot ────────────────────────────────────────────────────────────────────

const unlisteners: UnlistenFn[] = [];

/**
 * Whether a `hud_state` event has been applied yet.
 *
 * Guards the boot-time adopt below against a lost update: `hud_current_state`
 * returns whatever the backend had recorded when the command *ran*, but a live
 * event can arrive while that call is still in flight. Applying the older
 * snapshot afterwards would clobber the newer state — reintroducing, in a
 * narrower window, the very "island disagrees with the backend" bug the adopt
 * exists to fix. A live event always wins.
 */
let liveStateApplied = false;

async function init(): Promise<void> {
  renderState('collapsed-idle');
  document.body.style.opacity = '1';

  // One listener for the window's whole lifetime — the pill is rebuilt on every
  // state change, so per-pill move listeners would accumulate.
  unlisteners.push(await watchForDrops());

  // Expand/collapse on hover is driven entirely by the cursor poll in
  // `hud/panel.rs`, not by DOM mouse events: the window is click-through while
  // collapsed (see `HudState::needs_mouse_events`), so it receives no mouseenter
  // at all in the one state where expanding matters.
  unlisteners.push(await listen<string>('hud_state', (event) => {
    const parsed = parseHudStatePayload(event.payload);
    if (!parsed) {
      console.warn('unknown hud_state payload:', event.payload);
      return;
    }
    liveStateApplied = true;
    setState(parsed.state, parsed.label);
  }));

  unlisteners.push(await listen<number>('audio_level', (event) => {
    currentLevel = Math.min(Math.max(event.payload, 0), 1);
  }));

  // Adopt whatever state the backend is already in. The island is painted from
  // Rust while this webview is still loading, and Tauri events aren't buffered —
  // so the paint that happened before the listener above existed was dropped, and
  // the backend won't re-send it because it only emits on change. Without this the
  // island can sit visibly collapsed while the backend considers it expanded (and
  // therefore clickable), which is exactly the state a hover during startup
  // produces.
  const pending = await invoke<string | null>('hud_current_state');
  if (pending !== null && !liveStateApplied) {
    const parsed = parseHudStatePayload(pending);
    if (parsed) setState(parsed.state, parsed.label);
  }
}

function dispose(): void {
  while (unlisteners.length) {
    unlisteners.pop()?.();
  }
  if (rafHandle !== null) {
    cancelAnimationFrame(rafHandle);
    rafHandle = null;
  }
}

init().catch(console.error);

if (import.meta.hot) {
  import.meta.hot.dispose(() => dispose());
}
