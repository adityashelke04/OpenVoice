/** Window router.
 *
 * Tauri opens two windows against the same bundle and distinguishes them by query
 * string: `?window=overlay` gets the always-on-top capsule, everything else gets the
 * panel. `?window=preview` is a development-only harness that shows the overlay over
 * a stand-in editor, so its behaviour can be reviewed in a browser without building
 * the Rust shell. */

import { useEffect } from "react";
import { Main } from "./windows/Main";
import { Overlay } from "./windows/Overlay";
import { useEngine } from "./engine/useEngine";
import "./styles/global.css";

export default function App() {
  const { view, engine, dismissNotice } = useEngine();
  const which = new URLSearchParams(location.search).get("window") ?? "main";

  useEffect(() => {
    document.body.dataset.window = which === "main" ? "main" : "overlay";
  }, [which]);

  useEffect(() => {
    if (which !== "preview") return;
    engine.simulate(3200);
    const id = setInterval(() => engine.simulate(3200), 6800);
    return () => clearInterval(id);
  }, [which, engine]);

  if (which === "overlay") return <Overlay view={view} />;
  if (which === "preview") return <OverlayPreview view={view} />;
  return <Main view={view} engine={engine} dismissNotice={dismissNotice} />;
}

/** The overlay as it actually appears, over a stand-in for the user's editor.
 *  It is only ever seen in peripheral vision for a couple of seconds, so reviewing
 *  it on a blank page would flatter it unfairly. */
function OverlayPreview({ view }: { view: ReturnType<typeof useEngine>["view"] }) {
  return (
    <div className="preview">
      <pre className="preview-code">{`fn on_released(&mut self, at: Millis, fx: &mut Vec<Effect>) {
    let Some(active) = self.capturing.as_mut() else {
        return;
    };
    if active.released_at.is_some() {
        return; // already releasing; ignore duplicate key-up
    }
    active.released_at = Some(at);
    fx.push(Effect::StopCapture { session: active.id });
}`}</pre>
      <Overlay view={view} />
    </div>
  );
}
