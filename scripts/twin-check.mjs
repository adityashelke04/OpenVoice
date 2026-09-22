/**
 * The "digital twin" check: render the reference design and the real Hub side by
 * side, pixel-diff them, and fail on anything beyond anti-aliasing.
 *
 * The reference (`docs/redesign/reference/reference.html`) is the visual source
 * of truth for the Hub redesign. Looking at two screenshots and deciding they
 * "match" is how a 2 px row shift or a wrong icon weight ships; this script makes
 * the decision mechanical (spec section 10).
 *
 * Usage:
 *   npm run dev:ui                                   # in one terminal (port 5199)
 *   npm run twin -- [--size 1100x740] [--only <screen>] [--state <state>]
 *                   [--theme t] [--mode m] [--region full|sidebar|top] [--axe]
 *                   [--no-setup]
 *   npm run twin -- --compare a.png b.png [--region r]   # plain two-image diff
 *
 * --no-setup skips the hover and typing steps, and so their setup errors. It is
 * for region runs (--region sidebar|top) while the screens those steps need are
 * not built yet; the sidebar and top bar do not depend on them. The "app never
 * rendered" check still applies, and a full-page run refuses the flag: a full
 * shot without its hover or typing would prove nothing.
 *
 * Output: docs/redesign/twin/<name>-{ref,app,diff}.png and twin-report.json
 * (twin-report-<region>.json for a region run; shots run with --no-setup are
 * marked `noSetup: true`), one
 * line per shot on stdout, exit 1 if any shot failed. <name> is the file stem
 * `render-reference.mjs` uses, plus `-<region>` when a region is cropped.
 *
 * Both pages are rendered by the same headless Chrome at DPR 1, so font
 * rasterisation is identical on both sides and only real differences remain:
 *
 * - Reference: Google Fonts is blocked and the same @fontsource files the app
 *   bundles are installed under the family names the reference asks for. Two
 *   builds of "the same" font differ in hinting, which is enough to fail a
 *   pixel diff on every line of text. Phosphor still comes from its CDN, as the
 *   reference intends; the app draws the same glyphs from @phosphor-icons/react.
 * - App: the Tauri bridge is the fixture stub (`screenshot-fixtures.mjs`) with
 *   the clock frozen at the reference's instant, prefs written to localStorage
 *   before load, and `prefers-reduced-motion: reduce` so nothing is mid-motion.
 */

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { basename, dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { launchChrome, openPage, sleep } from "./cdp.mjs";
import { tauriStub } from "./screenshot-fixtures.mjs";
import { crop, judge, matrix, shotName } from "./twin-lib.mjs";

const REPO = dirname(dirname(fileURLToPath(import.meta.url)));
const OUT = join(REPO, "docs", "redesign", "twin");
// OV_REFERENCE_HTML points the check at another copy of the reference, e.g. an
// older one, to prove the check fails where the two disagree.
const REFERENCE = pathToFileURL(process.env.OV_REFERENCE_HTML ?? join(REPO, "docs", "redesign", "reference", "reference.html")).href;
const BASE = process.env.OV_UI_URL ?? "http://localhost:5199";
const PORT = 9223; // not screenshots.mjs's 9222, so the two can run side by side

// npm packages live in apps/ui; resolve them from there rather than adding a
// root node_modules for a dev script.
const require = createRequire(join(REPO, "apps", "ui", "package.json"));
const { PNG } = require("pngjs");
const pixelmatch = (await import(pathToFileURL(require.resolve("pixelmatch")).href)).default;

/* -- Flags ------------------------------------------------------------------ */

const argv = process.argv.slice(2);
const flag = (name) => argv.includes(`--${name}`);
const arg = (name, fallback) => {
  const i = argv.indexOf(`--${name}`);
  return i > -1 && argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[i + 1] : fallback;
};

/* -- Image helpers ---------------------------------------------------------- */

function cropPng(png, { x, y, w, h }) {
  if (x === 0 && y === 0 && w === png.width && h === png.height) return png;
  const out = new PNG({ width: w, height: h });
  PNG.bitblt(png, out, x, y, w, h, 0, 0);
  return out;
}

/** Diff two same-sized PNGs. Returns the diff image, the mismatch ratio, the
 *  diff clusters (largest first) and the spec 10.3 verdict. Anti-aliased pixels
 *  are painted yellow and excluded; only red pixels count. */
function diffPngs(a, b) {
  if (a.width !== b.width || a.height !== b.height) {
    throw new Error(`size mismatch: ${a.width}x${a.height} vs ${b.width}x${b.height}`);
  }
  const { width: w, height: h } = a;
  const diff = new PNG({ width: w, height: h });
  return { diff, ...judge({ a: a.data, b: b.data, out: diff.data, w, h, pixelmatch }) };
}

const pct = (r) => `${(r * 100).toFixed(2)}%`;
const box = (c) => `${c.w}x${c.h}@${c.x},${c.y}`;

/* -- --compare a.png b.png -------------------------------------------------- */

async function compare(fileA, fileB, region) {
  const a = PNG.sync.read(readFileSync(fileA));
  const b = PNG.sync.read(readFileSync(fileB));
  const r = crop(region, a.width, a.height);
  const res = diffPngs(cropPng(a, r), cropPng(b, r));
  mkdirSync(OUT, { recursive: true });
  const out = join(OUT, `compare-${basename(fileA, ".png")}-vs-${basename(fileB, ".png")}-diff.png`);
  writeFileSync(out, PNG.sync.write(res.diff));
  console.log(`${res.pass ? "PASS" : "FAIL"} ${pct(res.ratio)} ${basename(fileA)} vs ${basename(fileB)}`);
  console.log(`  clusters: ${res.clusters.length}, over 24x24 or a text line: ${res.big.length}`);
  for (const c of res.clusters.slice(0, 10)) console.log(`  ${box(c)} (${c.pixels} px)`);
  console.log(`  diff: ${out}`);
  return res.pass;
}

/* -- Rendering -------------------------------------------------------------- */

/** The @fontsource files the app bundles, as FontFace descriptors for the
 *  family names the reference's Google Fonts link asks for. Latin and latin-ext
 *  cover every character on the page. The unicode ranges are read from the
 *  packages' own CSS so they cannot drift from the files. */
function referenceFonts() {
  const faces = [
    { family: "Geist", css: "@fontsource-variable/geist/index.css", file: "geist-%s-wght-normal.woff2", weight: "100 900" },
    { family: "Geist Mono", css: "@fontsource-variable/geist-mono/index.css", file: "geist-mono-%s-wght-normal.woff2", weight: "100 900" },
    // The opsz files carry both axes the reference uses (opsz 12..96, wght 200..800).
    { family: "Bricolage Grotesque", css: "@fontsource-variable/bricolage-grotesque/opsz.css", file: "bricolage-grotesque-%s-opsz-normal.woff2", weight: "200 800" },
    { family: "Borel", css: "@fontsource/borel/index.css", file: "borel-%s-400-normal.woff2", weight: "400" },
  ];
  const out = [];
  for (const f of faces) {
    const cssPath = require.resolve(f.css);
    const css = readFileSync(cssPath, "utf8");
    for (const subset of ["latin", "latin-ext"]) {
      const file = f.file.replace("%s", subset);
      const block = css.split("@font-face").find((b) => b.includes(`/${file}`));
      const range = block?.match(/unicode-range:\s*([^;]+);/)?.[1].trim();
      if (!range) throw new Error(`no unicode-range for ${file} in ${cssPath}`);
      const data = readFileSync(join(dirname(cssPath), "files", file)).toString("base64");
      out.push({ family: f.family, weight: f.weight, range, data });
    }
  }
  return out;
}

/** Installed before the reference's own scripts. FontFace objects with binary
 *  sources are added straight to `document.fonts`, which needs no DOM yet and no
 *  file:// cross-origin exception (Chrome refuses file:// fonts from a file://
 *  page without a command-line flag). */
function fontScript(fonts) {
  return `(() => {
  const FACES = ${JSON.stringify(fonts)};
  for (const f of FACES) {
    const bin = atob(f.data), bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    const face = new FontFace(f.family, bytes.buffer, { weight: f.weight, style: "normal", unicodeRange: f.range, display: "block" });
    document.fonts.add(face);
  }
})();`;
}

function navigate(page, url) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`load timed out: ${url}`)), 20_000);
    page.once("Page.loadEventFired", () => { clearTimeout(timer); resolve(); });
    page.send("Page.navigate", { url }).then((r) => {
      if (r.errorText) { clearTimeout(timer); reject(new Error(`${url}: ${r.errorText}`)); }
    }, reject);
  });
}

