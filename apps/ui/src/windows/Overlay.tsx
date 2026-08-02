/** The Flow Bar window.
 *
 * Frameless, transparent, always on top, and non-activating (`WS_EX_NOACTIVATE`,
 * applied on the Rust side). If this window ever takes focus, the caret in the
 * user's editor is lost and the dictated text goes nowhere.
 *
 * `WS_EX_NOACTIVATE` prevents *focus*, not *input* — so the bar can be dragged and
 * right-clicked while the editor keeps the caret. That is the only reason an
 * interactive always-on-top overlay is viable for a dictation tool.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { FlowBar } from "../ui";
import { playCompletionChime, playStartTone } from "../ui/sound";
import { elapsed, useLiveEngine } from "../engine/useLiveEngine";
import { useSettings } from "../screens/Settings";
import "./overlay.css";

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call(cmd: string, args?: Record<string, unknown>) {
  if (!inTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(cmd, args);
}

export function Overlay() {
  const { view, levelRef } = useLiveEngine();
  const { settings } = useSettings();
  const [menu, setMenu] = useState(false);
  const dragging = useRef(false);

  const live = view.state === "listening";
  const working = view.state === "transcribing" || view.state === "injecting";
  const soundEnabled = settings?.config.sound_enabled ?? true;

  // Two tones: one when the hotkey engages, one when a dictation finishes and
  // actually landed. Tracked off the raw state transition rather than the
  // `live`/`working` booleans above so a clipboard-fallback completion --
  // which settles to "idle" exactly the same way a real success does, per
  // `useLiveEngine`'s reducer -- doesn't get the success chime just because it
  // isn't a hard failure. `view.notice` is set the moment that fallback
  // happens and nothing in this window clears it, so its presence at the
  // instant of the idle transition is a reliable signal the completion wasn't
  // clean, independent of exactly when the Notice and Finished events arrive
  // relative to each other.
  const prevState = useRef(view.state);
  useEffect(() => {
    const prev = prevState.current;
    prevState.current = view.state;
    if (!soundEnabled) return;

    if (prev !== "listening" && view.state === "listening") {
      playStartTone();
    } else if (
      (prev === "transcribing" || prev === "injecting") &&
      view.state === "idle" &&
      !view.notice
    ) {
      playCompletionChime();
    }
  }, [view.state, view.notice, soundEnabled]);

  // Persist the position after a native drag. Snapping happens in Rust so the
  // rules live in one place rather than being split across the boundary.
  useEffect(() => {
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    let timer = 0;

    (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const win = getCurrentWindow();
      un = await win.onMoved(({ payload }) => {
        if (!dragging.current) return;
        // Debounced: onMoved fires continuously through a drag, and only the
        // resting place is worth storing.
        window.clearTimeout(timer);
        timer = window.setTimeout(async () => {
          const scale = await win.scaleFactor();
          call("overlay_move", { x: payload.x / scale, y: payload.y / scale });
          dragging.current = false;
        }, 220);
      });
    })();

    return () => {
      window.clearTimeout(timer);
      un?.();
    };
  }, []);

  const startDrag = useCallback(async (e: React.MouseEvent) => {
    if (e.button !== 0 || !inTauri()) return;
    setMenu(false);
    dragging.current = true;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().startDragging();
  }, []);

  // The window is sized to exactly what is painted, and resized whenever that
  // changes.
  //
  // Two reasons, and both were bugs before:
  //
  //  1. Any window area not covered by the pill shows as a translucent rectangle.
  //     Webview transparency on Windows is unreliable, so rather than depending on
  //     it, there is simply no spare area to reveal.
  //  2. A transparent window still swallows OS-level clicks across its whole
  //     rectangle — `pointer-events: none` governs the webview, not the window. An
  //     oversized window would punch a dead zone into whatever is underneath.
  const width = menu ? 280 : live ? 218 : working ? 170 : 150;
  const height = menu ? 226 : 40;

  useEffect(() => {
    if (!inTauri()) return;
    (async () => {
      const { getCurrentWindow, LogicalSize } = await import("@tauri-apps/api/window");
      await getCurrentWindow().setSize(new LogicalSize(width, height));
    })();
  }, [width, height]);

  // Dismiss the menu on any outside interaction, including losing the pointer.
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(false);
    window.addEventListener("blur", close);
    return () => window.removeEventListener("blur", close);
  }, [menu]);

  // State changes are announced. A person using a screen reader gets no benefit
  // from a waveform, and the whole point of this window is knowing whether the
  // microphone is open. Lost in an earlier rewrite; restored here.
  const spoken = live
    ? "Listening"
    : working
      ? "Writing your words"
      : "Ready. Hold the shortcut to dictate.";

  return (
    <div className="overlay-root">
      <span className="sr-only" role="status" aria-live="polite">
        {spoken}
      </span>
      <div
        className="overlay-hit"
        onMouseDown={startDrag}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu((m) => !m);
        }}
        title="Drag to move · right-click for options"
      >
        <FlowBar
          live={live}
          levelRef={levelRef}
          elapsed={elapsed(view.elapsedMs)}
          hint={view.ready?.shortcut ?? "Right Ctrl"}
          working={working}
        />
      </div>

      {menu && (
        <div className="overlay-menu" role="menu">
          <button role="menuitem" onClick={() => { call("show_hub_cmd"); setMenu(false); }}>
            Open OpenVoice
          </button>
          <button role="menuitem" onClick={() => { call("paste_last"); setMenu(false); }}>
            Paste last transcript
          </button>
          <div className="overlay-menu-sep" />
          <button role="menuitem" onClick={() => { call("overlay_snooze", { minutes: 60 }); setMenu(false); }}>
            Hide for an hour
          </button>
          <button role="menuitem" onClick={() => { call("overlay_always_visible", { on: false }); setMenu(false); }}>
            Only show while dictating
          </button>
        </div>
      )}
    </div>
  );
}
