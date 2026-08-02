# OpenVoice UI Revamp — Step-by-Step Implementation Guide

This is a standalone implementation spec. It assumes the reader has not seen any
prior conversation about this project — everything needed to execute each step is
included inline: exact file paths, exact current code, exact proposed changes, why
each one matters, and how to verify it worked. Read mode note for whoever executes
this: it's written to be followed top to bottom, not searched — each step is
self-contained, but the "What not to change" section right after the summary
applies to all of them and is worth reading once before starting.

## At a glance

| # | Title | Severity | Layer | Files touched | Rough effort |
|---|---|---|---|---|---|
| 1 | Flow Bar shows nothing on failure | **P0** | Frontend only | `Overlay.tsx`, `ui/index.tsx`, `ui.css` | 30–45 min |
| 2 | No feedback on successful completion | **P1** | Frontend (+ optional Rust for persistence) | `Overlay.tsx`, `ui/index.tsx`, `ui.css`, new `sound.ts`; optionally `config.rs`, `settings.ts`, `Settings.tsx` | 1–2 hr |
| 3 | No real glow | **P1** | Frontend only — **corrected below**, no Rust needed | `Overlay.tsx`, `overlay.css` | 45–60 min |
| 4 | Waveform motion doesn't match DESIGN.md | — | Frontend only | `ui/index.tsx` | 45 min |
| 5 | No cancel-while-recording | **P2** | Frontend + Rust | `Overlay.tsx`, `main.rs`, possibly `session.rs`/`engine.rs` | varies — audit existing `Cancelled` path first |
| 6 | DESIGN.md drift | **P3** | Docs only | `DESIGN.md` | 20 min |
| 7 | Minor polish (3 items) | — | Frontend only | `Hub.tsx`, `Dictionary.tsx` | 30 min total |

Execute top to bottom; the reasoning for that order is at the very end, after
Step 7.

## What not to change (applies to every step below)

Pulled directly from `DESIGN.md`'s own prohibitions list — a UI pass that fixes
five things while quietly breaking a sixth is not a net improvement:

- No shadows anywhere **except** the two documented exceptions (the overlay
  menu, already shipped, and the Flow Bar's own glow once Step 3 lands — nowhere
  else).
- No gradients except the waveform's own falloff.
- **No green (`--live`, `--live-hi`, `--live-dim`, `--live-soft`) on anything
  that is not live state, the record action, or a focus ring.** Step 1's new
  failure state uses `--danger`, not a dimmed green, specifically to avoid
  violating this.
