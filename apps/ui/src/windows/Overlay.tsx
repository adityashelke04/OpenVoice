/** The recording overlay.
 *
 * ~280x64, frameless, always-on-top, and **non-activating**. If this window ever
 * takes focus the caret position is lost and the entire product stops working, so
 * the Tauri side sets WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW and nothing here is
 * focusable.
 *
 * It is read in peripheral vision, for about two seconds at a time, while the user's
 * attention is on their own code. That is the whole design constraint: state must
 * resolve at a glance, from shape and lamp colour, before any text is read. */

import { Meter } from "../components/Panel";
import { formatElapsed, stateLegend, stateSignal } from "../engine/useEngine";
import type { EngineView } from "../engine/useEngine";
import "./overlay.css";

export function Overlay({ view }: { view: EngineView }) {
  const signal = stateSignal(view.state);
  const visible = view.state !== "idle";

  return (
    <div className="ov" data-visible={visible} data-signal={signal}>
      <div className="ov-meter" aria-hidden="true">
        <Meter level={view.level} peak={view.peak} />
      </div>

      <div className="ov-centre">
        {/* aria-live so a screen-reader user gets what the lamp gives a sighted one */}
        <span className="ov-state" aria-live="polite">
          {stateLegend(view.state)}
        </span>
        <span className="ov-profile">{view.profile}</span>
      </div>

      <div className="ov-right">
        <span className="ov-time">{formatElapsed(view.elapsedMs)}</span>
        <span className="ov-hint">RIGHT CTRL</span>
      </div>
    </div>
  );
}
