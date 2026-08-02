/**
 * Capture the README screenshots from the real UI.
 *
 * Runs the actual components against the dev server rather than mocking them, so
 * a screenshot cannot drift from the interface it claims to show. Drives headless
 * Chrome over the DevTools protocol, which — unlike `chrome --screenshot` — can
 * click through to each screen before capturing.
 *
 * Usage:
 *   npm --prefix apps/ui run dev -- --port 5199 --strictPort   # in one terminal
 *   node scripts/screenshots.mjs                               # in another
 *
 * Images land in docs/images/. Anything the running app cannot reach without a
 * live engine (the first-run download, an active recording) is not faked here;
 * capture those from the built app instead.
 */

import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = dirname(dirname(fileURLToPath(import.meta.url)));
const OUT = join(REPO, "docs", "images");
const BASE = process.env.OV_UI_URL ?? "http://localhost:5199";
const PORT = 9222;

const CHROME = [
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
  "/usr/bin/google-chrome",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
].find((p) => existsSync(p));

/**
 * Screens to capture.
 *
 * Only screens that render truthfully without a backend are listed. Dictionary,
 * Writing style, Speech model and Settings all read their state through Tauri
 * commands, so outside the app they sit on loading skeletons forever — a
 * screenshot of one shows an empty grey rectangle and nothing else. Those belong
 * in a capture taken from the built app, not here.
 */
const SHOTS = [
  {
    name: "hub-home",
    url: `${BASE}/?window=hub`,
    width: 1100,
    height: 760,
  },
  {
    name: "design-system",
    url: `${BASE}/?window=sheet`,
    width: 1100,
    height: 900,
  },
  {
    name: "flow-bar",
    url: `${BASE}/?window=overlay`,
    // Sized to the pill itself. Extra height would be captured as transparent
    // padding, which reads in a README as a misaligned image.
    width: 420,
    height: 56,
    // The overlay window is transparent by design; a black plate here would
    // misrepresent how it sits over whatever the user is looking at.
    transparent: true,
  },
];

/** Minimal CDP client. One WebSocket, request ids, awaited replies. */
class Devtools {
  #ws;
  #id = 0;
  #pending = new Map();

  static async attach(wsUrl) {
    const d = new Devtools();
    d.#ws = new WebSocket(wsUrl);
    d.#ws.addEventListener("message", (e) => {
      const msg = JSON.parse(e.data);
      const resolve = d.#pending.get(msg.id);
      if (resolve) {
        d.#pending.delete(msg.id);
        resolve(msg);
      }
    });
    await new Promise((ok, fail) => {
      d.#ws.addEventListener("open", ok, { once: true });
      d.#ws.addEventListener("error", fail, { once: true });
    });
    return d;
  }

  send(method, params = {}) {
    const id = ++this.#id;
    return new Promise((resolve, reject) => {
      this.#pending.set(id, (msg) =>
        msg.error ? reject(new Error(`${method}: ${msg.error.message}`)) : resolve(msg.result),
      );
      this.#ws.send(JSON.stringify({ id, method, params }));
    });
  }

  close() {
    this.#ws.close();
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function targetUrl() {
  // Chrome needs a moment to open its debugging port; poll rather than guess.
  for (let i = 0; i < 50; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${PORT}/json/version`);
      return (await res.json()).webSocketDebuggerUrl;
    } catch {
      await sleep(200);
    }
  }
  throw new Error("headless Chrome never opened its debugging port");
}

async function main() {
  if (!CHROME) throw new Error("Chrome not found; set a path in scripts/screenshots.mjs");

  // Fail early and clearly. Without this the first capture is a screenshot of
  // Chrome's own connection-error page, which is easy to miss and worse than an
  // error, because it looks like a successful run.
  try {
    const res = await fetch(BASE, { signal: AbortSignal.timeout(3000) });
    if (!res.ok) throw new Error(String(res.status));
  } catch (e) {
    throw new Error(`no dev server at ${BASE} (${e.message}). Start it with:\n  npm --prefix apps/ui run dev -- --port 5199 --strictPort`);
  }

  mkdirSync(OUT, { recursive: true });

  const chrome = spawn(
    CHROME,
    [
      "--headless=new",
      `--remote-debugging-port=${PORT}`,
      "--disable-gpu",
      "--hide-scrollbars",
      // Retina-class output: a 1x screenshot of a dark UI looks muddy on the
      // high-density displays most people read a README on.
      "--force-device-scale-factor=2",
      "--no-first-run",
      "--no-default-browser-check",
      "--user-data-dir=" + join(REPO, "target", "screenshot-profile"),
      "about:blank",
    ],
    { stdio: "ignore" },
  );

  try {
    const browser = await Devtools.attach(await targetUrl());
    const { targetId } = await browser.send("Target.createTarget", { url: "about:blank" });

    for (const shot of SHOTS) {
      const { webSocketDebuggerUrl } = await fetch(`http://127.0.0.1:${PORT}/json/list`)
        .then((r) => r.json())
        .then((list) => list.find((t) => t.id === targetId));
      const page = await Devtools.attach(webSocketDebuggerUrl);

      await page.send("Page.enable");
      await page.send("Runtime.enable");
      await page.send("Emulation.setDeviceMetricsOverride", {
        width: shot.width,
        height: shot.height,
        deviceScaleFactor: 2,
        mobile: false,
      });
      if (shot.transparent) {
        await page.send("Emulation.setDefaultBackgroundColorOverride", {
          color: { r: 0, g: 0, b: 0, a: 0 },
        });
      }

      await page.send("Page.navigate", { url: shot.url });
      // Fonts, then the entrance animations, then the shot. Capturing mid-motion
      // produces a half-faded screenshot that looks like a rendering bug.
      await sleep(1400);
      if (shot.prepare) {
        await page.send("Runtime.evaluate", { expression: shot.prepare, awaitPromise: true });
        await sleep(700);
      }

      const { data } = await page.send("Page.captureScreenshot", {
        format: "png",
        captureBeyondViewport: false,
      });
      const file = join(OUT, `${shot.name}.png`);
      writeFileSync(file, Buffer.from(data, "base64"));
      console.log(`${shot.name.padEnd(16)} ${shot.width}x${shot.height}  ${file}`);
      page.close();
    }

    browser.close();
  } finally {
    chrome.kill();
  }
}

await main();
