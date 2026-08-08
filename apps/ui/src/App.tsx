/** Window router.
 *
 * Tauri opens each window against the same bundle, distinguished by query string:
 * `?window=hub` is the main window, `?window=overlay` is the Flow Bar. `?window=sheet`
 * is the design-system review surface and is not part of the app.
 *
 * Which one this is arrives as a prop rather than being read here. `main.tsx` has
 * to know it before rendering anything — it tags the document with it, and for the
 * overlay that tag decides whether the window paints a background — so resolving it
 * twice would be two places to keep in step for no gain.
 */

import { Hub } from "./windows/Hub";
import { Overlay } from "./windows/Overlay";
import { Sheet } from "./windows/Sheet";
import "./styles/global.css";

export default function App({ window: which }: { window: string }) {
  if (which === "overlay") return <Overlay />;
  if (which === "sheet") return <Sheet />;
  return <Hub />;
}
