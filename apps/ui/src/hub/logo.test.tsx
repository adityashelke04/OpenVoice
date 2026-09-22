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

/** The scaleY each bar is currently at, or null where nothing has been written. */
const scales = (c: HTMLElement) =>
  transforms(c).map((t) => {
    const m = /scaleY\(([\d.]+)\)/.exec(t);
    return m ? Number(m[1]) : null;
  });

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
  /* The mark is fed a ~25 Hz level stream and drawn at 60-165 Hz. Writing the
     latest sample straight to `scaleY` left the bars frozen on three quarters of
     the frames and then jumping by up to 0.65 of their travel in one of them,
     which is what "the logo is laggy" actually looked like. The loop has to
     interpolate on every frame, not on every sample. */
  it("keeps moving on frames where no new level has arrived", () => {
    const level = { current: 0.6 };
    const { container } = render(<Logo levelRef={level} listening />);
    act(() => step(0));
    const seen: (number | null)[] = [];
    for (let t = 16; t <= 16 * 8; t += 16) {
      act(() => step(t));
      seen.push(scales(container)[0]);
    }
    for (let i = 1; i < seen.length; i++) expect(seen[i]).not.toBe(seen[i - 1]);
  });

  it("snaps up and eases down", () => {
    const level = { current: 1 };
    const { container } = render(<Logo levelRef={level} listening />);
    let t = 0;
    const run = (ms: number) => {
      for (let i = 0; i < ms / 16; i++) {
        t += 16;
        act(() => step(t));
      }
    };
    act(() => step(t));
    run(400);
    const high = scales(container)[0]!;
    expect(high).toBeGreaterThan(0.9);

    level.current = 0;
    run(160);
    const after = scales(container)[0]!;
    expect(after).toBeLessThan(high);
    expect(high - after).toBeLessThan(0.5);
  });

  it("never leaves a bar outside its drawn range", () => {
    const level = { current: 0 };
    const { container } = render(<Logo levelRef={level} listening />);
    let t = 0;
    for (let i = 0; i < 60; i++) {
      level.current = i % 2 ? 1 : 0;
      t += 16;
      act(() => step(t));
      for (const v of scales(container)) {
        expect(v).toBeGreaterThanOrEqual(0.35);
        expect(v).toBeLessThanOrEqual(1);
      }
    }
  });
});
