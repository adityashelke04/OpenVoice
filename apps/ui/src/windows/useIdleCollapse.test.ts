/** The idle clock's refusal rules.
 *
 * These exist because the rules are a list, and a list is the shape of thing
 * that quietly loses an entry. The one that matters most is `speaking`: if a
 * failure message can be swallowed by a timer, a dictation that did not land
 * becomes a dictation that silently did not land.
 */

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { collapseBlocked, useIdleCollapse, type CollapseBlockers } from "./useIdleCollapse";

/** Nothing holding the bar open. */
const CLEAR: CollapseBlockers = {
  live: false,
  working: false,
  menu: false,
  hovering: false,
  moving: false,
  speaking: false,
};

const DELAY = 5000;

describe("collapseBlocked", () => {
  it("lets a bar with nothing to say collapse", () => {
    expect(collapseBlocked(CLEAR)).toBe(false);
  });

  // Written as a loop over the keys rather than six separate assertions so that
  // adding a blocker to the type without honouring it here fails immediately.
  it.each(Object.keys(CLEAR) as (keyof CollapseBlockers)[])(
    "holds the bar open while %s",
    (key) => {
      expect(collapseBlocked({ ...CLEAR, [key]: true })).toBe(true);
    },
  );
});

describe("useIdleCollapse", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const setup = (props: {
    blockers?: Partial<CollapseBlockers>;
    delayMs?: number;
    enabled?: boolean;
    wake?: number;
  }) =>
    renderHook(
      ({ blockers, delayMs, enabled, wake }) =>
        useIdleCollapse({ ...CLEAR, ...blockers }, delayMs ?? DELAY, enabled ?? true, wake ?? 0),
      { initialProps: props },
    );

  it("does not collapse before the delay is up", () => {
    const { result } = setup({});
    act(() => void vi.advanceTimersByTime(DELAY - 1));
    expect(result.current).toBe(false);
  });

  it("collapses once the delay has passed", () => {
    const { result } = setup({});
    act(() => void vi.advanceTimersByTime(DELAY));
    expect(result.current).toBe(true);
  });

  /** The whole point of the feature being a preference. */
  it("never collapses when the user has turned it off", () => {
    const { result } = setup({ enabled: false });
    act(() => void vi.advanceTimersByTime(DELAY * 10));
    expect(result.current).toBe(false);
  });

  it("does not start counting while something holds it open", () => {
    const { result } = setup({ blockers: { live: true } });
    act(() => void vi.advanceTimersByTime(DELAY * 3));
    expect(result.current).toBe(false);
  });

  /**
   * The regression that would hurt most.
   *
   * A message about text that did not reach the editor is the single most
   * important thing this window displays. If it arrives while the bar is already
   * collapsed, the bar must come back — not on the next tick, but on the render
   * that saw it.
   */
  it("springs back open the moment it has something to say", () => {
    const { result, rerender } = setup({});
    act(() => void vi.advanceTimersByTime(DELAY));
    expect(result.current).toBe(true);

    rerender({ blockers: { speaking: true } });
    expect(result.current).toBe(false);
  });

  it("comes back for a session and stays back for its whole length", () => {
    const { result, rerender } = setup({});
    act(() => void vi.advanceTimersByTime(DELAY));
    expect(result.current).toBe(true);

    rerender({ blockers: { live: true } });
    act(() => void vi.advanceTimersByTime(DELAY * 5));
    expect(result.current).toBe(false);
  });

  /**
   * A *hold* clearing restarts the count rather than resuming it.
   *
   * A session that just ended, a message that was just dismissed, a menu that
   * was just closed — each leaves the bar with something the user has only this
   * moment finished with, and each earns the full delay before it goes away.
   *
   * Hover is deliberately not in this class; see the peek tests below.
   */
  it("restarts the count from zero after a hold clears", () => {
    const { result, rerender } = setup({ blockers: { live: true } });
    act(() => void vi.advanceTimersByTime(DELAY * 2));

    rerender({ blockers: { live: false } });
    act(() => void vi.advanceTimersByTime(DELAY - 1));
    expect(result.current).toBe(false);

    act(() => void vi.advanceTimersByTime(1));
    expect(result.current).toBe(true);
  });

  /**
   * A peek is not a reset.
   *
   * The bar is already put away; the pointer arrives, it unfurls to be read, and
   * the pointer leaves. It has to fold back on that render — not five seconds
   * later. A glance costing a full delay of full-size bar is the whole reason
   * hover stopped being a blocker like the others.
   */
  it("folds straight back when a peek at a put-away bar ends", () => {
    const { result, rerender } = setup({});
    act(() => void vi.advanceTimersByTime(DELAY));
    expect(result.current).toBe(true);

    rerender({ blockers: { hovering: true } });
    expect(result.current).toBe(false);

    rerender({ blockers: { hovering: false } });
    expect(result.current).toBe(true);
  });

  /** However long the pointer rests on it. Dwelling is still a peek. */
  it("folds straight back after a long peek", () => {
    const { result, rerender } = setup({});
    act(() => void vi.advanceTimersByTime(DELAY));

    rerender({ blockers: { hovering: true } });
    act(() => void vi.advanceTimersByTime(DELAY * 10));
    expect(result.current).toBe(false);

    rerender({ blockers: { hovering: false } });
    expect(result.current).toBe(true);
  });

  /**
   * Hovering an open bar pauses the clock; it neither resets nor advances it.
   *
   * Three seconds in, the pointer arrives and stays for a while. What is owed
   * when it leaves is the two seconds that were left, not a fresh five and not
   * nothing at all.
   */
  it("banks the remaining time while the pointer rests on an open bar", () => {
    const { result, rerender } = setup({});
    act(() => void vi.advanceTimersByTime(3000));

    rerender({ blockers: { hovering: true } });
    act(() => void vi.advanceTimersByTime(30_000));
    expect(result.current).toBe(false);

    rerender({ blockers: { hovering: false } });
    act(() => void vi.advanceTimersByTime(1999));
    expect(result.current).toBe(false);

    act(() => void vi.advanceTimersByTime(1));
    expect(result.current).toBe(true);
  });

  /**
   * A hold that lands during a peek still wins, and still resets.
   *
   * Someone hovering the bar when a failure arrives must get the full delay to
   * read it after they move away — the banked remainder from before the message
   * existed has nothing to do with it.
   */
  it("gives a full delay when a hold arrives during a peek", () => {
    const { result, rerender } = setup({});
    act(() => void vi.advanceTimersByTime(DELAY));

    rerender({ blockers: { hovering: true } });
    rerender({ blockers: { hovering: true, speaking: true } });
    rerender({ blockers: { hovering: false, speaking: true } });
    expect(result.current).toBe(false);

    rerender({ blockers: { hovering: false, speaking: false } });
    act(() => void vi.advanceTimersByTime(DELAY - 1));
    expect(result.current).toBe(false);
    act(() => void vi.advanceTimersByTime(1));
    expect(result.current).toBe(true);
  });

  /**
   * A click is a commitment, where a hover is a glance.
   *
   * Waking the bar by clicking the stroke has to survive the pointer moving off
   * it — otherwise the bar would vanish the instant you stopped touching the
   * thing you just deliberately brought back.
   */
  it("keeps a woken bar open after the pointer leaves", () => {
    const { result, rerender } = setup({});
    act(() => void vi.advanceTimersByTime(DELAY));

    rerender({ blockers: { hovering: true }, wake: 1 });
    rerender({ blockers: { hovering: false }, wake: 1 });
    expect(result.current).toBe(false);

    act(() => void vi.advanceTimersByTime(DELAY - 1));
    expect(result.current).toBe(false);
    act(() => void vi.advanceTimersByTime(1));
    expect(result.current).toBe(true);
  });

  /** A click on the put-away bar brings it back and restarts the clock. */
  it("wakes on demand and then collapses again", () => {
    const { result, rerender } = setup({});
    act(() => void vi.advanceTimersByTime(DELAY));
    expect(result.current).toBe(true);

    rerender({ wake: 1 });
    expect(result.current).toBe(false);

    act(() => void vi.advanceTimersByTime(DELAY));
    expect(result.current).toBe(true);
  });

  /**
   * A second wake has to work exactly as the first did.
   *
   * This is why `wake` is a counter and not a boolean: a flag that is already
   * `true` cannot express "again", so the second click on a re-collapsed bar
   * would do nothing at all.
   */
  it("wakes again on a second click", () => {
    const { result, rerender } = setup({});
    act(() => void vi.advanceTimersByTime(DELAY));

    rerender({ wake: 1 });
    act(() => void vi.advanceTimersByTime(DELAY));
    expect(result.current).toBe(true);

    rerender({ wake: 2 });
    expect(result.current).toBe(false);
  });

  it("honours a changed delay", () => {
    const { result } = setup({ delayMs: 12000 });
    act(() => void vi.advanceTimersByTime(5000));
    expect(result.current).toBe(false);
    act(() => void vi.advanceTimersByTime(7000));
    expect(result.current).toBe(true);
  });
});
