# Whisp2

macOS menu bar app for voice-to-text. Hold a hotkey, speak, release — transcribed text is typed directly into whatever app you're using. No clipboard. No switching windows.

Built with [Tauri 2](https://tauri.app) (Rust + React), targeting macOS 13+.

## Features

- **Press-and-hold or toggle** recording mode
- **Multiple transcription providers** — OpenAI Whisper, Groq, Gemini, or fully on-device via local Whisper (GGML)
- **Direct text injection** via CGEvent Unicode posting — works in any app, including terminals
- **Floating HUD** shows recording state without interrupting focus
- **Dictionary** — define word substitutions applied after every transcription
- **History** — searchable log of past transcriptions with per-entry copy/delete
- **No clipboard** — text is injected directly, never touches your clipboard

## Requirements

- macOS 13+
- Apple Silicon (aarch64)
- Three system permissions: **Accessibility**, **Input Monitoring**, **Microphone**

## Installation

Download the latest `.dmg` from [Releases](https://github.com/amirsalaar/whisp2/releases), mount it, and drag **Whisp2.app** to `/Applications`.

On first launch, grant the three required permissions from the Permissions tab in Settings.

> If macOS blocks the app on first open, right-click → **Open**.

## Transcription Providers

| Provider | Requires |
|---|---|
| OpenAI Whisper | OpenAI API key |
| Groq | Groq API key |
| Gemini | Google AI API key |
| Local Whisper | GGML `.bin` model file (downloadable from Settings) |

API keys are stored in the macOS Keychain, never on disk.

## Building from Source

**Prerequisites:** Rust (stable), Node 22+, Xcode Command Line Tools.

```sh
# Install frontend deps (first time only)
make ui-install

# Full dev session with hot-reload
make dev

# Production .app bundle
make build
```

Other useful commands:

```sh
make test      # Rust unit tests
make check     # Fast type-check (no link)
make lint-rs   # Clippy
make ui-lint   # ESLint
make fmt       # rustfmt
```

## Hotkeys

Configurable from Settings. Supported keys: Right ⌘, Left ⌥, Right ⌥, Left ⌘, Right ⌃, Fn/Globe.

## Data & Privacy

- Config: `~/Library/Application Support/com.whisp.whisp-rs/config.json`
- History: `~/Library/Application Support/com.whisp.whisp-rs/history.db` (SQLite)
- Whisper models: `~/Library/Application Support/com.whisp.whisp-rs/models/`
- API keys: macOS Keychain (`com.whisp.whisp-rs`)

Audio is never stored to disk. When using cloud providers, audio is sent directly to the provider's API and discarded.

## License

[MIT](LICENSE)
