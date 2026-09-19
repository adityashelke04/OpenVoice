import { expect, it } from "vitest";
import { dictionaryHits, groupByDay, outcomeInfo, wordCount } from "./history";
import type { Row } from "../engine/stats";
const NOW = new Date(2026, 8, 18, 14, 43).getTime();
const row = (p: Partial<Row>): Row => ({ created_at: NOW, outcome: "delivered", raw_text: "", final_text: "", profile: "prose", target_app: "", audio_ms: 0, latency_ms: 0, ...p });
const DICT = [{ written: "useEffect", spoken: ["use effect", "you seffect"] }, { written: "JSON", spoken: ["jason", "j son"] }, { written: "kubectl", spoken: ["cube control"] }];

it("outcomeInfo", () => {
  expect(outcomeInfo(row({ outcome: "delivered" }))).toBe("delivered");
  expect(outcomeInfo(row({ outcome: "clipboard_fallback" }))).toBe("clipboard");
  expect(outcomeInfo(row({ outcome: "injection_failed" }))).toBe("failed");
});
it("groupByDay keeps order and labels", () => {
  const g = groupByDay([row({ created_at: NOW - 60_000 }), row({ created_at: new Date(2026, 8, 17, 18, 5).getTime() }), row({ created_at: new Date(2026, 8, 16, 9, 44).getTime() })], NOW);
  expect(g.map((d) => [d.label, d.rows.length])).toEqual([["Today", 1], ["Yesterday", 1], ["Wed 16 Sep", 1]]);
  expect(groupByDay([], NOW)).toEqual([]);
});
it("dictionaryHits: whole words, case-insensitive spoken, written present", () => {
  expect(dictionaryHits(row({ raw_text: "um so we need to call use effect here", final_text: "So we need to call useEffect here" }), DICT)).toEqual([{ spoken: "use effect", written: "useEffect" }]);
  expect(dictionaryHits(row({ raw_text: "send the Jason file", final_text: "send the JSON file" }), DICT)).toEqual([{ spoken: "jason", written: "JSON" }]);
  expect(dictionaryHits(row({ raw_text: "the jasonic era", final_text: "the JSON era" }), DICT)).toEqual([]);
  expect(dictionaryHits(row({ raw_text: "use effect", final_text: "use effect" }), DICT)).toEqual([]); // not applied
});
it("wordCount", () => { expect(wordCount("  a b\tc\n")).toBe(3); expect(wordCount("")).toBe(0); });
