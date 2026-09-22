import { expect, it } from "vitest";
import { markChanges } from "./diff";
const changed = (s: ReturnType<typeof markChanges>) => s.filter((x) => x.changed).map((x) => x.text);
const joined = (s: ReturnType<typeof markChanges>) => s.map((x) => x.text).join("");

it("dictionary try-out: ignores case and the final full stop", () => {
  const before = "um so we need to call use effect here comma then return null";
  const after = "So we need to call useEffect here, then return null.";
  const s = markChanges(before, after, { ignoreCase: true, ignoreFinalPeriod: true });
  expect(changed(s)).toEqual(["useEffect", ","]);
  expect(joined(s)).toBe(after);
});
it("advanced: case change marks only the changed letters", () => {
  const s = markChanges("so we need to call useEffect here, then return null", "So we need to call useEffect here, then return null");
  expect(changed(s)).toEqual(["S"]);
});
it("advanced: spoken comma becomes a marked comma", () => {
  expect(changed(markChanges("so we need to call useEffect here comma then return null", "so we need to call useEffect here, then return null"))).toEqual([","]);
});
it("pure deletions mark nothing", () => expect(changed(markChanges("um so we", "so we"))).toEqual([]));
it("identical text is one unchanged segment", () => expect(markChanges("a b.", "a b.")).toEqual([{ text: "a b.", changed: false }]));
it("keeps internal punctuation inside words", () => expect(changed(markChanges("node js at ten thirty", "Node.js at 10:30", { ignoreCase: true }))).toEqual(["Node.js", "10:30"]));
