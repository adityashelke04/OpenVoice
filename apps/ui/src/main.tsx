import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { cachedMaterial, initMaterial, syncNativeTheme } from "./hub/material";
import { applyPrefs, readPrefs, type Mode } from "./hub/theme";

/** Which window this document is: `?window=hub`, `overlay`, or `sheet`. */
const which = new URLSearchParams(location.search).get("window") ?? "hub";

// Tag the document before React exists, not from an effect.
//
// Effects run after the first paint. For the Flow Bar that first paint was the
// window's whole rectangle filled with the app canvas — a black box drawn around
// a pill that is supposed to float over the user's editor — and it stayed until
// React mounted, which on a machine busy loading the speech model is not a frame
// or two. `global.css` now leaves the canvas to whichever window opts into it, so
// the untagged interval paints nothing at all; this closes it anyway, because the
// stylesheet is applied before any script runs and only the tag can tell them
// apart.
document.documentElement.dataset.window = which;
document.body.dataset.window = which;

// Same reasoning for theme: applied before the first paint, so the Hub never
// flashes the wrong theme while React boots. Only the Hub reads/writes these
// attributes (themes.css is keyed on them); the overlay and sheet ignore them.
if (which === "hub") {
  applyPrefs(readPrefs());
  // Mica or not, from last launch's answer: the window is transparent, so the page
  // has to know before its first paint whether to draw its own backdrop. The live
  // answer follows a moment later and corrects a stale cache (a new machine, or
  // Transparency effects turned off since).
  const root = document.documentElement;
  root.dataset.material = cachedMaterial();
  // Title bar now, while the window is still hidden (it is shown when this page
  // finishes loading), and again once the live answer may have changed the effect.
  const sync = () => syncNativeTheme(root.dataset.mode as Mode, "solid" in root.dataset);
  sync();
  void initMaterial().then(sync);
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App window={which} />
  </StrictMode>,
);
