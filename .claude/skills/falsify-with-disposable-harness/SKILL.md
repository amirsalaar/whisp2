---
name: falsify-with-disposable-harness
description: >
  Crack bugs that resist fixes by building a throwaway harness that runs the suspect code paths
  side-by-side in one process to falsify the leading hypothesis with controlled evidence. Reach for
  this whenever a bug "won't die" after multiple fix attempts, the provided logs or repro look
  contaminated or impossible, the failure is intermittent or environment-dependent, or two code
  paths "should be identical" but one misbehaves. Also use it before trusting that a fix worked:
  verify behavior from the actual built artifact, not the source or config. Use even when the user
  just says "this was never fixed" or hands you logs to read — untrustworthy evidence is the trigger.
  This is a debugging technique, not test authoring: it does NOT apply to writing a permanent test
  suite, a load/performance benchmark harness, or general logging/instrumentation.
---

# Falsify With a Disposable Harness

## The core idea

Bugs that survive several fix attempts usually survive because everyone is reasoning from bad
evidence: contaminated logs, an intermittent repro, or an assumption nobody has actually tested.
Each "fix" treats a symptom of a hypothesis that was never confirmed, so the bug walks right
through it.

The way out is to stop arguing with the evidence you were handed and manufacture *clean* evidence:
write a small throwaway program that runs the suspect paths under controlled conditions, in one
process, side by side — designed specifically to **prove the leading hypothesis wrong**. If it
survives a genuine attempt to falsify it, you can trust it. If it dies, you've learned the real
shape of the bug instead of patching a phantom.

Then you delete the harness. It is scaffolding, not a deliverable.

This is the missing concrete move inside systematic debugging: that skill tells you to "add
instrumentation" — this one tells you exactly what instrumentation to build and how to read it.

## When this pays off

- A bug has been "fixed" two or more times and keeps coming back. The repeat is the signal that
  the diagnosis, not the patch, is wrong.
- The logs or reproduction you were given look contaminated or physically impossible (duplicate
  startup lines, counts that can't happen in the elapsed time, events out of causal order). Treat
  impossible evidence as *no* evidence — do not reason from it.
- "These two paths are equivalent, but one fails." Equivalence is a hypothesis. Test it.
- The failure is intermittent, timing-dependent, or only happens in one environment.
- You're about to claim a fix works. Behavior must be checked at the boundary that actually
  matters (the built app, the rendered output, the real device), not the source that feeds it.

If the bug is obvious and reproduces cleanly on the first try, you don't need this — just fix it.

## The loop

### 1. Name the load-bearing assumption

Write down, in one sentence, the belief every prior fix depended on. "System Default records
silence because `default_input_device()` is broken." "The two code paths capture identical audio."
"The build is stamped with the configured version." This is your target. The harness exists to
attack it.

If you can't state the assumption, you're not ready to write the harness — you're still guessing.

### 2. Build the smallest program that isolates the variable

The harness should exercise the suspect paths **in a single process, back to back**, so that
everything you're *not* testing (permissions, device state, working directory, env) is held
constant and can't explain a difference. That control is the whole point — it's what your
contaminated logs lacked.

Make it print hard, comparable numbers, not vibes: sample counts, byte sizes, RMS, exit codes,
whatever distinguishes "worked" from "failed." Put the two paths' outputs next to each other so a
difference (or the absence of one) is obvious at a glance.

Bias toward the cheapest thing that runs the *real* code. For a Rust lib, an example or a
`src/bin/` throwaway that calls the actual functions beats reimplementing them. For a script,
a few lines that import the real module. You want to test the code that ships, not a paraphrase.

### 3. Try hard to falsify, then read what survives

Run it. The useful outcomes are both informative:

- **The assumption dies.** The two "identical" paths print different numbers; the "broken"
  function actually works. Now you know the real bug is elsewhere — often in *which* thing the
  code selected, not *how* it ran. Re-aim and repeat.
- **The assumption holds under a real attack.** Now it's earned trust, not inherited trust.

Narrow iteratively. Each run should change one variable and answer one question. A typical
progression: does it reproduce structurally → does it reproduce with realistic input → is it a
timing/warm-up effect → enumerate every candidate and probe each. Let each result pick the next
question instead of planning all the runs up front.

When you need ground-truth the program doesn't expose (e.g. why the OS picked device X), query the
authoritative source directly (a platform API, the real file on disk) rather than inferring.

### 4. Verify the fix at the boundary that matters

A fix is confirmed by observing corrected behavior at the layer the user actually experiences, not
by the source you edited. The config says `1.2.1`? Read `CFBundleShortVersionString` out of the
*built* `.app`. The function looks right? Run it in the harness and read the numbers. "The source
says X" is a claim about intent; "the artifact does X" is the fact you can ship on.

### 5. Delete the harness

It was scaffolding to find and confirm the bug. Keeping it around clutters the tree and rots. The
durable artifacts are: the real fix, a proper unit test that locks in the corrected behavior
(extract the harness's core check into one), and — if the diagnosis was surprising — a note in the
commit or memory explaining the actual root cause so the *next* person doesn't re-chase the
phantom. Remove the throwaway program before committing.

## Worked example

A menu-bar app's "System Default" microphone recorded nothing; four prior fixes (threshold tweaks,
a library upgrade, fallback handling) hadn't stuck. The handed-over logs showed `376291328 samples`
captured in under a second — impossible at 48 kHz — plus duplicate "tap installed" lines: multiple
dev instances had been fighting over the mic. Contaminated evidence.

The load-bearing assumption across every prior fix: *the default-device code path itself produces
silence.* A throwaway `src/bin/miccheck.rs` recorded from the default path and the explicit-by-name
path back-to-back in one process and printed sample count + RMS for each. They were byte-identical.
The assumption was false — the capture code was fine.

Re-aimed: probe *every* input device individually. Two of them (a virtual loopback driver and an
idle Continuity device) returned non-empty all-zero buffers, while the physical mic returned real
audio. Querying CoreAudio's transport-type property confirmed the mechanism: macOS was reporting a
*silent virtual device* as the system default. The bug was in **which device got selected**, not
how recording ran — invisible from the logs, and exactly what the prior fixes kept missing. Fix:
divert away from silent transports. The harness's per-device check became a unit test; the harness
was deleted.

## Anti-patterns

- **Reasoning from impossible evidence.** If a log can't be real, it tells you nothing about the
  bug — only that your instrumentation or environment is dirty. Don't build a theory on it.
- **A harness that reimplements instead of invokes.** If it doesn't run the real shipping code, a
  passing harness proves nothing about the real failure.
- **Confirming a fix from the source.** Source is intent; verify the artifact.
- **Keeping the harness "just in case."** Promote its essential check into a real test, then delete
  it. Scaffolding left in the tree becomes tomorrow's confusion.
- **Planning all the runs up front.** The first result usually changes the question. Go one
  variable at a time.
