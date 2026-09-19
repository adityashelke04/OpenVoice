/**
 * Pure helpers for the twin harness (`twin-check.mjs`).
 *
 * Kept free of Chrome, the file system and npm packages so the parts that decide
 * whether a shot passes can be tested on their own (`twin-lib.test.mjs`). A
 * harness whose verdict nobody has checked is worse than no harness: it turns a
 * wrong screen green.
 */

export const THEMES = ["glacier", "graphite", "lagoon"];
export const MODES = ["dark", "light"];
export const SCREENS = ["home", "history", "dictionary", "style", "models", "settings", "advanced"];
export const HOME_STATES = ["normal", "failed", "first", "loading", "error"];

/** The file name stem `render-reference.mjs` writes, so a twin shot and its
 *  reference PNG can be matched by name alone. "normal" is left out, as there. */
export function shotName({ screen, state, theme, mode, solid, w, h }) {
  return [screen, state && state !== "normal" ? state : null, theme, mode, solid ? "solid" : null, `${w}x${h}`]
    .filter(Boolean).join("-");
}

/** Every shot the reference renders, in render-reference.mjs order, narrowed by
 *  whichever of the command-line filters were given. */
export function matrix({ only, state, theme, mode } = {}) {
  const shots = [];
  for (const screen of SCREENS) {
    const states = screen === "home" ? HOME_STATES : [undefined];
    for (const st of states) for (const t of THEMES) for (const m of MODES)
      for (const solid of screen === "home" && st === "normal" ? [false, true] : [false]) {
        if (only && only !== screen) continue;
        if (state && (st ?? "normal") !== state) continue;
        if (theme && theme !== t) continue;
        if (mode && mode !== m) continue;
        shots.push({ screen, state: st, theme: t, mode: m, solid });
      }
  }
  return shots;
}

/** Square (Chebyshev) dilation of a 0/1 mask by `r` pixels: every set pixel
 *  sets the (2r+1)x(2r+1) square around it, clipped at the edges. Separable
 *  (a row pass, then a column pass), so it costs O(w*h*r), not O(w*h*r*r).
 *  Returns a new mask; the input is left alone. */
export function dilate(mask, w, h, r) {
  if (r <= 0) return mask.slice();
  const rows = new Uint8Array(w * h), out = new Uint8Array(w * h);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    if (!mask[y * w + x]) continue;
    for (let nx = Math.max(0, x - r); nx <= Math.min(w - 1, x + r); nx++) rows[y * w + nx] = 1;
  }
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    if (!rows[y * w + x]) continue;
    for (let ny = Math.max(0, y - r); ny <= Math.min(h - 1, y + r); ny++) out[ny * w + x] = 1;
  }
  return out;
}

/** 8-connected components of a 0/1 mask, as bounding boxes. Iterative (an
 *  explicit stack), because a full-screen diff is one component of ~800k pixels
 *  and recursion would overflow long before that.
 *
 *  With `dilate: r`, components are found on the mask dilated by r, so red
 *  pixels up to 2r apart belong to one cluster. That is what makes a text
 *  line present on one side only (glyphs a few pixels apart) one cluster
 *  instead of a spray of glyph-sized specks that each pass the size rule. The
 *  box and pixel count are still those of the original red pixels, so a
 *  cluster is never reported bigger than what actually differs. */
export function clusters(mask, w, h, { dilate: r = 0 } = {}) {
  const grown = r > 0 ? dilate(mask, w, h, r) : mask;
  const seen = new Uint8Array(w * h), out = [], stack = [];
  for (let i = 0; i < mask.length; i++) {
    if (!mask[i] || seen[i]) continue;
    let minX = w, minY = h, maxX = 0, maxY = 0, n = 0;
    stack.push(i); seen[i] = 1;
    while (stack.length) {
      const p = stack.pop(), x = p % w, y = (p / w) | 0;
      if (mask[p]) {
        n++;
        if (x < minX) minX = x;
        if (x > maxX) maxX = x;
        if (y < minY) minY = y;
        if (y > maxY) maxY = y;
      }
      for (let dy = -1; dy <= 1; dy++) for (let dx = -1; dx <= 1; dx++) {
        const nx = x + dx, ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= w || ny >= h) continue;
        const q = ny * w + nx;
        if (grown[q] && !seen[q]) { seen[q] = 1; stack.push(q); }
      }
    }
    out.push({ x: minX, y: minY, w: maxX - minX + 1, h: maxY - minY + 1, pixels: n });
  }
  return out;
}

