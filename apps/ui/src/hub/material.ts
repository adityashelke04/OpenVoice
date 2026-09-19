/** Windows 11 Mica behind the Hub, and keeping the native window in step with the
 *  page's theme.
 *
 *  The Hub window is created transparent with Mica requested (tauri.conf.json).
 *  Whether Mica is really there is a Rust question (build number, Transparency
 *  effects), asked once through `window_material` and written to
 *  `<html data-material>`: "mica" lets the page tint over the wallpaper, "none"
 *  means the page paints its own ambient backdrop. The answer is cached in
 *  `ov.material` so main.tsx can set it before the first paint of the next launch
 *  instead of flashing the wrong backdrop while the command round-trips.
 *
 *  Native calls go through Tauri's window API, which is denied unless granted in
 *  capabilities/default.json (allow-set-theme, allow-set-effects). A denied call
 *  rejects rather than throwing; every one is caught and logged so a missing grant
 *  shows up in the console instead of as a title bar that silently never changes. */
import { Effect, getCurrentWindow } from "@tauri-apps/api/window";
import { windowMaterial, windowsTransparency } from "./api";
import { applyPrefs, onApplied, readPrefs, setForcedSolid, type Mode } from "./theme";

export type Material = "mica" | "none";

const KEY = "ov.material";
const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function initMaterial(root: HTMLElement = document.documentElement): Promise<void> {
  const [answer, transparency] = await Promise.all([windowMaterial(), windowsTransparency()]);
  // Only an explicit "mica" / `false` counts: the screenshot stub answers every
  // command it does not know with null, and the twin must render as "none", glass.
  const material: Material = answer === "mica" ? "mica" : "none";
  root.dataset.material = material;
  try { localStorage.setItem(KEY, material); } catch { /* next launch starts from "none" and corrects itself here */ }
  // Windows transparency off forces solid through theme.ts, so every later apply
  // keeps it. The saved switch is left alone: turning Windows transparency back on
  // should bring the glass back without the user having to find the in-app switch.
  setForcedSolid(transparency === false);
  // Re-apply so the forcing lands now, and so `followTheme` listeners re-sync the
  // native window against the live material rather than last launch's cache.
  applyPrefs(readPrefs(), root);
}

/** Keep the native window (title bar theme, Mica on/off) in step with every
 *  applied prefs change: the in-app switches, another window's change, and a live
 *  system light/dark flip. Returns the unsubscribe. */
export function followTheme(): () => void {
  return onApplied(syncNativeTheme);
}

/** The cached answer from the last launch, for the pre-paint tag in main.tsx. */
export function cachedMaterial(): Material {
  try { return localStorage.getItem(KEY) === "mica" ? "mica" : "none"; } catch { return "none"; }
}

function attempt(what: string, run: () => Promise<void>): Promise<void> {
  // `run()` is called here, synchronously: the IPC message is posted before this
  // returns, and only the reply is awaited.
  try {
    return run().catch((e) => console.warn(`[material] ${what} failed`, e));
  } catch (e) {
    console.warn(`[material] ${what} failed`, e);
    return Promise.resolve();
  }
}

/** Title bar follows the page's light/dark; Mica comes off under Reduce transparency.
 *
 *  `setTheme` is dispatched before this returns. The Hub is shown when its page
 *  finishes loading (main.rs), and main.tsx calls this during module evaluation,
 *  which is before that. Measured with the window API imported lazily instead: the
 *  title bar arrived dark and faded to light about half a second after the window
 *  appeared. Nothing is awaited by the caller; the window manager's replies are. */
export function syncNativeTheme(mode: Mode, solid: boolean): void {
  // A plain browser (dev server, screenshots without the stub) has no window to theme.
  if (!inTauri()) return;
  const mica = document.documentElement.dataset.material === "mica";
  let win: ReturnType<typeof getCurrentWindow>;
  try {
    win = getCurrentWindow();
  } catch (e) {
    console.warn("[material] no native window", e);
    return;
  }
  const theme = attempt("setTheme", () => win.setTheme(mode));
  void theme.then(() => {
    if (solid) return attempt("clearEffects", () => win.clearEffects());
    if (mica) return attempt("setEffects", () => win.setEffects({ effects: [Effect.Mica] }));
  });
}
