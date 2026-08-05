#!/bin/bash
# Validates the floating island end-to-end: static checks that can be automated,
# then a printed manual checklist for the parts that need a real screen.
#
# The island lives in a separate WebviewWindow whose contract with the backend is
# entirely string-based (`hud_state` payloads, CSS class names). That contract has
# silently drifted before — hud.css and hud.ts once shared *zero* class names, so
# the island rendered nothing while every test still passed. The grep checks below
# exist to catch exactly that: names that must agree across the Rust/TS/CSS boundary.

set -euo pipefail
cd "$(dirname "$0")/../.."

export PATH="$HOME/.cargo/bin:$PATH"

PANEL=src-tauri/src/hud/panel.rs
HUD_TS=src-ui/src/hud.ts
HUD_CSS=src-ui/src/hud.css

fail=0
check() {
  if eval "$2" >/dev/null 2>&1; then
    echo "  ok   $1"
  else
    echo "  FAIL $1"
    fail=1
  fi
}

# `--package whisp-rs --lib` is required: this is a workspace, and a bare
# `cargo test <filter>` from the root silently matches 0 tests.
echo "==> Rust tests (geometry, level meter, state mapping)"
cargo test --quiet --package whisp-rs --lib hud:: 2>&1 | tail -3
cargo test --quiet --package whisp-rs --lib audio::level 2>&1 | tail -3

echo
echo "==> Clippy + frontend build"
cargo clippy --quiet -- -D warnings
npm --prefix src-ui run build >/dev/null
npm --prefix src-ui run lint >/dev/null 2>&1 || echo "  (eslint warnings present — see 'make ui-lint')"
echo "  ok   builds clean"

echo
echo "==> Wire-format contract: every HudState renders in TS and has a CSS rule"
# Extract the wire names from HudState::as_str, e.g. `Self::Error => "error",`
states=$(grep -oE '=> "[a-z-]+",' "$PANEL" | grep -oE '"[a-z-]+"' | tr -d '"' | sort -u)
if [ -z "$states" ]; then
  echo "  FAIL could not extract any HudState wire names from $PANEL"
  exit 1
fi
for s in $states; do
  check "'$s' is in the TS HudState union" "grep -q \"'$s'\" $HUD_TS"
  # 'hidden' is styled by a shared rule; every other state needs its own sizing.
  check "'$s' has a .hud-pill.$s CSS rule" "grep -q '\.hud-pill\.$s' $HUD_CSS"
done

echo
echo "==> Class names hud.ts creates must exist in hud.css"
for cls in hud-pill hud-title hud-subtitle hud-status waveform waveform-bar \
           hud-btn stop-square hud-spinner hud-error-icon; do
  check ".$cls is styled" "grep -q '\.$cls' $HUD_CSS"
done

echo
echo "==> Commands hud.ts invokes must be registered in lib.rs"
# The `<...>` alternative matters: `invoke<string | null>('hud_current_state')` is
# a real call site, and matching only `invoke('` silently skipped it — the check
# then reported all-ok while leaving one command unverified.
for cmd in $(grep -oE "invoke(<[^>]*>)?\('[a-z_]+'" "$HUD_TS" | grep -oE "'[a-z_]+'" | tr -d "'" | sort -u); do
  check "$cmd is in the invoke_handler" "grep -q 'commands::hud::$cmd' src-tauri/src/lib.rs"
done

echo
echo "==> Events the backend emits must have a listener in hud.ts"
for ev in hud_state audio_level; do
  check "$ev is listened for" "grep -q \"listen<.*>('$ev'\" $HUD_TS"
  check "$ev is emitted"      "grep -rq '\"$ev\"' src-tauri/src"
done

echo
echo "==> Hover hot zone matches the pill actually drawn"
# The cursor hit test is computed in Rust from constants that mirror the CSS. If
# the two drift the island expands where nothing is drawn (or stops expanding
# where something is) — the exact bug this replaced, where the hot zone was the
# 340x88 *window* plus 48pt of padding around a 44x5 nub: 365x the visible target.
# So assert the numbers still agree. Each pair is "css-selector rust-const w h".
while read -r selector rust_const w h; do
  # The dimensions as CSS sees them: the rule block following the selector.
  css_w=$(sed -n "/^\.hud-pill\.$selector {/,/}/p" "$HUD_CSS" | grep -oE 'width: [0-9]+px' | grep -oE '[0-9]+')
  css_h=$(sed -n "/^\.hud-pill\.$selector {/,/}/p" "$HUD_CSS" | grep -oE 'height: [0-9]+px' | grep -oE '[0-9]+')
  check ".hud-pill.$selector is ${w}x${h} in CSS" "[ '$css_w' = '$w' ] && [ '$css_h' = '$h' ]"
  check "$rust_const agrees with it" \
    "grep -qE '$rust_const: \(f64, f64\) = \($w\.0, $h\.0\)' $PANEL"
