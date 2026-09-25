/** The boxes the window is clipped to, one per thing the bar can say.
 *
 * `geometry()` is the single sizing authority: Rust clips the window's region to
 * what it returns and the pill paints inside it, so a wrong number here is a bar
 * with its ends sliced off or a dead zone punched into the app underneath. */
import { describe, expect, it } from "vitest";
import { geometry } from "./geometry";

const base = { mini: false, edge: "bottom" as const, menu: false };

describe("geometry", () => {
  // The shortcut is no longer written on the bar, so no shortcut, however long,
  // can widen or clip it: `geometry` does not take one.
  it("rests as a small capsule around the mark", () => {
    expect(geometry({ ...base, mode: "idle" })).toEqual({ w: 88, h: 40 });
  });

  it("widens for the wave and settles for the thread", () => {
    expect(geometry({ ...base, mode: "live" })).toEqual({ w: 240, h: 40 });
    expect(geometry({ ...base, mode: "working" })).toEqual({ w: 156, h: 40 });
  });

  it("compact holds the mark, and nine bars while listening", () => {
    expect(geometry({ ...base, mini: true, mode: "idle" })).toEqual({ w: 48, h: 22 });
    expect(geometry({ ...base, mini: true, mode: "working" })).toEqual({ w: 48, h: 22 });
    expect(geometry({ ...base, mini: true, mode: "live" })).toEqual({ w: 78, h: 22 });
  });

  it("docked, the column is tall enough for the mark stood on end", () => {
    expect(geometry({ ...base, edge: "left", mode: "idle" })).toEqual({ w: 34, h: 60 });
    expect(geometry({ ...base, edge: "left", mini: true, mode: "idle" })).toEqual({ w: 22, h: 36 });
  });

  it("clamps a long message at 380 rather than growing past the window", () => {
    expect(geometry({ ...base, mode: "failed", text: "x".repeat(400) }).w).toBe(380);
  });

  it("never lets a short message make the bar narrower than its resting box", () => {
    expect(geometry({ ...base, mode: "notice", text: "Discarded" }).w).toBeGreaterThanOrEqual(88);
  });
});
