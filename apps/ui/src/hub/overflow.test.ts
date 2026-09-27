/** The Hub's screens must be reachable at any window size.
 *
 * `section.scroll` is the box every screen renders into, and for most of the
 * redesign it was `overflow: hidden` — a container named `scroll` that could
 * not. The reference page is a still at one size, so nothing in the pixel diff
 * or the screenshots ever asked what happens when a screen is taller than the
 * window: Settings lost its Privacy card below the fold and Advanced lost the
 * Files and Engine cards, with no scrollbar and no way to reach either.
 *
 * jsdom does not lay out, so it cannot be asked to scroll something. What it
 * can do is read the rule, which is where the bug was. Two properties matter
 * and both are asserted: the box scrolls when it overflows, and the window
 * itself still does not (the shell is a fixed frame; a scrolling `body` would
 * drag the sidebar and the top bar out of view with it).
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/** Read a stylesheet as text. `import.meta.glob` cannot: Vite hands a `.css`
 *  module to its own pipeline before `?raw` is honoured, and the entry comes
 *  back undefined. */
const css = (file: string) =>
  readFileSync(fileURLToPath(new URL(file, import.meta.url)), "utf8");

/** The declarations of the rule written with exactly this selector.
 *
 *  Exact, not "contains": `.scroll` and `.rows.scrolling` would both match a
 *  substring search, and the answer has to be about the one rule that decides
 *  whether a screen can be reached. */
export function declarations(source: string, selector: string): string {
  for (const match of source.matchAll(/(^|\n)\s*([^{}\n]+?)\s*\{([^}]*)\}/g)) {
    if (match[2].trim() === selector) return match[3].replace(/\s+/g, " ").trim();
  }
  throw new Error(`no rule for \`${selector}\``);
}

describe("Hub overflow", () => {
  it("reads the rule it claims to read", () => {
    const source = ".a { color: red; }\n.a .b { color: blue; }\n";
    expect(declarations(source, ".a")).toBe("color: red;");
    expect(declarations(source, ".a .b")).toBe("color: blue;");
    expect(() => declarations(source, ".c")).toThrow("no rule for `.c`");
  });

  it("scrolls a screen that is taller than the window", () => {
    const scroll = declarations(css("./ui/controls.css"), ".scroll");
    expect(scroll).toContain("overflow-y: auto");
    expect(scroll).not.toContain("overflow: hidden");
    // Without this the scrollbar's appearance changes the content width, and
    // every card shifts sideways the moment a screen grows past the window.
    expect(scroll).toContain("scrollbar-gutter: stable");
  });

  it("keeps the window itself fixed, so the sidebar cannot scroll away", () => {
    expect(declarations(css("./shell.css"), 'html[data-window="hub"] body')).toContain("overflow: hidden");
  });
});