const evaluate = async (page, expression) => {
  const { result, exceptionDetails } = await page.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
  return result.value;
};

async function capture(page) {
  const { data } = await page.send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
  return PNG.sync.read(Buffer.from(data, "base64"));
}

async function renderReference(browser, fonts, shot, w, h) {
  const { page, targetId } = await openPage(browser, PORT);
  try {
    await page.send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: 1, mobile: false });
    await page.send("Network.enable");
    await page.send("Network.setBlockedURLs", { urls: ["*fonts.googleapis.com*", "*fonts.gstatic.com*"] });
    await page.send("Page.addScriptToEvaluateOnNewDocument", { source: fonts });
    const q = new URLSearchParams({ screen: shot.screen, theme: shot.theme, mode: shot.mode, still: "1" });
    if (shot.state) q.set("state", shot.state);
    if (shot.solid) q.set("solid", "1");
    await navigate(page, `${REFERENCE}?${q}`);
    // Phosphor's web bundle injects its own @font-face after it loads, so
    // `document.fonts.ready` alone can resolve before the icon font exists.
    // (`document.fonts.check` is no help: it answers true for a family that has
    // no face at all yet.) Wait for a loaded Phosphor face, then for all fonts.
    const phosphor = await evaluate(page, `(async () => {
      const t0 = performance.now();
      const loaded = () => [...document.fonts].some((f) => /Phosphor/i.test(f.family) && f.status === "loaded");
      while (!loaded() && performance.now() - t0 < 5000) {
        const el = document.querySelector(".ph");
        if (el && el.offsetWidth > 0) [...document.fonts].forEach((f) => /Phosphor/i.test(f.family) && f.load().catch(() => {}));
        await new Promise((r) => setTimeout(r, 100));
      }
      await document.fonts.ready;
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      return loaded();
    })()`);
    if (!phosphor) console.warn(`  warning: Phosphor icons did not load for the reference (network?)`);
    return await capture(page);
  } finally {
    page.close();
    await browser.send("Target.closeTarget", { targetId });
  }
}

