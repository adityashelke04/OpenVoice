/**
 * Tests for the pure helpers behind the twin harness. Run with
 * `node --test "scripts/*.test.mjs"` (Node 24 reads a bare directory argument as
 * a file to run, not a folder to search). The harness itself needs Chrome and a dev server;
 * these are the parts that decide pass or fail, so they are pinned here.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import { shotName, clusters, dilate, judge, verdict, crop, matrix } from "./twin-lib.mjs";

test("shot names match render-reference.mjs", () => {
  assert.equal(shotName({ screen: "home", state: "normal", theme: "glacier", mode: "dark", w: 1100, h: 740 }), "home-glacier-dark-1100x740");
  assert.equal(shotName({ screen: "home", state: "failed", theme: "lagoon", mode: "light", w: 1100, h: 740 }), "home-failed-lagoon-light-1100x740");
  assert.equal(shotName({ screen: "home", state: "normal", theme: "graphite", mode: "dark", solid: true, w: 1440, h: 900 }), "home-graphite-dark-solid-1440x900");
  assert.equal(shotName({ screen: "models", theme: "glacier", mode: "light", w: 1100, h: 740 }), "models-glacier-light-1100x740");
});

test("clusters finds 8-connected diff blobs with bounding boxes", () => {
  const w = 40, h = 40, mask = new Uint8Array(w * h);
  for (let y = 2; y < 5; y++) for (let x = 2; x < 5; x++) mask[y * w + x] = 1;   // 3x3
  for (let y = 10; y < 36; y++) for (let x = 10; x < 36; x++) mask[y * w + x] = 1; // 26x26
  const c = clusters(mask, w, h).sort((a, b) => a.w - b.w);
  assert.deepEqual(c.map(({ w, h }) => [w, h]), [[3, 3], [26, 26]]);
});

test("verdict fails over 0.5% or on a cluster larger than 24x24 in both directions", () => {
  assert.equal(verdict({ ratio: 0.004, clusters: [{ w: 300, h: 3 }] }).pass, true);  // a thin text line is AA noise
  assert.equal(verdict({ ratio: 0.006, clusters: [] }).pass, false);
  assert.equal(verdict({ ratio: 0.001, clusters: [{ w: 25, h: 25 }] }).pass, false);
});

test("crop regions", () => {
  assert.deepEqual(crop("sidebar", 1100, 740), { x: 0, y: 0, w: 224, h: 740 });
  assert.deepEqual(crop("top", 1100, 740), { x: 224, y: 0, w: 876, h: 68 });
  assert.deepEqual(crop("full", 1100, 740), { x: 0, y: 0, w: 1100, h: 740 });
});

test("matrix covers the render-reference.mjs set and filters by flag", () => {
  // 5 home states x 6 combos + 6 solid + 6 other screens x 6 combos.
  assert.equal(matrix().length, 30 + 6 + 36);
  assert.equal(matrix({ only: "home", state: "normal", theme: "glacier", mode: "dark" }).length, 2);
  assert.deepEqual(matrix({ only: "models", theme: "glacier", mode: "dark" }), [
    { screen: "models", state: undefined, theme: "glacier", mode: "dark", solid: false },
  ]);
});

/* Real pixels: the error-state Earlier list's last day (label + row), cropped
 * 560x70 from twin shots (home-error glacier dark). The app shows the row; the
 * reference before d129723 did not; the reference after it does. pngjs and
 * pixelmatch are the same packages twin-check resolves from apps/ui. */
const require = createRequire(new URL("../apps/ui/package.json", import.meta.url));
const { PNG } = require("pngjs");
const pixelmatch = (await import(pathToFileURL(require.resolve("pixelmatch")).href)).default;
const fixture = (name) => PNG.sync.read(readFileSync(new URL(`./fixtures/twin-error-row-${name}.png`, import.meta.url)));
const judgePair = (a, b) => judge({ a: a.data, b: b.data, out: Buffer.alloc(a.data.length), w: a.width, h: a.height, pixelmatch });

test("a row missing from the reference fails the twin (real pixels)", () => {
  const res = judgePair(fixture("app"), fixture("ref-before"));
  assert.equal(res.pass, false);
  // The row's text is one dilated cluster, long and dense: a line of text.
  assert.ok(res.big.some((c) => c.w > 300 && c.h <= 14), JSON.stringify(res.big));
});

