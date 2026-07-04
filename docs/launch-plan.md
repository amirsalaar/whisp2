# Whisp Launch Plan

Tagline: **the free Wispr Flow**.

Open-source, MIT-licensed voice-to-text for macOS (and iPhone for developers). Hold a hotkey, speak, release — text appears in whatever app you're in. No clipboard. No window switching.

---

## 1. Objective

- **Primary**: 1,000 GitHub stars + 5 outside contributors within 30 days of launch.
- **Secondary**:
  - Show HN front page (top 30).
  - 100k impressions on the X thread.
  - At least one outside PR merged.

---

## 2. Audience

**Primary**: macOS power users + indie devs who already pay $10–30/mo for Superwhisper, Wispr Flow, Aiko, MacWhisper — or who looked at them and bounced on price. Skews technical (HN / X / r/rust crowd) but not exclusively.

**Secondary**: iPhone 15 Pro / 16 / 17 owners who want their Action Button to do something useful. Smaller audience, but the novelty hook drives press / Reddit pickup.

**Pain points**:
- $15/mo SaaS fatigue.
- Closed-source dictation tools sending audio to vendor servers.
- No good Action Button apps.
- Dictation tools that paste via clipboard and clobber what was already there.

**Where they hang out**: HN, r/macapps, r/rust, r/iOSProgramming, X (the @simonw / @dhh / @levelsio orbit), Bluesky dev community.

---

## 3. Key Messages

**Core**: *Free, open-source, no clipboard. Hold a hotkey, speak, release — text appears in whatever app you're in. Mac + iPhone.*

**Supporting messages** (in priority order):

1. **Cost**: "Wispr Flow is $15/mo. Superwhisper is $9/mo. This is $0 and the source is right there."
2. **Provider choice**: "OpenAI, Groq, Gemini, or fully on-device via local Whisper. Your key, your bill, your audio."
3. **No clipboard**: "CGEvent Unicode injection. Text gets typed where your cursor is. Never touches the clipboard."
4. **iPhone Action Button + Live Activity**: "First open-source app that turns the Action Button into a real dictation button, with a Live Activity Stop control. (Developer build only.)"

---

## 4. Tone

Builder-to-builder, dry, specific. *"I got tired of paying $15/mo for dictation, so I built this. Rust + Tauri 2. Source: github.com/..."*. Honest, low-hype. Performs best on HN + dev Twitter, doesn't get nuked on Reddit for sounding like marketing.

No emoji-heavy launch posts. No 🚀. No "introducing".

---

## 5. CTA

**Star + contribute.** Frame it as "help wanted" — invites PRs, signals active project, makes the early contributor list look healthy.

Concrete asks pinned in each post:
- Star the repo.
- Open an issue if something breaks.
- The iOS Gemini provider needs an implementation.
- The Linux build needs an owner.
- The dictionary UX could use a designer.

---

## 6. Channel Strategy

| Channel | Why | Format | Effort |
|---|---|---|---|
| **GitHub README** | Where every other channel funnels to. Must be tight. | Hero banner (shipped) + 60-sec demo GIF + features table + comparison table (shipped) + Why section (shipped) | Medium |
| **X / Twitter thread** | Highest viral ceiling for indie dev tools. Demo-clip-driven. | 6-tweet thread, lead with demo video | Medium |
| **Show HN** | HN crowd loves Rust + Tauri + local-first + open source. | Long-form post, no hype, technical specifics | Low |
| **r/MacOS** | General mac power users; cost angle resonates strongest here | Screenshot + plain-English pitch | Low |
| **r/rust** | Tauri 2, cpal, sqlx, anyhow — they'll appreciate the stack | Architecture-focused | Low |
| **r/iOSProgramming** | AppIntent + Live Activity + cross-process App Group IPC is genuinely novel | Architecture-focused, tease the iOS implementation | Low |

---

## 7. Schedule