/** Spec 10.3. The cluster rule catches what a ratio alone misses: a wrong icon or
 *  a missing chip is a tiny share of 800k pixels but a solid block of diff. A
 *  long thin cluster (one line of text, re-hinted) is anti-aliasing and passes. */
export const MAX_RATIO = 0.005, MAX_CLUSTER = 24;
/** Diff pixels up to 2 * DILATE apart form one cluster (see `clusters`). */
export const DILATE = 3;
/** Spec 10.3 also says a wrong icon, a missing chip or a shifted row fails, and
 *  none of those is 24 px tall. Two shape rules catch them.
 *
 *  Text: longer than MAX_CLUSTER and at least LINE_DENSITY red pixels per pixel
 *  of length. Glyphs stack several red pixels in every column (a missing chip
 *  label is 29x9 with 142 red, about 5 per column; the missing "Thanks, that
 *  fixed it…" line 313x12 with 977, about 3). A re-rasterised card edge is one
 *  red pixel per column, even when dilation joins the specks at its rounded
 *  corners into a taller box.
 *
 *  Icon: no longer than MAX_CLUSTER, at least ICON_AREA px of box, and at least
 *  ICON_FILL of the box red. A different or missing glyph is a solid block;
 *  the same glyph drawn by the icon font on one side and SVG on the other
 *  differs only along its edges, well under half its box.
 *
 *  Re-hinted text is painted yellow by pixelmatch and never counts. */
export const LINE_DENSITY = 2, ICON_AREA = 64, ICON_FILL = 0.5;
export function verdict({ ratio, clusters }) {
  const big = clusters.filter((c) => {
    const length = Math.max(c.w, c.h), area = c.w * c.h, px = c.pixels ?? 0;
    if (c.w > MAX_CLUSTER && c.h > MAX_CLUSTER) return true;
    if (length > MAX_CLUSTER) return px / length >= LINE_DENSITY;
    return area >= ICON_AREA && px / area >= ICON_FILL;
  });
  return { pass: ratio <= MAX_RATIO && big.length === 0, big };
}

/** The whole decision for one pair of same-sized RGBA buffers: pixelmatch
 *  paints `out` (red = differs, yellow = anti-aliasing, which is excluded),
 *  the red pixels become a mask, the mask is clustered with dilation, and the
 *  verdict is read off the clusters. `pixelmatch` is passed in so this file
 *  stays free of npm packages; twin-check and the tests hand it the same one. */
export function judge({ a, b, out, w, h, pixelmatch }) {
  const n = pixelmatch(a, b, out, w, h, {
    threshold: 0.1,
    includeAA: false,
    diffColor: [255, 0, 0],
    aaColor: [255, 255, 0],
  });
  const mask = new Uint8Array(w * h);
  for (let i = 0; i < w * h; i++) {
    const p = i * 4;
    mask[i] = out[p] === 255 && out[p + 1] === 0 && out[p + 2] === 0 ? 1 : 0;
  }
  // Dilated first, so a text line present on one side only is one cluster
  // rather than a row of glyph-sized specks that each pass.
  const blobs = clusters(mask, w, h, { dilate: DILATE }).sort((p, q) => q.pixels - p.pixels);
  const ratio = n / (w * h);
  return { ratio, clusters: blobs, ...verdict({ ratio, clusters: blobs }) };
}

/** Regions for `--region`: the 224 px sidebar and the 68 px top bar are shared
 *  by every screen, so they get checked on their own while the shell is built. */
export function crop(region, W, H) {
  if (region === "sidebar") return { x: 0, y: 0, w: 224, h: H };
  if (region === "top") return { x: 224, y: 0, w: W - 224, h: 68 };
  return { x: 0, y: 0, w: W, h: H };
}