/** Hover targets named in spec 9: the reference shows history row 2 (14:39) and
 *  the first dictionary term in their hover state. */
const HOVER_ROW = new Set(["home:normal", "home:failed", "home:error", "history:normal"]);

async function renderApp(browser, shot, w, h, withAxe, noSetup = false) {
  const { page, targetId } = await openPage(browser, PORT);
  try {
    await page.send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: 1, mobile: false });
    await page.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "reduce" }] });
    await page.send("Page.addScriptToEvaluateOnNewDocument", {
      source: tauriStub({ variant: shot.state ?? "normal", prefs: { theme: shot.theme, mode: shot.mode, solid: shot.solid } }),
    });
    await navigate(page, `${BASE}/?window=hub&screen=${shot.screen}`);
    await evaluate(page, "document.fonts.ready.then(() => true)");

    // Setup steps that did not happen fail the shot. A hover that silently
    // missed, or a Dictionary field that was never typed into, produces a
    // capture of a different state than the reference, and a diff of that
    // proves nothing either way.
    const setup = [];

    // Rendered, not merely loaded: the shell's <main> is in the DOM and no
    // skeleton is left (except in the loading state, whose whole point is the
    // skeleton). Then a short settle for layout and the hover transition.
    const loading = shot.state === "loading";
    const ready = await evaluate(page, `(async () => {
      const t0 = performance.now();
      const ok = () => !!document.querySelector("main") && (${loading} || !document.querySelector(".sk, .skeleton"));
      while (!ok() && performance.now() - t0 < 10000) await new Promise((r) => setTimeout(r, 100));
      return ok();
    })()`);
    if (!ready) setup.push(loading ? "app never rendered (no <main>)" : "app never rendered (no <main>, or a skeleton is still showing)");
    await sleep(400);

    let target = null;
    if (!noSetup && HOVER_ROW.has(`${shot.screen}:${shot.state ?? "normal"}`)) target = "row";
    if (!noSetup && shot.screen === "dictionary") {
      // React owns the field's value, so the write goes through the native
      // setter and a bubbling input event (as in screenshots.mjs).
      const typed = await evaluate(page, `(() => {
        const fields = [...document.querySelectorAll("textarea, input")];
        const byLabel = fields.find((el) => {
          const label = el.getAttribute("aria-label") || (el.labels && el.labels[0] && el.labels[0].textContent) || "";
          return /What OpenVoice heard/i.test(label);
        });
        const el = byLabel;
        if (!el) return false;
        const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
        Object.getOwnPropertyDescriptor(proto, "value").set.call(el, "um so we need to call use effect here comma then return null");
        el.dispatchEvent(new Event("input", { bubbles: true }));
        el.blur();
        return true;
      })()`);
      if (!typed) setup.push('Dictionary field "What OpenVoice heard" missing');
      target = "term";
    }
    if (target) {
      const point = await evaluate(page, `(() => {
        let el = null;
        if (${JSON.stringify(target)} === "row") {
          const t = [...document.querySelectorAll("time")].find((x) => x.textContent.trim() === "2:39 PM");
          el = t && (t.closest(".row, li, [role=row], button, a") || t.parentElement);
        } else {
          el = document.querySelector(".term");
        }
        if (!el) return null;
        const r = el.getBoundingClientRect();
        return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
      })()`);
      if (point) await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: point.x, y: point.y });
      else setup.push(target === "row" ? "hover target missing (no row with <time>2:39 PM</time>)" : "hover target missing (no .term)");
      await sleep(150);
    }

    const png = await capture(page);
    let axe = null;
    if (withAxe) {
      await evaluate(page, readFileSync(require.resolve("axe-core/axe.min.js"), "utf8") + ";true");
      axe = await evaluate(page, `axe.run(document, { resultTypes: ["violations"] }).then((r) =>
        r.violations.map((v) => ({ id: v.id, impact: v.impact, help: v.help, nodes: v.nodes.length })))`);
    }
    return { png, axe, setup };
  } finally {
    page.close();
    await browser.send("Target.closeTarget", { targetId });
  }
}

