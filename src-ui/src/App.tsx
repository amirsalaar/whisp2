import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

interface AppConfig {
  provider: "open_a_i" | "groq" | "gemini";
  recording_mode: "press_and_hold" | "toggle";
  hotkey:
    | "left_option"
    | "right_option"
    | "left_command"
    | "right_command"
    | "right_control";
  openai_api_url: string;
  openai_model: string;
  groq_api_url: string;
  groq_model: string;
  gemini_model: string;
  play_completion_sound: boolean;
  save_history: boolean;
  show_hud: boolean;
  language: string | null;
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

type Tab = "settings" | "history" | "dictionary";

export default function App() {
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
  const [accessibility, setAccessibility] = useState(true);
  const [statusMsg, setStatusMsg] = useState("");
  const [dictEntries, setDictEntries] = useState<DictEntry[]>([]);
  const [dictFrom, setDictFrom] = useState("");
  const [dictTo, setDictTo] = useState("");

  useEffect(() => {
    invoke<AppConfig>("get_config").then(setConfig);
    invoke<boolean>("check_accessibility").then(setAccessibility);
    invoke<string | null>("get_api_key", { keyName: "openai_api_key" }).then(
      (k) => k && setOpenaiKey("••••••••")
    );
    invoke<string | null>("get_api_key", { keyName: "groq_api_key" }).then(
      (k) => k && setGroqKey("••••••••")
    );
    invoke<string | null>("get_api_key", { keyName: "gemini_api_key" }).then(
      (k) => k && setGeminiKey("••••••••")
    );
  }, []);

  useEffect(() => {
    if (tab === "history") {
      invoke<HistoryEntry[]>("get_history", { limit: 100 }).then(setHistory);
    }
  }, [tab]);

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
      setStatusMsg("API key saved to Keychain.");
      setTimeout(() => setStatusMsg(""), 2000);
    } catch (e) {
      setStatusMsg(`Error: ${e}`);
    }
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

  if (!config) {
    return <div className="loading">Loading...</div>;
  }

  return (
    <div className="app">
      <nav className="sidebar">
        <div className="brand">
          <span className="brand-icon">🎙</span>
          <span className="brand-name">Whisp</span>
        </div>
        <button
          className={`nav-item ${tab === "settings" ? "active" : ""}`}
          onClick={() => setTab("settings")}
        >
          Settings
        </button>
        <button
          className={`nav-item ${tab === "history" ? "active" : ""}`}
          onClick={() => setTab("history")}
        >
          History
        </button>
      </nav>

      <main className="content">
        {!accessibility && (
          <div className="banner warning">
            <strong>Accessibility permission required</strong> — hotkey
            recording is disabled.{" "}
            <button
              className="link-btn"
              onClick={() => invoke("open_accessibility_settings")}
            >
              Open Settings →
            </button>
          </div>
        )}

        {tab === "settings" && (
          <div className="panel">
            <h2>Transcription</h2>

            <div className="field">
              <label>Provider</label>
              <select
                value={config.provider}
                onChange={(e) =>
                  setConfig({ ...config, provider: e.target.value as AppConfig["provider"] })
                }
              >
                <option value="open_a_i">OpenAI Whisper</option>
                <option value="groq">Groq Whisper</option>
                <option value="gemini">Gemini</option>
              </select>
            </div>

            {config.provider === "open_a_i" && (
              <>
                <div className="field">
                  <label>OpenAI API Key</label>
                  <div className="input-row">
                    <input
                      type={openaiKeyMasked ? "password" : "text"}
                      value={openaiKey}
                      onChange={(e) => setOpenaiKey(e.target.value)}
                      placeholder="sk-..."
                      onFocus={() => {
                        if (openaiKeyMasked) setOpenaiKey("");
                        setOpenaiKeyMasked(false);
                      }}
                    />
                    <button
                      className="btn-secondary"
                      onClick={() =>
                        saveKey("openai_api_key", openaiKey, () => {
                          setOpenaiKey("••••••••");
                          setOpenaiKeyMasked(true);
                        })
                      }
                    >
                      Save
                    </button>
                  </div>
                </div>

                <div className="field">
                  <label>API URL</label>
                  <input
                    type="text"
                    value={config.openai_api_url}
                    onChange={(e) =>
                      setConfig({ ...config, openai_api_url: e.target.value })
                    }
                  />
                </div>

                <div className="field">
                  <label>Model</label>
                  <select
                    value={config.openai_model}
                    onChange={(e) =>
                      setConfig({ ...config, openai_model: e.target.value })
                    }
                  >
                    <option value="whisper-1">whisper-1</option>
                    <option value="gpt-4o-transcribe">gpt-4o-transcribe</option>
                    <option value="gpt-4o-mini-transcribe">gpt-4o-mini-transcribe</option>
                  </select>
                </div>
              </>
            )}

            {config.provider === "groq" && (
              <>
                <div className="field">
                  <label>Groq API Key</label>
                  <div className="input-row">
                    <input
                      type={groqKeyMasked ? "password" : "text"}
                      value={groqKey}
                      onChange={(e) => setGroqKey(e.target.value)}
                      placeholder="gsk_..."
                      onFocus={() => {
                        if (groqKeyMasked) setGroqKey("");
                        setGroqKeyMasked(false);
                      }}
                    />
                    <button
                      className="btn-secondary"
                      onClick={() =>
                        saveKey("groq_api_key", groqKey, () => {
                          setGroqKey("••••••••");
                          setGroqKeyMasked(true);
                        })
                      }
                    >
                      Save
                    </button>
                  </div>
                </div>

                <div className="field">
                  <label>API URL</label>
                  <input
                    type="text"
                    value={config.groq_api_url}
                    onChange={(e) =>
                      setConfig({ ...config, groq_api_url: e.target.value })
                    }
                  />
                </div>

                <div className="field">
                  <label>Model</label>
                  <select
                    value={config.groq_model}
                    onChange={(e) =>
                      setConfig({ ...config, groq_model: e.target.value })
                    }
                  >
                    <option value="whisper-large-v3-turbo">whisper-large-v3-turbo</option>
                    <option value="whisper-large-v3">whisper-large-v3</option>
                    <option value="distil-whisper-large-v3-en">distil-whisper-large-v3-en</option>
                  </select>
                </div>
              </>
            )}

            {config.provider === "gemini" && (
              <>
                <div className="field">
                  <label>Gemini API Key</label>
                  <div className="input-row">
                    <input
                      type={geminiKeyMasked ? "password" : "text"}
                      value={geminiKey}
                      onChange={(e) => setGeminiKey(e.target.value)}
                      placeholder="AIza..."
                      onFocus={() => {
                        if (geminiKeyMasked) setGeminiKey("");
                        setGeminiKeyMasked(false);
                      }}
                    />
                    <button
                      className="btn-secondary"
                      onClick={() =>
                        saveKey("gemini_api_key", geminiKey, () => {
                          setGeminiKey("••••••••");
                          setGeminiKeyMasked(true);
                        })
                      }
                    >
                      Save
                    </button>
                  </div>
                </div>

                <div className="field">
                  <label>Model</label>
                  <select
                    value={config.gemini_model}
                    onChange={(e) =>
                      setConfig({ ...config, gemini_model: e.target.value })
                    }
                  >
                    <option value="gemini-2.0-flash">gemini-2.0-flash</option>
                    <option value="gemini-1.5-flash">gemini-1.5-flash</option>
                    <option value="gemini-1.5-pro">gemini-1.5-pro</option>
                  </select>
                </div>
              </>
            )}

            <div className="field">
              <label>Language (optional)</label>
              <input
                type="text"
                value={config.language ?? ""}
                placeholder="e.g. en, fa, de"
                onChange={(e) =>
                  setConfig({
                    ...config,
                    language: e.target.value || null,
                  })
                }
              />
            </div>

            <h2>Recording</h2>

            <div className="field">
              <label>Hotkey</label>
              <select
                value={config.hotkey}
                onChange={(e) =>
                  setConfig({ ...config, hotkey: e.target.value as AppConfig["hotkey"] })
                }
              >
                <option value="right_command">Right Command ⌘</option>
                <option value="left_option">Left Option ⌥</option>
                <option value="right_option">Right Option ⌥</option>
                <option value="left_command">Left Command ⌘</option>
                <option value="right_control">Right Control ⌃</option>
              </select>
            </div>

            <div className="field">
              <label>Mode</label>
              <select
                value={config.recording_mode}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    recording_mode: e.target.value as AppConfig["recording_mode"],
                  })
                }
              >
                <option value="press_and_hold">Press and Hold</option>
                <option value="toggle" disabled>
                  Toggle (coming soon)
                </option>
              </select>
            </div>

