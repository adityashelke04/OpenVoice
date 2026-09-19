/** A live `matchMedia` answer, and the Hub's "hold still" switches.
 *
 *  Motion that CSS drives is gated in CSS; motion that script writes directly
 *  (the Logo's bars) has to ask these itself, since neither the media query nor
 *  MotionConfig reaches a hand-written `style.transform`. */
import { useCallback, useSyncExternalStore } from "react";

function matches(query: string): boolean {
  try { return window.matchMedia(query).matches; } catch { return false; }
}

export function useMedia(query: string): boolean {
  const subscribe = useCallback((f: () => void) => {
    let mq: MediaQueryList;
    try { mq = window.matchMedia(query); } catch { return () => {}; }
    mq.addEventListener("change", f);
    return () => mq.removeEventListener("change", f);
  }, [query]);
  return useSyncExternalStore(subscribe, () => matches(query), () => false);
}

export const REDUCED_MOTION = "(prefers-reduced-motion: reduce)";
/** Below this width the sidebar is a 64 px icon rail (shell.css). */
export const ICON_RAIL = "(max-width: 959px)";

/** `?still=1`: screenshots and the twin; nothing moves at all. */
export function isStill(search: string = typeof location === "undefined" ? "" : location.search): boolean {
  return !!new URLSearchParams(search).get("still");
}
