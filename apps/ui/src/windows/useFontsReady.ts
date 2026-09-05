/** The faces the Flow Bar measures itself with, and when they can be measured.
 *
 * # The bug this exists for
 *
 * The pill sizes itself from its own text: `geometry()` in `Overlay.tsx` asks a
 * canvas how wide the shortcut is and adds the chrome around it. A canvas
 * measures with whatever face is *loaded at that instant*, and it will not load
 * one for you — `measureText` silently falls back and returns the fallback's
 * metrics.
 *
 * The idle pill is the first thing in this window to use the mono face: the
 * shortcut cap only exists once the engine is ready, which on a cold launch is
 * four seconds in. So on the very first idle render the face had never been
 * asked for, "Right Ctrl" measured 60.5px in the fallback instead of 66.0px in
 * Geist Mono, and the pill was sized at 168 instead of 173. That 168 is what
 * crossed to Rust, and Rust clipped the window to it. Milliseconds later the
 * face arrived, the pill was laid out at its real 173 — and the window was still
 * clipped to 168, so 2.5px came off each end: the rounded caps and the border,
 * sliced flat. Measured on this machine, on every cold launch.
 *
 * The same swap happens to the sans face a moment earlier, which is the 200 ->
 * 204 pair in the log at startup. It is invisible only because both values were
 * sent.
 *
 * # Why `document.fonts.ready` is not the fix
 *
 * `ready` resolves when nothing is *pending*. A face that nothing has drawn yet
 * is not pending — it is absent, and `ready` is already resolved. Waiting on it
 * at startup returns immediately and proves nothing about the face the bar is
 * about to measure with. The faces have to be asked for by name.
 */

import { useEffect, useState } from "react";

/**
 * The `font` shorthands the bar measures with, before family substitution.
 *
 * Exported and consumed by `geometry()` rather than written out at both ends:
 * this list is only correct if it is the same list, and a tier added with a new
 * size or weight has to arrive here too or it reintroduces the bug for itself.
 */
export const MONO_11 = "500 11px $mono";
export const SANS_12 = "400 12px $sans";

/** Every shorthand the bar measures with. */
const MEASURED = [MONO_11, SANS_12];

/**
 * Substitute the family stacks the design tokens name into a `font` shorthand.
 *
 * One implementation, used by the measurement and by the preload, so the face
 * that is loaded is by construction the face that is measured.
 */
export function resolveFont(font: string): string {
  const root = getComputedStyle(document.documentElement);
  return font
    .replace("$mono", root.getPropertyValue("--font-mono").trim() || "monospace")
    .replace("$sans", root.getPropertyValue("--font-sans").trim() || "sans-serif");
}

/**
 * A counter that changes when a face the bar measures with becomes available.
 *
 * Two jobs, and the first one is the actual fix. On mount it *asks* for those
 * faces, so they are loaded long before the idle pill exists and the first
 * measurement that matters is already the right one. The counter is the second
 * job: if a face still arrives late, its arrival has to reach React, because a
 * font loading is not a state change and nothing would otherwise re-render — the
 * pill would be re-laid-out by the browser at its true width while the window
 * stayed clipped to the width the canvas guessed.
 *
 * A counter rather than a boolean: a page can load faces in more than one batch,
 * and each batch can change a measurement.
 */
export function useFontsReady(): number {
  const [generation, setGeneration] = useState(0);

  useEffect(() => {
    const faces = typeof document === "undefined" ? undefined : document.fonts;
    if (!faces) return;
    let live = true;
    const bump = () => {
      if (live) setGeneration((g) => g + 1);
    };
    // `allSettled`, because an unknown family is a rejected promise and a bar
    // that threw here would be a bar that never sized itself.
    void Promise.allSettled(MEASURED.map((f) => faces.load(resolveFont(f)))).then(bump);
    // Belt and braces for faces this list does not name — a family swapped in a
    // stylesheet, or a fallback in the stack winning and then losing.
    faces.addEventListener("loadingdone", bump);
    return () => {
      live = false;
      faces.removeEventListener("loadingdone", bump);
    };
  }, []);

  return generation;
}
