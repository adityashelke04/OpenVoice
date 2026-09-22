import { describe, expect, it } from "vitest";
import { fitChildren } from "./useFit";

/** A list box whose children sit at the given [top, bottom] spans. */
function box(bottom: number, kids: [cls: string, top: number, bottom: number, open?: boolean][]) {
  const el = document.createElement("div");
  el.getBoundingClientRect = () => ({ top: 0, bottom } as DOMRect);
  for (const [cls, t, b, open] of kids) {
    const k = document.createElement("div");
    k.className = cls;
    if (open) { const t = document.createElement("button"); t.setAttribute("aria-expanded", "true"); k.appendChild(t); }
    k.getBoundingClientRect = () => ({ top: t, bottom: b } as DOMRect);
    el.appendChild(k);
  }
  return el;
}
const hidden = (el: HTMLElement) => [...el.children].map((k) => (k as HTMLElement).hidden);

describe("fitChildren", () => {
  it("hides rows that cross the bottom and a day label left with no row", () => {
    const el = box(300, [["day", 0, 25], ["row", 25, 73], ["row", 73, 121], ["day", 121, 146], ["row", 146, 194], ["day", 194, 219], ["row", 219, 267], ["day", 267, 292], ["row", 292, 340]]);
    fitChildren(el);
    expect(hidden(el)).toEqual([false, false, false, false, false, false, false, true, true]);
  });
  it("un-hides everything first, so a taller box gets its rows back", () => {
    const el = box(100, [["day", 0, 25], ["row", 25, 73], ["row", 73, 121]]);
    fitChildren(el);
    expect(hidden(el)).toEqual([false, false, true]);
    el.getBoundingClientRect = () => ({ top: 0, bottom: 200 } as DOMRect);
    fitChildren(el);
    expect(hidden(el)).toEqual([false, false, false]);
  });
  it("hides nothing while a row is open: the list scrolls instead", () => {
    const el = box(100, [["day", 0, 25], ["row", 25, 73], ["row", 73, 200, true], ["day", 200, 225], ["row", 225, 273]]);
    let top = 55;
    Object.defineProperty(el, "scrollTop", { configurable: true, get: () => top, set: (v: number) => { top = v; } });
    fitChildren(el);
    expect(hidden(el)).toEqual([false, false, false, false, false]);
    expect(top).toBe(55); // the open row's scroll position is kept
  });
  it("returns to the top when it fits again", () => {
    const el = box(100, [["day", 0, 25], ["row", 25, 73]]);
    let top = 55;
    Object.defineProperty(el, "scrollTop", { configurable: true, get: () => top, set: (v: number) => { top = v; } });
    fitChildren(el);
    expect(top).toBe(0);
  });
  it("measures every row once, before hiding any of them", () => {
    // Reading a rect after each write made the browser re-run layout per row:
    // 114.9 ms against 4.33 ms over 208 children, measured in the real window,
    // and a resize drag runs this on every frame. Pin the order.
    const el = box(100, [["row", 0, 48], ["row", 48, 96], ["row", 96, 144], ["row", 144, 192]]);
    const order: string[] = [];
    for (const k of [...el.children] as HTMLElement[]) {
      const rect = k.getBoundingClientRect.bind(k);
      k.getBoundingClientRect = () => { order.push("read"); return rect(); };
      let h = false;
      Object.defineProperty(k, "hidden", {
        configurable: true,
        get: () => h,
        set: (v: boolean) => { if (v) order.push("write"); h = v; },
      });
    }
    fitChildren(el);
    expect(order).toContain("write");
    expect(order.indexOf("write")).toBeGreaterThan(order.lastIndexOf("read"));
  });
});
