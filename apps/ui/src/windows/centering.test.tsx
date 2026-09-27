/** The content-centring invariant has to see the Aurora bar's content.
 *
 * It measures from a list of class names. When those names are the old bar's,
 * it finds nothing inside the new one, returns `null`, and the alarm is silently
 * disarmed — which is the one way an invariant can fail without anyone noticing. */
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { FlowBar } from "../ui";
import { checkContentCentering } from "./overlay-trace";

/** jsdom has no layout: give each element a horizontal box by class. */
function lay(root: HTMLElement, boxes: Record<string, [number, number]>) {
  for (const [sel, [l, r]] of Object.entries(boxes)) {
    root.querySelectorAll<HTMLElement>(sel).forEach((el) => {
      el.getBoundingClientRect = () =>
        ({ left: l, right: r, width: r - l, top: 0, bottom: 20, height: 20, x: l, y: 0 }) as DOMRect;
    });
  }
}

describe("checkContentCentering", () => {
  it("finds the resting mark", () => {
    const { container } = render(<FlowBar live={false} elapsed="0:00" />);
    lay(container, { ".flowbar": [0, 88], ".flowbar-mark": [24, 63] });
    const r = checkContentCentering(container as HTMLElement);
    expect(r).not.toBeNull();
    expect(r!.leftGap).toBe(24);
    expect(r!.rightGap).toBe(25);
  });

  it("finds a message and its glyph", () => {
    const { container } = render(<FlowBar live={false} failed message="No text" elapsed="0:00" />);
    lay(container, { ".flowbar": [0, 200], ".flowbar-icon": [13, 29], ".flowbar-msg": [38, 187] });
    const r = checkContentCentering(container as HTMLElement);
    expect(r?.leftGap).toBe(13);
    expect(r?.rightGap).toBe(13);
  });

  it("finds the writing thread", () => {
    const { container } = render(<FlowBar live={false} working elapsed="0:00" />);
    lay(container, { ".flowbar": [0, 156], ".flowbar-thread-wrap": [13, 143] });
    expect(checkContentCentering(container as HTMLElement)?.centered).toBe(true);
  });
});