**Launch day = Tuesday or Wednesday, 9am PT** (HN's sweet spot). Avoid Friday and weekends.

| Day | Asset | Channel | Owner | Status |
|---|---|---|---|---|
| Day -3 | Record 30-sec demo GIF/MP4 (Mac hotkey flow) | Asset | Amir | TODO |
| Day -3 | Record 10-sec demo (iPhone Action Button + Live Activity Stop) | Asset | Amir | TODO |
| Day -2 | Polish README — confirm "Why?" + comparison + iOS-unofficial callout (all done), add demo GIF | GitHub | Amir | Mostly done |
| Day -2 | Open ~5 `good-first-issue` tickets (iOS Gemini, Linux build, dictionary UX, semantic correction, etc.) | GitHub | Amir | TODO |
| Day -2 | Pre-launch sanity: `make test`, `make ios-typecheck`, `make build` clean | Repo | Amir | TODO |
| Day -1 | Draft + dry-run all posts (X, HN, r/MacOS, r/rust, r/iOSProgramming) | Drafts | Amir | TODO |
| **Day 0, 9am PT** | **Show HN goes live** | HN | Amir | TODO |
| Day 0, 10am PT | X thread goes live, links to HN post | X | Amir | TODO |
| Day 0, 11am PT | r/MacOS post | Reddit | Amir | TODO |
| Day 1 | r/rust + r/iOSProgramming posts | Reddit | Amir | TODO |
| Day 3 | Reply-pass on every surface, update README with reception, fix obvious bugs reported | All | Amir | TODO |
| Day 7 (optional) | Retro post: "What I learned launching an open-source dictation app" | X / blog | Amir | OPTIONAL |

---

## 8. Content Pieces Needed

| Asset | Priority | Notes |
|---|---|---|
| 30-sec demo GIF/MP4 (macOS hotkey flow) | **must-have** | Without this, X + Reddit are dead on arrival |
| iPhone Action Button + Live Activity demo (10 sec) | **must-have** | Single most viral asset — screen-record on device, Action Button → speak → tap Stop on Dynamic Island → text appears in next app |
| Banner SVG | done | `docs/banner.svg` |
| README "Why?" section + comparison table | done | Shipped in `0cf55ba`, `8abd52b`, `1d37f49` |
| iOS-unofficial callout in README | done | Shipped in `0cf55ba` |
| 5 first-time-contributor labeled GitHub issues | should-have | Required to make the "PRs welcome" CTA real |
| Show HN draft | **must-have** | See §10b |
| X thread draft | **must-have** | See §10a |
| r/MacOS, r/rust, r/iOSProgramming drafts | **must-have** | See §10c–e |

---

## 9. Success Metrics

| KPI | Target | Tracking |
|---|---|---|
| GitHub stars | 1,000 in 30 days | github-stars history; daily check |
| Outside contributors | 5 PRs merged | GH insights |
| Show HN rank | Top 30 (front page) | HN frontpage scrape |
| X thread impressions | 100k+ | Twitter analytics |
| Reddit upvotes (combined) | 500+ | Reddit |
| `.dmg` downloads | 5,000 in 30 days | GH releases download counter |

Reporting: daily check Day 0 → Day 7, weekly thereafter.

---

## 10. Post Drafts

### 10a. X / Twitter thread

> **1/** I got tired of paying for dictation apps so I built one.
>
> Whisp — open source, $0, Mac + iPhone. Hold a hotkey, speak, release. Text appears in whatever app you're in. No clipboard.
>
> github.com/amirsalaar/whisp2
>
> [demo.mp4]

> **2/** Why: Wispr Flow is $15/mo. Superwhisper is $9/mo. The actual hard part — text injection without the clipboard — is like 100 lines of CGEvent Unicode posting. The rest is a Whisper API call.

> **3/** On iPhone, the Action Button starts a recording. A Live Activity shows up on the Lock Screen / Dynamic Island with a real Stop button. Tap it, transcript types into the next app you open. (iOS is developer-build-only — clone, sign, and run via Xcode.)

> **4/** Stack: Tauri 2, Rust backend, React frontend, Swift Live Activity extension. cpal for audio, sqlx for history, security_framework for Keychain. macOS 13+, iOS 17+.

> **5/** Providers: OpenAI Whisper, Groq, Gemini, or 100% on-device via local Whisper (GGML). Your key, your bill, your audio. API keys live in the Keychain, never on disk.

> **6/** MIT licensed. Issues are tagged. PRs welcome — the iOS Gemini path needs an implementation, the Linux build needs an owner, and the dictionary UX could use a designer.
>
> github.com/amirsalaar/whisp2

---

### 10b. Show HN

> **Show HN: Whisp – open-source voice-to-text for macOS + iOS (Rust/Tauri)**
>
> Hi HN. I've been paying $15/mo for Wispr Flow and got tired of it, so I rebuilt the parts I actually use.
>
> Whisp is a menu bar app on macOS and an Action Button app on iOS. Hold a hotkey (or tap the Action Button), speak, release — the transcribed text gets typed directly into whatever app is focused. No clipboard, no window switching.
>
> A few things I think are worth talking about:
>
> - **Text injection**: macOS uses CGEvent Unicode posting in 20-char UTF-16 chunks on the main thread. Terminals get longer per-chunk delays because they drop fast keystrokes. No clipboard, no AX-tree manipulation.
> - **Provider choice**: OpenAI, Groq, Gemini cloud, or fully on-device via whisper.cpp (GGML). API keys live in the macOS Keychain, never on disk.
> - **iOS Live Activity**: the Action Button kicks off an AppIntent that starts a recording. A Live Activity shows up with a Stop button (interactive `Button(intent:)`, iOS 17+). Stop signal travels via App Group UserDefaults because the Live Activity runs in a different process from the host.
> - **Tauri 2 unified entry point**: same `lib.rs::run()` handles macOS desktop and iOS via `#[cfg_attr(mobile, tauri::mobile_entry_point)]`. Nice DX.
>
> Stack: Rust backend, React/TS frontend, Swift Live Activity extension, sqlx + SQLite for history, anyhow for errors.
>
> macOS ships a `.dmg`. iOS is developer-build-only — no TestFlight, no App Store; you clone, sign, and run via Xcode.
>
> Repo: github.com/amirsalaar/whisp2 (MIT). README has install + screenshots.
>
> Happy to answer architecture or design-tradeoff questions. iOS Gemini path and Linux build are open issues if anyone wants to pick them up.

---

### 10c. Reddit — r/MacOS

> **I built a free, open-source alternative to Wispr Flow / Superwhisper**
>
> Hold a hotkey, speak, release — transcribed text gets typed into whatever app you're using. Works in Slack, Notes, terminal, Xcode, anywhere.
>
> Free, MIT licensed, source on GitHub. macOS 13+. Use OpenAI / Groq / Gemini, or 100% on-device via local Whisper.
>
> github.com/amirsalaar/whisp2
>
> Built it because I was paying $15/mo for Wispr Flow and the actual feature is small enough that one person can maintain it.

---

### 10d. Reddit — r/rust

> **Whisp – Rust + Tauri 2 voice-to-text app for macOS + iOS**
>
> Wrote up a small Rust + Tauri 2 voice-to-text tool over the last couple months. Open source, MIT, ships a real `.dmg`.
>
> Bits I think r/rust will care about:
>
> - Tauri 2 unified entry: same `lib.rs::run()` handles desktop and iOS via `#[cfg_attr(mobile, tauri::mobile_entry_point)]`
> - cpal for audio capture (`(stop_tx, pcm_rx)` channel pattern — drop the sender to stop)
> - sqlx + SQLite for history, async all the way through
> - 3-attempt exponential-backoff retry around provider calls in `transcription/manager.rs`
> - Local Whisper inference via `whisper.cpp` Rust bindings (GGML models, on-device)
> - macOS-only modules (`hotkey/`, `injection/`, `hud/`) gated by `#[cfg(target_os = "macos")]`
>
> github.com/amirsalaar/whisp2
>
> Open issues: Linux build (no maintainer), iOS Gemini provider impl. PRs welcome.

---

### 10e. Reddit — r/iOSProgramming

> **Open-source iPhone Action Button voice-to-text using AppIntent + Live Activity**
>
> Couldn't find a good open-source example of using the Action Button for a real workflow with a Live Activity Stop button, so I built one.
>
> Architecture:
>
> - Action Button → `WhispRecordIntent` (`AppIntent`) foregrounds the host app and starts an `Activity` (Live Activity)
> - `WhispRecorder` runs `AVAudioRecorder` with no hard duration cap
> - Live Activity has an interactive `Button(intent: WhispStopIntent(sessionId:))` (iOS 17+)
> - Stop signal travels cross-process via App Group `UserDefaults` keyed by sessionId — Live Activity widget runs in a separate process from the host, so this is the rendezvous
> - Recorder polls the stop key every 100ms; also stops on `UIApplication.didBecomeActiveNotification` (re-foregrounding the app)
> - Audio → multipart upload → text typed via the host app on next launch
>
> Whole thing is Swift + Rust (transcription path is shared with macOS). Source: github.com/amirsalaar/whisp2
>
> Note: developer build only — no TestFlight, no App Store. Clone, sign, run via Xcode.
>
> Roast the design.

---

## 11. Risks + Mitigations

- **Launch falls flat (no traction)** → have a second post angle ready: lead with the iOS Action Button novelty for r/iOSProgramming + HN if the cost angle underperforms.
- **HN crowd nitpicks something legitimately broken** → run `make test`, `make ios-typecheck`, `make build` the day before. Don't launch on a broken `main`.
- **PRs flood in, none can be reviewed** → pre-write a short CONTRIBUTING.md, label 5+ scoped first-timer issues. Aim for "review same day" for the first week.
- **Contender (Superwhisper / Wispr Flow) reads the post and copies the iOS Action Button play** → not really a risk; would just validate the idea. Keep moving.
- **People expect TestFlight for iOS and get angry** → mitigated: README + every iOS post explicitly says "developer build only". Repeat in HN comments if it comes up.
- **Negative tone-policing on cost-comparison angle ("you're being mean to other devs")** → keep voice dry-not-snarky. Lead with what we built, not what they failed at.

---

## 12. Open Issues (Pre-Launch Blockers)

These need to ship before we hit Day 0:

- [ ] Record macOS demo GIF (30 sec, hotkey → speak → text appears)
- [ ] Record iOS demo clip (10 sec, Action Button → speak → Stop → text)
- [ ] Add demo GIF to README (top, under banner)
- [ ] Open ~5 labeled `good-first-issue` tickets:
  - [ ] iOS Gemini provider implementation
  - [ ] Linux build target
  - [ ] Dictionary UX redesign
  - [ ] Semantic correction provider plumbing
  - [ ] (one more — TBD when triaging)
- [ ] Write minimal `CONTRIBUTING.md`
- [ ] Pre-launch sanity: `make test && make ios-typecheck && make build` clean

---

## 13. Out of Scope for This Launch

- Product Hunt launch (different audience, different tone — schedule for ~2 weeks after HN if traction lands).
- Paid ads. Open-source launches don't need them; the hook + demo carry it.
- Press outreach (TechCrunch, The Verge). Their interest threshold is way past 1k stars; revisit if the launch actually pops.
- A landing page beyond GitHub. README + repo are the landing page.
- Discord / community. Premature; revisit if there's actual demand after launch.
