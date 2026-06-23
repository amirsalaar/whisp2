# Changelog

All notable changes to Whisp2 are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
