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

/** 8-connected components of a 0/1 mask, as bounding boxes. Iterative (an
 *  explicit stack), because a full-screen diff is one component of ~800k pixels
 *  and recursion would overflow long before that. */
export function clusters(mask, w, h) {
  const seen = new Uint8Array(w * h), out = [], stack = [];
  for (let i = 0; i < mask.length; i++) {
    if (!mask[i] || seen[i]) continue;
    let minX = w, minY = h, maxX = 0, maxY = 0, n = 0;
    stack.push(i); seen[i] = 1;
    while (stack.length) {
      const p = stack.pop(), x = p % w, y = (p / w) | 0; n++;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
      for (let dy = -1; dy <= 1; dy++) for (let dx = -1; dx <= 1; dx++) {
        const nx = x + dx, ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= w || ny >= h) continue;
        const q = ny * w + nx;
        if (mask[q] && !seen[q]) { seen[q] = 1; stack.push(q); }
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
export function verdict({ ratio, clusters }) {
  const big = clusters.filter((c) => c.w > MAX_CLUSTER && c.h > MAX_CLUSTER);
  return { pass: ratio <= MAX_RATIO && big.length === 0, big };
}

/** Regions for `--region`: the 224 px sidebar and the 68 px top bar are shared
 *  by every screen, so they get checked on their own while the shell is built. */
export function crop(region, W, H) {
  if (region === "sidebar") return { x: 0, y: 0, w: 224, h: H };
  if (region === "top") return { x: 224, y: 0, w: W - 224, h: 68 };
  return { x: 0, y: 0, w: W, h: H };
}
