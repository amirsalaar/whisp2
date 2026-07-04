# Plan: Fix Parakeet multi-language UX

## Problem (root cause, evidence-backed)

User reports "multi-language doesn't work with the NVIDIA model." Disposable-harness
testing against the real model proved:

- **The model works.** Spanish, French, German clips all transcribed correctly via
  auto-detection (every timestamp mode). Not a model bug.
- **Farsi returns garbage** ("Dot." for a Persian clip) because Parakeet-TDT-0.6b-v3
  supports only 25 **European** languages: `en es fr de bg hr cs da nl et fi el hu it
  lv lt mt pl pt ro sk sl sv ru uk`. No Farsi, Arabic, Chinese, Japanese, etc.
- **The Language setting is silently ignored for Parakeet.** The `parakeet-rs` crate's
  `ParakeetTDT` has NO language-forcing API (auto-detect only), and our provider never
  reads `config.language`. But the settings UI shows a free-text Language field for ALL
  providers, with placeholder `"en, fa, de…"` — literally suggesting Farsi.

Net: a user types "fa", the field does nothing, the model can't do Persian anyway, and
they conclude multi-language is broken. The bug is UX/expectations, not transcription.

## Desired outcome

Multi-language is correct AND honest for Parakeet:
- Supported languages auto-detect and transcribe (already works — don't regress it).
- The UI stops implying a language-forcing control that does nothing, and stops
  suggesting unsupported languages.
- The user can see which languages Parakeet supports.

## Scope

macOS-only feature. Frontend-only change plus a small backend constant to keep the
supported-language list as single source of truth.

### In scope
1. **Backend: expose the supported-language list.** Add a `parakeet_supported_languages()`
   helper (or a field on the catalog) returning the 25 codes, surfaced via the existing
   `list_parakeet_models` command or a small dedicated command. Single source of truth so
   the list can't drift between Rust and TS.
2. **Frontend: replace the free-text Language field with provider-accurate UI.** When
   `provider === "parakeet"`, do NOT show the free-text `language` input. Instead show a
   short read-only note: "Parakeet auto-detects among 25 supported languages" with the
   list visible (tooltip or expandable). The free-text field stays for
   whisper/openai/groq/gemini (they honor `config.language`).
3. **Honesty about `config.language` for Parakeet.** Since the model ignores it, don't
   send a stale/misleading value. Leave `config.language` untouched in storage (so
   switching back to Whisper keeps the user's setting) but make clear it has no effect
   for Parakeet.

### NOT in scope
- Forcing a language on Parakeet — the crate offers no API; auto-detect is the only mode.
- Adding a second Parakeet model for non-European languages (Farsi/Arabic/CJK). Would be
  a separate model download + provider work. Defer to TODOS.
- Changing how Whisper/cloud providers handle `config.language` (they work; untouched).

## Approach (revised after independent review)

- **`src-ui/src/App.tsx` only** — no Rust change. The 25 supported languages are static
  model metadata not consumed by any Rust logic, so a Tauri command + IPC + a
  static-vs-static unit test is over-engineering. Hardcode a `PARAKEET_LANGUAGES`
  constant (readable name + code) at the top of `App.tsx`. Upgrading the model
  checkpoint would require explicit code changes anyway — no real drift risk.
- Gate the Language `settings-row`: render the free-text input for
  whisper/openai/groq/gemini (they honor `config.language`); for Parakeet render a
  read-only note instead — "Parakeet auto-detects the language" — plus an inline
  collapsed **"Show supported languages"** expander (not a hover tooltip — works with
  touch/accessibility) listing the 25 as **readable names** ("Spanish", "French",
  "Croatian", …), and one explicit exclusion line: "Non-European languages (Farsi,
  Arabic, Chinese, Japanese, …) are not supported — use OpenAI Whisper or Groq for those."
- Leave `config.language` untouched in storage (inert for Parakeet since the provider
  never reads it; preserved for a later switch back to Whisper).

## Follow-on (noted, not this change)
- **Detected-language feedback**: nothing tells the user which language was detected, so
  a wrong result is ambiguous (model error vs unsupported language). A small HUD/log
  signal would close the loop. Defer to TODOS.

## Test plan
- Manual/harness: re-confirm es/fr/de still transcribe (no regression); confirm the UI
  shows the auto-detect note + expander (not the free-text field) when Parakeet is
  selected, and the free-text field still shows for the other providers.
- tsc + vite build clean; clippy -D warnings clean; existing 56 tests pass.

## Decision Audit Trail
<!-- AUTONOMOUS DECISION LOG -->

| # | Decision | Classification | Principle | Rationale |
|---|----------|----------------|-----------|-----------|
| 1 | Hide free-text Language field for Parakeet; show read-only auto-detect note | Mechanical | Explicit-over-clever | Model has one mode (auto-detect); any input control implies agency that doesn't exist |
| 2 | Hardcode 25-language list in TS, NOT a Rust command | Taste→resolved | Explicit-over-clever, pragmatic | List is static model metadata, consumed by no Rust logic; command+IPC+test is ceremony for display data |
| 3 | Show readable language names, not bare ISO codes | Mechanical | Completeness | Users don't know hr=Croatian, mt=Maltese; bare codes leave the "is my language supported?" question half-answered |
| 4 | Add explicit exclusion line (Farsi/Arabic/CJK → use Whisper/Groq) | Mechanical | Completeness | Preempts the exact complaint that triggered this fix; one sentence, high user value |
| 5 | Inline expander, not hover tooltip | Mechanical | Explicit-over-clever | Tooltips are hover-only, broken for touch/accessibility |
| 6 | Leave config.language untouched in storage | Mechanical | Pragmatic | Inert for Parakeet (provider ignores it); preserves user's Whisper setting on switch-back |
| 7 | Detected-language feedback → defer to follow-on | Taste→resolved | Bias toward action | Out of blast radius for a settings-form fix; real value but separate scope |