            <h2>Preferences</h2>

            <div className="field checkbox">
              <label>
                <input
                  type="checkbox"
                  checked={config.show_hud}
                  onChange={(e) =>
                    setConfig({ ...config, show_hud: e.target.checked })
                  }
                />
                Show floating HUD while recording
              </label>
            </div>

            <div className="field checkbox">
              <label>
                <input
                  type="checkbox"
                  checked={config.play_completion_sound}
                  onChange={(e) =>
                    setConfig({ ...config, play_completion_sound: e.target.checked })
                  }
                />
                Play completion sound after transcription
              </label>
            </div>

            <div className="field checkbox">
              <label>
                <input
                  type="checkbox"
                  checked={config.save_history}
                  onChange={(e) =>
                    setConfig({ ...config, save_history: e.target.checked })
                  }
                />
                Save transcription history
              </label>
            </div>

            <div className="actions">
              <button className="btn-primary" onClick={saveConfig} disabled={saving}>
                {saving ? "Saving..." : "Save Settings"}
              </button>
              {statusMsg && <span className="status">{statusMsg}</span>}
            </div>
          </div>
        )}

        {tab === "history" && (
          <div className="panel">
            <div className="history-header">
              <h2>Transcription History</h2>
              {history.length > 0 && (
                <button className="btn-secondary" onClick={clearHistory}>
                  Clear All
                </button>
              )}
            </div>
            {history.length === 0 ? (
              <p className="empty">No transcriptions yet.</p>
            ) : (
              <ul className="history-list">
                {history.map((entry) => (
                  <li key={entry.id} className="history-entry">
                    <div className="entry-text">{entry.text}</div>
                    <div className="entry-meta">
                      <span>{new Date(entry.created_at).toLocaleString()}</span>
                      <span>{entry.word_count}w</span>
                      {entry.source_app && <span>{entry.source_app}</span>}
                      <button
                        className="copy-btn"
                        onClick={() => copyEntry(entry.id, entry.text)}
                      >
                        {copiedId === entry.id ? "✓" : "Copy"}
                      </button>
                      <button
                        className="delete-btn"
                        onClick={() => deleteEntry(entry.id)}
                      >
                        ✕
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </main>
    </div>
  );
}
