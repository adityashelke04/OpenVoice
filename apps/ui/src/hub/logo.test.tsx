import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render } from "@testing-library/react";
import { Logo } from "./Logo";
import { setMedia } from "../test/setup";

const REDUCE = "(prefers-reduced-motion: reduce)";

// A hand-driven rAF: frames run only when the test calls step().
let queue = new Map<number, FrameRequestCallback>();
let nextId = 1;
const step = (t: number) => {
  const due = queue;
  queue = new Map();
  due.forEach((f) => f(t));
};

beforeEach(() => {
  queue = new Map();
  nextId = 1;
  vi.stubGlobal("requestAnimationFrame", (f: FrameRequestCallback) => { const id = nextId++; queue.set(id, f); return id; });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => { queue.delete(id); });
});
afterEach(() => {
  vi.unstubAllGlobals();
  setMedia(REDUCE, false);
});

const transforms = (c: HTMLElement) => [...c.querySelectorAll("rect")].map((r) => r.style.transform);

describe("Logo", () => {
  it("follows the level while listening", () => {
    const level = { current: 1 };
    const { container } = render(<Logo levelRef={level} listening />);
    act(() => step(16));
    expect(transforms(container).every((t) => t.startsWith("scaleY("))).toBe(true);
  });

  it("stays still under prefers-reduced-motion", () => {
    setMedia(REDUCE, true);
    const { container } = render(<Logo levelRef={{ current: 1 }} listening />);
    expect(queue.size).toBe(0);
    act(() => step(16));
    expect(transforms(container).every((t) => t === "")).toBe(true);
  });

  it("stays still under ?still=1", () => {
    const { container } = render(<Logo levelRef={{ current: 1 }} listening still />);
    expect(queue.size).toBe(0);
    expect(transforms(container).every((t) => t === "")).toBe(true);
  });

  it("cancels the loop and clears transforms when listening stops", () => {
    const level = { current: 0.8 };
    const { container, rerender } = render(<Logo levelRef={level} listening />);
    act(() => step(16));
    expect(queue.size).toBe(1);
    rerender(<Logo levelRef={level} listening={false} />);
    expect(queue.size).toBe(0);
    expect(transforms(container).every((t) => t === "")).toBe(true);
  });

  it("cancels the loop and clears transforms on unmount", () => {
    const level = { current: 0.8 };
    const { container, unmount } = render(<Logo levelRef={level} listening />);
    act(() => step(16));
    const rects = [...container.querySelectorAll("rect")];
    unmount();
    expect(queue.size).toBe(0);
    expect(rects.every((r) => r.style.transform === "")).toBe(true);
  });
});
