/** The last line of defence against a Flow Menu that will not go away.
 *
 * Split out of `Overlay.tsx` for the same reason `useIdleCollapse` was: it is
 * pure policy — no window, no geometry, no IPC — and a timer buried in the middle
 * of 1,600 lines of window behaviour is a thing nobody can check.
 *
 * # Why this exists when the click-away hook already works
 *
 * Every other route that closes this menu runs through Rust and through Windows:
 * a `WH_MOUSE_LL` hook for clicks elsewhere, a WinEvent hook for another app
 * coming forward, the global keyboard hook for Escape. Each of them can fail to
 * install, and the mouse hook can be silently evicted by Windows if its callback
 * ever runs long. None of those failures announces itself.
 *
 * The bug being fixed here was a menu that stayed open forever, and "forever" is
 * the part that made it intolerable — the bar sat as an opaque 280px panel over
 * somebody's work with no way to dismiss it that they knew about. A timer in the
 * same process as the state it closes cannot fail the way a hook can. It does not
 * make the menu nicer; it makes "forever" impossible.
 */

import { useEffect, useRef } from "react";

/**
 * How long an unattended menu stays up.
 *
 * Chosen against the cost of being wrong in each direction. Too short and it
 * closes under someone still deciding, which is worse than the bug. Too long and
 * the panel outstays its welcome over another application's window. Fifteen
 * seconds is comfortably longer than reading nine short labels and comfortably
 * shorter than "why is this still here".
 */
export const MENU_TIMEOUT_MS = 15_000;

/**
 * Close an open menu that nobody is attending to.
 *
 * Hovering *pauses* the clock rather than resetting it — the same distinction
 * `useIdleCollapse` draws between a hold and a peek, and for the same reason. A
 * glance at the menu should not buy a whole new lease on it.
 *
 * @param open     whether the menu is on screen
 * @param hovering whether the pointer is over the bar or the menu
 * @param close    what to run when the clock runs out
 * @param ms       the budget; defaults to {@link MENU_TIMEOUT_MS}
 */
export function useMenuTimeout(
  open: boolean,
  hovering: boolean,
  close: () => void,
  ms: number = MENU_TIMEOUT_MS,
): void {
  /** Milliseconds still owed before the menu may close. */
  const owed = useRef(ms);
  /** When the running timer started, or 0 when none is running. */
  const startedAt = useRef(0);
  /**
   * The latest `close`, so a caller passing an inline arrow does not restart the
   * clock on every render. The effect below deliberately does not depend on it —
   * if it did, the menu would be immortal for as long as anything else on the bar
   * was re-rendering, which during a dictation is every frame.
   */
  const onClose = useRef(close);
  onClose.current = close;

  // A closed menu has no clock. Resetting here rather than in the timer effect
  // means reopening starts from the full budget instead of inheriting whatever
  // was left of the previous visit.
  useEffect(() => {
    if (!open) {
      owed.current = ms;
      startedAt.current = 0;
    }
  }, [open, ms]);

  useEffect(() => {
    if (!open) return;

    if (hovering) {
      // Bank what is left and stop. Guarded on a running timer because this
      // branch can be re-entered without one, and subtracting against a stale
      // start would spend the same time twice.
      if (startedAt.current !== 0) {
        owed.current = Math.max(0, owed.current - (Date.now() - startedAt.current));
        startedAt.current = 0;
      }
      return;
    }

    startedAt.current = Date.now();
    const id = window.setTimeout(() => {
      owed.current = ms;
      startedAt.current = 0;
      onClose.current();
    }, owed.current);
    return () => window.clearTimeout(id);
  }, [open, hovering, ms]);
}
