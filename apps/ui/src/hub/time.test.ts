import { describe, expect, it } from "vitest";
import { clockTime, dayLabel, daysBetween, relativeTime } from "./time";
const NOW = new Date(2026, 8, 18, 14, 43).getTime();
const at = (d: number, h: number, m: number) => new Date(2026, 8, 18 - d, h, m).getTime();

it("clockTime is 12-hour with a plain space", () => {
  expect(clockTime(at(0, 14, 39))).toBe("2:39 PM");
  expect(clockTime(at(0, 0, 5))).toBe("12:05 AM");
  expect(clockTime(at(0, 12, 0))).toBe("12:00 PM");
  expect(clockTime(at(0, 9, 44))).toBe("9:44 AM");
});
describe("dayLabel", () => {
  it("today, yesterday, then weekday", () => {
    expect(dayLabel(at(0, 0, 1), NOW)).toBe("Today");
    expect(dayLabel(at(1, 23, 59), NOW)).toBe("Yesterday");
    expect(dayLabel(at(2, 17, 52), NOW)).toBe("Wed 16 Sep");
  });
  it("midnight boundary", () => {
    const justAfterMidnight = new Date(2026, 8, 18, 0, 1).getTime();
    expect(dayLabel(new Date(2026, 8, 17, 23, 59).getTime(), justAfterMidnight)).toBe("Yesterday");
    expect(daysBetween(new Date(2026, 8, 17, 23, 59).getTime(), justAfterMidnight)).toBe(1);
  });
});
it("relativeTime", () => {
  expect(relativeTime(NOW - 20_000, NOW)).toBe("just now");
  expect(relativeTime(NOW - 2 * 60_000, NOW)).toBe("2 min ago");
  expect(relativeTime(NOW - 59 * 60_000, NOW)).toBe("59 min ago");
  expect(relativeTime(NOW - 3 * 3_600_000, NOW)).toBe("3 h ago");
  expect(relativeTime(at(1, 10, 30), NOW)).toBe("Yesterday, 10:30 AM");
  expect(relativeTime(at(2, 16, 30), NOW)).toBe("Wed 16 Sep, 4:30 PM");
  expect(relativeTime(NOW + 5_000, NOW)).toBe("just now"); // clock skew
});
