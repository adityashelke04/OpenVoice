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
 * Images land in docs/images/.
 *
 * Screens that read their state through Tauri get a stubbed bridge — see
 * `screenshot-fixtures.mjs` for what is canned and why the data in it is not
 * invented. The components are never mocked, only the far side of `invoke`.
 *
 * What is still out of reach: anything that needs the engine to be *doing*
 * something at capture time. The first-run download bar and a live recording
 * both belong to the event stream rather than to a command, and faking those
 * would mean driving the callbacks by hand for a result the Flow Bar's own
 * review surface (`?window=flowbar`) already renders honestly.
 */

import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { tauriStub } from "./screenshot-fixtures.mjs";

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
 * `stub: true` installs the fake Tauri bridge from `screenshot-fixtures.mjs`
 * before the page's own JavaScript runs. Without it every screen but Home sits
 * on a loading skeleton forever, because `inTauri()` is false in a plain browser
 * and the commands those screens read from never resolve.
 *
 * The components are the real ones either way. Only the far side of `invoke` is
 * canned, which is also what makes a capture repeatable: the previous version of
 * this file photographed whatever happened to be in the operator's own history
 * database.
 *
 * `?window=flowbar` needs no stub. It is a review surface that renders every
 * Flow Bar state against four backdrops from a synthetic speech envelope, so it
 * is already independent of the engine.
 */
const SHOTS = [
  {
    name: "hub-home",
    url: `${BASE}/?window=hub`,
    width: 1100,
    // Cuts between two history rows rather than through one. The list is meant
    // to run off the bottom — it scrolls — but a slice through a line of text
    // reads as a rendering fault rather than as more content.
    height: 742,
    stub: true,
  },
  {
    name: "hub-dictionary",
    url: `${BASE}/?window=hub`,
    width: 1100,
    height: 760,
    stub: true,
    // Click through, then put a phrase in the live preview. An empty box is the
    // one part of this screen that explains itself only once it has something in
    // it. React owns the input's value, so the write goes through the native
    // setter and a bubbling input event — assigning `.value` directly is
    // discarded on the component's next render.
    prepare: `(() => {
      [...document.querySelectorAll(".nav-item")].find((b) => b.textContent.trim().startsWith("Dictionary"))?.click();
      requestAnimationFrame(() => {
        const el = [...document.querySelectorAll("input")].find((i) => (i.placeholder || "").includes("call use effect"));
        if (!el) return;
        const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
        set.call(el, "um so we need to call use effect here comma then return null");
        el.dispatchEvent(new Event("input", { bubbles: true }));
      });
    })()`,
  },
  {
    name: "hub-writing-style",
    url: `${BASE}/?window=hub`,
    width: 1100,
    height: 820,
    stub: true,
    prepare: `[...document.querySelectorAll(".nav-item")].find((b) => b.textContent.trim().startsWith("Writing style"))?.click()`,
  },
  {
    name: "hub-speech-model",
    url: `${BASE}/?window=hub`,
    width: 1100,
    // Trimmed to the three cards and the note under them. The screen is short;
    // a taller frame is a third of the image spent on empty canvas.
    height: 570,
    stub: true,
    prepare: `[...document.querySelectorAll(".nav-item")].find((b) => b.textContent.trim().startsWith("Speech model"))?.click()`,
  },
  {
    name: "hub-settings",
    url: `${BASE}/?window=hub`,
    width: 1100,
    height: 900,
    stub: true,
    prepare: `[...document.querySelectorAll(".nav-item")].find((b) => b.textContent.trim().startsWith("Settings"))?.click()`,
  },
  {
    name: "hub-advanced",
    url: `${BASE}/?window=hub`,
    width: 1100,
    // Cuts below the Files card. The screen continues past it, but the trace
    // table at the top is what this image is for.
    height: 730,
    stub: true,
    prepare: `[...document.querySelectorAll(".nav-item")].find((b) => b.textContent.trim().startsWith("Advanced"))?.click()`,
  },
  {
    name: "design-system",
    url: `${BASE}/?window=sheet`,
    width: 1100,
    height: 900,
  },
  {
    // Every Flow Bar state over every backdrop, including the two -- listening
    // and working -- that cannot be photographed from the idle overlay because
    // reaching them needs a real microphone.
    name: "flow-bar-states",
    url: `${BASE}/?window=flowbar`,
    width: 1100,
    // A starting height only; `fit` replaces it once the page has laid out. The
    // review surface repeats all six states over four backdrops, which is right
    // for reviewing and far too tall for a README — one backdrop is the asset.
    height: 900,
    fit: ".fbs-plate-section",
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

      // Before the page's own scripts, not after: `inTauri()` is read during the
      // first render, and a bridge that arrives later is a bridge that arrives
      // too late.
      let stubId = null;
      if (shot.stub) {
        ({ identifier: stubId } = await page.send("Page.addScriptToEvaluateOnNewDocument", {
          source: tauriStub(),
        }));
      }

      await page.send("Page.navigate", { url: shot.url });
      // Fonts, then the entrance animations, then the shot. Capturing mid-motion
      // produces a half-faded screenshot that looks like a rendering bug.
      await sleep(1400);
      if (shot.prepare) {
        await page.send("Runtime.evaluate", { expression: shot.prepare, awaitPromise: true });
        await sleep(700);
      }

      // Trim the viewport to one element instead of to a number guessed here.
      // A hard-coded height silently starts cutting through content the first
      // time that section grows.
      if (shot.fit) {
        const { result } = await page.send("Runtime.evaluate", {
          expression: `(() => {
            const el = document.querySelector(${JSON.stringify(shot.fit)});
            return el ? Math.ceil(el.getBoundingClientRect().bottom + 24) : 0;
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
          await sleep(250);
        }
      }

      const { data } = await page.send("Page.captureScreenshot", {
        format: "png",
        captureBeyondViewport: false,
      });
      // The target is reused across shots, so a stub left installed would follow
      // the design sheet and the overlay into their captures.
      if (stubId) await page.send("Page.removeScriptToEvaluateOnNewDocument", { identifier: stubId });

      const file = join(OUT, `${shot.name}.png`);
      writeFileSync(file, Buffer.from(data, "base64"));
      console.log(`${shot.name.padEnd(20)} ${shot.width}x${shot.height}  ${file}`);
      page.close();
    }

    browser.close();
  } finally {
    chrome.kill();
  }
}

await main();
