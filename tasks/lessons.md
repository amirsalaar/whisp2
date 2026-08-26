# Lessons

Hard-won, non-obvious things about this codebase. Each entry is a trap that cost
real debugging time — the symptom first, so a future search finds it.

## A synchronous Tauri command runs on the MAIN THREAD. Never block in one.

**Symptom:** the floating island freezes mid-state, global NSEvent monitors stop
delivering, and `run_on_main_thread` closures queue up without ever running.
Looks exactly like "the monitor is flaky" or "AppKit is broken".

**Cause:** `#[tauri::command] pub fn ...` (no `async`) is invoked on the main
thread. A Keychain read blocks there indefinitely when macOS puts up an "allow
access" prompt — which it does whenever the binary's signature no longer matches
the one that stored the item, i.e. after every ad-hoc re-sign. While the main
thread is parked, `NSApp`'s runloop stops turning and everything main-thread
dies with it.

**Fix:** make the command `async` so it runs on Tauri's async runtime. See the
doc comment on `commands::config::get_api_key`. Startup keychain reads in `setup`
are fine sync — no webview exists yet to freeze.

**Diagnostic:** `sample <pid>` and read the main thread's stack. It named the
culprit in one shot after a long stretch of guessing at AppKit.

## Tauri 2 capabilities are per-window-label, and a missing one hangs silently.

A window whose label is in no capability's `windows` array gets no core
permissions. The failure mode is the worst kind: `await listen(...)` never
resolves *and* never rejects, so the whole `init()` just stops with no error.
The island's webview is inert without `src-tauri/capabilities/hud.json`.

## Tauri events are not buffered — emit-on-change needs a pull-based catch-up.

An `emit_to` that happens before the target webview has registered its `listen()`
is dropped, not queued. Combined with an FSM that emits only on *change*, a
dropped event is never re-sent: the island sat visibly collapsed while Rust
believed it was expanded and clickable. Hence `LAST_PAYLOAD` + the
`hud_current_state` command, which the webview adopts once its listeners are up.
Any new emit-on-change event needs the same treatment.

## Frontend assets are compiled into the Rust binary.

`frontendDist` is embedded, so a change to `src-ui` needs a full
`cargo tauri build` (i.e. `./tasks/run-dev.sh`). `npm run build` alone changes
nothing about the running app — which reads as "my fix had no effect".

## VERIFY YOUR TEST INSTRUMENT BEFORE TRUSTING A NEGATIVE RESULT.

The single biggest time sink in the island work. Every "the code is broken"
conclusion below was actually a broken probe:

- `mv.swift` ignored its arguments and always swept to a hardcoded point, so
  every run "coincidentally" left the cursor near the island.
- `goto` used `CGWarpMouseCursorPosition`, which moves the pointer *without
  posting an event* — so `NSEvent.mouseLocation` kept reporting the old spot and
  five separate runs showed zero proximity transitions.
- `warp` reported success while leaving the cursor a third of the way there.
- `allwin.swift` filtered on a hardcoded, long-dead pid and printed nothing.
- A hand-rolled `flagsChanged` event (bare `CGEvent(source:)` with the type
  reassigned) is silently dropped; the keyboard-event form in `rec.swift`
  (`CGEvent(keyboardEventSource:virtualKey:keyDown:)`) works.

Rule: make the probe report its own read-back and assert the effect happened
(see `place.swift`: warp, post a real `mouseMoved`, then verify). A probe that
can't fail loudly will lie quietly.

Corollary — `screencapture` is a bad island probe. Two rounds of captures showed
"the pill isn't expanding" while the pill *was* expanding: the captures raced the
cursor drifting back out of the hot zone, and a transparent always-on-top window
doesn't reliably appear in a region capture anyway. Instrumenting the proximity
poll with one `tracing::warn!` of its own inputs and result settled in one run what
the screenshots got backwards — and note the log must be read line-by-line, since
`uniq -c` over separate fields silently pairs a cursor value from one tick with a
verdict from another.

## Synthetic CGEvents cannot drive `startDragging` (window drags).

**Symptom:** a scripted drag of the island posts a clean `leftMouseDown` → dense
`leftMouseDragged` stream → `leftMouseUp`, reports every coordinate correctly, and
the window does not move one pixel. `hud_position` is unchanged afterwards.

**Cause:** `startDragging` calls AppKit's `performWindowDragWithEvent:`, which runs
its own nested event-tracking loop driven by the real HID stream. Posted events
don't feed it. Tried both with and without `CGWarpMouseCursorPosition` during the
gesture, and with 30–40 dragged events at 25–35ms spacing.

