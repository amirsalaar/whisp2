import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./Onboarding.css";

interface ModelInfo {
  name: string;
  filename: string;
  size_mb: number;
  description: string;
}

interface DownloadProgress {
  model_name: string;
  downloaded: number;
  total: number;
}

type Provider = "open_a_i" | "groq" | "gemini" | "local_whisper";
type Hotkey = "right_command" | "left_option" | "right_option" | "left_command" | "right_control" | "fn";

const STEPS = ["welcome", "microphone", "engine", "engine_setup", "hotkey", "permissions", "done"] as const;

const ENGINE_LABELS: Record<Provider, string> = {
  open_a_i: "OpenAI Whisper",
  groq: "Groq",
  gemini: "Google Gemini",
  local_whisper: "Local (on-device)",
};

const HOTKEY_OPTS: { value: Hotkey; label: string; glyph: string }[] = [
  { value: "right_command", label: "Right ⌘ Command", glyph: "⌘" },
  { value: "right_option", label: "Right ⌥ Option", glyph: "⌥" },
  { value: "left_option", label: "Left ⌥ Option", glyph: "⌥" },
  { value: "left_command", label: "Left ⌘ Command", glyph: "⌘" },
  { value: "right_control", label: "Right ⌃ Control", glyph: "⌃" },
  { value: "fn", label: "Fn / Globe 🌐", glyph: "🌐" },
];

function MicSvg({ size, pulse }: { size: number; pulse?: boolean }) {
  return (
    <svg
      width={size} height={size} viewBox="0 0 44 44" fill="none"
      className={pulse ? "ob-mic-pulse" : undefined}
    >
      <rect x="15" y="5" width="14" height="20" rx="7" fill="#e8a928" opacity="0.9" />
      <path d="M9 22c0 7.18 5.82 13 13 13s13-5.82 13-13" stroke="#e8a928" strokeWidth="2.5" strokeLinecap="round" fill="none" opacity="0.7" />
      <line x1="22" y1="35" x2="22" y2="41" stroke="#e8a928" strokeWidth="2.5" strokeLinecap="round" opacity="0.6" />
      <line x1="16" y1="41" x2="28" y2="41" stroke="#e8a928" strokeWidth="2.5" strokeLinecap="round" opacity="0.6" />
    </svg>
  );
}

