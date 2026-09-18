/** Theme, light/dark and Reduce transparency: one store shared by every component,
 *  persisted in localStorage (shared by the Hub and Flow Bar webviews), applied as
 *  data attributes on <html> so themes.css can do the rest. */
import { useSyncExternalStore } from "react";

export type ThemeName = "glacier" | "graphite" | "lagoon";
export type ModeChoice = "system" | "light" | "dark";
export type Mode = "light" | "dark";
export interface Prefs { theme: ThemeName; mode: ModeChoice; solid: boolean }

export const THEMES: readonly ThemeName[] = ["glacier", "graphite", "lagoon"];
export const DEFAULT_PREFS: Prefs = { theme: "glacier", mode: "system", solid: false };
const KEY = { theme: "ov.theme", mode: "ov.mode", solid: "ov.solid" } as const;
const DARK = "(prefers-color-scheme: dark)";
const SOLID = "(prefers-reduced-transparency: reduce)";

function read(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}
function write(key: string, value: string) {
  try { localStorage.setItem(key, value); } catch { /* storage unavailable: the choice lasts for this session */ }
}
function media(query: string): boolean {
  try { return window.matchMedia(query).matches; } catch { return false; }
}

export function readPrefs(): Prefs {
  const theme = read(KEY.theme), mode = read(KEY.mode);
  return {
    theme: THEMES.includes(theme as ThemeName) ? (theme as ThemeName) : DEFAULT_PREFS.theme,
    mode: mode === "system" || mode === "light" || mode === "dark" ? mode : DEFAULT_PREFS.mode,
    solid: read(KEY.solid) === "1",
  };
}

export function writePrefs(p: Prefs) {
  write(KEY.theme, p.theme);
  write(KEY.mode, p.mode);
  write(KEY.solid, p.solid ? "1" : "0");
}

export function resolveMode(choice: ModeChoice): Mode {
  if (choice !== "system") return choice;
  return media(DARK) ? "dark" : "light";
}

export function applyPrefs(
  p: Prefs,
  root: HTMLElement = document.documentElement,
  opts: { forceDark?: boolean; forceSolid?: boolean } = {},
) {
  root.dataset.theme = p.theme;
  root.dataset.mode = opts.forceDark ? "dark" : resolveMode(p.mode);
  root.dataset.modeChoice = p.mode;
  // Windows "Transparency effects" off forces solid panels whatever the switch says.
  // `forceSolid` is the same forcing from the app side (Task 4): the OS setting is
  // read once at startup and passed down rather than polled through a media query.
  if (opts.forceSolid || p.solid || media(SOLID)) root.dataset.solid = "";
  else delete root.dataset.solid;
}

let current: Prefs | null = null;
const subscribers = new Set<() => void>();
let detach: (() => void) | null = null;

const get = () => (current ??= readPrefs());
const notify = () => subscribers.forEach((f) => f());

export function setPrefs(next: Partial<Prefs>) {
  current = { ...get(), ...next };
  writePrefs(current);
  applyPrefs(current);
  notify();
}

function attach() {
  const onStorage = (e: StorageEvent) => {
    if (e.key && !e.key.startsWith("ov.")) return;
    current = readPrefs();
    applyPrefs(current);
    notify();
  };
  // A fresh object, so useSyncExternalStore re-renders and `mode` re-resolves.
  const onSystem = () => { current = { ...get() }; applyPrefs(current); notify(); };
  window.addEventListener("storage", onStorage);
  const dark = window.matchMedia(DARK), solid = window.matchMedia(SOLID);
  dark.addEventListener("change", onSystem);
  solid.addEventListener("change", onSystem);
  return () => {
    window.removeEventListener("storage", onStorage);
    dark.removeEventListener("change", onSystem);
    solid.removeEventListener("change", onSystem);
  };
}

function subscribe(f: () => void) {
  subscribers.add(f);
  if (!detach) detach = attach();
  return () => {
    subscribers.delete(f);
    if (subscribers.size === 0) { detach?.(); detach = null; }
  };
}

export function useTheme() {
  const prefs = useSyncExternalStore(subscribe, get);
  // Re-read on render so a system flip shows up in `mode` after notify().
  return {
    prefs,
    mode: resolveMode(prefs.mode),
    setTheme: (theme: ThemeName) => setPrefs({ theme }),
    setMode: (mode: ModeChoice) => setPrefs({ mode }),
    setSolid: (solid: boolean) => setPrefs({ solid }),
  };
}

export function __resetThemeStore() {
  current = null;
  detach?.();
  detach = null;
  subscribers.clear();
}
