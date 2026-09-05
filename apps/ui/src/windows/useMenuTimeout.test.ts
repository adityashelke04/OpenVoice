import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { act } from "react";

import { MENU_TIMEOUT_MS, useMenuTimeout } from "./useMenuTimeout";

describe("useMenuTimeout", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  // The guarantee this hook exists for. Every other dismissal path runs through
  // Rust -- a mouse hook, a foreground hook, a keyboard hook -- and each of them
  // can fail to install without saying so. This one cannot: it is a timer in the
  // same process as the state it closes.
  it("closes an unattended menu", () => {
    const close = vi.fn();
    renderHook(() => useMenuTimeout(true, false, close));
    expect(close).not.toHaveBeenCalled();
    act(() => void vi.advanceTimersByTime(MENU_TIMEOUT_MS));
    expect(close).toHaveBeenCalledTimes(1);
  });

  // A menu being read is not a menu left behind. Someone comparing two items, or
  // reading a label twice, must not have it taken away mid-sentence.
  it("does not close a menu the pointer is resting on", () => {
    const close = vi.fn();
    renderHook(() => useMenuTimeout(true, true, close));
    act(() => void vi.advanceTimersByTime(MENU_TIMEOUT_MS * 3));
    expect(close).not.toHaveBeenCalled();
  });

  // Hovering pauses; it does not put time back. Same rule as `useIdleCollapse`,
  // and for the same reason: a glance should not buy a full new lease.
  it("resumes the clock where the pointer left it", () => {
    const close = vi.fn();
    const { rerender } = renderHook(
      ({ hover }: { hover: boolean }) => useMenuTimeout(true, hover, close),
      { initialProps: { hover: false } },
    );
    act(() => void vi.advanceTimersByTime(MENU_TIMEOUT_MS - 1000));
    rerender({ hover: true });
    act(() => void vi.advanceTimersByTime(60_000));
    expect(close).not.toHaveBeenCalled();
    rerender({ hover: false });
    act(() => void vi.advanceTimersByTime(999));
    expect(close).not.toHaveBeenCalled();
    act(() => void vi.advanceTimersByTime(1));
    expect(close).toHaveBeenCalledTimes(1);
  });

  // A closed menu has no clock, and re-opening starts a fresh one rather than
  // inheriting the remainder of the last visit.
  it("arms nothing while closed and starts fresh on reopen", () => {
    const close = vi.fn();
    const { rerender } = renderHook(
      ({ open }: { open: boolean }) => useMenuTimeout(open, false, close),
      { initialProps: { open: false } },
    );
    act(() => void vi.advanceTimersByTime(MENU_TIMEOUT_MS * 2));
    expect(close).not.toHaveBeenCalled();

    rerender({ open: true });
    act(() => void vi.advanceTimersByTime(MENU_TIMEOUT_MS - 1));
    expect(close).not.toHaveBeenCalled();
    rerender({ open: false });
    rerender({ open: true });
    act(() => void vi.advanceTimersByTime(MENU_TIMEOUT_MS - 1));
    expect(close).not.toHaveBeenCalled();
    act(() => void vi.advanceTimersByTime(1));
    expect(close).toHaveBeenCalledTimes(1);
  });

  // An inline arrow is the natural way to call this, and a fresh identity on
  // every render must not restart the countdown -- which would make the menu
  // immortal for exactly as long as anything else on the bar was re-rendering.
  it("does not restart the clock when the callback identity changes", () => {
    const close = vi.fn();
    const { rerender } = renderHook(() => useMenuTimeout(true, false, () => close()));
    act(() => void vi.advanceTimersByTime(MENU_TIMEOUT_MS - 1));
    rerender();
    rerender();
    act(() => void vi.advanceTimersByTime(1));
    expect(close).toHaveBeenCalledTimes(1);
  });
});
