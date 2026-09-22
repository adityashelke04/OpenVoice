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

import { lazy, Suspense } from "react";
import "./styles/global.css";

// Lazy: each window then only ever pulls in its own component's CSS (and the
// modules it imports), never every other window's. Without this, one bundle
// containing all four windows would mean the overlay's global selectors and
// the Hub's both land in the same stylesheet — exactly what the CSS isolation
// in Task 6 depends on not happening.
const Hub = lazy(() => import("./windows/Hub").then((m) => ({ default: m.Hub })));
const Overlay = lazy(() => import("./windows/Overlay").then((m) => ({ default: m.Overlay })));
const Sheet = lazy(() => import("./windows/Sheet").then((m) => ({ default: m.Sheet })));
const FlowBarStates = lazy(() => import("./windows/FlowBarStates").then((m) => ({ default: m.FlowBarStates })));

export default function App({ window: which }: { window: string }) {
  // Suspense fallback is null: the window background is already painted by
  // CSS (see global.css's per-window canvas rules) before any chunk loads.
  return (
    <Suspense fallback={null}>
      {which === "overlay" ? <Overlay /> : which === "sheet" ? <Sheet /> : which === "flowbar" ? <FlowBarStates /> : <Hub />}
    </Suspense>
  );
}
