/** When the Flow Bar is allowed to put itself away, and when it must not.
 *
 * Split out of `Overlay.tsx` because it is the one piece of this feature that is
 * pure policy: no window, no geometry, no IPC. `Overlay.tsx` is already 1,200
 * lines of hard-won window behaviour, and a timer with seven refusal conditions
 * buried in the middle of it is a thing nobody can check.
 *
 * The collapse itself is not implemented here. This decides *whether* the idle
 * clock has run out; `geometry()` decides what shape that means, and Rust clips
 * the window to it. One sizing authority, as ever — this is an input to it.
 */

import { useEffect, useState } from "react";

/**
 * Everything that keeps the bar at full size.
 *
 * Exported and pure so the rules can be read in one place and asserted without
 * a DOM. Each field is a reason the bar has something to say or something to do,
 * and a bar with something to say does not shrink out from under you.
 */
export type CollapseBlockers = {
  /** A session is open. The microphone being live is the whole point of the window. */
  live: boolean;
  /** Transcribing or injecting. The result is still on its way. */
  working: boolean;
  /** The Flow Menu is open, so the bar is being used as a control. */
  menu: boolean;
  /** The pointer is on the bar. */
  hovering: boolean;
  /** Mid-drag, or within reach of a snap line. */
  moving: boolean;
  /**
   * The bar is carrying words: a failure, a notice, the engine being down, or
   * the engine still starting.
   *
   * This is the clause that matters. A message about text that did not land is
   * the single most important thing this window ever displays, and a timer that
   * swallowed it would turn a recoverable mistake into a silent one.
   */
  speaking: boolean;
};

/** Whether any blocker is holding the bar open. */
export function collapseBlocked(v: CollapseBlockers): boolean {
  return v.live || v.working || v.menu || v.hovering || v.moving || v.speaking;
}

/**
 * Whether the idle clock has run out.
 *
 * Returns `false` the instant anything blocks, rather than waiting for the timer
 * to notice — a bar that stayed collapsed for a moment after a failure appeared
 * would be showing an empty line where a sentence belongs.
 *
 * The timer is restarted rather than resumed whenever a blocker clears. Someone
 * who hovers the bar, reads it, and moves away has just told you they were
 * looking at it; giving them the tail of an old countdown would snatch it away.
 *
 * @param blockers what is currently holding it open
 * @param delayMs  how long the quiet has to last
 * @param enabled  the user's standing preference; `false` disables the clock
 *                 entirely and the bar keeps whatever size it had
 */
export function useIdleCollapse(
  blockers: CollapseBlockers,
  delayMs: number,
  enabled: boolean,
  /**
   * Bumped to wake the bar deliberately — a click on the collapsed line.
   *
   * A counter rather than a boolean because waking is an event, not a state: the
   * second click has to restart the countdown just as the first one did, and a
   * boolean that is already `true` cannot say "again".
   */
  wake = 0,
): boolean {
  const [elapsed, setElapsed] = useState(false);
  const blocked = collapseBlocked(blockers);

  useEffect(() => {
    if (!enabled || blocked) {
      // Not a cleanup detail: this is what makes the bar spring back open the
      // moment anything needs it, and it must happen on the same render that
      // saw the blocker rather than one timer tick later.
      setElapsed(false);
      return;
    }
    // A wake starts the count again from zero. Someone who just reached for the
    // bar gets the full delay, not the remainder of one they never saw.
    setElapsed(false);
    const id = window.setTimeout(() => setElapsed(true), delayMs);
    return () => window.clearTimeout(id);
  }, [blocked, delayMs, enabled, wake]);

  return enabled && !blocked && elapsed;
}
