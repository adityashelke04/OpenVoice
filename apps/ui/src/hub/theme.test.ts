import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { setMedia } from "../test/setup";
import { __resetThemeStore, applyPrefs, DEFAULT_PREFS, onApplied, readPrefs, resolveMode, setForcedSolid, setPrefs, useTheme, writePrefs } from "./theme";

const DARK = "(prefers-color-scheme: dark)";
const root = document.documentElement;

beforeEach(() => {
  localStorage.clear();
  __resetThemeStore();
  setMedia(DARK, false);
  for (const k of ["theme", "mode", "modeChoice", "solid"]) delete root.dataset[k];
  vi.restoreAllMocks();
});

describe("readPrefs", () => {
  it("defaults when storage is empty", () => expect(readPrefs()).toEqual(DEFAULT_PREFS));
  it("reads stored values", () => {
    localStorage.setItem("ov.theme", "lagoon"); localStorage.setItem("ov.mode", "light"); localStorage.setItem("ov.solid", "1");
    expect(readPrefs()).toEqual({ theme: "lagoon", mode: "light", solid: true });
  });
  it("rejects unknown values", () => {
    localStorage.setItem("ov.theme", "neon"); localStorage.setItem("ov.mode", "dim");
    expect(readPrefs()).toEqual(DEFAULT_PREFS);
  });
  it("falls back when storage throws", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("denied"); });
    expect(readPrefs()).toEqual(DEFAULT_PREFS);
  });
});

it("writePrefs swallows storage errors", () => {
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("full"); });
  expect(() => writePrefs(DEFAULT_PREFS)).not.toThrow();
});

it("system mode follows prefers-color-scheme", () => {
  expect(resolveMode("system")).toBe("light");
  setMedia(DARK, true);
  expect(resolveMode("system")).toBe("dark");
  expect(resolveMode("light")).toBe("light");
});

it("applyPrefs tags the root, and forceDark pins dark", () => {
  applyPrefs({ theme: "graphite", mode: "light", solid: true });
  expect([root.dataset.theme, root.dataset.mode, root.dataset.modeChoice, root.dataset.solid]).toEqual(["graphite", "light", "light", ""]);
  applyPrefs({ theme: "graphite", mode: "light", solid: false }, root, { forceDark: true });
  expect(root.dataset.mode).toBe("dark");
  expect(root.dataset.solid).toBeUndefined();
});

it("forceSolid pins data-solid regardless of the stored preference", () => {
  applyPrefs({ theme: "graphite", mode: "light", solid: false }, root, { forceSolid: true });
  expect(root.dataset.solid).toBe("");
});

it("useTheme shares one store, persists and re-applies", () => {
  const a = renderHook(() => useTheme()), b = renderHook(() => useTheme());
  act(() => a.result.current.setTheme("lagoon"));
  expect(b.result.current.prefs.theme).toBe("lagoon");
  expect(localStorage.getItem("ov.theme")).toBe("lagoon");
  expect(root.dataset.theme).toBe("lagoon");
});

it("re-resolves system mode live", () => {
  const h = renderHook(() => useTheme());
  act(() => h.result.current.setMode("system"));
  act(() => setMedia(DARK, true));
  expect(root.dataset.mode).toBe("dark");
  expect(h.result.current.mode).toBe("dark");
});

it("picks up changes from another window via the storage event", () => {
  const h = renderHook(() => useTheme());
  localStorage.setItem("ov.theme", "graphite");
  act(() => { window.dispatchEvent(new StorageEvent("storage", { key: "ov.theme" })); });
  expect(h.result.current.prefs.theme).toBe("graphite");
  expect(root.dataset.theme).toBe("graphite");
});

it("setPrefs outside React also applies", () => {
  setPrefs({ solid: true });
  expect(root.dataset.solid).toBe("");
});

describe("forced solid (Windows transparency off)", () => {
  it("survives a prefs change that does not pass forceSolid", () => {
    setForcedSolid(true);
    applyPrefs(readPrefs());
    expect(root.dataset.solid).toBe("");
    setPrefs({ theme: "lagoon" });
    expect(root.dataset.theme).toBe("lagoon");
    expect(root.dataset.solid).toBe("");
    // The saved switch is untouched: only the OS forced it.
    expect(localStorage.getItem("ov.solid")).toBe("0");
  });

  it("an explicit forceSolid still wins over the flag", () => {
    setForcedSolid(true);
    applyPrefs(DEFAULT_PREFS, root, { forceSolid: false });
    expect(root.dataset.solid).toBeUndefined();
  });

  it("is cleared by the store reset", () => {
    setForcedSolid(true);
    __resetThemeStore();
    applyPrefs(DEFAULT_PREFS);
    expect(root.dataset.solid).toBeUndefined();
  });
});

describe("onApplied", () => {
  it("hears every applied change with the resolved mode and solid", () => {
    const seen: [string, boolean][] = [];
    const off = onApplied((mode, solid) => seen.push([mode, solid]));
    setPrefs({ mode: "dark" });
    setPrefs({ solid: true });
    expect(seen).toEqual([["dark", false], ["dark", true]]);
    off();
    setPrefs({ mode: "light" });
    expect(seen).toHaveLength(2);
  });

  // Nothing else may be subscribed (the Hub shell is not wired to useTheme yet),
  // so registering a listener must itself start watching the system and storage.
  it("follows a live system flip and another window's change on its own", () => {
    const seen: string[] = [];
    onApplied((mode) => seen.push(mode));
    setPrefs({ mode: "system" });
    setMedia(DARK, true);
    expect(seen.at(-1)).toBe("dark");
    localStorage.setItem("ov.mode", "light");
    window.dispatchEvent(new StorageEvent("storage", { key: "ov.mode" }));
    expect(seen.at(-1)).toBe("light");
  });
});
