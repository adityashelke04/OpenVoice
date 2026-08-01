/** Window router.
 *
 * Tauri opens each window against the same bundle, distinguished by query string:
 * `?window=hub` is the main window, `?window=overlay` is the Flow Bar. `?window=sheet`
 * is the design-system review surface and is not part of the app.
 */

import { useEffect } from "react";
import { Hub } from "./windows/Hub";
import { Overlay } from "./windows/Overlay";
import { Sheet } from "./windows/Sheet";
import "./styles/global.css";

export default function App() {
  const which = new URLSearchParams(location.search).get("window") ?? "hub";

  useEffect(() => {
    // Both, not just body: the root element paints its own background, and leaving
    // it opaque draws the overlay window's rectangle around the floating pill.
    document.documentElement.dataset.window = which;
    document.body.dataset.window = which;
  }, [which]);

  if (which === "overlay") return <Overlay />;
  if (which === "sheet") return <Sheet />;
  return <Hub />;
}
