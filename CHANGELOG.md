# Changelog

All notable changes to Whisp2 are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Local Parakeet transcription (on-device, no API key).** A new provider runs
  NVIDIA's Parakeet-TDT-0.6B-v3 model fully on your Mac via ONNX Runtime — no
  network once the model is downloaded, nothing sent to a cloud service. Pick
  "Local Parakeet (on-device)" in Settings → Transcription, download the
  quantized model (~685 MB) with one click, and dictate as usual. It
  auto-detects and transcribes 25 European languages (English, Spanish, French,
  German, Italian, Portuguese, Dutch, Russian, Ukrainian, Polish, and more) with
  punctuation and capitalization, and on Apple Silicon it transcribes roughly
  40x faster than real time on the CPU alone. The Settings screen lists the
  supported languages and notes that non-European languages (Farsi, Arabic,
  Chinese, Japanese, etc.) need OpenAI Whisper or Groq instead — Parakeet
  auto-detects and can't be forced to a specific language. Sits alongside the
  existing Local Whisper option as a faster, more accurate on-device choice.

### Fixed
- **Saving settings no longer corrupts your API key.** When a key was already
  stored, the field showed bullets (••••••••) as a placeholder. Clicking "Save"
  without retyping stored those bullets as the actual key, so the next
  transcription failed with "Invalid API Key" (a 401) — most visibly on Groq.
  The app now refuses to persist the placeholder (or an empty value) both in the
  UI and in the backend, so a real stored key survives an accidental save. Groq
  API errors are also now labeled "Groq" in the logs instead of "OpenAI" (Groq
  reuses the OpenAI-compatible client), so failures are no longer misattributed.
- **Dictionary substitutions are now applied to transcriptions.** Stored
  word/phrase corrections were silently ignored: the matcher only fired on
  lowercase, unpunctuated, space-separated text, so real transcripts (which are
  capitalized and punctuated, e.g. "Whisp rs.") never matched and the
  substitution was dropped. Corrections now match whole words case-insensitively
  and survive surrounding punctuation, including keys with symbol endpoints like
  ".net" or "C++" — while still leaving substrings inside larger words untouched.
- **App version is reported correctly in Get Info.** The bundled app was stamped
  with the `0.1.0` placeholder because the version lived only in CI and was never
  committed back to the source. The real version now ships in
  `tauri.conf.json`/`Cargo.toml`, and the release workflow commits each bump back
  to `main` so local builds match what's published.

## [1.2.1] - 2026-06-24

### Fixed
- **"System Default" microphone now records reliably.** macOS often reports a
  silent device as the default input — a virtual loopback driver (e.g. Microsoft
  Teams/Zoom audio) or an idle Continuity device (e.g. an iPhone mic) — which
  captured pure silence. Whisp now detects these by audio transport type and
  records from a real physical mic (built-in, USB, Bluetooth, etc.) instead, so
  "System Default" works without manually picking a mic.

## [1.2.0] - 2026-06-23

### Added
- **Shareable logs.** Settings → Diagnostics now has an "Application logs" card:
  view the recent logs in-app, copy them to the clipboard for a bug report, or
  open the logs folder in Finder to attach the full files. Logs stay on your
  machine — nothing is uploaded.
- **30-day log retention.** Daily log files older than 30 days are pruned on
  startup, so the logs directory never grows without bound.

## [1.1.5] - 2026-06-22

### Fixed
- **Microphone failures are no longer silent.** When your selected mic is
  unavailable and the app falls back to the system default, the menu bar icon
  turns red and names the substitute device instead of switching silently. A
  dead or muted mic (digital silence) now surfaces a clear error telling you to
  check the device and Microphone permission, instead of dropping the recording.
- **Quiet recordings are kept.** Lowered the silence threshold so soft-spoken
  or low-gain mic recordings transcribe instead of being discarded as silence.
- **No more phantom "Thank you."** Near-silent clips are skipped before they
  reach the transcription model, eliminating the hallucinated "Thank you."
  output Whisper produces on silence.

[1.2.0]: https://github.com/amirsalaar/whisp2/releases/tag/v1.2.0
[1.1.5]: https://github.com/amirsalaar/whisp2/releases/tag/v1.1.5
