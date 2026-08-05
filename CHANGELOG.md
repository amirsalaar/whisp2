# Changelog

All notable changes to Whisp2 are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- **The floating island no longer expands when the cursor is nowhere near it.**
  Hovering was triggered by a large invisible zone around the island — roughly
  365 times the area of the small nub you actually see — so the pill would pop
  open while the cursor was drifting past well above or to the side of it. It now
  expands only when you move onto the nub itself, with just enough slack to make
  such a small target easy to hit. Once open, the pill stays open as long as the
  cursor is on it, so moving up to read it doesn't make it collapse. The nub also
  picked up a hairline edge so it stays visible against light wallpapers, and
  opening is now a touch snappier than closing.

## [1.5.0] - 2026-08-05

### Added
- **A floating island that shows what Whisp is doing, wherever you're typing.**
  A small pill sits above the Dock (draggable anywhere — it remembers where you
  put it, and comes back on screen even if you moved it to a display you later
  unplugged). At rest it's a dim nub that clicks straight through to the app
  behind it, so it never gets in your way. Move the cursor near it and it expands
  to remind you of your hotkey. While you dictate it becomes a live waveform that
  tracks your voice — loud is tall, silence is flat — with Stop and Discard
  buttons so you can finish or throw away a recording without touching the
  keyboard. It then shows "Transcribing…" while the text is being produced, and
  surfaces failures (a dead or missing mic) as a readable red pill instead of
  only a tray tooltip. Turn it off any time in Settings; the toggle takes effect
  immediately, without a restart.

### Fixed
- **The app no longer freezes while reading a stored API key.** Reading, saving,
  or deleting a key ran on the UI thread, so whenever macOS interrupted with an
  "allow access to Keychain" prompt the whole interface locked up — the floating
  island froze mid-state and stopped responding to the cursor entirely. Keychain
  access now happens off the UI thread, so a slow or prompting Keychain can't
  wedge the app.
- **The floating island responds to the cursor again.** It could open completely
  inert — hovering it did nothing — because its window was missing the permissions
  it needed to talk to the app at all, and the failure was silent.
- **The island no longer opens showing the wrong state.** If it finished loading a
  fraction of a second after Whisp had already decided what to show, that first
  update was lost and never re-sent: the island looked collapsed while the app
  considered it expanded, leaving an invisible click target. It now asks for the
  current state as soon as it's ready.
- **Dragging the island somewhere else now sticks.** Changing any unrelated
  setting used to snap it back to the bottom-center of the screen. Pausing
  partway through a drag no longer records the pause spot as where you dropped
  it, and simply *clicking* the island no longer pins it in place — previously a
  single click on a never-moved island saved its position permanently, so it
  stopped re-centering when you changed resolution, moved the Dock, or unplugged
  a display.
- **Discarding a recording no longer leaves your microphone turned up.**
  Recording temporarily boosts the input volume; pressing the island's ✕ to throw
  a recording away skipped the step that puts it back, so the system input level
  stayed raised until the next recording you let finish.
- **The island expands on hover with more than one display connected.** The hover
  zone was measured against whichever screen held the focused window instead of
  the primary display, so on a mixed-resolution setup it sat hundreds of points
  away from the island itself — hovering did nothing, or the island expanded with
  the cursor nowhere near it, depending on which app you'd last clicked.
- **Long error messages stay inside the island.** A wordy failure (a dead mic
  naming a long device) overflowed the red pill and painted onto the desktop
  behind it; it's now truncated with an ellipsis.
- **The island keeps working when Accessibility permission isn't granted.** It
  previously froze; hovering, dragging, and status display now work regardless
  (the hotkey itself still needs the permission).
- **Back-to-back failures show the newer message.** When one error followed
  another, the island kept displaying the first one while the menu bar already
  reported the second.
- **The island respects Reduce Motion and idles quietly.** Its waveform shimmer
  ignored the system Reduce Motion setting (the bars are drawn in JavaScript, so
  turning off CSS animation made them *steppier*, not calmer) and the animation
  loop kept running even with nothing on screen to animate. The shimmer now stops
  entirely under Reduce Motion, the loop sleeps whenever no waveform is showing,
  and the bars animate at a consistent speed on 120 Hz ProMotion displays instead
  of running at double rate.

## [1.4.0] - 2026-07-06

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

[1.5.0]: https://github.com/amirsalaar/whisp2/releases/tag/v1.5.0
[1.4.0]: https://github.com/amirsalaar/whisp2/releases/tag/v1.4.0
[1.2.1]: https://github.com/amirsalaar/whisp2/releases/tag/v1.2.1
[1.2.0]: https://github.com/amirsalaar/whisp2/releases/tag/v1.2.0
[1.1.5]: https://github.com/amirsalaar/whisp2/releases/tag/v1.1.5
