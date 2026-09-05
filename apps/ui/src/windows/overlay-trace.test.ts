/** The viewport contract check, which is the alarm for the bug it names.
 *
 * Tested for its *edge* behaviour rather than its message. An alarm that fires
 * on every frame while a condition holds is one nobody reads, and this condition
 * holds for as long as the app is running — the incident that prompted it lasted
 * from a monitor waking until the user gave up and restarted. Two lines, an onset
 * and a recovery, is the whole design.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { noteViewportContract, viewportNow } from "./overlay-trace";

/** The window the Flow Bar is created at. `OVERLAY_W`/`OVERLAY_H` in Overlay.tsx. */
const W = 404;
const H = 640;

/** Pretend the layout viewport is this size. */
function layoutAt(w: number, h: number) {
  vi.spyOn(document.documentElement, "clientWidth", "get").mockReturnValue(w);
  vi.spyOn(document.documentElement, "clientHeight", "get").mockReturnValue(h);
}

describe("viewportNow", () => {
  afterEach(() => vi.restoreAllMocks());

  it("reads the layout viewport, which is what the pill is centred in", () => {
    layoutAt(505, 800);
    expect(viewportNow()).toEqual({ w: 505, h: 800 });
  });
});

describe("noteViewportContract", () => {
  let err: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    err = vi.spyOn(console, "error").mockImplementation(() => {});
    // Module state, so every test starts from agreement.
    noteViewportContract({ w: W, h: H }, W, H);
    err.mockClear();
  });

  afterEach(() => vi.restoreAllMocks());

  it("says nothing while the webview lays out in its own window", () => {
    for (let i = 0; i < 5; i += 1) noteViewportContract({ w: W, h: H }, W, H);
    expect(err).not.toHaveBeenCalled();
  });

  /** The reported failure: WebView2 dropped to devicePixelRatio 1 on a 125%
   *  panel, so a 404x640 window laid out as 505x800 CSS pixels. */
  it("reports the desync that made the bar vanish", () => {
    noteViewportContract({ w: 505, h: 800 }, W, H);
    expect(err).toHaveBeenCalledTimes(1);
    expect(String(err.mock.calls[0][0])).toContain("505x800");
    expect(String(err.mock.calls[0][0])).toContain("404x640");
  });

  /** The property that makes it readable. The condition persisted for minutes
   *  and would have written a line per resize, per poll, forever. */
  it("reports the onset once, not once per check", () => {
    noteViewportContract({ w: 505, h: 800 }, W, H);
    for (let i = 0; i < 20; i += 1) noteViewportContract({ w: 505, h: 800 }, W, H);
    expect(err).toHaveBeenCalledTimes(1);
  });

  it("arms again once the webview comes back, so a second episode is seen", () => {
    noteViewportContract({ w: 505, h: 800 }, W, H);
    noteViewportContract({ w: W, h: H }, W, H);
    noteViewportContract({ w: 505, h: 800 }, W, H);
    expect(err).toHaveBeenCalledTimes(2);
  });

  /** A viewport that is wrong in one axis is still wrong. The webview can be
   *  resized without its scale changing, and the region is derived from both. */
  it("does not need both axes to disagree", () => {
    noteViewportContract({ w: W, h: 800 }, W, H);
    expect(err).toHaveBeenCalledTimes(1);
  });
});
