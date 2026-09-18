/**
 * Tests for the pure helpers behind the twin harness. Run with
 * `node --test "scripts/*.test.mjs"` (Node 24 reads a bare directory argument as
 * a file to run, not a folder to search). The harness itself needs Chrome and a dev server;
 * these are the parts that decide pass or fail, so they are pinned here.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { shotName, clusters, verdict, crop, matrix } from "./twin-lib.mjs";

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
