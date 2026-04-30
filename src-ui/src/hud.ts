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

const BAR_WEIGHTS = [0.24, 0.42, 0.68, 0.92, 0.68, 0.42, 0.24];

let currentLevel = 0;
let animTime = 0;
let currentState: HudState = 'collapsed-idle';

function getRoot(): HTMLElement {
  return document.getElementById('hud-root')!;
}

function clearRoot(root: HTMLElement): void {
  while (root.firstChild) {
    root.removeChild(root.firstChild);
  }
}

function createWaveform(count: number): HTMLElement {
  const waveform = document.createElement('div');
  waveform.className = 'waveform';
  waveform.id = 'waveform';
  for (let i = 0; i < count; i++) {
    const bar = document.createElement('div');
    bar.className = 'waveform-bar';
    bar.style.height = '4px';
    waveform.appendChild(bar);
  }
  return waveform;
}

function renderPill(state: HudState): void {
  const root = getRoot();
  clearRoot(root);

  const pill = document.createElement('div');
  pill.className = `hud-pill ${state}`;

  if (state === 'expanded-idle') {
    const title = document.createElement('div');
    title.className = 'hud-title';
    title.textContent = 'Whisp';
    const sub = document.createElement('div');
    sub.className = 'hud-subtitle';
    sub.textContent = 'Hold ⌘ to start dictating';
    pill.appendChild(title);
    pill.appendChild(sub);
  } else if (state === 'shortcut-listening') {
    pill.appendChild(createWaveform(7));
  } else if (state === 'recording-controls') {
    const cancel = document.createElement('button');
    cancel.className = 'hud-btn';
    cancel.textContent = '✕';
    cancel.addEventListener('click', () =>
      invoke('hud_cancel_recording').catch(console.error)
    );
    const stop = document.createElement('button');
    stop.className = 'hud-btn stop';
    stop.textContent = '■';
    stop.addEventListener('click', () =>
      invoke('hud_stop_recording').catch(console.error)
    );
    pill.appendChild(cancel);
    pill.appendChild(createWaveform(5));
    pill.appendChild(stop);
  } else if (state === 'processing') {
    const status = document.createElement('div');
    status.className = 'hud-status';
    status.textContent = 'Transcribing…';
    pill.appendChild(status);
  }

  root.appendChild(pill);
}

function animateWaveform(): void {
  animTime += 1 / 60;
  const waveform = document.getElementById('waveform');
  if (waveform) {
    const bars = waveform.querySelectorAll<HTMLElement>('.waveform-bar');
    bars.forEach((bar, i) => {
      const weight = BAR_WEIGHTS[i] ?? 0.5;
      const idle = Math.sin(animTime * 6 + i * 0.8) * 0.3 + 0.3;
      const driven = currentLevel * weight;
      const height = Math.max(idle, driven) * 20 + 4;
      bar.style.height = `${height}px`;
    });
  }
  requestAnimationFrame(animateWaveform);
}

function setState(state: HudState): void {
  if (state === currentState) return;
  currentState = state;
  renderPill(state);
}

async function init(): Promise<void> {
  renderPill('collapsed-idle');
  document.body.style.opacity = '1';

  await listen<string>('hud_state', (event) => {
    const state = event.payload as HudState;
    if (ALL_STATES.includes(state)) setState(state);
  });

  await listen<number>('audio_level', (event) => {
    currentLevel = Math.min(event.payload, 1.0);
  });

  await listen<boolean>('hud_proximity', (event) => {
    if (currentState === 'collapsed-idle' && event.payload) {
      setState('expanded-idle');
    } else if (currentState === 'expanded-idle' && !event.payload) {
      setState('collapsed-idle');
    }
  });

  requestAnimationFrame(animateWaveform);
}

init().catch(console.error);
