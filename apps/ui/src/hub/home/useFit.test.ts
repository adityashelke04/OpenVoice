import { describe, expect, it } from "vitest";
import { fitChildren } from "./useFit";

/** A list box whose children sit at the given [top, bottom] spans. */
function box(bottom: number, kids: [cls: string, top: number, bottom: number, open?: boolean][]) {
  const el = document.createElement("div");
  el.getBoundingClientRect = () => ({ top: 0, bottom } as DOMRect);
  for (const [cls, t, b, open] of kids) {
    const k = document.createElement("div");
    k.className = cls;
    if (open) k.setAttribute("aria-expanded", "true");
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
  it("never hides the open row", () => {
    const el = box(100, [["day", 0, 25], ["row", 25, 73], ["row", 73, 200, true]]);
    fitChildren(el);
    expect(hidden(el)).toEqual([false, false, false]);
  });
});