export default function Onboarding({ onComplete }: { onComplete: () => void }) {
  const [stepIdx, setStepIdx] = useState(0);
  const [animKey, setAnimKey] = useState(0);

  // microphone step
  const [micGranted, setMicGranted] = useState<boolean | null>(null);
  const [micRequesting, setMicRequesting] = useState(false);

  // engine step
  const [provider, setProvider] = useState<Provider>("open_a_i");

  // engine setup step
  const [apiKey, setApiKey] = useState("");
  const [keySaved, setKeySaved] = useState(false);
  const [savingKey, setSavingKey] = useState(false);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [downloadedModels, setDownloadedModels] = useState<string[]>([]);
  const [selectedModel, setSelectedModel] = useState("");
  const [downloadingModel, setDownloadingModel] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgress | null>(null);

  // hotkey step
  const [hotkey, setHotkey] = useState<Hotkey>("right_command");
  const [inputMonitoring, setInputMonitoring] = useState<boolean | null>(null);

  // permissions step
  const [accessibility, setAccessibility] = useState<boolean | null>(null);

  const step = STEPS[stepIdx];
  const progressPct = Math.round((stepIdx / (STEPS.length - 1)) * 100);

  useEffect(() => {
    invoke<boolean>("check_microphone").then(setMicGranted);
    invoke<boolean>("check_accessibility").then(setAccessibility);
    invoke<boolean>("check_input_monitoring").then(setInputMonitoring);
    invoke<ModelInfo[]>("list_whisper_models").then(setModels);
    invoke<string[]>("get_downloaded_models").then((dl) => {
      setDownloadedModels(dl);
      if (dl.length > 0) setSelectedModel(dl[0]);
    });

    const onFocus = async () => {
      const [m, a, im] = await Promise.all([
        invoke<boolean>("check_microphone"),
        invoke<boolean>("check_accessibility"),
        invoke<boolean>("check_input_monitoring"),
      ]);
      setMicGranted(m);
      setAccessibility(a);
      setInputMonitoring(im);
    };
    window.addEventListener("focus", onFocus);

    let unlisten: (() => void) | undefined;
    listen<DownloadProgress>("model_download_progress", (e) => {
      setDownloadProgress(e.payload);
    }).then((fn) => { unlisten = fn; });

    return () => {
      window.removeEventListener("focus", onFocus);
      unlisten?.();
    };
  }, []);

  function goTo(idx: number) {
    setStepIdx(idx);
    setAnimKey((k) => k + 1);
  }

  function next() { goTo(Math.min(stepIdx + 1, STEPS.length - 1)); }
  function prev() { goTo(Math.max(stepIdx - 1, 0)); }

  async function requestMic() {
    setMicRequesting(true);
    await invoke("request_microphone");
    setTimeout(async () => {
      const ok = await invoke<boolean>("check_microphone");
      setMicGranted(ok);
      setMicRequesting(false);
    }, 800);
  }

  async function saveApiKey() {
    if (!apiKey.trim()) return;
    setSavingKey(true);
    const keyName =
      provider === "open_a_i" ? "openai_api_key" :
      provider === "groq" ? "groq_api_key" :
      "gemini_api_key";
    try {
      await invoke("set_api_key", { keyName, value: apiKey.trim() });
      setKeySaved(true);
    } finally {
      setSavingKey(false);
    }
  }

  async function downloadModel(name: string) {
    setDownloadingModel(name);
    setDownloadProgress(null);
    try {
      const path = await invoke<string>("download_whisper_model", { modelName: name });
      setDownloadedModels((prev) => [...prev, name]);
      setSelectedModel(name);
      await invoke("set_config", {
        config: await invoke("get_config").then((cfg: any) => ({
          ...cfg,
          local_whisper_model_path: path,
          provider: "local_whisper",
        })),
      });
    } finally {
      setDownloadingModel(null);
      setDownloadProgress(null);
    }
  }

  async function finish() {
    const cfg = await invoke<any>("get_config");
    const keyName =
      provider === "open_a_i" ? "openai_api_key" :
      provider === "groq" ? "groq_api_key" :
      "gemini_api_key";

    if (provider !== "local_whisper" && keySaved) {
      await invoke("set_api_key", { keyName, value: apiKey.trim() });
    }

    let modelPath = cfg.local_whisper_model_path;
    if (provider === "local_whisper" && selectedModel) {
      const dir = await invoke<string>("get_models_dir");
      const found = models.find((m) => m.name === selectedModel);
      if (found) modelPath = `${dir}/${found.filename}`;
    }

    await invoke("set_config", {
      config: { ...cfg, provider, hotkey, local_whisper_model_path: modelPath },
    });

    onComplete();
  }

  const canContinue = (() => {
    if (step === "microphone") return micGranted === true;
    if (step === "engine_setup") {
      if (provider === "local_whisper") return downloadedModels.length > 0;
      return keySaved;
    }
    return true;
  })();

  const isFirst = stepIdx === 0;
  const isLast = step === "done";

  return (
    <div className="ob-shell">
      <div className="ob-progress">
        <div className="ob-progress-fill" style={{ width: `${progressPct}%` }} />
      </div>

      <div className="ob-body">
        <div className="ob-step" key={animKey}>
          {step === "welcome" && <WelcomeStep />}
          {step === "microphone" && (
            <MicrophoneStep
              granted={micGranted}
              requesting={micRequesting}
              onRequest={requestMic}
              onOpenSettings={() => invoke("open_microphone_settings")}
            />
          )}
          {step === "engine" && (
            <EngineStep provider={provider} onSelect={setProvider} />
          )}
          {step === "engine_setup" && (
            <EngineSetupStep
              provider={provider}
              apiKey={apiKey}
              keySaved={keySaved}
              savingKey={savingKey}
              models={models}
              downloadedModels={downloadedModels}
              selectedModel={selectedModel}
              downloadingModel={downloadingModel}
              downloadProgress={downloadProgress}
              onApiKeyChange={(v) => { setApiKey(v); setKeySaved(false); }}
              onSaveKey={saveApiKey}
              onSelectModel={setSelectedModel}
              onDownloadModel={downloadModel}
              onAbort={() => invoke("abort_model_download")}
            />
          )}
          {step === "hotkey" && (
            <HotkeyStep
              hotkey={hotkey}
              inputMonitoring={inputMonitoring}
              onSelect={setHotkey}
              onOpenInputMonitoring={() => invoke("open_input_monitoring_settings")}
              onRefresh={async () => setInputMonitoring(await invoke<boolean>("check_input_monitoring"))}
            />
          )}
          {step === "permissions" && (
            <PermissionsStep
              accessibility={accessibility}
              onOpenAccessibility={() => invoke("open_accessibility_settings")}
              onRefresh={async () => setAccessibility(await invoke<boolean>("check_accessibility"))}
            />
          )}
          {step === "done" && (
            <DoneStep provider={provider} hotkey={hotkey} />
          )}
        </div>
      </div>

      <div className="ob-footer">
        <div className="ob-footer-left">
          {!isFirst && !isLast && (
            <button className="ob-btn-ghost" onClick={prev}>← Back</button>
          )}
        </div>
        <div className="ob-footer-right">
          {isLast ? (
            <button className="ob-btn-primary" onClick={finish}>Open Whisp →</button>
          ) : step === "welcome" ? (
            <button className="ob-btn-primary" onClick={next}>Get Started →</button>
          ) : (
            <>
              {step !== "microphone" && step !== "engine_setup" && (
                <button className="ob-btn-ghost" style={{ marginRight: 12 }} onClick={next}>Skip</button>
              )}
              <button className="ob-btn-primary" onClick={next} disabled={!canContinue}>
                Continue →
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

// ── Step components ────────────────────────────────────────────

function WelcomeStep() {
  return (
    <>
      <div className="ob-icon">
        <MicSvg size={72} pulse />
      </div>
      <h1 className="ob-title">Welcome to Whisp</h1>
      <p className="ob-subtitle">
        Voice-to-text, right from your menu bar.<br />
        Let's get you set up in under a minute.
      </p>
    </>
  );
}

function MicrophoneStep({
  granted, requesting, onRequest, onOpenSettings,
}: {
  granted: boolean | null;
  requesting: boolean;
  onRequest: () => void;
  onOpenSettings: () => void;
}) {
  return (
    <>
      <div className="ob-icon">
        <MicSvg size={52} />
      </div>
      <h1 className="ob-title">Allow Microphone</h1>
      <p className="ob-subtitle">
        Whisp needs access to your microphone to capture audio for transcription.
        Your audio is processed and never stored permanently.
      </p>
      {granted ? (
        <div className="ob-key-saved" style={{ marginBottom: 28 }}>
          <span>✓</span> Microphone access granted
        </div>
      ) : (
        <div className="ob-perm-actions" style={{ marginBottom: 28 }}>
          <button className="ob-btn-secondary" onClick={onRequest} disabled={requesting}>
            {requesting ? "Requesting…" : "Allow Access"}
          </button>
          <button className="ob-btn-ghost" onClick={onOpenSettings}>
            Open Settings ↗
          </button>
        </div>
      )}
    </>
  );
}

function EngineStep({ provider, onSelect }: { provider: Provider; onSelect: (p: Provider) => void }) {
  const opts: { value: Provider; label: string; desc: string; tag: string; tagClass: string }[] = [
    { value: "open_a_i", label: "OpenAI Whisper", desc: "Cloud · Fast and accurate", tag: "Popular", tagClass: "ob-tag-fast" },
    { value: "groq", label: "Groq", desc: "Cloud · Blazing fast inference", tag: "Fast", tagClass: "ob-tag-fast" },
    { value: "gemini", label: "Google Gemini", desc: "Cloud · Multimodal model", tag: "Free tier", tagClass: "ob-tag-free" },
    { value: "local_whisper", label: "Local Whisper", desc: "On-device · Private, no API key", tag: "Offline", tagClass: "ob-tag-local" },
  ];
  return (
    <>
      <h1 className="ob-title">Choose Your Engine</h1>
      <p className="ob-subtitle">Select how Whisp transcribes your voice. You can change this later in Settings.</p>
      <div className="ob-cards">
        {opts.map((o) => (
          <button key={o.value} className={`ob-card${provider === o.value ? " selected" : ""}`} onClick={() => onSelect(o.value)}>
            <div className="ob-card-dot" />
            <div className="ob-card-body">
              <div className="ob-card-title">{o.label}</div>
              <div className="ob-card-desc">{o.desc}</div>
            </div>
            <span className={`ob-card-tag ${o.tagClass}`}>{o.tag}</span>
          </button>
        ))}
      </div>
    </>
  );
}

function EngineSetupStep({
  provider, apiKey, keySaved, savingKey,
  models, downloadedModels, selectedModel,
  downloadingModel, downloadProgress,
  onApiKeyChange, onSaveKey, onSelectModel, onDownloadModel, onAbort,
}: {
  provider: Provider;
  apiKey: string;
  keySaved: boolean;
  savingKey: boolean;
  models: ModelInfo[];
  downloadedModels: string[];
  selectedModel: string;
  downloadingModel: string | null;
  downloadProgress: DownloadProgress | null;
  onApiKeyChange: (v: string) => void;
  onSaveKey: () => void;
  onSelectModel: (m: string) => void;
  onDownloadModel: (m: string) => void;
  onAbort: () => void;
}) {
  const isLocal = provider === "local_whisper";
  const placeholder = provider === "open_a_i" ? "sk-…" : provider === "groq" ? "gsk_…" : "AIza…";

  const pct = downloadProgress && downloadProgress.total > 0
    ? Math.round((downloadProgress.downloaded / downloadProgress.total) * 100)
    : 0;

  const notDownloaded = models.filter((m) => !downloadedModels.includes(m.name));

  return (
    <>
      <h1 className="ob-title">Set Up {ENGINE_LABELS[provider]}</h1>
      {!isLocal && (
        <>
          <p className="ob-subtitle">Enter your API key. It's saved securely to the macOS Keychain.</p>
          <div className="ob-key-wrap">
            <div className="ob-key-label">API Key</div>
            <div className="ob-key-row">
              <input
                className="ob-key-input"
                type="password"
                placeholder={placeholder}
                value={apiKey}
                onChange={(e) => onApiKeyChange(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && onSaveKey()}
                autoFocus
              />
              <button className="ob-btn-secondary" onClick={onSaveKey} disabled={savingKey || !apiKey.trim()}>
                {savingKey ? "Saving…" : "Save"}
              </button>
            </div>
            {keySaved && (
              <div className="ob-key-saved">
                <span>✓</span> Saved to Keychain
              </div>
            )}
          </div>
        </>
      )}
      {isLocal && (
        <>
          <p className="ob-subtitle">Download a Whisper model to transcribe audio on-device. No API key required.</p>
          {downloadedModels.length > 0 && (
            <div className="ob-key-wrap">
              <div className="ob-key-label">Active model</div>
              <select className="ob-model-select" value={selectedModel} onChange={(e) => onSelectModel(e.target.value)}>
                {downloadedModels.map((n) => <option key={n} value={n}>{n}</option>)}
              </select>
            </div>
          )}
          {downloadingModel ? (
            <div className="ob-dl-progress">
              <div className="ob-key-label" style={{ marginBottom: 10 }}>Downloading {downloadingModel}…</div>
              <div className="ob-dl-bar">
                <div className="ob-dl-fill" style={{ width: `${pct}%` }} />
              </div>
              <div className="ob-dl-meta">
                <span>{pct}%</span>
                <button className="ob-btn-ghost" style={{ fontSize: 11, padding: 0 }} onClick={onAbort}>Cancel</button>
              </div>
            </div>
          ) : notDownloaded.length > 0 && (
            <div className="ob-cards">
              {notDownloaded.map((m) => (
                <button key={m.name} className="ob-card" onClick={() => onDownloadModel(m.name)}>
                  <div className="ob-card-body" style={{ textAlign: "left" }}>
                    <div className="ob-card-title">{m.name}</div>
                    <div className="ob-card-desc">{m.description} · {m.size_mb} MB</div>
                  </div>
                  <span className="ob-card-tag ob-tag-local">Download</span>
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </>
  );
}

function HotkeyStep({
  hotkey, inputMonitoring, onSelect, onOpenInputMonitoring, onRefresh,
}: {
  hotkey: Hotkey;
  inputMonitoring: boolean | null;
  onSelect: (h: Hotkey) => void;
  onOpenInputMonitoring: () => void;
  onRefresh: () => void;
}) {
  return (
    <>
      <h1 className="ob-title">Choose Your Hotkey</h1>
      <p className="ob-subtitle">Hold this key to start dictating. You can change it later in Settings.</p>
      <div className="ob-hotkeys">
        {HOTKEY_OPTS.map((o) => (
          <button key={o.value} className={`ob-hotkey-opt${hotkey === o.value ? " selected" : ""}`} onClick={() => onSelect(o.value)}>
            <span className="ob-hotkey-glyph">{o.glyph}</span>
            <span className="ob-hotkey-label">{o.label}</span>
          </button>
        ))}
      </div>
      <div className="ob-perm-row" style={{ marginBottom: 0 }}>
        <span className="ob-perm-label">Input Monitoring</span>
        {inputMonitoring
          ? <span className="ob-perm-granted">✓ Granted</span>
          : <span className="ob-perm-missing">Required to detect hotkeys</span>
        }
      </div>
      {!inputMonitoring && (
        <div className="ob-perm-actions" style={{ marginTop: 8 }}>
          <button className="ob-btn-secondary" onClick={onOpenInputMonitoring}>Open Settings ↗</button>
          <button className="ob-btn-ghost" onClick={onRefresh}>Refresh</button>
        </div>
      )}
    </>
  );
}

function PermissionsStep({
  accessibility, onOpenAccessibility, onRefresh,
}: {
  accessibility: boolean | null;
  onOpenAccessibility: () => void;
  onRefresh: () => void;
}) {
  return (
    <>
      <h1 className="ob-title">Grant Accessibility</h1>
      <p className="ob-subtitle">
        Accessibility access lets Whisp inject transcribed text into any app and detect your hotkey globally.
      </p>
      <div className="ob-perm-row">
        <span className="ob-perm-label">Accessibility</span>
        {accessibility
          ? <span className="ob-perm-granted">✓ Granted</span>
          : <span className="ob-perm-missing">Not granted yet</span>
        }
      </div>
      <div className="ob-perm-actions">
        <button className="ob-btn-secondary" onClick={onOpenAccessibility}>Open System Settings ↗</button>
        <button className="ob-btn-ghost" onClick={onRefresh}>Refresh</button>
      </div>
      <p style={{ fontSize: 12, color: "rgba(240,237,232,0.38)", margin: 0, maxWidth: 360 }}>
        After granting access in System Settings, click Refresh — macOS doesn't notify apps automatically.
      </p>
    </>
  );
}

function DoneStep({ provider, hotkey }: { provider: Provider; hotkey: Hotkey }) {
  const hotkeyLabel = HOTKEY_OPTS.find((o) => o.value === hotkey)?.label ?? hotkey;
  return (
    <>
      <div className="ob-icon" style={{ fontSize: 52, lineHeight: 1 }}>◎</div>
      <h1 className="ob-title">You're All Set</h1>
      <p className="ob-subtitle">Whisp is ready. Hold your hotkey and start dictating.</p>
      <div className="ob-summary">
        <div className="ob-summary-row">
          <span className="ob-summary-key">Engine</span>
          <span className="ob-summary-val">{ENGINE_LABELS[provider]}</span>
        </div>
        <div className="ob-summary-row">
          <span className="ob-summary-key">Hotkey</span>
          <span className="ob-summary-val">{hotkeyLabel}</span>
        </div>
      </div>
    </>
  );
}
