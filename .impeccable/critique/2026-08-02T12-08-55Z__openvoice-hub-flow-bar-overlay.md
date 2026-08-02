---
target: OpenVoice Hub + Flow Bar overlay
total_score: 27
max_score: 40
na_heuristics: 
p0_count: 1
p1_count: 2
timestamp: 2026-08-02T12-08-55Z
slug: openvoice-hub-flow-bar-overlay
---
Method: dual-agent (A: design review · B: detector+browser)
⚠️ Partial degradation: the Chrome browser bridge was unavailable in this environment for *both* agents (confirmed independently, not a single fluke — three separate connection attempts across two agents and the parent all failed with "extension not connected"). Assessment A's visual judgments are derived from a full read of every screen's source and CSS, not from rendered screenshots — flagged inline wherever it matters. Assessment B's mechanical CLI scan ran cleanly and is full-fidelity; only its browser/console-injection portion is missing.

## Design Health Score

| # | Heuristic | Score | Key Issue |
|---|-----------|-------|-----------|
| 1 | Visibility of System Status | 2 | Overlay handles idle/listening/working cleanly but has **no visual branch for the `fault` state at all**. |
| 2 | Match Between System and Real World | 4 | Consistently plain language throughout ("Writing style" not "Profiles", "Accurate"/"Light"/"Fastest" not model IDs). |
| 3 | User Control and Freedom | 2 | No cancel/abort affordance while recording; overlay menu dismissible, settings reversible. |
| 4 | Consistency and Standards | 3 | Token system applied faithfully everywhere; undercut by DESIGN.md-vs-shipped-CSS drift (shadow/blur promise, VU ballistics, `--faint` value). |
| 5 | Error Prevention | 3 | Max-recording-duration failsafe exists; Dictionary "Remove" has no confirm/undo. |
| 6 | Recognition Rather Than Recall | 3 | All-text nav; overlay's right-click menu is discoverable only via a hover tooltip a glancing user will never trigger. |
| 7 | Flexibility and Efficiency of Use | 2 | No shortcut remapping UI; no tab-switch accelerators; no cancel/bulk actions. |
| 8 | Aesthetic and Minimalist Design | 4 | The strongest heuristic — disciplined surface ladder, one rationed accent, no clutter. |
| 9 | Error Recovery | 2 | `StartupError` in the Hub is genuinely excellent; the Flow Bar — on-screen during a mid-dictation failure — shows nothing (same root cause as #1). |
| 10 | Help and Documentation | 2 | No in-app help surface; empty-state copy is a good substitute for first-run guidance but is the entire budget. |
| **Total** | | **27/40** | **Acceptable** (67.5%) — a well-crafted shell with two structural gaps sitting exactly on the surface you asked me to scrutinize hardest. |

## Design Specificity Verdict

**Genuinely authored, not a reskin — with one real asterisk.** The waveform is simultaneously the brand mark and the live-state indicator; the Home screen benchmarks your speaking speed against typing and average speech as an actual argument, not a vanity stat; copy throughout ("That is 3 novels," the CUDA/memory-specific `StartupError` text) is written for this product's actual failure modes, not generic SaaS boilerplate.

The asterisk: the Flow Bar — the one surface you named as top priority — falls short of its *own document's* ambition, and the code says so explicitly. `DESIGN.md` promises the overlay a shadow-and-blur exception ("it floats over content this design system does not control... a soft two-layer shadow and a backdrop blur"). The shipped `overlay.css` has neither, with a comment explaining a real Windows constraint (webview transparency makes `backdrop-filter` sample the window's own backdrop, not the desktop) — a legitimate engineering reason, but it means the one visual indulgence promised for this surface doesn't exist. Same pattern with the waveform: `DESIGN.md` describes "VU ballistics — 60ms attack, 380ms release, 800ms peak hold... that asymmetry is what makes it read as an instrument." The shipped code uses one symmetric interpolation constant with no attack/release split and no peak hold. The instrument metaphor is written down, not built.

**Deterministic scan** (Assessment B, exit code 2, 2 findings — both the same rule, `layout-transition`, both in `apps/ui/src/windows/hub.css`): animating `width` on `.scale-fill` (line 198) and `.first-run-fill` (line 288). Both sit next to code comments explaining the timing was deliberately tuned to counter visible stutter from ~5Hz discrete value updates — a legitimate perf-pattern flag landing on code with real deliberate intent behind it, so likely a justified false positive rather than an oversight. Not something I'd act on without a measured perf complaint first.

**Visual overlays**: not available — browser injection could not run this session (see the degradation note above), so there is no in-page overlay evidence to point you to.

## Overall Impression

The Hub is a real, disciplined design system executed with unusual consistency for a project this size — the token ladder, the "color lives in a dot, never a fill" rule, the plain-language copy are all followed in practice, not just documented. The single biggest opportunity is that the Flow Bar — the surface you use dozens of times a day and the one you asked me to focus on — currently has no way to tell you a dictation failed, no signature moment when it succeeds, and no actual glow, even though your own design doc promises one. The main window is polish; the overlay is where the real gap is.

## What's Working

1. **The token system is real, not aspirational.** Every screen reuses the same `Card`/`Row`/`Badge`/`Stat`/`Notice` primitives with zero rogue colors — the "tone lives in a dot, never a fill" rule is followed with discipline across the whole app, which is harder to sustain in practice than to write down.
2. **The copy is specific and confident.** The CUDA/memory-specific error text, "That is 3 novels," the full empty-state walkthrough — none of it reads as generic SaaS filler.
3. **The Home screen's speaking-speed comparison** (typing vs. you vs. average speech) turns a bare metric into an actual argument for the product, and it was built, not just proposed in the research doc.

## Priority Issues