done <<'PAIRS'
collapsed-idle COLLAPSED_PILL 44 5
expanded-idle EXPANDED_PILL 260 62
PAIRS
# The pill's bottom offset inside the window is mirrored the same way.
css_inset=$(sed -n '/^\.hud-pill {/,/}/p' "$HUD_CSS" | grep -oE 'bottom: [0-9]+px' | grep -oE '[0-9]+')
check "PILL_BOTTOM_INSET matches .hud-pill's 'bottom'" \
  "grep -qE \"PILL_BOTTOM_INSET: f64 = ${css_inset}\.0\" src-tauri/src/hud/position.rs"

echo
if [ "$fail" -ne 0 ]; then
  echo "STATIC CHECKS FAILED"
  exit 1
fi
echo "All static checks passed."

cat <<'MANUAL'

==> Manual QA (needs a real screen — run ./tasks/run-dev.sh first)

Already verified against a running build (kept here as regression steps):
  [x] Island appears bottom-center, above the Dock, on launch.
  [x] Collapsed nub is click-through: a click on the pill's exact spot while
      collapsed produces NO DOM event in the island (it hits the app behind).
  [x] Moving the cursor near the nub expands it; moving away collapses it.
      Deterministic across 5 fresh launches.
  [x] Dragging the expanded pill writes the new position to config.json, and a
      relaunch reopens the island there rather than at bottom-center.
  [x] The recording pill's waveform tracks live mic level (audio_level arrives
      with varying values, not just the idle shimmer).
  [x] hud_position [9000, 9000] is clamped back on screen at the next launch.
  [x] Clicking the island without moving it does NOT write hud_position. Verified
      by driving ~40 synthetic clicks/hovers onto the pill and confirming
      config.json's hud_position never changed. (Under the old mousedown-triggered
      save, every one of those would have persisted a position — pinning a
      never-dragged island so it stopped recentering on Dock/resolution changes.)
  [x] Cursor proximity is computed against the right screen height. Instrumented
      the poll and swept the cursor across the pill: Rust's sampled cursor tracked
      the synthetic position exactly and the hit test flipped to true over the
      pill (29 consecutive hits), with the rect and flip height both correct.
  [x] Hovering only expands ON the nub, not near it. Instrumented the poll and
      warped the cursor to five points that the old window+48pt zone accepted —
      75pt above, 40pt above, 120pt left, 120pt right, 30pt below the nub — each
      approached from far away so the island was collapsed first. Zero
      transitions; only the on-nub point expanded it. Approaching from collapsed
      is the whole test: once expanded the pill really is 260x62, so a point 40pt
      above the nub is legitimately on it, and probing without collapsing first
      shows a "stay open" that proves nothing about the reported bug.

Still needs a hand-run (all of these need a build whose signature still holds
Accessibility — `run-dev.sh` re-signs ad-hoc, which revokes the grant, so
re-grant in System Settings after building and relaunch before testing):
  [ ] Hold the hotkey: island becomes the recording pill.
  [ ] Press the island's stop button — transcription runs and text is injected.
  [ ] Start again and press the ✕ — recording is discarded, no text injected,
      island returns to idle.
  [ ] Release the hotkey normally: island shows the spinner + "Transcribing…",
      then returns to idle.
  [ ] Unplug/mute the mic and record: island shows the red error pill with the
      failure text, then auto-returns to idle after ~4s.
  [ ] Change an unrelated setting (e.g. completion sound). The island must NOT
      jump back to bottom-center (guards the set_config hud_position preserve;
      the pure half is unit-tested, this checks the real settings form).
  [ ] Drag the pill, pausing ~1s partway, then release somewhere else. The
      position saved must be where you RELEASED, not where you paused. Needs a
      human hand: `startDragging` hands off to an AppKit tracking loop that
      synthetic CGEvents cannot drive — verified that a fully synthetic drag
      (with and without warping mid-gesture, dense posted mouseDragged streams)
      leaves the window at its original rect, i.e. the gesture never engages. So
      an unchanged position under synthesis proves nothing about this path.
  [ ] Discard a recording with the island's ✕, then check System Settings →
      Sound → Input that the input volume is back where it started (recording
      boosts it 1.5x; the Cancel path now restores it like Stop does).
  [ ] Settings → toggle the island OFF: it disappears immediately (no restart).
      Toggle back ON: it reappears at the remembered position.
  [ ] Full-screen an app (e.g. Safari). The island stays visible on top.
MANUAL
