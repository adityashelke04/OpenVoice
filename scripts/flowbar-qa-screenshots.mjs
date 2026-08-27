/**
 * Digital Instrumentation & QA Screenshot Capture for Flow Bar
 * Drives headless Chrome to capture high-res Retina (2x) screenshots of the fixed Flow Bar.
 */

import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { tauriStub } from "./screenshot-fixtures.mjs";

const REPO = dirname(dirname(fileURLToPath(import.meta.url)));
const OUT = join(REPO, "docs", "images", "qa");
const BASE = process.env.OV_UI_URL ?? "http://localhost:5199";
const PORT = 9222;

const CHROME = [
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
  "C:/Program Files/Google/Chrome Beta/Application/chrome.exe",
  "C:/Program Files/Chromium/Application/chrome.exe",
  "/usr/bin/google-chrome",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
].find((p) => existsSync(p));

const QA_SHOTS = [
  {
    name: "qa-flowbar-idle",
    url: `${BASE}/?window=overlay`,
    width: 420,
    height: 70,
    transparent: true,
    stub: true,
  },
  {
    name: "qa-flowbar-menu-open",
    url: `${BASE}/?window=overlay`,
    width: 420,
    height: 380,
    transparent: true,
    stub: true,
    prepare: `(() => {
      const hit = document.querySelector(".overlay-hit");
      if (hit) hit.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    })()`,
  },
  {
    name: "qa-backdrop-white-document",
    url: `${BASE}/?window=flowbar`,
    width: 1060,
    height: 700,
    fit: '.fbs-plate[data-plate="document"]',
  },
  {
    name: "qa-backdrop-dark-editor",
    url: `${BASE}/?window=flowbar`,
    width: 1060,
    height: 700,
    fit: '.fbs-plate[data-plate="editor"]',
  },
  {
    name: "qa-backdrop-busy-photo",
    url: `${BASE}/?window=flowbar`,
    width: 1060,
    height: 700,
    fit: '.fbs-plate[data-plate="photo"]',
  },
  {
    name: "qa-backdrop-app-canvas",
    url: `${BASE}/?window=flowbar`,
    width: 1060,
    height: 700,
    fit: '.fbs-plate[data-plate="canvas"]',
  },
  {
    name: "qa-flowbar-edge-cases-and-menu",
    url: `${BASE}/?window=flowbar`,
    width: 1060,
    height: 700,
    fit: '.fbs-plate[data-plate="editor"]',
    prepare: `(() => {
      const rows = [...document.querySelectorAll(".fbs-row")];
      const menuRow = rows.find(r => r.textContent.includes("Menu"));
      if (menuRow) menuRow.scrollIntoView();
    })()`,
  },
];

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
  for (let i = 0; i < 50; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${PORT}/json/version`);
      return (await res.json()).webSocketDebuggerUrl;
    } catch {
      await sleep(150);
    }
  }
  throw new Error("headless Chrome never opened its debugging port");
}

async function main() {
  if (!CHROME) throw new Error("Chrome not found; verify Chrome installation path.");

  try {
    const res = await fetch(BASE, { signal: AbortSignal.timeout(3000) });
    if (!res.ok) throw new Error(String(res.status));
  } catch (e) {
    throw new Error(`No dev server running at ${BASE} (${e.message}). Start with: npm --prefix apps/ui run dev -- --port 5199 --strictPort`);
  }

  mkdirSync(OUT, { recursive: true });

  const chrome = spawn(
    CHROME,
    [
      "--headless=new",
      `--remote-debugging-port=${PORT}`,
      "--disable-gpu",
      "--hide-scrollbars",
      "--force-device-scale-factor=2",
      "--no-first-run",
      "--no-default-browser-check",
      "--user-data-dir=" + join(REPO, "target", "qa-screenshot-profile"),
      "about:blank",
    ],
    { stdio: "ignore" },
  );

  try {
    const browser = await Devtools.attach(await targetUrl());
    const { targetId } = await browser.send("Target.createTarget", { url: "about:blank" });

    for (const shot of QA_SHOTS) {
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

      let stubId = null;
      if (shot.stub) {
        ({ identifier: stubId } = await page.send("Page.addScriptToEvaluateOnNewDocument", {
          source: tauriStub(),
        }));
      }

      await page.send("Page.navigate", { url: shot.url });
      await sleep(1400);

      if (shot.prepare) {
        await page.send("Runtime.evaluate", { expression: shot.prepare, awaitPromise: true });
        await sleep(600);
      }

      if (shot.fit) {
        const { result } = await page.send("Runtime.evaluate", {
          expression: `(() => {
            const el = document.querySelector(${JSON.stringify(shot.fit)});
            if (!el) return 0;
            const rect = el.getBoundingClientRect();
            return Math.ceil(rect.bottom + 32);
          })()`,
          returnByValue: true,
        });
        if (result.value > 0) {
          await page.send("Emulation.setDeviceMetricsOverride", {
            width: shot.width,
            height: result.value,
            deviceScaleFactor: 2,
            mobile: false,
          });
          shot.height = result.value;
          await sleep(200);
        }
      }

      const { data } = await page.send("Page.captureScreenshot", {
        format: "png",
        captureBeyondViewport: false,
      });

      if (stubId) await page.send("Page.removeScriptToEvaluateOnNewDocument", { identifier: stubId });

      const file = join(OUT, `${shot.name}.png`);
      writeFileSync(file, Buffer.from(data, "base64"));
      console.log(`[QA Shot Captured] ${shot.name.padEnd(30)} ${shot.width}x${shot.height} -> ${file}`);
      page.close();
    }

    browser.close();
  } finally {
    chrome.kill();
  }
}

await main();
