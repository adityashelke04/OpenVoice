/** How tall the Flow Menu actually is, measured from the menu itself.
 *
 * # Why this is measured and not computed
 *
 * The window's region is clipped to the shape the frontend asks for, and the
 * overlay window is transparent *only where the DOM paints* — that is the
 * contract stated at the top of `overlay.css`, and it exists because webview
 * transparency on Windows is unreliable. A region larger than the paint is not a
 * see-through margin. It is the webview's own background, and it shows as a white
 * rectangle hanging over whatever the user was working in.
 *
 * So the region and the paint have to agree exactly, and there are only two ways
 * to arrange that: derive the paint from the number, or derive the number from
 * the paint. CSS owns the paint — padding, row height, separator margins, border,
 * and the font metrics that decide how tall a button rounds to. A TypeScript
 * function adding those up is a second copy of a rule CSS already owns, and the
 * two drifted by 6px, which is the bug this replaces.
 *
 * Measuring is not merely more accurate; it is the only version that cannot
 * silently stop being accurate.
 *
 * # Why the height is zero until it is known
 *
 * A first render has no menu to measure. Answering with a guess would ask for a
 * region before knowing whether the paint will fill it — the exact mistake this
 * module exists to prevent — so it answers `0`, which the caller turns into "just
 * the pill". The region then grows on the next frame, once there is something
 * real to measure. Growing late is invisible: the menu's own 140ms entrance
 * animation is still running, and Rust applies growth immediately. Shrinking late
 * is what shows white, and this never does that.
 */

import { useCallback, useLayoutEffect, useRef, useState } from "react";

/**
 * Measure an element's height and keep it current.
 *
 * Returns a callback ref to put on the menu, and the height in CSS pixels — `0`
 * while there is nothing mounted to measure.
 *
 * `useLayoutEffect` rather than `useEffect` so the measurement happens before the
 * browser paints, and a `ResizeObserver` rather than a one-shot read because the
 * menu's height changes without React re-rendering it: a font finishing loading
 * re-flows every button in it, and the row for "Start dictating" becomes "Stop
 * dictating" mid-session.
 */
export function useMenuHeight(): [(el: HTMLElement | null) => void, number] {
  const [height, setHeight] = useState(0);
  const el = useRef<HTMLElement | null>(null);
  const observer = useRef<ResizeObserver | null>(null);

  // Rounded up. A fractional height at a fractional device pixel ratio rounds
  // down somewhere, and half a pixel of unpainted region is still unpainted.
  // `region_box` in overlay.rs rounds outward for the same reason.
  const measure = useCallback(() => {
    const node = el.current;
    setHeight(node ? Math.ceil(node.getBoundingClientRect().height) : 0);
  }, []);

  const ref = useCallback(
    (node: HTMLElement | null) => {
      observer.current?.disconnect();
      observer.current = null;
      el.current = node;
      if (!node) {
        // Unmounted. Report nothing rather than the last height: the caller uses
        // this to size a region, and a region sized for a menu that is no longer
        // on screen is precisely the white rectangle.
        setHeight(0);
        return;
      }
      measure();
      if (typeof ResizeObserver !== "undefined") {
        observer.current = new ResizeObserver(measure);
        observer.current.observe(node);
      }
    },
    [measure],
  );

  // Disconnect on unmount of the *host*, not just of the menu. The callback ref
  // above handles the menu closing; this handles the window going away underneath
  // an open one.
  useLayoutEffect(() => () => observer.current?.disconnect(), []);

  return [ref, height];
}
