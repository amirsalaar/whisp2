import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import "./App.css";
import Onboarding from "./Onboarding";

interface AppConfig {
  provider: "open_a_i" | "groq" | "gemini" | "local_whisper";
  recording_mode: "press_and_hold" | "toggle";
  hotkey:
    | "left_option"
    | "right_option"
    | "left_command"
    | "right_command"
    | "right_control"
    | "fn";
  openai_api_url: string;
  openai_model: string;
  groq_api_url: string;
  groq_model: string;
  gemini_model: string;
  play_completion_sound: boolean;
  save_history: boolean;
  show_hud: boolean;
  language: string | null;
  max_history_entries: number | null;
  local_whisper_model_path: string | null;
  input_device: string | null;
}

interface HistoryEntry {
  id: string;
  text: string;
  source_app: string | null;
  provider: string;
  word_count: number;
  char_count: number;
  created_at: string;
}

interface DictEntry {
  from: string;
  to: string;
}

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

type Tab = "settings" | "history" | "dictionary" | "permissions";

// ── Toggle component ──────────────────────────────────────
function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="toggle">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className="toggle-track" />
    </label>
  );
}

// ── Nav icons ─────────────────────────────────────────────
function IconSettings() {
  return (
    <svg className="nav-icon" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M8 10a2 2 0 100-4 2 2 0 000 4z" stroke="currentColor" strokeWidth="1.3"/>
      <path d="M12.7 6.5l.8-1.4-1.4-1.4-1.4.8A4 4 0 009 3.8L8.7 2h-2l-.3 1.8a4 4 0 00-1.7.7l-1.4-.8L1.9 5.1l.8 1.4A4 4 0 002.5 8c0 .38.05.75.15 1.1l-.79 1.35 1.41 1.41 1.35-.79c.52.35 1.1.6 1.72.73L6.68 14h2.64l.29-1.2c.62-.13 1.2-.38 1.72-.73l1.35.79 1.41-1.41-.79-1.35A4 4 0 0013.5 8c0-.54-.1-1.06-.27-1.5h-.53z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round"/>
    </svg>
  );
}

function IconHistory() {
  return (
    <svg className="nav-icon" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
      <circle cx="8" cy="8" r="5.5" stroke="currentColor" strokeWidth="1.3"/>
      <path d="M8 5.5V8l1.5 1.5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round"/>
    </svg>
  );
}

function IconDict() {
  return (
    <svg className="nav-icon" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M4 2h7a1 1 0 011 1v10a1 1 0 01-1 1H4a1 1 0 01-1-1V3a1 1 0 011-1z" stroke="currentColor" strokeWidth="1.3"/>
      <path d="M5.5 5.5h5M5.5 8h5M5.5 10.5h3" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round"/>
    </svg>
  );
}

function IconShield() {
  return (
    <svg className="nav-icon" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M8 1.5L2.5 3.5v4c0 3 2.3 5.3 5.5 6.5 3.2-1.2 5.5-3.5 5.5-6.5v-4L8 1.5z" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round"/>
      <path d="M5.5 8l1.5 1.5L10.5 6" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round"/>
    </svg>
  );
}

