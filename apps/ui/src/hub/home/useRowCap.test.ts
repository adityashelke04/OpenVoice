import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { capFor, DEFAULT_CAP, MIN_CHILD_PX, useRowCap } from "./useRowCap";

describe("capFor", () => {
  it("never returns fewer children than a box of that height can hold", () => {
    // 400 px of 25 px day labels is 16 children; the cap must cover them plus slack.
    expect(capFor(400)).toBeGreaterThanOrEqual(400 / MIN_CHILD_PX);
    expect(capFor(400)).toBe(Math.ceil(400 / MIN_CHILD_PX) + 2);
  });
  it("is far below the 200 rows the screen used to render", () => {
    expect(capFor(400)).toBeLessThan(40);
  });
  it("falls back to the default for an unmeasured box", () => {
    expect(capFor(0)).toBe(DEFAULT_CAP);
  });
});

describe("useRowCap", () => {
  it("starts at the default when there is no element yet", () => {
    const { result } = renderHook(() => useRowCap({ current: null }));
    expect(result.current).toBe(DEFAULT_CAP);
  });

  it("measures the box it is given", () => {
    const box = document.createElement("div");
    box.getBoundingClientRect = () => ({ height: 400 }) as DOMRect;
    const { result } = renderHook(() => useRowCap({ current: box }));
    expect(result.current).toBe(capFor(400));
  });
});
