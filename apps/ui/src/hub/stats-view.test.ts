import { expect, it } from "vitest";
import { multiple, savedParts, sentenceCase, speedBar, weekBars } from "./stats-view";
const NOW = new Date(2026, 8, 18, 14, 43).getTime();
const day = (d: number) => new Date(2026, 8, 18 - d, 12).getTime();
it("speedBar", () => { expect(speedBar(152)).toEqual({ fillPct: 76, typingPct: 20 }); expect(speedBar(260).fillPct).toBe(100); });
it("multiple", () => { expect(multiple(152)).toBe("3.8"); expect(multiple(40)).toBe("1.0"); });
it("weekBars: 7 days ending today, oldest first", () => {
  expect(weekBars([0, 1, 2, 3, 4, 5].map(day), NOW)).toEqual([false, true, true, true, true, true, true]);
  expect(weekBars([], NOW)).toEqual(Array(7).fill(false));
});
it("savedParts", () => {
  expect(savedParts(9540 / 40 - 9540 / 152)).toEqual({ big: "2h", small: "56m" });
  expect(savedParts(0.4)).toEqual({ big: "<1", small: "min" });
  expect(savedParts(45)).toEqual({ big: "45", small: "min" });
  expect(savedParts(180)).toEqual({ big: "3", small: "hours" });
  expect(savedParts(119.9)).toEqual({ big: "2", small: "hours" });
  expect(savedParts(60 * 30)).toEqual({ big: "1.3", small: "days" });
});
it("sentenceCase", () => expect(sentenceCase("about a dissertation chapter")).toBe("About a dissertation chapter"));