test("the same row present on both sides passes (real pixels)", () => {
  const res = judgePair(fixture("app"), fixture("ref-after"));
  assert.equal(res.pass, true, JSON.stringify(res.big));
});

/** One cluster through the verdict: does it fail the shot? */
const fails = (c) => !verdict({ ratio: 0.001, clusters: [c] }).pass;

test("verdict fails text present on one side only: a row, a chip label, a short word, a time", () => {
  // Clusters measured from the error twin run against the old reference (the
  // missing "Wed 16 Sep / 5:52 PM Thanks, that fixed it… / Slack" row).
  assert.equal(fails({ w: 313, h: 12, pixels: 977 }), true); // the row's text
  assert.equal(fails({ w: 29, h: 9, pixels: 142 }), true);   // the "Slack" chip label
  assert.equal(fails({ w: 26, h: 8, pixels: 58 }), true);    // "5:52" of the time
  assert.equal(fails({ w: 13, h: 8, pixels: 52 }), true);    // "PM"
  assert.equal(fails({ w: 34, h: 8, pixels: 111 }), true);   // half the day label
  assert.equal(fails({ w: 24, h: 8, pixels: 98 }), true);    // the other half
  assert.equal(fails({ w: 47, h: 12, pixels: 150 }), true);  // a short word ("git status")
  assert.equal(fails({ w: 12, h: 60, pixels: 200 }), true);  // a column of glyphs
});

test("verdict fails a wrong or missing icon: a small solid block", () => {
  assert.equal(fails({ w: 10, h: 11, pixels: 90 }), true);   // the 13px clipboard icon against an 11.5px one
  assert.equal(fails({ w: 12, h: 12, pixels: 100 }), true);
  assert.equal(fails({ w: 8, h: 8, pixels: 32 }), true);     // 64 px area, half filled: the smallest that fails
});

test("verdict passes sparse icon-edge speckle and hairline card edges", () => {
  assert.equal(fails({ w: 10, h: 11, pixels: 46 }), false);  // SVG vs icon-font edges on one glyph
  assert.equal(fails({ w: 8, h: 11, pixels: 34 }), false);
  assert.equal(fails({ w: 7, h: 7, pixels: 40 }), false);    // solid, but under 64 px of area
  assert.equal(fails({ w: 218, h: 1, pixels: 218 }), false); // a card edge re-rasterised
  // The same edge with a speck at each rounded corner, joined by dilation: a
  // 240x6 box, but still about one red pixel per column (home-error glacier dark).
  assert.equal(fails({ w: 240, h: 6, pixels: 245 }), false);
  assert.equal(fails({ w: 228, h: 2, pixels: 221 }), false);
});

test("dilate grows each red pixel into a (2r+1) square, clipped at the edges", () => {
  const w = 10, h = 10, mask = new Uint8Array(w * h);
  mask[5 * w + 5] = 1;
  mask[0] = 1;
  const d = dilate(mask, w, h, 2);
  assert.equal(d.reduce((a, b) => a + b, 0), 25 + 9);
  assert.equal(d[3 * w + 3], 1);
  assert.equal(d[2 * w + 2], 1); // the far corner of (0,0)'s clipped square
  assert.equal(d[3 * w + 8], 0);
  assert.equal(mask.reduce((a, b) => a + b, 0), 2); // the input is untouched
});

test("dilated clusters join a broken line of glyphs into one box in original coordinates", () => {
  // Six 5x8 "glyphs" 4 px apart: a line of text present on one side only.
  const w = 80, h = 30, mask = new Uint8Array(w * h);
  for (let g = 0; g < 6; g++) for (let y = 10; y < 18; y++) for (let x = 5 + g * 9; x < 10 + g * 9; x++) mask[y * w + x] = 1;
  assert.equal(clusters(mask, w, h).length, 6);
  const c = clusters(mask, w, h, { dilate: 3 });
  assert.equal(c.length, 1);
  assert.deepEqual(c[0], { x: 5, y: 10, w: 50, h: 8, pixels: 6 * 5 * 8 });
});

test("dilation bridges up to 2r clear pixels and no more", () => {
  const w = 40, h = 10, pair = (gap) => {
    const mask = new Uint8Array(w * h);
    mask[5 * w + 5] = 1;
    mask[5 * w + 6 + gap] = 1;
    return clusters(mask, w, h, { dilate: 3 }).length;
  };
  assert.equal(pair(6), 1); // 6 clear pixels: one cluster at r = 3
  assert.equal(pair(7), 2); // 7: two specks
});
