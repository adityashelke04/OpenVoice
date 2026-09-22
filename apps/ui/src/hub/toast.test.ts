import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { dismissToast, pushToast, useToasts } from "./toast";

afterEach(() => vi.useRealTimers());

describe("toast store", () => {
  it("returns increasing ids and keeps push order", () => {
    const h = renderHook(() => useToasts());
    let a = 0, b = 0;
    act(() => { a = pushToast({ tone: "warn", message: "one" }); b = pushToast({ tone: "danger", message: "two" }); });
    expect(b).toBeGreaterThan(a);
    expect(h.result.current.map((t) => t.message)).toEqual(["one", "two"]);
    act(() => { dismissToast(a); dismissToast(b); });
    expect(h.result.current).toHaveLength(0);
  });
  it("dismissing early cancels the timer and leaves the others", () => {
    vi.useFakeTimers();
    const h = renderHook(() => useToasts());
    let a = 0;
    act(() => { a = pushToast({ tone: "info", message: "a" }); });
    act(() => { vi.advanceTimersByTime(2000); pushToast({ tone: "info", message: "b" }); });
    act(() => { dismissToast(a); });
    expect(h.result.current.map((t) => t.message)).toEqual(["b"]);
    act(() => { vi.advanceTimersByTime(3000); });
    expect(h.result.current.map((t) => t.message)).toEqual(["b"]);
    act(() => { vi.advanceTimersByTime(2000); });
    expect(h.result.current).toHaveLength(0);
  });
});