- No font weight above 600, anywhere, including any new copy these steps add.
- No new uppercase tracked "eyebrow" labels.
- No radius above 12px (`--r-xl`) in app chrome.
- Every new bit of copy follows the plain-language rule already established
  elsewhere (`PRODUCT.md`: "legible to someone who has never opened a
  terminal") — none of the fixes below should introduce a technical term
  ("ASR," "VAD," "IPC") into anything user-visible.

## Before you touch anything

1. Read `PRODUCT.md` and `DESIGN.md` at the repo root in full. They are the
   durable product and visual-system truth for this app.
2. Read `.impeccable/critique/2026-08-02T12-08-55Z__openvoice-hub-flow-bar-overlay.md`
   for the full design critique this guide is derived from (heuristic scores,
   persona red flags, the reasoning behind each priority below).
3. **Do not touch the app icon, wordmark, or the seven-bar waveform mark.** That
   was explicitly settled and is out of scope for this pass.
4. Run these after every step, from the repo root:
   ```
   npx tsc --noEmit --project apps/ui
   ```
   and, if any Rust file changed:
   ```
   cargo fmt --all --check
   cargo clippy --workspace --all-targets --all-features
   cargo test --workspace --all-features
   ```
   A dev build is already runnable via `cargo tauri dev` from `crates/ov-app`
   (or whatever task already starts it). The app also has a component sheet
   reachable via the Advanced screen for previewing primitives — `FlowBar`,
   `Waveform`, `Badge`, etc. — without needing a live dictation to trigger every
   state; use it to check each new prop/variant in isolation before testing the
   real overlay.

---

## Visual reference: the Flow Bar's states, before and after

Useful to have in view before editing anything. Widths/heights are logical
pixels, matching the actual `setSize()` calls in `Overlay.tsx`.

**Idle (150×40, unchanged by this guide)**
```
┌────────────────────┐
│ ●   Hold ⌐Right Ctrl⌐│    ● = 7px dot, --mute (grey)
└────────────────────┘       border: 1px, neutral hairline
```

**Listening — today (218×40)**
```
┌──────────────────────────────┐
│ ●  ıl.ıllı.ılı..lıı    0:04   │   ● = --live (green), border: --live-dim
└──────────────────────────────┘   (very dark green — critique's P1 finding:
                                     this barely reads as green in the periphery)
```

**Listening — after Step 3 (246×68, glow margin included)**
```
   ╭╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╮   ← soft --live-soft glow,
  ╎ ┌──────────────────────────────┐ ╎     painted into 14px of
  ╎ │ ●  ıl.ıllı.ılı..lıı    0:04   │ ╎     window space that didn't
  ╎ └──────────────────────────────┘ ╎     exist before (Step 3)
   ╰╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╯   border now full-chroma --live, 1.5px
```

**Working — today and after (170×40, unchanged)**
```
┌───────────────────┐
│ ●   Writing…        │   ● = --body (neutral) — deliberately not green;
└───────────────────┘    "green means the mic is open," per DESIGN.md
```

**Just finished — new in Step 2 (~320ms transition, 170→150px)**
```
┌───────────────────┐        ┌────────────────────┐
│ ◉   Writing…        │  →   │ ●   Hold ⌐Right Ctrl⌐│
└───────────────────┘        └────────────────────┘
  ◉ = --live-hi flash            settles back to --mute
  (+ optional two-tone chime, see Step 2)
```

**Failed — new in Step 1 (240×40)**
```
┌──────────────────────────────────────┐
│ ●   Copied to clipboard — press Ctrl+V │   ● and border = --danger (#f45b5b)
└──────────────────────────────────────┘   ~6.5:1 contrast on pure black —
                                             comfortably readable at a glance
```

---

## Step 1 [P0] — The Flow Bar shows nothing when a dictation fails

### The problem, precisely

`apps/ui/src/windows/Overlay.tsx` currently derives its entire visual state from
two booleans:

```tsx
const live = view.state === "listening";
const working = view.state === "transcribing" || view.state === "injecting";
```

`view` comes from `useLiveEngine()` (`apps/ui/src/engine/useLiveEngine.ts`), whose
`LiveView.state` can also be `"fault"`, and which separately carries a
`notice: { level: "info" | "warn" | "error"; message: string } | null` field.
Neither is read anywhere in `Overlay.tsx`.

Trace exactly what triggers each one (`apps/ui/src/engine/useLiveEngine.ts`,
`reduce()`, and `apps/ui/src/engine/types.ts`, `Outcome`):

- A **microphone/capture failure** or **transcription failure**
  (`Outcome.kind === "asr_failed" | "capture_failed"`) sets `state: "fault"` via
  the `Finished` case. This is the one case `state === "fault"` alone would catch.
- A **failed auto-paste that fell back to the clipboard**
  (`Outcome.kind === "clipboard_fallback"`) does **not** set `state` to `"fault"`
  at all — it is **not** in the `failed` check in the `Finished` case:
  ```ts
  case "Finished": {
    const failed =
      e.outcome.kind === "asr_failed" || e.outcome.kind === "capture_failed";
    return { ...v, state: failed ? "fault" : v.state, /* ... */ };
  }
  ```
  The only signal for the clipboard-fallback case — the single most common
  failure a user will actually hit, since it covers every app that blocks
  synthetic paste — is a separate `Notice` event with `level: "warn"`, and the
  reducer's `Notice` case only flips `state` to `"fault"` for `level: "error"`:
  ```ts
  case "Notice":
    return {
      ...v,
      notice: { level: e.level, message: e.message },
      state: e.level === "error" ? "fault" : v.state,
    };
  ```
  **Consequence, stated plainly: checking `state === "fault"` alone will
  silently miss the clipboard-fallback case.** A fix that only adds a `fault`
  branch and calls it done will still fail on the single most common failure
  path. You must render based on `view.notice` directly, independent of
  `state`, in addition to the `fault` branch.

Right now: hold the hotkey in an app that blocks synthetic paste (a UAC-elevated
window, some Electron apps, some games), release it, and the Flow Bar shrinks
back to "Hold Right Ctrl" — pixel-identical to having said nothing at all —
while the actual transcript sits on the clipboard with zero on-screen
indication that anything happened, let alone that a fallback occurred.

### The fix

Edit `apps/ui/src/windows/Overlay.tsx`.

**1.** Pull `dismissNotice` out of the hook (currently only `view` and
`levelRef` are destructured):

```tsx
const { view, levelRef, dismissNotice } = useLiveEngine();
```

**2.** Add a `failed` flag and prefer the live notice message, with a generic
fallback so a bare `fault` state (no notice attached, or the notice already
expired before this render) never silently renders as empty idle:

```tsx
const live = view.state === "listening";
const working = view.state === "transcribing" || view.state === "injecting";
const failed = view.state === "fault" || view.notice !== null;
const failMessage =
  view.notice?.message ?? (view.state === "fault" ? "Something went wrong" : "");
```

**3.** Auto-clear the notice after a few seconds so it doesn't linger forever on
a peripheral surface nobody manually dismisses (the Hub has a dismiss control
for its own notice display; the overlay doesn't, and — being glanced at for a
second at a time, per `PRODUCT.md` — shouldn't need one):

```tsx
useEffect(() => {
  if (!view.notice) return;
  const id = window.setTimeout(dismissNotice, 6000);
  return () => window.clearTimeout(id);
}, [view.notice, dismissNotice]);
```

**4.** Update the screen-reader announcement so the failure is spoken, not just
shown — this one line is the entire difference between the fix being
accessible and being sighted-only:

```tsx
const spoken = failed
  ? failMessage
  : live
    ? "Listening"
    : working
      ? "Writing your words"
      : "Ready. Hold the shortcut to dictate.";
```

(This replaces the existing three-way `spoken` ternary — it feeds the same
`<span className="sr-only" role="status" aria-live="polite">{spoken}</span>`
already present in the file.)

**5.** Add a width tier — 240px comfortably fits the longest current notice
copy ("Copied to clipboard — press Ctrl+V" is 34 characters at 12px caption
size) with room to spare for anything shorter — and pass the new props into
`FlowBar`:

```tsx
const width = menu ? 280 : failed ? 240 : live ? 218 : working ? 170 : 150;
```
```tsx
<FlowBar
  live={live}
  levelRef={levelRef}
  elapsed={elapsed(view.elapsedMs)}
  hint={view.ready?.shortcut ?? "Right Ctrl"}
  working={working}
  failed={failed}
  failMessage={failMessage}
/>
```

Now edit `apps/ui/src/ui/index.tsx`, the `FlowBar` component. **6.** Extend the
props and render a fourth branch, checked first so a failure always wins over a
stale `live`/`working` read:

```tsx
export function FlowBar({
  live,
  level,
  levelRef,
  elapsed,
  hint = "Right Ctrl",
  working,
  failed,
  failMessage,
}: {
  live: boolean;
  level?: number;
  levelRef?: { current: number };
  elapsed: string;
  hint?: string;
  working?: boolean;
  /** A dictation ended in trouble — asr/capture failure, or a paste that fell
   *  back to the clipboard. This is the one state this window must never show
   *  identically to idle; see docs/UI-REVAMP-GUIDE.md Step 1. */
  failed?: boolean;
  failMessage?: string;
}) {
  return (
    <div className="flowbar" data-live={live} data-working={working} data-failed={failed}>
      <span className="flowbar-mic" />
      {failed ? (
        <span className="t-caption flowbar-fail-msg">{failMessage}</span>
      ) : live ? (
        <>
          <div className="flowbar-wave">
            <Waveform level={level} levelRef={levelRef} bars={32} />
          </div>
          <span className="flowbar-time">{elapsed}</span>
        </>
      ) : (
        <div className="flowbar-idle">
          {working ? (
            <span className="t-caption" style={{ color: "var(--body)" }}>
              Writing…
            </span>
          ) : (
            <>
              <span className="t-caption" style={{ color: "var(--mute)" }}>
                Hold
              </span>
              <Kbd>{hint}</Kbd>
            </>
          )}
        </div>
      )}
    </div>
  );
}
```

Now edit `apps/ui/src/ui/ui.css` — add the fail-state styling next to the
existing `.flowbar[data-live="true"]` / `.flowbar[data-working="true"]` rules
(around lines 493–542):

```css
/* A dictation ended in trouble -- asr/capture failure, or a paste that fell back
 * to the clipboard. --danger, not the --warn amber the History badge uses for
 * this same outcome elsewhere -- this window has one glance to be understood
 * with no time to read a legend, so it borrows the app's one unambiguous
 * "stop and look" color rather than a color whose meaning has to be learned
 * from a legend the user has never seen. Never --live-dim or any green here:
 * green means the microphone is open, full stop, per DESIGN.md. */
.flowbar[data-failed="true"] {
  border-color: var(--danger);
}

.flowbar[data-failed="true"] .flowbar-mic {
  background: var(--danger);
}

.flowbar-fail-msg {
  color: var(--ink);
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
```

`--danger` is `#f45b5b`. Against the pill's near-black background
(`rgb(14 14 16 / 98%)`, effectively `#0e0e10`), its contrast ratio is
**~6.5:1** — comfortably above the 4.5:1 AA minimum for the small caption text
it's used for here, and close to the 7:1 AAA threshold. No new contrast risk
introduced.

### Acceptance criteria

- Dictate normally into a text field: no change from current behavior.
- Force a clipboard-fallback failure (e.g. dictate into a window that blocks
  synthetic paste, or point the target at an elevated-privilege window) — the
  Flow Bar must turn its border/dot **red** and show "Copied to clipboard —
  press Ctrl+V" (or whatever the live `Notice` message is) for ~6 seconds, then
  return to idle without any manual dismissal.
- Force a capture/mic failure (unplug the mic, or trigger whatever produces
  `capture_failed`/`asr_failed` in this build) — same red state, with either
  the specific message carried on the event or the "Something went wrong"
  fallback if none was attached.
- Read the `sr-only` live region via dev tools (or an actual screen reader) —
  it must announce the failure message, not silence.
- Nothing about this step should make the bar flash red on a **successful**
  dictation — verify by dictating normally several times in a row.

---

## Step 2 [P1, partially frontend] — No signature feedback when a dictation completes successfully

### The problem

On a successful `Finished`, `Overlay.tsx`'s `working` flag goes `false` and the
bar just shrinks back to its idle width. No color change, no motion, no sound.
`PRODUCT.md` itself frames the overlay as something "seen for two seconds at a
time, in peripheral vision" — the one moment this product exists to deliver
(you spoke, it worked) currently produces zero glanceable confirmation. Direct
competitors treat this moment as worth designing on purpose: Wispr Flow's own
marketing materials describe a glow ring tied to the start of dictation as a
signature visual beat, and Superwhisper ships explicit, independently-toggleable
start/end sound effects specifically so a user doesn't have to look at the
screen to know a dictation landed. OpenVoice currently does neither.

### Fix, part A: a visual "confirmation blink" (pure frontend)

In `apps/ui/src/windows/Overlay.tsx`, track the transition from `working` to
neither `live` nor `working` (i.e. a successful completion — a failure is
already fully handled by Step 1's `failed` branch, so this effect must not also
fire then):

```tsx
const [justFinished, setJustFinished] = useState(false);
const wasWorking = useRef(false);

useEffect(() => {
  if (wasWorking.current && !working && !live && !failed) {
    setJustFinished(true);
    const id = window.setTimeout(() => setJustFinished(false), 320);
    return () => window.clearTimeout(id);
  }
  wasWorking.current = working;
}, [working, live, failed]);
```

Pass `justFinished` into `FlowBar` as a new `confirm` prop, applied as a
`data-confirm` attribute on `.flowbar-mic` alongside the existing
`data-live`/`data-failed` conditions already added in Step 1:

```tsx
<span className="flowbar-mic" data-confirm={justFinished} />
```

```css
/* A brief acknowledgement that a dictation landed -- distinct from "listening"
 * (--live, while the mic is open) and distinct from --danger (Step 1's failure
 * state). A pulse the instant text is confirmed placed, nothing more. */
.flowbar-mic[data-confirm="true"] {
  background: var(--live-hi);
  transition: background 320ms var(--ease);
}
```

This is a ~320ms brighten-then-settle on the mic dot only. It must stay subtle
and short — this is a peripheral utility window, not a celebration animation,
and per the "no green outside live state" rule the confirm flash should still
read as *a variant of* the mic-open color family, not an unrelated new hue.

### Fix, part B: an optional completion sound

No audio assets exist anywhere in this repo today. Do **not** add a shipped
audio file — synthesize a short two-tone chime with the Web Audio API instead,
so there is nothing to bundle, license, localize, or ship at a particular
sample rate:

```tsx
// apps/ui/src/ui/sound.ts (new file)
let ctx: AudioContext | null = null;

/** A quiet two-tone confirmation chime, synthesized rather than shipped as an
 *  asset -- see docs/UI-REVAMP-GUIDE.md Step 2. Deliberately tiny: two sine
 *  tones roughly a fifth apart, ~140ms total including the tail, well under
 *  anything that would be jarring in a shared room. */
export function playCompletionChime() {
  ctx ??= new AudioContext();
  const now = ctx.currentTime;
  [880, 1320].forEach((freq, i) => {
    const osc = ctx!.createOscillator();
    const gain = ctx!.createGain();
    osc.frequency.value = freq;
    osc.type = "sine";
    gain.gain.setValueAtTime(0, now + i * 0.05);
    gain.gain.linearRampToValueAtTime(0.05, now + i * 0.05 + 0.01);
    gain.gain.linearRampToValueAtTime(0, now + i * 0.05 + 0.09);
    osc.connect(gain).connect(ctx!.destination);
    osc.start(now + i * 0.05);
    osc.stop(now + i * 0.05 + 0.1);
  });
}
```

Call `playCompletionChime()` in the same effect as `setJustFinished(true)`
above, gated by whichever persistence option below is chosen.

**Persisting the setting — two options, pick based on how much backend scope is
in play for this pass:**

- **Frontend-only (stays in pure-UI scope):** gate the call on
  `localStorage.getItem("ov.completionSound") !== "off"`, with a small toggle
  added wherever is convenient in the existing Settings screen (or even just
  the overlay's own right-click menu, next to "Only show while dictating").
  Simplest option; resets if the user clears browser storage, and isn't visible
  to `ov-cli` or synced anywhere else.
- **Real persisted setting (small, well-precedented Rust change):** add
  `pub completion_sound: bool` (default `true`) to `Config` in
  `crates/ov-core/src/config.rs`. The struct already uses `#[serde(default)]`
  at the struct level — the `language` field was added the exact same way
  earlier in this project's history with no schema-version bump required, so
  this has a direct precedent to copy. Thread it through
  `apps/ui/src/engine/settings.ts`'s `Config` interface, and add a `Toggle` row
  in `SettingsScreen` (`apps/ui/src/screens/Settings.tsx`) following the exact
  pattern already used for `privacy.retain_audio`:
  ```tsx
  <Toggle
    on={settings.config.completion_sound}
    onChange={(v) => patch((s) => (s.config.completion_sound = v))}
    label="Play a sound when a dictation finishes"
  />
  ```
  This is the better long-term answer — do it if backend changes are in scope.

### Acceptance criteria

- Successful dictation: mic dot briefly brightens to `--live-hi` then eases
  back to `--body`/`--mute` over ~320ms as the bar shrinks — one smooth
  "landed" moment, not a strobe.
- With the sound enabled, a quiet ascending two-tone chime plays on completion;
  with it disabled (via whichever persistence option was chosen), nothing
  plays, and no console error appears from a suspended/missing `AudioContext`.
- The confirm blink and the chime must **never** fire on a `failed` completion
  — verify by forcing the Step 1 failure path again and confirming silence and
  no green flash, only the red state.
- Rapid back-to-back dictations (start a second one within the 320ms window):
  the blink timeout must not leak or stack — check via the `useEffect` cleanup
  already written above (`clearTimeout` on every re-run) rather than adding a
  second, uncoordinated timer.

---

## Step 3 [P1 — pure frontend, no Rust needed] — The Flow Bar has no real glow

**Scope correction from an earlier pass of this guide:** this step was
originally flagged as needing a Rust change. It doesn't. `tauri.conf.json`'s
`"resizable": false` on the overlay window only disables the user
manually dragging the OS-level resize handles — it does not block programmatic
`setSize()` calls, which is exactly the mechanism `Overlay.tsx` already uses
every time `live`/`working`/`menu` changes today. There is also no
`min_size`/`max_size` set anywhere in `crates/ov-app/src/overlay.rs`. Confirmed
by reading both files directly before writing this step — don't take this on
faith, but there is no Rust-side obstacle here.

### The problem

`DESIGN.md` documents one sanctioned exception to "no shadows anywhere": the
Flow Bar is supposed to get "a soft two-layer shadow and a backdrop blur"
because it floats over content this design system doesn't control. The shipped
`apps/ui/src/windows/overlay.css` has neither, and says exactly why in its own
comments:

```css
/* Nearly opaque, and no backdrop-filter.
 * `backdrop-filter` on a transparent window samples the *window's* backdrop,
 * not the desktop behind it -- so it blurs nothing... */
```
```css
/* No shadow.
 * The window is now sized to the pill exactly (see Overlay.tsx)... a shadow
 * clipped at the window edge would read as a hard line anyway. */
```

Both are correct, real Windows-webview-transparency constraints — not
something to just "turn back on." But the *reason* a shadow was removed (the
window has zero spare pixels around the pill) is also the thing that's fixable:
`Overlay.tsx` already proves spare window area works fine for exactly this
purpose. Its own context menu does it today:

```css
/* apps/ui/src/windows/overlay.css, .overlay-menu */
box-shadow: 0 2px 8px rgb(0 0 0 / 40%), 0 12px 32px rgb(0 0 0 / 36%);
```

That renders correctly because `height = menu ? 226 : 40` already reserves real
window space below the pill for the menu to occupy. The fix for the glow is the
same trick: reserve a small margin around the pill *only while listening*, and
paint the glow into that margin.

### The fix

In `apps/ui/src/windows/Overlay.tsx`, add a margin reserved only in the `live`
state — **and explicitly not when the context menu is also open**, since the
menu already claims the larger 280×226 box for its own purposes and stacking
the glow margin on top of that would both overshoot the intended size and try
to render a glow the CSS below has no matching space for:

```tsx
/** Extra window space reserved on every side, only while listening and only
 *  when the context menu is closed, so a real glow can render without being
 *  clipped at the window edge -- the same trick the right-click menu already
 *  uses for its own shadow (see the "no shadow" comment in overlay.css this
 *  step is fixing). Excluded whenever `menu` is true so the two enlarged
 *  states never compound into a wrong, unstyled size. */
const GLOW_MARGIN = 14;
const glowing = live && !menu;

const pillWidth = menu ? 280 : failed ? 240 : live ? 218 : working ? 170 : 150;
const pillHeight = menu ? 226 : 40;
const width = glowing ? pillWidth + GLOW_MARGIN * 2 : pillWidth;
const height = glowing ? pillHeight + GLOW_MARGIN * 2 : pillHeight;
```

Wrap the existing `.overlay-hit` pill with the reserved margin, and mark
`glowing` in the DOM so the CSS below can key off exactly the same condition
rather than reimplementing it:

```tsx
<div
  className="overlay-root"
  style={glowing ? { padding: GLOW_MARGIN, boxSizing: "border-box" } : undefined}
>
  {/* ... existing sr-only span ... */}
  <div
    className="overlay-hit"
    data-glow={glowing}
    onMouseDown={startDrag}
    onContextMenu={(e) => {
      e.preventDefault();
      setMenu((m) => !m);
    }}
    title="Drag to move · right-click for options"
  >
```

In `apps/ui/src/windows/overlay.css`, add the glow, keyed off the same
`data-glow` attribute set above so JS and CSS can never disagree about when it
applies:

```css
/* The one shadow exception DESIGN.md actually promises this window -- deferred
 * until now because the window had no spare pixels to render it into. Fixed by
 * reserving GLOW_MARGIN of real window space in Overlay.tsx, only while live
 * and only when the context menu isn't also open (see `glowing` there), the
 * same trick the right-click menu already uses for its own shadow. */
.overlay-hit {
  transition: box-shadow var(--t-slow) var(--ease);
}

.overlay-hit[data-glow="true"] {
  box-shadow: 0 0 24px 4px var(--live-soft), 0 0 48px 8px rgb(68 214 44 / 6%);
}
```

Also strengthen the border itself, since the critique flagged the existing
`--live-dim` (`#1d5a13`, an extremely dark desaturated green) as too faint at
1px on a near-black pill to register in peripheral vision — the entire job
this surface has:

```css
.flowbar[data-live="true"] {
  border-color: var(--live);
  border-width: 1.5px;
}
```

### Acceptance criteria

- Idle/working/failed: window size and appearance unchanged from before this
  step.
- Listening, menu closed: the pill visibly sits inside a soft green glow that
  isn't clipped at a hard rectangular edge, and the border reads as
  unambiguously green rather than near-black-with-a-hint-of-green.
- **Listening, then right-click to open the menu**: the window must resize to
  the menu's own 280×226 box with no glow artifact and no leftover/incorrect
  margin — this is the specific edge case this revision of the guide added a
  fix for; test it explicitly, not just the two states independently.
- Drag-to-move and right-click-for-menu still work identically — the hit-target
  math in `Overlay.tsx` only changed the window's total size and padding, not
  the pill's own click/drag handlers.

---

## Step 4 — The waveform's motion doesn't match what DESIGN.md promises

### The problem

`DESIGN.md`'s Motion section: "the waveform... is real data at 30 Hz with VU
ballistics — 60ms attack, 380ms release, 800ms peak hold. That asymmetry is
what makes it read as an instrument rather than a progress bar."

The shipped `Waveform` component (`apps/ui/src/ui/index.tsx`) uses one
symmetric interpolation constant, applied identically whether the signal is
rising or falling:

```tsx
const CHASE = 0.28;
// ...
current[i] += (target[i] - current[i]) * CHASE;
```

There is no attack/release split and no peak-hold anywhere in the tick loop.
This is the concrete mechanism behind the critique's observation that even the
"peak" moment (mid-dictation, the waveform itself) reads flatter than the
design system claims — the doc describes an instrument; the code ships a
generic low-pass filter.

### The fix

Replace the single `CHASE` constant with attack/release-specific rates derived
from the documented time constants, and add a per-bar peak-hold value. This
stays inside the existing `requestAnimationFrame` tick — no new re-renders, no
new DOM nodes, and the ref-based (not React-state) level plumbing is untouched.

```tsx
useEffect(() => {
  const el = host.current;
  if (!el) return;

  const n = bars;
  const target = new Float32Array(n);
  const current = new Float32Array(n);
  const peak = new Float32Array(n);
  const peakHeldAt = new Float32Array(n);
  const nodes = Array.from(el.children) as HTMLElement[];

  let raf = 0;
  let lastShift = 0;
  let lastFrame = performance.now();

  const SHIFT_MS = 55;
  const MIN = 0.05;
  // DESIGN.md's documented VU ballistics: fast to rise, slow to fall, briefly
  // held at the top -- this asymmetry is what reads as an instrument rather
  // than a generic progress bar. Converted from a per-frame chase constant to
  // per-ms decay constants so the result is frame-rate independent.
  const ATTACK_MS = 60;
  const RELEASE_MS = 380;
  const PEAK_HOLD_MS = 800;

  const tick = (now: number) => {
    raf = requestAnimationFrame(tick);
    const dt = now - lastFrame;
    lastFrame = now;

    if (now - lastShift >= SHIFT_MS) {
      lastShift = now;
      const raw = external.current ? external.current.current : latest.current;
      target.copyWithin(0, 1);
      target[n - 1] = isIdle.current ? 0 : Math.min(1, Math.max(0, raw));
      peak.copyWithin(0, 1);
      peakHeldAt.copyWithin(0, 1);
      if (target[n - 1] > peak[n - 1]) {
        peak[n - 1] = target[n - 1];
        peakHeldAt[n - 1] = now;
      }
    }

    for (let i = 0; i < n; i++) {
      const rising = target[i] > current[i];
      // Exponential approach to a per-ms time constant: 1 - 0.5^(dt/halflife)
      // approximates the classic VU-meter charge/discharge curve closely
      // enough for a 2px bar, without needing a full envelope-follower model.
      const halflife = rising ? ATTACK_MS : RELEASE_MS;
      const rate = 1 - Math.pow(0.5, dt / halflife);
      current[i] += (target[i] - current[i]) * rate;

      // Peak hold: briefly show the loudest recent value even as `current`
      // relaxes back down, then let it fall away once the hold window passes.
      const heldFor = now - peakHeldAt[i];
      const shown =
        heldFor < PEAK_HOLD_MS ? Math.max(current[i], peak[i]) : current[i];

      const v = MIN + Math.sqrt(shown) * (1 - MIN);
      nodes[i].style.transform = `scaleY(${v.toFixed(4)})`;
    }
  };

  raf = requestAnimationFrame(tick);
  return () => cancelAnimationFrame(raf);
}, [bars]);
```

Update the existing doc comment on `Waveform` — the current paragraph
explaining "0.28 settles ~90% within one shift interval" describes a constant
that no longer exists after this change. Replace it with a short note on the
attack/release/peak-hold values instead, so the comment stays true to the code
(this is exactly the kind of drift Step 6 exists to fix elsewhere — don't
reintroduce a fresh instance of it here).

### Acceptance criteria

- Speaking a sudden loud word: the corresponding bar should visibly snap up
  fast (60ms) rather than glide.
- Going quiet after a loud word: the bar should visibly linger near its peak
  for a beat before easing down — not truncate to silence instantly, and not
  glide down at the same speed it went up.
- Idle: identical to before this step — bars settle at `MIN` height,
  `data-idle="true"` styling unchanged, since `isIdle.current` still forces
  `target` to 0 and the peak-hold logic decays along with it.
- No new re-renders introduced — verify with the React DevTools profiler that
  this component still renders once per mount and does all animation via
  direct DOM writes (`nodes[i].style.transform`), never through `setState`.

---

## Step 5 [P2, requires Rust changes] — No way to cancel a dictation once started

### The problem

Push-to-talk means releasing the key always transcribes and injects — there is
no discard path. `Overlay.tsx` has no `Escape`-key handler, and the right-click
menu (Open OpenVoice / Paste last transcript / Hide for an hour / Only show
while dictating) has no "discard this" item. Wispr Flow's equivalent overlay
ships explicit Cancel/Stop controls during recording; this app has neither.

### Scope note

This is **not** a pure-UI fix — discarding an in-flight recording requires the
Rust engine to actually stop capture and skip transcription/injection for that
session. Before writing any new code, check whether the plumbing already
exists: `crates/ov-input/src/hook.rs` already emits `HotkeyEvent::Cancelled` on
a system-wide `Escape` press (see its handling in that file), and
`crates/ov-core/src/session.rs` may already have a corresponding `Input`
variant that discards a session without transcribing it — read both files
before assuming this needs to be built from scratch. If it already exists,
this step may only need a new `#[tauri::command]` in `crates/ov-app/src/main.rs`
that calls into it, plus the frontend listener below. If it doesn't fully
exist, it needs a new `Effect`/`Input` path through `session.rs` and
`crates/ov-app/src/engine.rs`.

**If this pass is meant to stay pure-frontend, skip this step and come back to
it separately** — it's included here only so "the entire UI" plan doesn't
silently omit the one interaction gap that isn't CSS-fixable.

### The fix (once backend support is confirmed or added)

In `apps/ui/src/windows/Overlay.tsx`, add a listener scoped to the `live`
state only, so Escape does nothing unexpected while idle/working/failed (where
a stray Escape press should not, for instance, dismiss the failure notice
early — that already has its own 6-second timer from Step 1):

```tsx
useEffect(() => {
  if (!live) return;
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") call("cancel_dictation");
  };
  window.addEventListener("keydown", onKey);
  return () => window.removeEventListener("keydown", onKey);
}, [live]);
```

(`call()` is the existing local helper already defined near the top of
`Overlay.tsx` that no-ops outside Tauri — reuse it rather than importing
`invoke` directly, for consistency with every other command call in this
file.) Wire the matching `#[tauri::command]` in `crates/ov-app/src/main.rs`
alongside the existing `overlay_snooze`/`paste_last` commands.

### Acceptance criteria

- Start a dictation, press Escape before releasing the hotkey: nothing gets
  transcribed or injected, and the Flow Bar returns to idle — not `working`,
  not `fault`.
- History has no new row for the cancelled attempt.
- Pressing Escape while idle, working, or in the Step-1 failure state does
  nothing (no console error, no state change) — confirm the `live`-only guard
  actually prevents the listener from firing outside that state.

---

## Step 6 [P3] — DESIGN.md has drifted from the shipped code

Three concrete disagreements to reconcile, all in `DESIGN.md`:

1. **The shadow/blur promise for the Flow Bar** — true again once Step 3 lands.
   If Step 3 is skipped, rewrite this section to describe the actual
   Windows-safe glow mechanism (a reserved-margin box-shadow, not a
   `backdrop-filter` blur) instead of an effect that can't exist in this
   window type.
2. **The VU-ballistics description** — true again once Step 4 lands. If
   skipped, rewrite to describe the actual single-rate interpolation instead
   of the attack/release/peak-hold values that wouldn't yet be implemented.
3. **The `--faint` value**: `DESIGN.md` documents `#6b6b6b` (3.4:1, "disabled
   only"); `apps/ui/src/styles/tokens.css` ships a different value, with a
   separate `--disabled` token having since taken over the disabled-only role.
   Read the current `tokens.css` color-ladder section in full and reconcile
   `DESIGN.md`'s Color section to match exactly — including documenting the
   `--disabled` token and `--live-ring` (used for input focus glow), neither
   of which currently appears in `DESIGN.md` at all.

Do this step **last**, after Steps 3 and 4 land (or are explicitly deferred),
so it documents the true end state rather than needing a second pass
immediately after.

---

## Step 7 — Minor polish (independent of the steps above; do any time)

**1. The Hub sidebar's brand-mark waveform looks broken at rest.**
`apps/ui/src/windows/Hub.tsx`, ~line 141:
```tsx
<Waveform levelRef={levelRef} bars={5} idle={!live} />
```
When `idle`, every bar settles at `MIN` height (5%) with `--hairline-strong`
color — five flat, barely-visible dashes. A first-time viewer has no way to
know this is a mark that moves rather than a rendering glitch. Fix: give the
idle brand-mark a fixed, deliberately-uneven static profile instead of
flattening every bar to `MIN`. Add an optional `staticProfile?: number[]` prop
to `Waveform` (used only by the sidebar mark, never by the overlay, which
always has real or zeroed live data) that seeds `current[i]` directly from the
array instead of chasing `target` toward 0:
```tsx
// In Waveform, when `idle && staticProfile` is provided, skip the chase entirely
// and paint the fixed profile once rather than animating toward MIN:
if (idle && staticProfile) {
  nodes.forEach((node, i) => {
    const v = MIN + Math.sqrt(staticProfile[i % staticProfile.length]) * (1 - MIN);
    node.style.transform = `scaleY(${v.toFixed(4)})`;
  });
  return; // skip starting the rAF loop for a mark that never animates at rest
}
```
Use a profile matching the "asymmetric utterance envelope" shape `DESIGN.md`'s
mark section already describes for the static icon, e.g.
`[0.3, 0.55, 0.4, 0.6, 0.35]`, so it reads as an intentional wordmark at rest
rather than a disabled control.

**2. History's Copy button gives no confirmation.**
`apps/ui/src/windows/Hub.tsx`, `HistoryRow`, ~line 446:
```tsx
<Button size="sm" variant="ghost" onClick={() => navigator.clipboard?.writeText(row.final_text)}>
  Copy
</Button>
```
Add local state so the label swaps briefly:
```tsx
const [copied, setCopied] = useState(false);
// ...
<Button
  size="sm"
  variant="ghost"
  onClick={() => {
    navigator.clipboard?.writeText(row.final_text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }}
>
  {copied ? "Copied" : "Copy"}
</Button>
```

**3. Dictionary's "Remove" has no undo.**
In `apps/ui/src/screens/Dictionary.tsx`, locate the remove handler for a
dictionary entry. Given this is a low-stakes, easily-re-added mapping, a full
undo system is overkill — instead, reuse the existing `Notice` primitive
(`apps/ui/src/ui/index.tsx`) to show a brief dismissible confirmation with an
"Undo" action, using the same `action` prop `Notice` already supports
elsewhere in the app, rather than introducing a new toast system for one
button.

---

## Definition of done

Before calling this pass complete, confirm every item below — this aggregates
the acceptance criteria from each step into one pre-handoff checklist:

- [ ] A clipboard-fallback failure turns the Flow Bar red with a legible
      message, auto-clearing after ~6s (Step 1).
- [ ] An asr/capture failure does the same, with either the real message or
      the generic fallback (Step 1).
- [ ] The `sr-only` region announces the failure text (Step 1).
- [ ] A successful dictation shows the confirm-blink and (if enabled) plays
      the chime; a failed one shows neither (Step 2).
- [ ] Listening shows a visible, non-clipped green glow; the border reads as
      unambiguously green (Step 3).
- [ ] Opening the right-click menu while listening resizes correctly with no
      glow artifact (Step 3 — the specific regression this revision guards
      against).
- [ ] The waveform visibly snaps up fast and eases down slower with a brief
      peak hold, both live and in the component sheet (Step 4).
- [ ] (If done) Escape during a live dictation discards it with no history
      row created (Step 5).
- [ ] `DESIGN.md` matches the shipped code on the shadow/blur, VU-ballistics,
      and `--faint`/`--disabled`/`--live-ring` points (Step 6).
- [ ] Sidebar mark looks intentional at rest; History Copy confirms; Dictionary
      Remove offers Undo (Step 7).
- [ ] `npx tsc --noEmit --project apps/ui` is clean.
- [ ] If any Rust file changed: `cargo fmt --all --check`,
      `cargo clippy --workspace --all-targets --all-features`, and
      `cargo test --workspace --all-features` all pass.
- [ ] Nothing in "What not to change" was violated — in particular, grep the
      diff for `--live` outside a genuinely live-state context, and for any
      new `box-shadow` outside Step 3's one addition.

---

## Suggested execution order, and why

1. **Step 1 (P0)** — fixes an actual information loss; do this first
   regardless of how much of the rest gets done. Everything else is
   improving something that already communicates; this fixes something that
   currently communicates nothing.
2. **Step 2 (P1)** — the release-feedback moment, the single thing named as
   top priority going into this guide.
3. **Step 3 (P1)** — the glow, same reason, and it shares enough surface area
   with Step 2 (both touch the `live`/`working` transition and the pill's
   visual chrome) that doing them back to back avoids re-deriving context.
4. **Step 4** — makes Steps 2 and 3's moment feel considered rather than
   merely correct; naturally follows once the surrounding chrome is settled.
5. **Step 7** — cheap, fully independent of everything else; slot in
   whenever there's a spare moment.
6. **Step 5** — only if backend scope is in play for this pass; otherwise
   defer entirely rather than doing it half-scoped.
7. **Step 6** — last, always, so it documents the true end state rather than
   needing a second pass immediately after.

After each step, re-run `npx tsc --noEmit --project apps/ui` and, for anything
touching `apps/ui/src/ui/index.tsx` or `ui.css`, sanity-check the component
sheet (reachable from the Advanced screen) since several primitives are
previewed there outside a live dictation — it's the fastest way to see a new
`FlowBar` prop or `Waveform` variant render without needing to reproduce a real
failure or completion first.
