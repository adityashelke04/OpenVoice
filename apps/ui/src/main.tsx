import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";

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

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App window={which} />
  </StrictMode>,
);