/* -- Main ------------------------------------------------------------------- */

async function main() {
  const region = arg("region", "full");
  if (!["full", "sidebar", "top"].includes(region)) throw new Error(`--region must be full, sidebar or top (got ${region})`);

  if (flag("compare")) {
    const i = argv.indexOf("--compare");
    const [a, b] = [argv[i + 1], argv[i + 2]];
    if (!a || !b) throw new Error("--compare needs two PNG paths");
    return compare(a, b, region);
  }

  const [w, h] = arg("size", "1100x740").split("x").map(Number);
  const shots = matrix({ only: arg("only"), state: arg("state"), theme: arg("theme"), mode: arg("mode") });
  if (shots.length === 0) throw new Error("no shots match those flags");
  const withAxe = flag("axe");
  const noSetup = flag("no-setup");
  if (noSetup && region === "full") throw new Error("--no-setup is only for --region sidebar|top: a full-page shot without its hover or typing proves nothing");
  // One report per region, so a sidebar run and a top run both survive.
  const reportFile = region === "full" ? "twin-report.json" : `twin-report-${region}.json`;

  // Fail early and clearly. Without this every capture is a screenshot of
  // Chrome's own connection-error page, diffed against the reference.
  try {
    const res = await fetch(BASE, { signal: AbortSignal.timeout(3000) });
    if (!res.ok) throw new Error(String(res.status));
  } catch (e) {
    throw new Error(`no dev server at ${BASE} (${e.message}). Start it with:\n  npm --prefix apps/ui run dev -- --port 5199 --strictPort`);
  }

  mkdirSync(OUT, { recursive: true });
  const fonts = fontScript(referenceFonts());
  // Grayscale text anti-aliasing on both sides. With LCD (subpixel) AA, the
  // reference's icon-font glyphs on an opaque (solid) surface get colour
  // fringes that the app's SVG icons can never have: the same glyph at the
  // same place then diffs as a solid block (seen: the Last dictation tag
  // icon, 10x10 with 51 red, in home graphite-light-solid only). Text is
  // text on both sides and is unaffected either way.
  const { chrome, browser } = await launchChrome({ port: PORT, dpr: 1, profile: join(REPO, "target", "twin-profile"), args: ["--disable-lcd-text"] });
  const report = [];
  let failed = 0;
  try {
    for (const shot of shots) {
      const name = shotName({ ...shot, w, h }) + (region === "full" ? "" : `-${region}`);
      const r = crop(region, w, h);
      const ref = cropPng(await renderReference(browser, fonts, shot, w, h), r);
      const { png, axe, setup } = await renderApp(browser, shot, w, h, withAxe, noSetup);
      const app = cropPng(png, r);
      const res = diffPngs(ref, app);
      const axeBad = (axe ?? []).filter((v) => v.impact === "serious" || v.impact === "critical");
      const pass = res.pass && axeBad.length === 0 && setup.length === 0;
      if (!pass) failed++;

      writeFileSync(join(OUT, `${name}-ref.png`), PNG.sync.write(ref));
      writeFileSync(join(OUT, `${name}-app.png`), PNG.sync.write(app));
      writeFileSync(join(OUT, `${name}-diff.png`), PNG.sync.write(res.diff));
      report.push({ name, ratio: res.ratio, pass, big: res.big, clusters: res.clusters.slice(0, 10), axe, setup, ...(noSetup ? { noSetup: true } : {}) });
      writeFileSync(join(OUT, reportFile), JSON.stringify(report, null, 2));

      let line = `${pass ? "PASS" : "FAIL"} ${pct(res.ratio)} ${name}`;
      if (res.big.length) line += `  ${res.big.length} cluster(s) over 24x24 or a text line, largest ${box(res.big.sort((p, q) => q.pixels - p.pixels)[0])}`;
      if (setup.length) line += `  setup: ${setup.join("; ")}`;
      if (axeBad.length) line += `  axe: ${axeBad.map((v) => `${v.id}(${v.impact})`).join(", ")}`;
      console.log(line);
    }
    browser.close();
  } finally {
    chrome.kill();
  }
  console.log(`${shots.length - failed}/${shots.length} passed. Images in ${OUT}`);
  return failed === 0;
}

try {
  if (!(await main())) process.exitCode = 1;
} catch (e) {
  console.error(e.message);
  process.exitCode = 1;
}
