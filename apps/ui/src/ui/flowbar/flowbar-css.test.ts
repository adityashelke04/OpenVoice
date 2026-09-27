/** The Flow Bar stylesheet's contract, read as text.
 *
 * These are the promises the design makes that no render in jsdom can see:
 * that the bar owns its look in every Hub theme, that nothing it paints spills
 * outside the pill onto the app underneath, and that "reduce motion" stills all
 * of it. */
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// Read from disk, not through `?raw`: Vitest does not process CSS, and a `?raw`
// import of a stylesheet quietly comes back as an empty string, which every
// assertion below would then pass.
const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");
// Comments stripped: they are prose, and a note above a rule would otherwise be
// read as part of its selector.
const css = read("./flowbar.css").replace(/\/\*[\s\S]*?\*\//g, "");
const legacy = read("../ui.css");

it("is reading real stylesheets", () => {
  expect(legacy.length).toBeGreaterThan(1000);
});

/** Tokens the Hub themes redefine per theme and mode. Reading any of them would
 *  make the bar change with the theme, which is exactly what it must not do. */
const HUB_TOKENS = /var\(--(glass|edge|accent|blob|solid|ink|body|mute|faint|warn|danger|live|surface|hairline)\b/;

/** Every `{ ... }` body whose selector is the bar itself, in any state. */
function barRules(): string[] {
  return [...css.matchAll(/(?:^|[}\s,])\.flowbar((?:\[[^\]]+\])*)\s*\{([^}]*)\}/g)].map((m) => m[2]);
}

describe("flowbar.css", () => {
  it("owns its look: reads no Hub theme token", () => {
    expect(css).not.toMatch(HUB_TOKENS);
  });

  it("paints no outer glow: every shadow on the bar is inset", () => {
    const bodies = barRules();
    expect(bodies.length).toBeGreaterThan(3);
    for (const body of bodies) {
      const shadow = /box-shadow:\s*([^;]+);/.exec(body)?.[1];
      if (!shadow) continue;
      for (const layer of shadow.split(/,(?![^(]*\))/)) {
        expect(layer.trim()).toMatch(/^(inset\b|none$)/);
      }
    }
  });

  it("stops every animation it declares when motion is reduced", () => {
    const names = [...css.matchAll(/@keyframes\s+([\w-]+)/g)].map((m) => m[1]);
    expect(names.length).toBeGreaterThan(3);
    const at = css.indexOf("prefers-reduced-motion");
    expect(at).toBeGreaterThan(-1);
    // From inside the media block, so its own `{` is not read as a rule.
    const reduced = css.slice(css.indexOf("{", at) + 1);
    // The selectors of rules in that block that actually say `animation: none`.
    // A selector merely appearing in the block is not enough: it might be there
    // for some other declaration and still animate.
    const stilled = new Set(
      [...reduced.matchAll(/([^{}]+)\{([^}]*)\}/g)]
        .filter(([, , body]) => /animation:\s*none/.test(body))
        .flatMap(([, sel]) => sel.split(",").map((x) => x.trim())),
    );
    expect(stilled.size).toBeGreaterThan(0);
    const before = css.slice(0, at);
    for (const n of names) {
      const users = [...before.matchAll(new RegExp(`([^{}]+)\\{[^}]*animation[^;]*\\b${n}\\b`, "g"))];
      expect(users.length, `@keyframes ${n} is never used`).toBeGreaterThan(0);
      for (const [, sel] of users) {
        for (const s of sel.split(",").map((x) => x.trim()).filter(Boolean)) {
          expect(stilled.has(s), `"${s}" still animates ${n} under reduced motion`).toBe(true);
        }
      }
    }
  });

  it("draws the approved spectrum", () => {
    for (const hex of ["#44d62c", "#52e3b0", "#5ad7f0", "#8aa8ff"]) {
      expect(css.toLowerCase()).toContain(hex);
    }
  });

  it("stays out of the Hub, like the rest of the overlay's primitives", () => {
    expect(css).toContain(':where(html:not([data-window="hub"]))');
  });

  it("never wraps a message onto a second line", () => {
    const msg = /\.flowbar-msg\s*\{([^}]*)\}/.exec(css)?.[1] ?? "";
    expect(msg).toMatch(/white-space:\s*nowrap/);
    expect(msg).toMatch(/text-overflow:\s*ellipsis/);
    expect(msg).toMatch(/min-width:\s*0/);
  });
});

describe("ui.css", () => {
  it("no longer styles the Flow Bar", () => {
    expect(legacy).not.toMatch(/\.flowbar/);
    expect(legacy).not.toMatch(/@keyframes flowbar-/);
  });
});
