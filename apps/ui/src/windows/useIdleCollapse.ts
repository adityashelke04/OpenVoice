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

import { useEffect, useRef, useState } from "react";

/**
 * Everything that keeps the bar at full size.
 *
 * Exported and pure so the rules can be read in one place and asserted without
 * a DOM. Each field is a reason the bar has something to say or something to do,
 * and a bar with something to say does not shrink out from under you.
 *
 * They are not all the same kind of reason, though, and that difference is the
 * whole of this module's behaviour: see `collapseHeld` for the five that own the
 * bar, and `hovering` for the one that is only borrowing it.
 */
export type CollapseBlockers = {
  /** A session is open. The microphone being live is the whole point of the window. */
  live: boolean;
  /** Transcribing or injecting. The result is still on its way. */
  working: boolean;
  /** The Flow Menu is open, so the bar is being used as a control. */
  menu: boolean;
  /**
   * The pointer is on the bar.
   *
   * A *peek*, and deliberately not a hold. It reveals the bar for as long as the
   * pointer is there and pauses the clock while it does, but it never puts time
   * back on it — so a glance at a bar that had already put itself away costs the
   * glance and nothing more. See `useIdleCollapse`.
   */
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

/**
 * Whether the bar is being *held* open — occupied, rather than merely looked at.
 *
 * Every one of these is the bar having something of its own to do, and each of
 * them ending is something the user has only this moment finished with. They
 * earn the full delay again when they clear.
 *
 * `hovering` is the one blocker not in here, because a pointer resting on the
 * bar is not the bar doing anything.
 */
export function collapseHeld(v: CollapseBlockers): boolean {
  return v.live || v.working || v.menu || v.moving || v.speaking;
}

/** Whether any blocker is keeping the bar on screen, a peek included. */
export function collapseBlocked(v: CollapseBlockers): boolean {
  return collapseHeld(v) || v.hovering;
}

/**
 * Whether the idle clock has run out.
 *
 * Returns `false` the instant anything blocks, rather than waiting for the timer
 * to notice — a bar that stayed collapsed for a moment after a failure appeared
 * would be showing an empty line where a sentence belongs.
 *
 * **A hold restarts the clock; a peek only pauses it.** The two are not the same
 * gesture, and they used to be treated as though they were — which is what made
 * the bar so irritating to glance at. Hovering the put-away stroke unfurled it,
 * and moving the pointer away then bought five more seconds of full-size bar for
 * a look that had lasted a fifth of one. Now the pointer arriving banks whatever
 * time was left and the pointer leaving spends it, so a peek at a bar that had
 * already gone away hands it straight back, and a peek at one still on its way
 * out neither hurries it nor reprieves it.
 *
 * The clock still runs from zero after `live`, `working`, `menu`, `moving` or
 * `speaking` clears, because each of those is the bar having been busy with
 * something the user was watching, and the delay afterwards is time to read the
 * result.
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
   *
   * A click is a commitment where a hover is a glance, which is why this buys
   * the full delay and a peek does not. Someone who went to the trouble of
   * hitting an 8px stroke meant to bring the bar back, and it must not vanish
   * again the moment their pointer drifts off it.
   */
  wake = 0,
): boolean {
  const [elapsed, setElapsed] = useState(false);
  const held = collapseHeld(blockers);
  const { hovering } = blockers;

  /**
   * Milliseconds of quiet still owed before the bar may go away.
   *
   * A ref rather than state because nothing renders from it: it is the clock's
   * own bookkeeping, read and written inside the effect below, and holding it in
   * state would re-run that effect on every tick it recorded.
   */
  const owed = useRef(delayMs);
  /** When the running timer was started, or 0 when none is running. */
  const startedAt = useRef(0);

  // A deliberate wake puts the whole delay back. Kept as its own effect, ahead
  // of the timer's, so it has already reset the books by the time the timer
  // reads them — and so that "the user asked for the bar back" is a rule you can
  // read on its own rather than a branch inside the arithmetic.
  useEffect(() => {
    owed.current = delayMs;
    startedAt.current = 0;
    setElapsed(false);
  }, [delayMs, wake]);

  useEffect(() => {
    if (!enabled || held) {
      // Not a cleanup detail: this is what makes the bar spring back open the
      // moment anything needs it, and it must happen on the same render that
      // saw the blocker rather than one timer tick later.
      //
      // The full delay goes back on the clock too. A hold that lands during a
      // peek therefore wins twice over — it reopens the bar now, and it earns
      // the whole delay afterwards, rather than inheriting whatever remainder
      // the peek had banked from before the message existed.
      owed.current = delayMs;
      startedAt.current = 0;
      setElapsed(false);
      return;
    }

    if (hovering) {
      // Pause. Bank what is left of the count and stop it there, leaving
      // `elapsed` exactly as it was — that untouched flag is what lets a bar
      // which had already put itself away fold straight back when the pointer
      // leaves, with no timer in between.
      //
      // Guarded on a running timer because this branch can be re-entered without
      // one — a wake, or a changed delay, while the pointer is still on the bar
      // — and subtracting against a stale start would spend the same time twice.
      if (startedAt.current !== 0) {
        owed.current = Math.max(0, owed.current - (Date.now() - startedAt.current));
        startedAt.current = 0;
      }
      return;
    }

    // Nothing owed: the clock ran out earlier, during a peek or before one. Say
    // so on this render rather than arming a zero-length timer, so the bar folds
    // back on the same frame the pointer leaves it.
    if (owed.current <= 0) {
      setElapsed(true);
      return;
    }

    startedAt.current = Date.now();
    const id = window.setTimeout(() => {
      owed.current = 0;
      startedAt.current = 0;
      setElapsed(true);
    }, owed.current);
    return () => window.clearTimeout(id);
  }, [held, hovering, delayMs, enabled, wake]);

  return enabled && !collapseBlocked(blockers) && elapsed;
}
