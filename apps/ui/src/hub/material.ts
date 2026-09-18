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
import { applyPrefs, readPrefs, type Mode } from "./theme";

export type Material = "mica" | "none";

const KEY = "ov.material";
let solidForced = false;

/** True when Windows "Transparency effects" is off: the Hub stays solid whatever
 *  the in-app switch says. Callers that re-apply prefs pass this as `forceSolid`. */
export function isSolidForced(): boolean {
  return solidForced;
}

export async function initMaterial(root: HTMLElement = document.documentElement): Promise<void> {
  const [answer, transparency] = await Promise.all([windowMaterial(), windowsTransparency()]);
  // Only an explicit "mica" / `false` counts: the screenshot stub answers every
  // command it does not know with null, and the twin must render as "none", glass.
  const material: Material = answer === "mica" ? "mica" : "none";
  root.dataset.material = material;
  try { localStorage.setItem(KEY, material); } catch { /* next launch starts from "none" and corrects itself here */ }
  solidForced = transparency === false;
  // The saved switch is left alone: turning Windows transparency back on should
  // bring the glass back without the user having to find the in-app switch too.
  if (solidForced) applyPrefs(readPrefs(), root, { forceSolid: true });
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
