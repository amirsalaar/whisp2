import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

type HudState =
  | 'collapsed-idle'
  | 'expanded-idle'
  | 'shortcut-listening'
  | 'recording-controls'
  | 'processing'
  | 'hidden';

const ALL_STATES: HudState[] = [
  'collapsed-idle', 'expanded-idle', 'shortcut-listening',
  'recording-controls', 'processing', 'hidden',
];

// Bar amplitude weights — matches Swift source pattern array
const BAR_PATTERN = [0.24, 0.42, 0.68, 0.92, 0.78, 0.54, 0.36, 0.62, 0.86, 0.58];

let currentLevel = 0;
let animClock = 0;  // seconds, incremented at 24fps
let currentState: HudState = 'collapsed-idle';

function getRoot(): HTMLElement {
  return document.getElementById('hud-root')!;
}

// ─── Waveform ────────────────────────────────────────────────────────────────

interface WaveformConfig {
  barCount: number;
  barWidth: number;   // px
  spacing: number;    // px gap between bars
  frameHeight: number;
  minBarHeight: number;
  animLift: number;
  voiceLiftScale: number;
}

const WAVEFORM_SHORTCUT: WaveformConfig = {
  barCount: 7, barWidth: 2, spacing: 2,
  frameHeight: 12, minBarHeight: 3, animLift: 2.4, voiceLiftScale: 5.2,
};

const WAVEFORM_RECORDING: WaveformConfig = {
  barCount: 10, barWidth: 3, spacing: 3,
  frameHeight: 24, minBarHeight: 5, animLift: 5, voiceLiftScale: 10,
};

function barHeight(index: number, cfg: WaveformConfig): number {
  const level = Math.max(0.08, Math.min(currentLevel, 1));
  const phase = (Math.sin(animClock * 7 + index * 0.8) + 1) / 2; // 0..1
  const animatedLift = 0.8 + phase * cfg.animLift;
  const voiceLift = level * (BAR_PATTERN[index % BAR_PATTERN.length] ?? 0.5) * cfg.voiceLiftScale;
  return cfg.minBarHeight + animatedLift + voiceLift;
}

function makeWaveform(cfg: WaveformConfig): HTMLElement {
  const wrap = document.createElement('div');
  wrap.className = 'waveform';
  wrap.id = 'waveform';
  wrap.style.height = `${cfg.frameHeight}px`;
  wrap.style.gap = `${cfg.spacing}px`;
  for (let i = 0; i < cfg.barCount; i++) {
    const bar = document.createElement('div');
    bar.className = 'waveform-bar';
    bar.style.width = `${cfg.barWidth}px`;
    bar.style.height = `${cfg.minBarHeight}px`;
    wrap.appendChild(bar);
  }
  return wrap;
}

let activeCfg: WaveformConfig | null = null;

function tickWaveform(): void {
  animClock += 1 / 24;
  const waveform = document.getElementById('waveform');
  if (waveform && activeCfg) {
    const cfg = activeCfg;
    const bars = waveform.querySelectorAll<HTMLElement>('.waveform-bar');
    bars.forEach((bar, i) => {
      bar.style.height = `${barHeight(i, cfg)}px`;
    });
  }
  requestAnimationFrame(tickWaveform);
}

// ─── State renderers ─────────────────────────────────────────────────────────

function renderCollapsedIdle(root: HTMLElement): void {
  activeCfg = null;
  const wrap = document.createElement('div');
  wrap.className = 'collapsed-idle-container';
  const bar = document.createElement('div');
  bar.className = 'collapsed-handle';
  wrap.appendChild(bar);
  // Client-side fallback: clicking the collapsed handle expands immediately
  // without waiting for the Rust proximity monitor to fire.
  wrap.addEventListener('click', () => setState('expanded-idle'));
  root.appendChild(wrap);
}

function renderExpandedIdle(root: HTMLElement, label: string): void {
  activeCfg = null;
  const wrap = document.createElement('div');
  wrap.className = 'expanded-idle-container';

  // Tooltip (hidden by default, shown on hover)
  const tooltip = document.createElement('div');
  tooltip.className = 'tooltip';
  tooltip.textContent = label;
  wrap.appendChild(tooltip);

  // Main pill
  const mainPill = document.createElement('div');
  mainPill.className = 'expanded-main-pill';
  const mainText = document.createElement('span');
  mainText.textContent = label;
  mainPill.appendChild(mainText);

  // Dots pill
  const dotsPill = document.createElement('div');
  dotsPill.className = 'dots-pill';
  for (let i = 0; i < 8; i++) {
    const dot = document.createElement('div');
    dot.className = 'dot';
    dotsPill.appendChild(dot);
  }

  wrap.appendChild(mainPill);
  wrap.appendChild(dotsPill);

  // Hover: show tooltip
  wrap.addEventListener('mouseenter', () => tooltip.classList.add('visible'));
  wrap.addEventListener('mouseleave', () => tooltip.classList.remove('visible'));

  root.appendChild(wrap);
}

