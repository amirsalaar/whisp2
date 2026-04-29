import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

interface AppConfig {
  provider: "open_a_i" | "gemini";
  recording_mode: "press_and_hold" | "toggle";
  hotkey:
    | "left_option"
    | "right_option"
    | "left_command"
    | "right_command"
    | "right_control";
  openai_api_url: string;
  openai_model: string;
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

type Tab = "settings" | "history";

export default function App() {
  const [tab, setTab] = useState<Tab>("settings");
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [apiKeyMasked, setApiKeyMasked] = useState(true);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [saving, setSaving] = useState(false);
  const [accessibility, setAccessibility] = useState(true);
  const [statusMsg, setStatusMsg] = useState("");

  useEffect(() => {
    invoke<AppConfig>("get_config").then(setConfig);
    invoke<boolean>("check_accessibility").then(setAccessibility);
    invoke<string | null>("get_api_key", { keyName: "openai_api_key" }).then(
      (k) => k && setApiKey("••••••••")
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

  async function saveApiKey() {
    try {
      await invoke("set_api_key", { keyName: "openai_api_key", value: apiKey });
      setApiKey("••••••••");
      setApiKeyMasked(true);
      setStatusMsg("API key saved to Keychain.");
      setTimeout(() => setStatusMsg(""), 2000);
    } catch (e) {
      setStatusMsg(`Error: ${e}`);
    }
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
                  setConfig({ ...config, provider: e.target.value as any })
                }
              >
                <option value="open_a_i">OpenAI Whisper</option>
                <option value="gemini" disabled>
                  Gemini (coming soon)
                </option>
              </select>
            </div>

            <div className="field">
              <label>OpenAI API Key</label>
              <div className="input-row">
                <input
                  type={apiKeyMasked ? "password" : "text"}
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                  placeholder="sk-..."
                  onFocus={() => {
                    if (apiKeyMasked) setApiKey("");
                    setApiKeyMasked(false);
                  }}
                />
                <button className="btn-secondary" onClick={saveApiKey}>
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
              </select>
            </div>

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
                  setConfig({ ...config, hotkey: e.target.value as any })
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
                    recording_mode: e.target.value as any,
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
            <h2>Transcription History</h2>
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
