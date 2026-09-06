/** The faces the Flow Bar measures itself with, and when they become measurable.
 *
 * This is the alarm-free half of the "bar is cropped on a cold launch" bug. The
 * pill's width is a canvas `measureText` of the shortcut, and a canvas measures
 * with whatever face is *loaded at that instant* — so the idle pill, which is the
 * first thing in the window to use the mono face, measured it before it existed
 * and got the fallback's metrics. 168px instead of 173. Rust clipped the window
 * to 168 and the pill then painted at 173, and the difference came off the
 * rounded ends.
 *
 * `document.fonts.ready` is not the answer on its own and that is the point of
 * the first test: it resolves when nothing is *pending*, and a face nothing has
 * drawn yet is not pending — it is simply absent. The faces have to be asked for.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

import { useFontsReady } from "./useFontsReady";

type Listener = () => void;

/** A `document.fonts` stand-in: `load` resolves only when the test says so. */
function fakeFontFaceSet() {
  const pending: Array<() => void> = [];
  const listeners = new Set<Listener>();
  const asked: string[] = [];
  return {
    asked,
    settle() {
      const all = pending.splice(0);
      all.forEach((r) => r());
      listeners.forEach((l) => l());
    },
    fonts: {
      status: "loaded" as const,
      ready: Promise.resolve(),
      check: () => false,
      load: (font: string) => {
        asked.push(font);
        return new Promise<unknown[]>((resolve) => pending.push(() => resolve([])));
      },
      addEventListener: (_: string, l: Listener) => listeners.add(l),
      removeEventListener: (_: string, l: Listener) => listeners.delete(l),
    },
  };
}

function install(f: ReturnType<typeof fakeFontFaceSet>) {
  Object.defineProperty(document, "fonts", { value: f.fonts, configurable: true });
}

describe("useFontsReady", () => {
  afterEach(() => vi.restoreAllMocks());

  it("asks for the faces the bar measures with rather than waiting on `ready`", () => {
    const f = fakeFontFaceSet();
    install(f);
    renderHook(() => useFontsReady());
    // Both the mono face the shortcut cap is set in and the sans face a message
    // is set in, because both tiers size themselves from their own text.
    expect(f.asked.some((s) => s.includes("11px"))).toBe(true);
    expect(f.asked.some((s) => s.includes("12px"))).toBe(true);
  });

  it("changes once the faces arrive, so the width is measured again", async () => {
    const f = fakeFontFaceSet();
    install(f);
    const { result } = renderHook(() => useFontsReady());
    const before = result.current;
    await act(async () => {
      f.settle();
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current).not.toBe(before));
  });
});
