import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useFlowConfirmation } from "./useFlowConfirmation";

afterEach(() => vi.useRealTimers());
describe("completion feedback", () => {
  it("expires even when a new recording interrupts the confirmation", () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ state, outcome, session }) => useFlowConfirmation(state, outcome, session), {
      initialProps: { state: "transcribing", outcome: null as string | null, session: 1 },
    });
    rerender({ state: "idle", outcome: "delivered", session: 1 });
    expect(result.current).toBe(true);
    act(() => vi.advanceTimersByTime(120));
    rerender({ state: "listening", outcome: null, session: 2 });
    expect(result.current).toBe(false);
    act(() => vi.advanceTimersByTime(500));
    expect(result.current).toBe(false);
    rerender({ state: "idle", outcome: "delivered", session: 2 });
    expect(result.current).toBe(true);
    act(() => vi.advanceTimersByTime(480));
    expect(result.current).toBe(false);
  });
  it("does not congratulate historical, discarded or clipboard results", () => {
    const { result, rerender } = renderHook(({ outcome }) => useFlowConfirmation("idle", outcome, 4), {
      initialProps: { outcome: "delivered" },
    });
    expect(result.current).toBe(false);
    rerender({ outcome: "cancelled" });
    expect(result.current).toBe(false);
    rerender({ outcome: "clipboard_fallback" });
    expect(result.current).toBe(false);
  });
});

it("waits for idle when delivery and state updates render separately", () => {
  vi.useFakeTimers();
  const { result, rerender } = renderHook(({ state, outcome }) => useFlowConfirmation(state, outcome, 9), {
    initialProps: { state: "injecting", outcome: null as string | null },
  });
  rerender({ state: "injecting", outcome: "delivered" });
  expect(result.current).toBe(false);
  rerender({ state: "idle", outcome: "delivered" });
  expect(result.current).toBe(true);
  act(() => vi.advanceTimersByTime(480));
  expect(result.current).toBe(false);
});