**Consequence for QA:** "the position didn't change" after a synthetic drag proves
**nothing** — the gesture never started. Discriminate by reading the window's rect
(`CGWindowListCopyWindowInfo`, filtered by the app's *live* pid): if the rect is
unchanged, the drag never engaged. Drag-drop behaviour needs a human hand; note it
as such in the checklist rather than recording a false pass.

**What synthetic events CAN do here:** hover/proximity (plain `mouseMoved` works,
verified by instrumenting the poll and sweeping across the pill), plain clicks, and
DOM event delivery into the WebView.

## Two coordinate flips that look identical and aren't: tao vs `NSScreen.mainScreen`.

Tauri's `position`/`outer_position` are top-left-origin; AppKit's are bottom-left.
tao flips against **`CGDisplay::main().pixels_high()`** — the *primary* display
(`bottom_left_to_top_left`). Flipping with `NSScreen::mainScreen().frame().height`
instead is wrong, because `mainScreen` is the screen holding the **key window** and
changes as the user focuses windows on other displays.

On a single-display Mac the two are the same number, so this is invisible locally
and only breaks for users with a mixed-resolution second monitor: the island's
hover zone lands off by the height difference, so hovering never expands it. Pinned
by `hud::position::appkit_bottom_from_tauri_top` plus a test that asserts the wrong
flip height actually misses the pill.

## Prefer a log channel over pixel archaeology.

Screenshotting and colour-diffing the island to infer its state was slow and
ambiguous. A temporary `hud_qadiag` command that let `hud.ts` write into the Rust
log — including a capture-phase `document` listener for `mousedown`/`pointerdown`
— answered in one run what a dozen screenshots couldn't: whether synthetic
CGEvents reach the WebView's DOM at all (they do).

## Two independent probes beat one clever probe.

Separating "is the tokio timer alive?" (probe A, no main-thread hop) from "does
the main thread still pump?" (probe B, fire-and-forget hop that never awaits)
localised the freeze immediately: A healthy, B queued with `ok=true` but 0
closures executed. Either probe alone would have been ambiguous.

## Accessibility is revoked by every ad-hoc re-sign.

`run-dev.sh` runs `codesign --force --deep --sign -`, so macOS treats the result
as a new binary and drops the Accessibility grant — the log says
`Accessibility not granted — hotkey recording disabled` and the hotkey silently
does nothing. Re-grant in System Settings after building. Worth knowing that the
island itself (hover, drag, paint) works fine without it.

## Hit-test what's drawn, not the window it's drawn in.

The island's hover zone was computed from its **window** rect (340x88) plus 48pt of
padding, while the thing the user can actually see at rest is a 44x5 nub anchored
`bottom: 8px` inside it. So the hot zone was 436x184 against a 220pt² target — 365x
the visible affordance, and the island expanded with the cursor visibly nowhere near
it. A transparent window is not its contents; if the affordance is smaller than the
window, derive its rect (`hud::position::pill_rect`) and test that.

Two consequences worth keeping:

- **The hit test must follow the state it's driving.** The pill *grows* on hover, so
  a cursor that landed on the nub ends up near the bottom of the 260x62 pill it just
  opened. Testing the nub while the expanded pill is on screen collapses it the
  moment the user moves up to read it, then reopens it — a flicker. So the poll's
  "currently expanded" flag selects which rect to test.
- **Slack is for acquirability, not proximity.** 10pt around a 5pt-tall nub buys the
  ~24pt a pointer needs to land reliably. Anything much larger and you are back to
  expanding where nothing is drawn.

## Probing a hover fix only means something from the collapsed state.

A probe that walked the cursor from "on the nub" to "40pt above the nub" reported
"stays open" and looked like the bug surviving. It wasn't: once expanded the pill
really is 260x62, so that point is legitimately on it. The bug only exists while
**collapsed**, so each candidate point must be approached from far away — collapse
first, then land. Same lesson as the timestamp mishap below: an ambiguous negative
is usually the probe, not the code.

## A log window that ends is not a condition that persists.

Chasing a phantom "the island oscillates ~1/sec with the cursor parked far away":
the flapping transitions were the previous probe's own warps, and the cursor
position I read was sampled *after* that window had closed. The timestamps said so —
they stopped and never resumed. Before diagnosing an anomaly, check the last
event's time against *now*; a fresh run with nothing touching the mouse logged zero
transitions in 28s and settled it.

## An SDK convenience field is not a wire field.

Gemini's transcription docs say "the complete transcript text is returned in
`interaction.output_text`", and the obvious parser reads `resp["output_text"]`.
The Interaction resource reference then says that field is "added by the SDK" —
it does not exist in the REST payload, and none of the sample responses populate
it. A provider built from the guide alone would have compiled, passed review, and
returned "response missing text field" on every single dictation.

The transcript actually has to be gathered the way the SDKs gather it: walk
`steps[]`, keep the step whose `type` is `model_output`, and concatenate the
`{"type":"text","text":...}` parts of its `content[]`. Note the response also
echoes the caller's turn back as a `user_input` step, so an unfiltered walk
prepends the prompt to the transcript.

Two habits fall out of this:

- **Read the resource reference, not just the guide.** Guides are written against
  the SDKs; the wire format lives in the API reference. When they disagree about a
  field, the reference wins.
- **Test the parser against a payload shaped like the reference's example**,
  including the parts you must ignore (`user_input` steps, `word_info`
  annotations, non-text content). Those tests are what encode the distinction.

## A dedicated model can mean a different endpoint, not just a different string.

`gemini-3.5-transcribe` is not a drop-in for `gemini-2.0-flash` in the existing
`:generateContent` call. It is served from `POST /v1beta/interactions`, takes
`model` + `input[]` instead of `contents[].parts[]`, spells inline audio `data`
rather than `inline_data.data`, and configures language/diarization through
`generation_config.transcription_config` instead of English prose in the prompt.
Swapping only the model ID in Settings would have produced a 404. Before adding a
"new model" to a provider, check whether it shares the provider's endpoint.

Related: every Gemini model Whisp had ever offered (2.0-flash, 1.5-flash,
1.5-pro) was shut down while nobody was looking, so that provider was
100% broken with no bug report. Model IDs are perishable config, and a provider
whose only options are dead models fails silently until someone selects it — a
`RETIRED_GEMINI_MODELS` sweep at config load is what carries existing users over.
