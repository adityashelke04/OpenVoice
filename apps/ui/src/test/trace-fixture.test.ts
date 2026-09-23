/** The screenshot and twin fixtures must answer `preview_format` the way the
 *  engine does.
 *
 *  `scripts/screenshot-fixtures.mjs` is the far side of `invoke` for every
 *  capture: the README screenshots, and the pixel diff that decides whether the
 *  built Hub matches the reference design. Its trace was copied from the
 *  reference page, which invented five stages named `raw, fillers, dictionary,
 *  commands, capitalize`. `ov-format` runs eight, in a different order, and
 *  starts with `parse`.
 *
 *  Because the mock and the reference told the same story, both checks passed
 *  on a trace three rows shorter than the real one, and the Advanced screen
 *  shipped with its Files and Engine cards pushed off the bottom of the window.
 *  Nothing in the pixel diff could have caught that: it was comparing the
 *  fiction against itself. This does — it reads the stage list out of the Rust
 *  and holds the fixture to it.
 */

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/** The workspace root, four levels up from `apps/ui/src/test`. Built with path
 *  joins rather than `new URL(\`../${p}\`, import.meta.url)`: Vite reads the
 *  second form at transform time and tries to bundle every file the pattern
 *  could reach, which fails on the first one it will not serve. */
const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const repo = (path: string) => readFileSync(join(ROOT, path), "utf8");

/** The stages `format_traced` records, in order: `parse`, then one per rule in
 *  `default_rules()`, each named by its own `Rule::name`. */
export function engineStages(lib: string, rules: string): string[] {
  const body = /fn default_rules\(\) -> Vec<Box<dyn Rule>> \{([\s\S]*?)\n\}/.exec(lib);
  if (!body) throw new Error("no default_rules() in ov-format/src/lib.rs");
  const structs = [...body[1].matchAll(/Box::new\((\w+)\)/g)].map((m) => m[1]);

  const named = new Map(
    [...rules.matchAll(/impl Rule for (\w+) \{\s*fn name\(&self\) -> &'static str \{\s*"([^"]+)"/g)]
      .map((m) => [m[1], m[2]] as const),
  );
  return [
    "parse",
    ...structs.map((s) => {
      const name = named.get(s);
      if (!name) throw new Error(`no Rule::name for ${s}`);
      return name;
    }),
  ];
}

/** The stage names one of the stub's traces hands back, in order. `name` is the
 *  function or the array that holds them, and the match runs to the first line
 *  that closes it at column 0. */
export function fixtureStages(source: string, name: string): string[] {
  const opens = new RegExp(`^(?:function ${name}\\(|const ${name} =)`, "m").exec(source);
  if (!opens) throw new Error(`no \`${name}\` in screenshot-fixtures.mjs`);
  const rest = source.slice(opens.index);
  const body = /^(?:\}|\];)/m.exec(rest.slice(1));
  const stages = [...rest.slice(0, body ? body.index + 1 : rest.length).matchAll(/\[\s*"([^"]+)"/g)]
    .map((m) => m[1]);
  if (stages.length === 0) throw new Error(`no stages inside \`${name}\``);
  return stages;
}

describe("preview_format fixture", () => {
  it("reads the stage list out of the engine", () => {
    const stages = engineStages(repo("crates/ov-format/src/lib.rs"), repo("crates/ov-format/src/rules.rs"));
    expect(stages).toEqual([
      "parse", "repeats", "fillers", "commands", "dictionary", "case", "capitalize", "profile",
    ]);
  });

  it("answers with the stages the engine runs, in the engine's order", () => {
    const engine = engineStages(repo("crates/ov-format/src/lib.rs"), repo("crates/ov-format/src/rules.rs"));
    const fixtures = repo("scripts/screenshot-fixtures.mjs");
    // Both traces the stub serves: the Dictionary/Advanced sentence, and the
    // Writing style sample. A screen that reads only the last line still reads
    // a line the engine never wrote if the stages before it are invented.
    expect(fixtureStages(fixtures, "previewFormat")).toEqual(engine);
    expect(fixtureStages(fixtures, "STYLE_TRACE")).toEqual(engine);
  });
});
