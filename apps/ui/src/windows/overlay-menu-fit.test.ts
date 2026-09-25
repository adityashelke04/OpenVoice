/** The Flow Menu has to fit in the window.
 *
 * The overlay window is a fixed 404x640 with the pill's top edge at 300, so a
 * menu opening upward has exactly 300px and one opening downward 300px (640 -
 * 300 - the 40px pill). Anything taller is cut off at the window's edge, and
 * the rows that get cut are the first ones — "Start dictating" among them. That
 * happened: the Aurora menu first shipped at 372px, with a header and 30px rows.
 *
 * jsdom has no layout, so the height is computed from the stylesheet's own
 * numbers and the real rows, which is the same sum the browser does. */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { flowMenuRows } from "./useFlowMenu";

// From the package root, which is where Vitest runs: `import.meta.url` is not a
// file URL in every test context here.
const css = readFileSync(resolve(process.cwd(), "src/windows/overlay.css"), "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

/** The first `px` value of `prop` inside the rule for exactly `selector`. */
function px(selector: string, prop: string): number {
  const esc = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const body = new RegExp(`(?:^|\\})\\s*${esc}\\s*\\{([^}]*)\\}`, "m").exec(css)?.[1];
  if (body === undefined) throw new Error(`no rule for ${selector}`);
  const v = new RegExp(`(?:^|;|\\s)${prop}:\\s*([\\d.]+)px`).exec(body)?.[1];
  if (v === undefined) throw new Error(`no ${prop} in ${selector}`);
  return Number(v);
}

const HEADROOM = 300;

describe("the Flow Menu", () => {
  it("fits in the space above or below the pill", () => {
    // Every row the menu can show: an idle bar that is not mid-transcription.
    const rows = flowMenuRows(
      { mini: false, live: false, working: false, autoCollapse: true },
      { call: () => {}, close: () => {}, setMini: () => {}, setAutoCollapse: () => {} },
    );
    const seps = rows.filter((r) => r.sep).length;

    const row = px(".overlay-menu button", "height");
    const pad = px(".overlay-menu", "padding");
    const sepH = px(".overlay-menu-sep", "height");
    const sepM = px(".overlay-menu-sep", "margin");
    const border = 1;

    const total = rows.length * row + seps * (sepH + 2 * sepM) + 2 * pad + 2 * border;
    expect(total).toBeLessThanOrEqual(HEADROOM);
  });

  it("has no header row eating that space", () => {
    expect(css).not.toMatch(/\.overlay-menu-head\b/);
  });
});