**[P0] The Flow Bar shows nothing when a dictation fails — it looks identical to idle.**
Why it matters: your own second product principle is "Never lose a word... falls back to clipboard plus history" — worthless if you never learn it happened. `Overlay.tsx` only branches on `listening`/`transcribing`/`injecting`; the `fault` state and the `notice` field (which already carries "Copied to clipboard — press Ctrl+V") are never read there — only `Hub.tsx` shows it, and the Hub isn't open mid-task by design. Mid-dictation in your editor, a failure currently looks exactly like nothing happened.
Fix: read `view.notice` and `state === "fault"` inside `Overlay.tsx`; add a distinct visual state (mic dot to `--danger`, brief inline message reusing the existing width tiers).
Suggested command: `/impeccable harden`

**[P1] The release/completion moment has no signature feedback — no sound, no glow, no confirmation.**
Why it matters: this is the exact moment you asked me to scrutinize. No audio assets exist anywhere in the repo; the pill just shrinks back to idle. Wispr Flow and Superwhisper both treat this moment deliberately (a glow ring; an optional completion tone). Given the overlay is "seen for two seconds at a time, in peripheral vision" by your own PRODUCT.md, an ending with zero glanceable signal wastes the one interaction the whole product exists to deliver well.
Fix: one authored micro-moment on `injecting → idle` — mic dot flashes `--live-hi` then fades over ~200-300ms, paired with an optional quiet completion sound (opt-out in Settings).
Suggested command: `/impeccable delight`

**[P1] The Flow Bar has no glow at all, and its only live-state color change is nearly imperceptible.**
Why it matters: `DESIGN.md` documents a shadow-and-blur exception for exactly this window; the shipped CSS correctly drops both because Windows webview transparency makes backdrop blur unreliable — a legitimate reason, but nothing replaced it. The only remaining live-state signal is a 1px border going from neutral grey to an extremely dark desaturated green (`--live-dim`, #1d5a13) — at 1px, on a near-black pill, this will not register in peripheral vision, which is the entire job this surface has.
Fix: since blur is unreliable on this window type, get the glow another way — a second, slightly larger always-on-top sibling window painted with a soft `--live` radial falloff behind the pill (a real glow layer, not a CSS shadow), or at minimum a 2px full-chroma `--live` ring instead of `--live-dim`.
Suggested command: `/impeccable overdrive`

**[P2] No way to cancel a dictation once started.**
Why it matters: push-to-talk means release always transcribes and injects; there's no discard path. Wispr Flow ships explicit Cancel/Stop controls. An accidental trigger has to be transcribed, injected, and manually undone.
Fix: an Escape-key listener (or a dismiss affordance visible only in the `live` state) that discards the in-progress utterance.
Suggested command: `/impeccable harden`

**[P3] DESIGN.md has drifted from the shipped code and will mislead the next person who trusts it.**
Why it matters: the `--faint` value, the shadow/blur promise, and the VU-ballistics description all disagree with what's actually shipped in three separate places.
Fix: regenerate the relevant DESIGN.md sections from the real implementation.
Suggested command: `/impeccable document`

## Persona Red Flags

**Alex (Power User)**: the activation shortcut is a static, non-editable `<Kbd>Right Ctrl</Kbd>` — no remap control anywhere, despite this being the single most-used binding in the product. No tab-switch accelerators across the 6 nav items. No cancel-while-recording path.

**Sam (Accessibility)**: genuine credit — the overlay has a correctly-scoped `sr-only role="status" aria-live="polite"` region, and focus-visible styling is consistent app-wide. Risk: the 7×7px mic dot is the primary at-a-glance state carrier, distinguishing green from grey by hue alone at a size a color-vision-deficient user may not resolve at a glance. The fault-state gap (P0) is also an accessibility failure in its own right — no announced state exists for it either.

**Mid-Task Glancer** (custom — the actual primary persona: a developer glancing at a peripheral overlay for under a second without breaking focus): the bar's most important glance — "did that work?" — currently has no correct answer available, since success and silent-clipboard-fallback look identical, and successful completion itself produces no distinguishing signal either. The right-click menu (built specifically for this persona: "hide for an hour," "paste last transcript") is discoverable only via a hover tooltip a glancer will never trigger.

## Minor Observations

- Sidebar nav sits at 6 items, one above the ≤4-5 comfort guideline — not severe since all are text-labeled and Advanced is a deliberate escape hatch, but worth watching before a 7th surface is added.
- Dictionary's "Remove" deletes a correction instantly with no undo/toast.
- History's "Copy" button gives no success confirmation.
- The Hub sidebar's brand-mark waveform is fully flat/idle by default — a first-time viewer sees static grey dashes before it ever moves, which could read as a broken icon.
- The detector's two `layout-transition` hits (animating `width` in `hub.css`) sit next to comments explaining deliberate tuning against visible stutter — likely a justified false positive, not worth acting on without a measured perf complaint.
- `tokens.css` defines `--live-ring` (input focus glow) which isn't documented in DESIGN.md's color ladder — folds into the P3 documentation-drift finding.

## Questions to Consider

1. If the Flow Bar's whole reason to exist is to be trusted at a glance, what happens the first time a word silently lands in the wrong window and the bar says nothing? Does "Never lose a word" survive contact with the one surface actually on screen when it matters?
2. DESIGN.md commits the Flow Bar to a shadow-and-blur exception the app can't technically deliver on Windows. Should the doc describe a Windows-safe equivalent instead of an effect that can't exist in the shipped window?
3. Wispr Flow, Superwhisper, and your own DESIGN.md all converge on treating the *end* of a dictation as a moment worth signature treatment. The shipped release moment is currently the quietest point in the whole interaction — what would giving it as much intention as the start already gets actually look like?
