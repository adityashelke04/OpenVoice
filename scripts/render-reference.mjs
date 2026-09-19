/**
 * Render the Hub reference design (docs/redesign/reference/reference.html) to PNGs.
 *
 * These PNGs are the "digital twin" targets: the built Hub, fed the fixtures in
 * scripts/screenshot-fixtures.mjs with the clock frozen, must match them. See
 * docs/superpowers/specs/2026-09-18-hub-redesign-design.md, section 10.
 *
 * Usage:
 *   node scripts/render-reference.mjs                 # full matrix at 1100x740
 *   node scripts/render-reference.mjs --size 1440x900
 *   node scripts/render-reference.mjs --only home     # one screen
 *
 * Output: docs/redesign/reference/png/<screen>[-<state>]-<theme>-<mode>[-solid]-<w>x<h>.png
 * Animations are off (?still=1) so every capture is the settled frame.
 */

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO = dirname(dirname(fileURLToPath(import.meta.url)));
const PAGE = pathToFileURL(join(REPO, "docs", "redesign", "reference", "reference.html")).href;
const OUT = join(REPO, "docs", "redesign", "reference", "png");

const CHROME = [
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
  "/usr/bin/google-chrome",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
].find((p) => existsSync(p));
if (!CHROME) throw new Error("Chrome not found");

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 ? process.argv[i + 1] : fallback;
};
const [w, h] = arg("size", "1100x740").split("x").map(Number);
const only = arg("only", null);

export const THEMES = ["glacier", "graphite", "lagoon"];
export const MODES = ["dark", "light"];
export const SHOTS = [
  { screen: "home", state: "normal" },
  { screen: "home", state: "failed" },
  { screen: "home", state: "first" },
  { screen: "home", state: "loading" },
  { screen: "home", state: "error" },
  { screen: "history" },
  { screen: "dictionary" },
  { screen: "style" },
  { screen: "models" },
  { screen: "settings" },
  { screen: "advanced" },
];

mkdirSync(OUT, { recursive: true });
let n = 0;
for (const shot of SHOTS.filter((s) => !only || s.screen === only)) {
  for (const theme of THEMES) {
    for (const mode of MODES) {
      for (const solid of shot.screen === "home" && shot.state === "normal" ? [false, true] : [false]) {
        const q = new URLSearchParams({ screen: shot.screen, theme, mode, still: "1" });
        if (shot.state) q.set("state", shot.state);
        if (solid) q.set("solid", "1");
        const name = [shot.screen, shot.state && shot.state !== "normal" ? shot.state : null, theme, mode, solid ? "solid" : null, `${w}x${h}`]
          .filter(Boolean)
          .join("-");
        const file = join(OUT, `${name}.png`);
        spawnSync(CHROME, [
          "--headless=new", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=1",
          `--window-size=${w},${h}`, "--virtual-time-budget=5000", `--screenshot=${file}`, `${PAGE}?${q}`,
        ], { stdio: "ignore" });
        n++;
      }
    }
  }
}
console.log(`rendered ${n} reference images to ${OUT}`);
