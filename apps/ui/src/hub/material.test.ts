import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  windowMaterial: vi.fn<() => Promise<"mica" | "none">>(),
  windowsTransparency: vi.fn<() => Promise<boolean>>(),
}));
vi.mock("./api", () => api);

const win = vi.hoisted(() => ({
  setTheme: vi.fn<(t: string) => Promise<void>>(),
  setEffects: vi.fn<(e: unknown) => Promise<void>>(),
  clearEffects: vi.fn<() => Promise<void>>(),
}));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => win, Effect: { Mica: "mica" } }));

import { followTheme, initMaterial, syncNativeTheme } from "./material";
import { __resetThemeStore, setPrefs } from "./theme";

const root = document.documentElement;
/** syncNativeTheme is fire-and-forget; let its lazy import and awaits settle. */
const settle = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  localStorage.clear();
  __resetThemeStore();
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  for (const k of ["material", "solid", "theme", "mode", "modeChoice"]) delete root.dataset[k];
  vi.clearAllMocks();
  vi.restoreAllMocks();
  api.windowMaterial.mockResolvedValue("mica");
  api.windowsTransparency.mockResolvedValue(true);
  win.setTheme.mockResolvedValue();
  win.setEffects.mockResolvedValue();
  win.clearEffects.mockResolvedValue();
});

describe("initMaterial", () => {
  it("sets data-material from the command and caches it", async () => {
    await initMaterial();
    expect(root.dataset.material).toBe("mica");
    expect(localStorage.getItem("ov.material")).toBe("mica");
  });

  it("records none when Mica is unavailable", async () => {
    api.windowMaterial.mockResolvedValue("none");
    await initMaterial();
    expect(root.dataset.material).toBe("none");
    expect(localStorage.getItem("ov.material")).toBe("none");
  });

  it("forces solid panels when Windows transparency is off, without touching the saved switch", async () => {
    api.windowsTransparency.mockResolvedValue(false);
    await initMaterial();
    expect(root.dataset.solid).toBe("");
    expect(localStorage.getItem("ov.solid")).toBeNull();
    // ...and it is still forced after the next prefs change, which does not know.
    setPrefs({ theme: "lagoon" });
    expect(root.dataset.solid).toBe("");
  });

  it("leaves panels glass when transparency is on", async () => {
    await initMaterial();
    expect(root.dataset.solid).toBeUndefined();
  });

  // The screenshot/twin Tauri stub answers every unlisted command with null. That
  // must read as "no Mica, transparency on", not as data-material="null" + solid.
  it("treats a null answer as no Mica and transparency on", async () => {
    api.windowMaterial.mockResolvedValue(null as never);
    api.windowsTransparency.mockResolvedValue(null as never);
    await initMaterial();
    expect(root.dataset.material).toBe("none");
    expect(root.dataset.solid).toBeUndefined();
  });

  it("survives storage that throws", async () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("denied"); });
    await initMaterial();
    expect(root.dataset.material).toBe("mica");
  });
});

describe("syncNativeTheme", () => {
  it("themes the title bar and keeps Mica on", async () => {
    root.dataset.material = "mica";
    syncNativeTheme("light", false);
    await settle();
    expect(win.setTheme).toHaveBeenCalledWith("light");
    expect(win.setEffects).toHaveBeenCalledWith({ effects: ["mica"] });
    expect(win.clearEffects).not.toHaveBeenCalled();
  });

  // The Hub is shown when its page finishes loading. A title bar theme that waits on
  // a lazy import lands after that, and the bar visibly fades from dark to light.
  it("dispatches setTheme before returning", () => {
    syncNativeTheme("light", false);
    expect(win.setTheme).toHaveBeenCalledWith("light");
  });

  it("clears the effect when solid", async () => {
    root.dataset.material = "mica";
    syncNativeTheme("dark", true);
    await settle();
    expect(win.setTheme).toHaveBeenCalledWith("dark");
    expect(win.clearEffects).toHaveBeenCalled();
    expect(win.setEffects).not.toHaveBeenCalled();
  });

  it("does not ask for Mica where there is none", async () => {
    root.dataset.material = "none";
    syncNativeTheme("dark", false);
    await settle();
    expect(win.setEffects).not.toHaveBeenCalled();
  });

  it("swallows a denied call with a warning", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    win.setTheme.mockRejectedValue(new Error("not allowed"));
    root.dataset.material = "mica";
    expect(() => syncNativeTheme("dark", false)).not.toThrow();
    await settle();
    expect(warn).toHaveBeenCalled();
    // A denied title bar must not stop the effect call after it.
    expect(win.setEffects).toHaveBeenCalled();
  });
});

describe("followTheme", () => {
  it("re-themes the title bar on every applied prefs change", async () => {
    followTheme();
    setPrefs({ mode: "light" });
    expect(win.setTheme).toHaveBeenLastCalledWith("light");
    setPrefs({ mode: "dark", solid: true });
    expect(win.setTheme).toHaveBeenLastCalledWith("dark");
    await settle();
    expect(win.clearEffects).toHaveBeenCalled();
  });
});

describe("in a plain browser", () => {
  it("does nothing and logs nothing", async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    syncNativeTheme("dark", false);
    await settle();
    expect(win.setTheme).not.toHaveBeenCalled();
    expect(warn).not.toHaveBeenCalled();
  });
});

/** Mica is the one state no rendered check can reach.
 *
 *  The reference page has no concept of it, and the twin's app always resolves
 *  to data-material="none" because the fixture stub answers window_material with
 *  null. So a rule written under html[data-material="mica"] is invisible to all
 *  72 pixel shots and to every screenshot in docs/. One such rule hid .ambient,
 *  and the Hub shipped to a real window with no gradient behind it and cards
 *  with nothing to lift off, while every gate stayed green.
 *
 *  Reading the stylesheet as text is crude, but it is the only check that can
 *  fail for this, and it fails loudly at the exact rule rather than in a render
 *  nobody produces. */
describe("the Mica backdrop", () => {
  it("never hides the ambient gradient or the grain", async () => {
    // Read from disk, not through Vite. `?raw` was tried first and silently
    // returned a CACHED copy: reintroducing the bug left this test green, which
    // is the exact failure mode it exists to prevent. node:fs cannot go stale.
    const { readFileSync, existsSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    // vitest's cwd is apps/ui when run through the package script, but walk up
    // so the test survives being run from the repo root.
    const found = ["src/hub/shell.css", "apps/ui/src/hub/shell.css"]
      .map((rel) => resolve(process.cwd(), rel))
      .find((abs) => existsSync(abs));
    expect(found, "could not locate shell.css from " + process.cwd()).toBeTruthy();
    const css = readFileSync(found!, "utf8");
    // Every rule whose selector mentions Mica, flattened to one line each.
    const micaRules = css
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .split("}")
      .map((block) => block.split("{"))
      .filter((parts) => parts.length === 2 && /\[data-material="mica"\]/.test(parts[0]))
      .map(([selector, body]) => ({ selector: selector.trim().replace(/\s+/g, " "), body: body.trim().replace(/\s+/g, " ") }));

    const hiding = micaRules.filter(
      (r) => /\.ambient|\.grain/.test(r.selector) && /display\s*:\s*none/.test(r.body),
    );
    expect(
      hiding.map((r) => r.selector),
      "Mica must not hide the page's own backdrop: Mica tints the wallpaper and goes flat over a dark one",
    ).toEqual([]);
  });
});