function renderShortcutListening(root: HTMLElement): void {
  activeCfg = WAVEFORM_SHORTCUT;
  const wrap = document.createElement('div');
  wrap.className = 'shortcut-listening-container';
  wrap.appendChild(makeWaveform(WAVEFORM_SHORTCUT));
  root.appendChild(wrap);
}

function renderRecordingControls(root: HTMLElement): void {
  activeCfg = WAVEFORM_RECORDING;
  const wrap = document.createElement('div');
  wrap.className = 'recording-controls-container';

  // Cancel button (X)
  const cancel = document.createElement('button');
  cancel.className = 'hud-cancel';
  cancel.textContent = '✕';
  cancel.addEventListener('click', () => invoke('hud_cancel_recording').catch(console.error));

  // Waveform
  const waveform = makeWaveform(WAVEFORM_RECORDING);

  // Stop button (red circle + white square)
  const stop = document.createElement('button');
  stop.className = 'hud-stop';
  const circle = document.createElement('div');
  circle.className = 'stop-circle';
  const square = document.createElement('div');
  square.className = 'stop-square';
  stop.appendChild(circle);
  stop.appendChild(square);
  stop.addEventListener('click', () => invoke('hud_stop_recording').catch(console.error));

  wrap.appendChild(cancel);
  wrap.appendChild(waveform);
  wrap.appendChild(stop);
  root.appendChild(wrap);
}

function renderProcessing(root: HTMLElement): void {
  activeCfg = null;
  const wrap = document.createElement('div');
  wrap.className = 'processing-container';
  const text = document.createElement('span');
  text.textContent = 'Transcribing…';
  wrap.appendChild(text);
  root.appendChild(wrap);
}

// ─── Main render ─────────────────────────────────────────────────────────────

// Label shown in expanded-idle depends on whether microphone is granted.
// We default to the hotkey hint; the Rust side can emit a custom label via
// the hud_state payload as "expanded-idle:<label>" if needed.
let expandedLabel = 'Click or hold fn to start dictating';

function renderState(state: HudState): void {
  const root = getRoot();
  // Clear
  while (root.firstChild) root.removeChild(root.firstChild);

  switch (state) {
    case 'collapsed-idle':     renderCollapsedIdle(root); break;
    case 'expanded-idle':      renderExpandedIdle(root, expandedLabel); break;
    case 'shortcut-listening': renderShortcutListening(root); break;
    case 'recording-controls': renderRecordingControls(root); break;
    case 'processing':         renderProcessing(root); break;
    case 'hidden':             /* empty — root stays blank */ break;
  }
}

function setState(state: HudState, label?: string): void {
  if (label) expandedLabel = label;
  if (state === currentState && !label) return;
  currentState = state;
  renderState(state);
}

// ─── Boot ─────────────────────────────────────────────────────────────────────

async function init(): Promise<void> {
  renderState('collapsed-idle');
  document.body.style.opacity = '1';

  requestAnimationFrame(tickWaveform);

  // Expand/collapse on cursor enter/leave the HUD window.
  // The global NSEvent monitor in Rust goes silent when the cursor is inside our
  // own window, so we use browser mouseenter/mouseleave on <html> as the
  // reliable trigger. The window already has setIgnoresMouseEvents:NO so these fire.
  document.documentElement.addEventListener('mouseenter', () => {
    if (currentState === 'collapsed-idle') setState('expanded-idle');
  });
  document.documentElement.addEventListener('mouseleave', () => {
    if (currentState === 'expanded-idle') setState('collapsed-idle');
  });

  await listen<string>('hud_state', (event) => {
    const raw = event.payload;
    // Support "expanded-idle:Click to allow microphone" payload format
    const colonIdx = raw.indexOf(':');
    if (colonIdx !== -1) {
      const stateStr = raw.slice(0, colonIdx) as HudState;
      const labelStr = raw.slice(colonIdx + 1);
      if (ALL_STATES.includes(stateStr)) setState(stateStr, labelStr);
    } else {
      const state = raw as HudState;
      if (ALL_STATES.includes(state)) setState(state);
    }
  });

  await listen<number>('audio_level', (event) => {
    currentLevel = Math.min(Math.max(event.payload, 0), 1);
  });

}

init().catch(console.error);
