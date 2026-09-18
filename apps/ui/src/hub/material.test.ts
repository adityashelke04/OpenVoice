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

import { initMaterial, isSolidForced, syncNativeTheme } from "./material";

const root = document.documentElement;
/** syncNativeTheme is fire-and-forget; let its lazy import and awaits settle. */
const settle = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  localStorage.clear();
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
    expect(isSolidForced()).toBe(true);
    expect(localStorage.getItem("ov.solid")).toBeNull();
  });

  it("leaves panels glass when transparency is on", async () => {
    await initMaterial();
    expect(root.dataset.solid).toBeUndefined();
    expect(isSolidForced()).toBe(false);
  });

  // The screenshot/twin Tauri stub answers every unlisted command with null. That
  // must read as "no Mica, transparency on", not as data-material="null" + solid.
  it("treats a null answer as no Mica and transparency on", async () => {
    api.windowMaterial.mockResolvedValue(null as never);
    api.windowsTransparency.mockResolvedValue(null as never);
    await initMaterial();
    expect(root.dataset.material).toBe("none");
    expect(root.dataset.solid).toBeUndefined();
    expect(isSolidForced()).toBe(false);
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
