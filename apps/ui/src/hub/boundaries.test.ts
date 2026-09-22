/** One rule, checked on the sources themselves: no Hub file reaches into `src/ui`.
 *
 * The legacy kit still ships (the Flow Bar and the design sheet use it) and its
 * class names collide with the Hub's own: `.btn`, `.badge`, `.card`, `.model`.
 * Importing any of it from a Hub file pulls `ui.css` back into the Hub bundle
 * and restyles screens nobody touched, silently. Grepping the sources is the
 * only check that catches that before the pixel diff does.
 *
 * The check resolves each import specifier rather than matching its text,
 * because the same text means different things at different depths: the Hub's
 * own kit is `src/hub/ui`, so `hub/home/Actions.tsx` says `../ui` and means the
 * right one, while `screens/Advanced.tsx` said `../ui` and meant the legacy one.
 * A pattern over the raw text cannot tell those apart, and making the sources
 * dodge one would leave `hub/home` spelling its imports oddly for no reason a
 * reader could see. Resolving is also strictly stricter: it catches `./../ui`,
 * `../../ui`, a deep `./ui` from inside `src/`, and any future spelling that
 * happens to land on the legacy kit. Please do not "simplify" it back.
 */
import { describe, expect, it } from "vitest";
import { posix } from "node:path";

/** Where the two kits live, as absolute posix paths in the glob's own space. */
const ROOT = "/src";
const LEGACY = `${ROOT}/ui`;
/** This file's folder, which is what `import.meta.glob` keys are relative to. */
const HERE = `${ROOT}/hub`;

/** Every static, side-effect and dynamic import specifier in a source file.
 *  `from "x"` covers imports, type imports and re-exports; `import "x"` the
 *  CSS side-effects; `import("x")` the lazy ones. */
const SPECIFIER = /(?:\bfrom\s*|\bimport\s*\(?\s*)["']([^"']+)["']/g;

/** The specifiers in `src` that resolve into the legacy kit, given the file they
 *  are written in. Bare specifiers (packages) never can, so they are skipped. */
export function legacyImports(file: string, src: string): string[] {
  const dir = posix.dirname(posix.resolve(HERE, file));
  return [...src.matchAll(SPECIFIER)]
    .map((m) => m[1])
    .filter((spec) => spec.startsWith("."))
    .filter((spec) => {
      const target = posix.resolve(dir, spec);
      return target === LEGACY || target.startsWith(`${LEGACY}/`);
    });
}

const files = import.meta.glob(["../hub/**/*.{ts,tsx}", "../screens/*.tsx", "../windows/Hub.tsx"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;

describe("Hub boundaries", () => {
  // Without this, a resolver that quietly matched nothing would leave the scan
  // below green forever, which is the one way this file could lie.
  it("tells the two kits apart by where the import is written", () => {
    const at = (file: string, spec: string) => legacyImports(file, `import { Button } from "${spec}";`);
    // The legacy kit, reached from each depth that can reach it.
    expect(at("../screens/Advanced.tsx", "../ui")).toEqual(["../ui"]);
    expect(at("../screens/Advanced.tsx", "./../ui")).toEqual(["./../ui"]);
    expect(at("../screens/Advanced.tsx", "../ui/index")).toEqual(["../ui/index"]);
    expect(at("../hub/home/Row.tsx", "../../ui")).toEqual(["../../ui"]);
    expect(at("../windows/Hub.tsx", "../ui")).toEqual(["../ui"]);
    // The Hub's own kit, spelled every way the sources spell it.
    expect(at("../hub/home/Row.tsx", "../ui")).toEqual([]);
    expect(at("../hub/Sidebar.tsx", "./ui")).toEqual([]);
    expect(at("../screens/Advanced.tsx", "../hub/ui")).toEqual([]);
    // Packages, and the app's other folders.
    expect(at("../hub/Sidebar.tsx", "motion/react")).toEqual([]);
    expect(at("../screens/Advanced.tsx", "../engine/settings")).toEqual([]);
  });

  it("the Hub never imports the legacy primitives", () => {
    for (const [path, src] of Object.entries(files)) expect(legacyImports(path, src), path).toEqual([]);
  });
});