export default function App() {
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [tab, setTab] = useState<Tab>("settings");
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [openaiKey, setOpenaiKey] = useState("");
  const [openaiKeyMasked, setOpenaiKeyMasked] = useState(true);
  const [groqKey, setGroqKey] = useState("");
  const [groqKeyMasked, setGroqKeyMasked] = useState(true);
  const [geminiKey, setGeminiKey] = useState("");
  const [geminiKeyMasked, setGeminiKeyMasked] = useState(true);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [accessibility, setAccessibility] = useState<boolean | null>(null);
  const [microphone, setMicrophone] = useState<boolean | null>(null);
  const [inputMonitoring, setInputMonitoring] = useState<boolean | null>(null);
  const [checkingPerms, setCheckingPerms] = useState(false);
  const [historySearch, setHistorySearch] = useState("");
  const [statusMsg, setStatusMsg] = useState("");
  const [dictEntries, setDictEntries] = useState<DictEntry[]>([]);
  const [dictFrom, setDictFrom] = useState("");
  const [dictTo, setDictTo] = useState("");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [downloadedModels, setDownloadedModels] = useState<string[]>([]);
  const [downloadingModel, setDownloadingModel] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgress | null>(null);
  const [inputDevices, setInputDevices] = useState<string[]>([]);

  async function refreshPermissions() {
    setCheckingPerms(true);
    const [a, m, im] = await Promise.all([
      invoke<boolean>("check_accessibility"),
      invoke<boolean>("check_microphone"),
      invoke<boolean>("check_input_monitoring"),
    ]);
    setAccessibility(a);
    setMicrophone(m);
    setInputMonitoring(im);
    setCheckingPerms(false);
  }

  useEffect(() => {
    if (localStorage.getItem("whisp_onboarding_done") !== "1") {
      setShowOnboarding(true);
    }
    invoke<AppConfig>("get_config").then(setConfig);
    refreshPermissions();
    invoke<string | null>("get_api_key", { keyName: "openai_api_key" }).then(
      (k) => k && setOpenaiKey("••••••••")
    );
    invoke<string | null>("get_api_key", { keyName: "groq_api_key" }).then(
      (k) => k && setGroqKey("••••••••")
    );
    invoke<string | null>("get_api_key", { keyName: "gemini_api_key" }).then(
      (k) => k && setGeminiKey("••••••••")
    );
    invoke<ModelInfo[]>("list_whisper_models").then(setModels);
    invoke<string[]>("get_downloaded_models").then(setDownloadedModels);
    invoke<string[]>("list_audio_input_devices").then(setInputDevices);

    // Re-check permissions when window regains focus (user may have visited System Settings)
    const onFocus = () => refreshPermissions();
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

  useEffect(() => {
    if (tab === "history") invoke<HistoryEntry[]>("get_history", { limit: 100 }).then(setHistory);
    if (tab === "dictionary") loadDictionary();
  }, [tab]);

  async function loadDictionary() {
    invoke<DictEntry[]>("get_dictionary").then(setDictEntries);
  }

  async function addEntry() {
    if (!dictFrom.trim()) return;
    await invoke("add_dictionary_entry", { from: dictFrom.trim(), to: dictTo.trim() });
    setDictFrom(""); setDictTo("");
    await loadDictionary();
  }

  async function removeEntry(from: string) {
    await invoke("remove_dictionary_entry", { from });
    await loadDictionary();
  }

  async function saveConfig() {
    if (!config) return;
    setSaving(true);
    try {
      await invoke("set_config", { config });
      setStatusMsg("Saved.");
      setTimeout(() => setStatusMsg(""), 2000);
    } catch (e) {
      setStatusMsg(`Error: ${e}`);
    } finally {
      setSaving(false);
    }
  }

  async function saveKey(keyName: string, value: string, onSaved: () => void) {
    try {
      await invoke("set_api_key", { keyName, value });
      onSaved();
      setStatusMsg("API key saved.");
      setTimeout(() => setStatusMsg(""), 2000);
    } catch (e) {
      setStatusMsg(`Error: ${e}`);
    }
  }

  async function pickModel() {
    const path = await openDialog({ filters: [{ name: "GGML Model", extensions: ["bin"] }] });
    if (path && config) setConfig({ ...config, local_whisper_model_path: path as string });
  }

  async function downloadModel(name: string) {
    if (!config) return;
    setDownloadingModel(name); setDownloadProgress(null);
    try {
      const path = await invoke<string>("download_whisper_model", { modelName: name });
      setDownloadedModels((prev) => [...prev, name]);
      const updated = { ...config, local_whisper_model_path: path };
      setConfig(updated);
      await invoke("set_config", { config: updated });
    } catch (e) {
      if (String(e) !== "Download aborted") {
        setStatusMsg(`Download failed: ${e}`);
        setTimeout(() => setStatusMsg(""), 4000);
      }
    } finally {
      setDownloadingModel(null); setDownloadProgress(null);
    }
  }

  async function selectDownloadedModel(m: ModelInfo) {
    if (!config) return;
    const dir = await invoke<string>("get_models_dir");
    const path = `${dir}/${m.filename}`;
    const updated = { ...config, local_whisper_model_path: path };
    setConfig(updated);
    await invoke("set_config", { config: updated });
  }

  async function clearHistory() {
    await invoke("clear_history");
    setHistory([]);
  }

  async function copyEntry(id: string, text: string) {
    await navigator.clipboard.writeText(text);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 1500);
  }

  async function deleteEntry(id: string) {
    await invoke("delete_history_entry", { id });
    setHistory((h) => h.filter((e) => e.id !== id));
  }

  if (showOnboarding) {
    return (
      <Onboarding
        onComplete={() => {
          localStorage.setItem("whisp_onboarding_done", "1");
          setShowOnboarding(false);
          invoke<AppConfig>("get_config").then(setConfig);
        }}
      />
    );
  }

  if (!config) return <div className="loading">Loading…</div>;

  const filteredHistory = history.filter(
    (e) => historySearch === "" || e.text.toLowerCase().includes(historySearch.toLowerCase())
  );

  return (
    <div className="app">
      {/* ── Sidebar ── */}
      <nav className="sidebar">
        <div className="brand">
          <svg className="brand-icon" width="18" height="18" viewBox="0 0 18 18" xmlns="http://www.w3.org/2000/svg">
            <rect x="0.5"  y="7.5" width="2.5" height="5"   rx="1.25" fill="currentColor" opacity="0.5"/>
            <rect x="4"    y="5"   width="2.5" height="9"   rx="1.25" fill="currentColor" opacity="0.75"/>
            <rect x="7.75" y="2"   width="2.5" height="14"  rx="1.25" fill="currentColor" opacity="1"/>
            <rect x="11.5" y="5"   width="2.5" height="9"   rx="1.25" fill="currentColor" opacity="0.75"/>
            <rect x="15"   y="7.5" width="2.5" height="5"   rx="1.25" fill="currentColor" opacity="0.5"/>
          </svg>
          <span className="brand-name">Whisp</span>
        </div>
        <div className="sidebar-nav">
          <button className={`nav-item ${tab === "settings" ? "active" : ""}`} onClick={() => setTab("settings")}>
            <IconSettings /> Settings
          </button>
          <button className={`nav-item ${tab === "history" ? "active" : ""}`} onClick={() => setTab("history")}>
            <IconHistory /> History
          </button>
          <button className={`nav-item ${tab === "dictionary" ? "active" : ""}`} onClick={() => setTab("dictionary")}>
            <IconDict /> Dictionary
          </button>
          <button className={`nav-item ${tab === "permissions" ? "active" : ""}`} onClick={() => setTab("permissions")}>
            <span className="nav-icon-wrap">
              <IconShield />
              {(accessibility === false || microphone === false || inputMonitoring === false) && <span className="nav-badge" />}
            </span>
            Permissions
          </button>
        </div>
      </nav>

      {/* ── Content ── */}
      <main className="content">

        {/* ── Settings tab ── */}
        {tab === "settings" && (
          <>
            <div className="page-header">
              <h1 className="page-title">Settings</h1>
            </div>

            {/* Transcription section */}
            <div className="section-group">
              <div className="section-label">Transcription</div>
              <div className="settings-card">
                <div className="settings-row">
                  <div className="row-label">
                    <span className="row-title">Provider</span>
                  </div>
                  <div className="row-control">
                    <select
                      className="row-select"
                      value={config.provider}
                      onChange={(e) => setConfig({ ...config, provider: e.target.value as AppConfig["provider"] })}
                    >
                      <option value="open_a_i">OpenAI Whisper</option>
                      <option value="groq">Groq Whisper</option>
                      <option value="gemini">Gemini</option>
                      <option value="local_whisper">Local (on-device)</option>
                    </select>
                  </div>
                </div>

                {/* OpenAI fields */}
                {config.provider === "open_a_i" && (<>
                  <div className="settings-row">
                    <div className="row-label">
                      <span className="row-title">API Key</span>
                    </div>
                    <div className="row-control key-row">
                      <input
                        className="row-input key-input"
                        type={openaiKeyMasked ? "password" : "text"}
                        value={openaiKey}
                        onChange={(e) => setOpenaiKey(e.target.value)}
                        placeholder="sk-..."
                        onFocus={() => { if (openaiKeyMasked) setOpenaiKey(""); setOpenaiKeyMasked(false); }}
                      />
                      <button className="btn-secondary" onClick={() => saveKey("openai_api_key", openaiKey, () => { setOpenaiKey("••••••••"); setOpenaiKeyMasked(true); })}>
                        Save
                      </button>
                    </div>
                  </div>
                  <div className="settings-row">
                    <div className="row-label">
                      <span className="row-title">API URL</span>
                    </div>
                    <div className="row-control">
                      <input className="row-input" type="text" value={config.openai_api_url}
                        onChange={(e) => setConfig({ ...config, openai_api_url: e.target.value })} />
                    </div>
                  </div>
                  <div className="settings-row">
                    <div className="row-label">
                      <span className="row-title">Model</span>
                    </div>
                    <div className="row-control">
                      <select className="row-select" value={config.openai_model}
                        onChange={(e) => setConfig({ ...config, openai_model: e.target.value })}>
                        <option value="whisper-1">whisper-1</option>
                        <option value="gpt-4o-transcribe">gpt-4o-transcribe</option>
                        <option value="gpt-4o-mini-transcribe">gpt-4o-mini-transcribe</option>
                      </select>
                    </div>
                  </div>
                </>)}

                {/* Groq fields */}
                {config.provider === "groq" && (<>
                  <div className="settings-row">
                    <div className="row-label"><span className="row-title">API Key</span></div>
                    <div className="row-control key-row">
                      <input className="row-input key-input"
                        type={groqKeyMasked ? "password" : "text"} value={groqKey}
                        onChange={(e) => setGroqKey(e.target.value)} placeholder="gsk_..."
                        onFocus={() => { if (groqKeyMasked) setGroqKey(""); setGroqKeyMasked(false); }}
                      />
                      <button className="btn-secondary" onClick={() => saveKey("groq_api_key", groqKey, () => { setGroqKey("••••••••"); setGroqKeyMasked(true); })}>
                        Save
                      </button>
                    </div>
                  </div>
                  <div className="settings-row">
                    <div className="row-label"><span className="row-title">API URL</span></div>
                    <div className="row-control">
                      <input className="row-input" type="text" value={config.groq_api_url}
                        onChange={(e) => setConfig({ ...config, groq_api_url: e.target.value })} />
                    </div>
                  </div>
                  <div className="settings-row">
                    <div className="row-label"><span className="row-title">Model</span></div>
                    <div className="row-control">
                      <select className="row-select" value={config.groq_model}
                        onChange={(e) => setConfig({ ...config, groq_model: e.target.value })}>
                        <option value="whisper-large-v3-turbo">whisper-large-v3-turbo</option>
                        <option value="whisper-large-v3">whisper-large-v3</option>
                        <option value="distil-whisper-large-v3-en">distil-whisper-large-v3-en</option>
                      </select>
                    </div>
                  </div>
                </>)}

                {/* Gemini fields */}
                {config.provider === "gemini" && (<>
                  <div className="settings-row">
                    <div className="row-label"><span className="row-title">API Key</span></div>
                    <div className="row-control key-row">
                      <input className="row-input key-input"
                        type={geminiKeyMasked ? "password" : "text"} value={geminiKey}
                        onChange={(e) => setGeminiKey(e.target.value)} placeholder="AIza..."
                        onFocus={() => { if (geminiKeyMasked) setGeminiKey(""); setGeminiKeyMasked(false); }}
                      />
                      <button className="btn-secondary" onClick={() => saveKey("gemini_api_key", geminiKey, () => { setGeminiKey("••••••••"); setGeminiKeyMasked(true); })}>
                        Save
                      </button>
                    </div>
                  </div>
                  <div className="settings-row">
                    <div className="row-label"><span className="row-title">Model</span></div>
                    <div className="row-control">
                      <select className="row-select" value={config.gemini_model}
                        onChange={(e) => setConfig({ ...config, gemini_model: e.target.value })}>
                        <option value="gemini-2.0-flash">gemini-2.0-flash</option>
                        <option value="gemini-1.5-flash">gemini-1.5-flash</option>
                        <option value="gemini-1.5-pro">gemini-1.5-pro</option>
                      </select>
                    </div>
                  </div>
                </>)}

                {/* Language always visible */}
                <div className="settings-row">
                  <div className="row-label">
                    <span className="row-title">Language</span>
                    <span className="row-desc">Leave empty for auto-detect</span>
                  </div>
                  <div className="row-control">
                    <input className="row-input" type="text" value={config.language ?? ""}
                      placeholder="en, fa, de…"
                      onChange={(e) => setConfig({ ...config, language: e.target.value || null })} />
                  </div>
                </div>
              </div>
            </div>

            {/* Local model catalog */}
            {config.provider === "local_whisper" && (
              <div className="section-group">
                <div className="section-label">Model</div>
                <div className="model-catalog">
                  {models.map((m) => {
                    const isDownloaded = downloadedModels.includes(m.name);
                    const isActive = !!config.local_whisper_model_path?.includes(m.filename);
                    const isDownloading = downloadingModel === m.name;
                    const progress = isDownloading ? downloadProgress : null;
                    const pct = progress && progress.total > 0 ? Math.round((progress.downloaded / progress.total) * 100) : 0;
                    return (
                      <div key={m.name} className={`model-row${isActive ? " active" : ""}`}>
                        <div className="model-info">
                          <span className="model-name">{m.name}</span>
                          <span className="model-meta">{m.description} · {m.size_mb} MB</span>
                        </div>
                        <div className="model-action">
                          {isActive && <span className="badge-active">Active</span>}
                          {isDownloaded && !isActive && (
                            <button className="btn-secondary" onClick={() => selectDownloadedModel(m)}>Use</button>
                          )}
                          {!isDownloaded && !isDownloading && (
                            <button className="btn-secondary" onClick={() => downloadModel(m.name)}>Download</button>
                          )}
                          {isDownloading && (
                            <div className="download-progress">
                              <div className="progress-bar"><div className="progress-fill" style={{ width: `${pct}%` }} /></div>
                              <span className="progress-pct">{pct}%</span>
                              <button className="btn-ghost" onClick={() => invoke("abort_model_download")} title="Abort">✕</button>
                            </div>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
                <div className="model-custom">
                  <span className="model-custom-label">Custom:</span>
                  <input className="model-path-input" type="text" value={config.local_whisper_model_path ?? ""} placeholder="No model selected" readOnly />
                  <button className="btn-secondary" onClick={pickModel}>Browse…</button>
                </div>
              </div>
            )}

            {/* Recording section */}
            <div className="section-group">
              <div className="section-label">Recording</div>
              <div className="settings-card">
                <div className="settings-row">
                  <div className="row-label">
                    <span className="row-title">Hotkey</span>
                    <span className="row-desc">Hold to record or toggle</span>
                  </div>
                  <div className="row-control">
                    <select className="row-select" value={config.hotkey}
                      onChange={(e) => setConfig({ ...config, hotkey: e.target.value as AppConfig["hotkey"] })}>
                      <option value="right_command">Right ⌘</option>
                      <option value="left_option">Left ⌥</option>
                      <option value="right_option">Right ⌥</option>
                      <option value="left_command">Left ⌘</option>
                      <option value="right_control">Right ⌃</option>
                      <option value="fn">Fn / Globe 🌐</option>
                    </select>
                  </div>
                </div>
                <div className="settings-row">
                  <div className="row-label">
                    <span className="row-title">Mode</span>
                  </div>
                  <div className="row-control">
                    <select className="row-select" value={config.recording_mode}
                      onChange={(e) => setConfig({ ...config, recording_mode: e.target.value as AppConfig["recording_mode"] })}>
                      <option value="press_and_hold">Press and Hold</option>
                      <option value="toggle">Toggle</option>
                    </select>
                  </div>
                </div>
                <div className="settings-row">
                  <div className="row-label">
                    <span className="row-title">Microphone</span>
                  </div>
                  <div className="row-control">
                    <select className="row-select" value={config.input_device ?? ""}
                      onChange={(e) => setConfig({ ...config, input_device: e.target.value || null })}>
                      <option value="">System Default</option>
                      {inputDevices.map((d) => <option key={d} value={d}>{d}</option>)}
                    </select>
                  </div>
                </div>
              </div>
            </div>

            {/* Preferences section */}
            <div className="section-group">
              <div className="section-label">Preferences</div>
              <div className="settings-card">
                <div className="settings-row">
                  <div className="row-label">
                    <span className="row-title">Show floating HUD</span>
                    <span className="row-desc">Displays while recording</span>
                  </div>
                  <div className="row-control">
                    <Toggle checked={config.show_hud} onChange={(v) => setConfig({ ...config, show_hud: v })} />
                  </div>
                </div>
                <div className="settings-row">
                  <div className="row-label">
                    <span className="row-title">Completion sound</span>
                    <span className="row-desc">Plays after transcription</span>
                  </div>
                  <div className="row-control">
                    <Toggle checked={config.play_completion_sound} onChange={(v) => setConfig({ ...config, play_completion_sound: v })} />
                  </div>
                </div>
                <div className="settings-row">
                  <div className="row-label">
                    <span className="row-title">Save history</span>
                  </div>
                  <div className="row-control">
                    <Toggle checked={config.save_history} onChange={(v) => setConfig({ ...config, save_history: v })} />
                  </div>
                </div>
                {config.save_history && (
                  <div className="settings-row">
                    <div className="row-label">
                      <span className="row-title">Keep at most</span>
                    </div>
                    <div className="row-control">
                      <select className="row-select" value={config.max_history_entries ?? ""}
                        onChange={(e) => setConfig({ ...config, max_history_entries: e.target.value ? Number(e.target.value) : null })}>
                        <option value="">Unlimited</option>
                        <option value="100">100 entries</option>
                        <option value="250">250 entries</option>
                        <option value="500">500 entries</option>
                        <option value="1000">1000 entries</option>
                      </select>
                    </div>
                  </div>
                )}
              </div>
            </div>

            <div className="save-row">
              <button className="btn-primary" onClick={saveConfig} disabled={saving}>
                {saving ? "Saving…" : "Save Settings"}
              </button>
              {statusMsg && <span className="status">{statusMsg}</span>}
            </div>
          </>
        )}

        {/* ── History tab ── */}
        {tab === "history" && (
          <>
            <div className="page-header">
              <h1 className="page-title">History</h1>
            </div>
            <div className="history-header">
              <input className="search-input" type="search" placeholder="Search transcriptions…"
                value={historySearch} onChange={(e) => setHistorySearch(e.target.value)} />
              {history.length > 0 && (
                <button className="btn-secondary" onClick={clearHistory}>Clear All</button>
              )}
            </div>
            {filteredHistory.length === 0 ? (
              <div className="empty-state">
                <div className="empty-state-icon">◎</div>
                <div className="empty-state-title">No transcriptions yet</div>
                <div className="empty-state-desc">Hold your hotkey and speak — your transcriptions will appear here.</div>
              </div>
            ) : (
              <ul className="history-list">
                {filteredHistory.map((entry) => (
                  <li key={entry.id} className="history-entry">
                    <div className="entry-text">{entry.text}</div>
                    <div className="entry-meta">
                      <span>{new Date(entry.created_at).toLocaleString()}</span>
                      <span>{entry.word_count}w</span>
                      {entry.source_app && <span>{entry.source_app}</span>}
                      <span className="entry-meta-spacer" />
                      <button className="copy-btn" onClick={() => copyEntry(entry.id, entry.text)}>
                        {copiedId === entry.id ? "✓" : "Copy"}
                      </button>
                      <button className="delete-btn" onClick={() => deleteEntry(entry.id)}>✕</button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}

        {/* ── Dictionary tab ── */}
        {tab === "dictionary" && (
          <>
            <div className="page-header">
              <h1 className="page-title">Dictionary</h1>
            </div>
            <div className="dict-add-row">
              <input className="row-input" type="text" placeholder="From (e.g. whisp rs)"
                value={dictFrom} onChange={(e) => setDictFrom(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && addEntry()} />
              <input className="row-input" type="text" placeholder="To (e.g. whisp-rs)"
                value={dictTo} onChange={(e) => setDictTo(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && addEntry()} />
              <button className="btn-primary" onClick={addEntry}>Add</button>
            </div>
            {dictEntries.length === 0 ? (
              <div className="empty-state">
                <div className="empty-state-icon">→</div>
                <div className="empty-state-title">No substitutions yet</div>
                <div className="empty-state-desc">Add word substitutions applied after every transcription. Matches whole words.</div>
              </div>
            ) : (
              <div className="settings-card">
                {dictEntries.map((entry) => (
                  <div key={entry.from} className="dict-entry">
                    <span className="dict-from">{entry.from}</span>
                    <span className="dict-arrow">→</span>
                    <span className="dict-to">{entry.to}</span>
                    <button className="delete-btn" style={{ marginLeft: "auto" }} onClick={() => removeEntry(entry.from)}>✕</button>
                  </div>
                ))}
              </div>
            )}
          </>
        )}
        {/* ── Permissions tab ── */}
        {tab === "permissions" && (
          <>
            <div className="page-header">
              <h1 className="page-title">Permissions</h1>
              <p className="page-subtitle">Whisp needs system access to record audio and detect hotkeys.</p>
            </div>

            {/* Microphone card */}
            <div className="section-group">
              <div className="section-label">Microphone</div>
              <div className="permission-card">
                <div className="permission-header">
                  <div className="permission-title-row">
                    <span className="permission-title">Microphone Access</span>
                    <PermissionBadge granted={microphone} checking={checkingPerms} />
                  </div>
                  <p className="permission-desc">Allows Whisp to capture audio from your microphone for transcription.</p>
                </div>
                <div className="permission-actions">
                  <button
                    className="btn-secondary"
                    disabled={microphone === true}
                    onClick={async () => {
                      await invoke("request_microphone");
                      setTimeout(() => refreshPermissions(), 1000);
                    }}
                  >
                    Request Access
                  </button>
                  <button className="btn-ghost" onClick={() => invoke("open_microphone_settings")}>
                    Open Settings ↗
                  </button>
                  <button className="btn-ghost" onClick={refreshPermissions} disabled={checkingPerms}>
                    {checkingPerms ? "Checking…" : "Refresh"}
                  </button>
                </div>
                <p className="permission-footer">Required — Whisp cannot record without microphone access.</p>
              </div>
            </div>

            {/* Accessibility card */}
            <div className="section-group">
              <div className="section-label">Accessibility</div>
              <div className="permission-card">
                <div className="permission-header">
                  <div className="permission-title-row">
                    <span className="permission-title">Accessibility Access</span>
                    <PermissionBadge granted={accessibility} checking={checkingPerms} />
                  </div>
                  <p className="permission-desc">Required to detect global hotkey presses (via CGEventTap) and inject transcribed text into other apps.</p>
                </div>
                <div className="permission-actions">
                  <button className="btn-secondary" onClick={() => invoke("open_accessibility_settings")}>
                    Open Settings ↗
                  </button>
                  <button className="btn-ghost" onClick={refreshPermissions} disabled={checkingPerms}>
                    {checkingPerms ? "Checking…" : "Refresh"}
                  </button>
                </div>
                <p className="permission-footer">After granting access in System Settings, click Refresh — macOS does not notify the app automatically.</p>
              </div>
            </div>

            {/* Input Monitoring card */}
            <div className="section-group">
              <div className="section-label">Input Monitoring</div>
              <div className="permission-card">
                <div className="permission-header">
                  <div className="permission-title-row">
                    <span className="permission-title">Input Monitoring</span>
                    <PermissionBadge granted={inputMonitoring} checking={checkingPerms} />
                  </div>
                  <p className="permission-desc">Monitors keyboard events globally to detect when your hotkey is pressed.</p>
                </div>
                <div className="permission-actions">
                  <button
                    className="btn-secondary"
                    disabled={inputMonitoring === true}
                    onClick={async () => {
                      await invoke("request_input_monitoring");
                      setTimeout(() => refreshPermissions(), 1000);
                    }}
                  >
                    Grant Access
                  </button>
                  <button className="btn-ghost" onClick={() => invoke("open_input_monitoring_settings")}>
                    Open Settings ↗
                  </button>
                  <button className="btn-ghost" onClick={refreshPermissions} disabled={checkingPerms}>
                    {checkingPerms ? "Checking…" : "Refresh"}
                  </button>
                </div>
                <p className="permission-footer">Required — without Input Monitoring, macOS may silently disable the CGEventTap used to detect your hotkey.</p>
              </div>
            </div>
          </>
        )}
      </main>
    </div>
  );
}

// ── Permission status badge ────────────────────────────────
function PermissionBadge({ granted, checking }: { granted: boolean | null; checking: boolean }) {
  if (checking || granted === null) return <span className="status-badge checking">● Checking…</span>;
  if (granted) return <span className="status-badge granted">● Granted</span>;
  return <span className="status-badge required">● Required</span>;
}
