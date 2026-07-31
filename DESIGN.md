# DESIGN.md — OpenVoice visual system

Durable visual decisions. Product truth lives in `PRODUCT.md`; architecture lives in
`docs/ARCHITECTURE.md`.

## Direction contract

**THESIS.** OpenVoice is an *instrument panel*, not an app. It reports the state of a
running machine to someone whose attention is elsewhere. It refuses the arrangement
this category always ships — near-black canvas, one neon accent, glowing rounded
cards, a centered marketing-grade settings pane — because that arrangement is
decorative where this product must be diagnostic.

**OWN-WORLD.** An aviation annunciator panel. Matte anodized charcoal, milled from a
single ground with hairline seams and no floating cards. Legends are engraved:
uppercase, tracked wide, small, off-white. Status is carried by **illuminated legend
capsules** — a rectangle that is dark when inactive and lit from within when active,
never a dot, never a pill-shaped badge. Three signal colors, each with exactly one
meaning: green = nominal, amber = caution, red = fault. Color is *never* decorative;
an element that is not reporting state is monochrome. Numbers are tabular and
right-aligned, always with units.

**STORY.** The user glances, learns the machine's state in under 200 ms, and looks
away. When they do open the panel deliberately, every value is legible, sourced, and
in the same units they were promised.

**FIRST VIEWPORT.** The overlay: a 280×64 capsule, bottom-center, dark charcoal with
a 1px lit edge in the current state color. Left: a 20-segment vertical-bar level
meter, LED-style, that fills from the bottom and holds a peak marker. Center: the
state legend, engraved caps — `LISTENING` / `TRANSCRIBING` / `INJECTING`. Right:
elapsed time, tabular, monospaced, always `M:SS`. The main window opens on DICTATE
with the annunciator row across the top, live status left, recent utterances below.

**FORM.** Aviation annunciator panel — candidate 5 of 7 on my resonance-ordered list,
assigned by seed `c1d261c1` (`--scope direction --mode operate`, ASSIGNED INDEX 5).
No staging challenger committed; the panel's own topology (fixed legend stations
across a fixed ground) already supplies the structure the rail staging offered.

---

## Color

Strategy: **restrained ground, semantic signal.** Neutrals own the surface; the three
signal colors appear only where they report machine state.

```css
--panel-900: #0e1012;  /* deepest recess — window ground        */
--panel-800: #141719;  /* panel face                            */
--panel-700: #1b1f22;  /* raised section                        */
--panel-600: #24292d;  /* unlit capsule face                    */
--seam:      #2c3237;  /* hairline milled seam, 1px             */
--seam-lit:  #3a4248;  /* seam catching light                   */

--legend:    #c9d1d6;  /* engraved legend, primary              */
--legend-dim:#7d878e;  /* secondary legend                      */
--legend-faint:#525b61;/* inactive / placeholder                */

--signal-green: #4ade80;  /* nominal, listening, delivered      */
--signal-amber: #fbbf24;  /* caution, working, degraded         */
--signal-red:   #f87171;  /* fault                              */
```

Each signal color also has a `-glow` (same hue at 18% alpha, for the lit capsule's
inner bloom) and a `-dim` (30% lightness, for an unlit-but-armed state).

**Rules.**
- A signal color may only appear on an element reporting state. Never on a heading,
  never on a link, never as a brand accent.
- Green is not "success styling", it is *nominal*. A delivered transcript is nominal.
- Amber means the machine is working or degraded, not "warning" in the toast sense.
- Never use hue alone to carry meaning: every lit capsule also carries its legend
  text, so the panel is fully readable in monochrome and to a colorblind user.

**Light theme.** Deferred, and honestly so. An annunciator panel is a dark object; a
light rendition would be a different world, not a variant. The physical scene — a
developer at a laptop, late, in a dim room — makes dark correct rather than default.
Recorded here so the omission is a decision, not an oversight.

## Type

System stacks only. The app is local-first with a strict CSP and no network access;
a webfont CDN would violate the product's central promise for the sake of a face.

```css
--font-legend: "Segoe UI", system-ui, -apple-system, sans-serif;
--font-data:   "Cascadia Mono", "Consolas", ui-monospace, monospace;
```

The engraved look comes from *treatment*, not from a display face:

| Role | Spec |
|---|---|
| Legend (capsule, section, tab) | 11px / 600 / `text-transform: uppercase` / `letter-spacing: .14em` |
| Legend, small | 10px / 600 / uppercase / `.16em` |
| Body | 13px / 400 / `.005em` |
| Data readout | 12px `--font-data` / `font-variant-numeric: tabular-nums` |
| Large readout | 20px `--font-data` / tabular |

Never set body copy in uppercase. Legends are uppercase because they are engraved
labels; sentences are not.

## Material

- **One ground, milled.** Sections are separated by 1px `--seam` hairlines, not by
  floating cards with shadows and rounded corners. Radius is `2px` everywhere — the
  radius of a milled edge, not of a web card. `0px` on capsules.
- **No drop shadows.** Depth comes from a 1px top highlight (`--seam-lit`) and a 1px
  bottom shadow on raised sections — a bevel, not a float.
- **Lit capsules** get an inset bloom: `inset 0 0 12px <signal>-glow`, plus a 1px
  border in the signal color and legend text in that color. Unlit capsules are
  `--panel-600` with `--legend-faint` text. The legend is *always* rendered, lit or
  not, exactly as on a real panel where you can read the label of a dark lamp.
- **Grid.** 4px base. Panel gutters 16px, section padding 12px 16px.

## Motion

The panel's native motion is **lamp behavior**: a filament comes up fast and decays
slower. Nothing slides, nothing bounces, nothing springs.

- Capsule illumination: 90 ms in, 220 ms out, `ease-out`.
- Level meter: 60 ms attack, 380 ms release, with a peak marker that holds 800 ms
  then falls. This is a real VU ballistic, not a linear tween — it is what makes the
  meter feel like an instrument rather than a progress bar.
- Overlay appear/dismiss: 120 ms opacity + 4px rise. No scale.
- `prefers-reduced-motion`: capsules switch instantly, the meter still tracks level
  (it is data, not decoration), the peak hold stops animating.

## Components

**Capsule** — the atom. `<Capsule legend state>` where state ∈ `off | green | amber |
red`. Fixed height 22px, horizontal padding 10px, legend centered.

**Meter** — 20 discrete segments, bottom-up fill, colored by zone (green below −12 dB,
amber to −3, red above). Discrete segments, never a continuous gradient: an LED
bargraph reads faster in peripheral vision.

**Readout** — right-aligned tabular value plus a dim unit suffix. Always shows units.

**Station** — a labeled row in a section: engraved legend left, control right,
hairline seam beneath. The panel's only list primitive.

## Accessibility

- Legend text is 11px but at 600 weight against a ≥12:1 ground; verified, not assumed.
- Every lit state carries text, never color alone.
- Focus is a 1px `--signal-green` outline at 2px offset — visible against every panel
  tone. Never removed.
- The overlay is `aria-live="polite"` and announces state changes, so a screen-reader
  user gets the same information the lamp gives a sighted one.

## Prohibitions

These ban devices the world does not use — checked against the world's own materials,
not added to silence a linter.

- No drop shadows, no floating cards, no radius above 2px.
- No gradients except the capsule's inner bloom.
- No signal color on anything not reporting machine state.
- No emoji, no icon-only controls without a legend.
- No webfonts, no external requests of any kind.
