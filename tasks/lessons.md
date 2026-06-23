# Lessons

## Microphone capture (macOS)

- cpal 0.17 binds a concrete CoreAudio device id identically whether you pass
  `None` (system default) or an explicit device name (`audio_unit_from_device`
  sets `kAudioOutputUnitProperty_CurrentDevice` either way). So "System Default"
  is not a distinct code path — it just follows whatever macOS reports as the
  default input, which is often NOT the user's real mic (a laptop mic, or a
  silent virtual driver installed by a conferencing app).
- Two silent-failure traps to avoid: (1) when a configured mic is absent,
  `capture::start_recording` falls back to the system default — surface this
  loudly (`RecordingSession.fell_back`), never switch invisibly; (2) a dead/
  silent device produces ~zero RMS — classify it as `DeadMic` and raise a loud
  `Error`, distinct from a merely-quiet clip (`Silent`) which is skipped to
  avoid Whisper hallucinating "Thank you." on near-silence.
- RMS bands live in `lib.rs`: `DEAD_MIC_RMS_THRESHOLD` (0.0005) <
  `SILENCE_RMS_THRESHOLD` (0.003). `classify_rms` is the single source of truth.

## Logging

- The file log layer's filter floor is `info`; `error` outranks `info`, so error
  events always land in the daily `whisp.log.YYYY-MM-DD` files. No separate
  error sink is needed.
- `prune_old_logs` runs in `init_logging` at startup and deletes `whisp.log*`
  files older than `LOG_RETENTION_DAYS` (30) by mtime. Keep retention here, not
  in a background task — startup is sufficient for a menu-bar app.

## Build / ops

- `make build` can fail at the DMG step if a stale `dmg.*` volume from an
  interrupted build is still mounted. Fix: `hdiutil detach <disk> -force` and
  remove `target/release/bundle/macos/rw.*.dmg`, then rebuild. Not a code issue.
- `gh` is authed only as the work account (not a collaborator on
  amirsalaar/whisp2). Push works via `GIT_SSH_COMMAND` with
  `~/.ssh/personal_id_ed25519`; PRs via the API do not. Release is triggered by
  a `[release]` trailer on a push to `main` (see `.github/workflows/release.yml`).
