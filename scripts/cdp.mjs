/**
 * Headless Chrome and a minimal DevTools-protocol client, shared by
 * `screenshots.mjs` (README images) and `twin-check.mjs` (pixel diff against the
 * reference design).
 *
 * No Puppeteer or Playwright: both want to download their own browser, and this
 * machine's C: drive has no room for one. The installed Chrome plus one WebSocket
 * is all either script needs.
 */

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";

export const CHROME = [
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
  "/usr/bin/google-chrome",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
].find((p) => existsSync(p));

/** Minimal CDP client. One WebSocket, request ids, awaited replies. Events are
 *  delivered to `on(method, fn)` / `once(method, fn)` listeners, which
 *  `twin-check.mjs` uses to wait for a page's load event. */
export class Devtools {
  #ws;
  #id = 0;
  #pending = new Map();
  #listeners = new Map();

  static async attach(wsUrl) {
    const d = new Devtools();
    d.#ws = new WebSocket(wsUrl);
    d.#ws.addEventListener("message", (e) => {
      const msg = JSON.parse(e.data);
      if (msg.method) {
        for (const fn of [...(d.#listeners.get(msg.method) ?? [])]) fn(msg.params);
        return;
      }
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

  on(method, fn) {
    if (!this.#listeners.has(method)) this.#listeners.set(method, []);
    this.#listeners.get(method).push(fn);
  }

  once(method, fn) {
    const wrapped = (params) => {
      const list = this.#listeners.get(method);
      list.splice(list.indexOf(wrapped), 1);
      fn(params);
    };
    this.on(method, wrapped);
  }

  close() {
    this.#ws.close();
  }
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function targetUrl(port = 9222) {
  // Chrome needs a moment to open its debugging port; poll rather than guess.
  for (let i = 0; i < 50; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/version`);
      return (await res.json()).webSocketDebuggerUrl;
    } catch {
      await sleep(200);
    }
  }
  throw new Error("headless Chrome never opened its debugging port");
}

/** Start headless Chrome and attach to the browser target. `profile` must be a
 *  directory on a drive with room (the callers put it under `<repo>/target/`). */
export async function launchChrome({ port = 9222, dpr = 1, profile }) {
  if (!CHROME) throw new Error("Chrome not found; add its path to scripts/cdp.mjs");
  const chrome = spawn(
    CHROME,
    [
      "--headless=new",
      `--remote-debugging-port=${port}`,
      "--disable-gpu",
      "--hide-scrollbars",
      `--force-device-scale-factor=${dpr}`,
      "--no-first-run",
      "--no-default-browser-check",
      `--user-data-dir=${profile}`,
      "about:blank",
    ],
    { stdio: "ignore" },
  );
  try {
    const browser = await Devtools.attach(await targetUrl(port));
    return { chrome, browser };
  } catch (e) {
    chrome.kill();
    throw e;
  }
}

/** Open a fresh tab and attach a page-level client to it. */
export async function openPage(browser, port = 9222) {
  const { targetId } = await browser.send("Target.createTarget", { url: "about:blank" });
  const { webSocketDebuggerUrl } = await fetch(`http://127.0.0.1:${port}/json/list`)
    .then((r) => r.json())
    .then((list) => list.find((t) => t.id === targetId));
  const page = await Devtools.attach(webSocketDebuggerUrl);
  await page.send("Page.enable");
  await page.send("Runtime.enable");
  return { page, targetId };
}
