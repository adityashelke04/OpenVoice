/** The bridge from `ov-core`'s event stream to React.
 *
 * This is a fold over events the engine already decided — it infers nothing. If a
 * value is wanted that only the engine can know, it goes in an event rather than
 * being derived here. That constraint is what keeps the whole pipeline testable
 * headlessly through `ov transcribe`, with no window manager involved.
 *
 * Outside Tauri (a plain browser, e.g. the component sheet) the listeners simply
 * never fire and the UI sits in its idle state, which is the honest thing for it
 * to do.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type { EngineEvent, EngineState } from "./types";

export interface Ready {
  model: string;
  device: string;
  shortcut: string;
  mic: string;
}

export interface LiveView {
  state: EngineState;
  profile: string;
  elapsedMs: number;
  lastText: string;
  lastLatencyMs: number | null;
  notice: { level: "info" | "warn" | "error"; message: string } | null;
  ready: Ready | null;
  error: string | null;
  sessions: number;
}

const INITIAL: LiveView = {
  state: "idle",
  profile: "prose",
  elapsedMs: 0,
  lastText: "",
  lastLatencyMs: null,
  notice: null,
  ready: null,
  error: null,
  sessions: 0,
};

/** Present only inside the Tauri webview. */
function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function api() {
  const [{ listen }, { invoke }] = await Promise.all([
    import("@tauri-apps/api/event"),
    import("@tauri-apps/api/core"),
  ]);
  return { listen, invoke };
}

export function useLiveEngine() {
  const [view, setView] = useState<LiveView>(INITIAL);
  const startedAt = useRef(0);

  /**
   * Live microphone level, deliberately **outside** React state.
   *
   * Level events arrive ~30 times a second. Routing them through `setState` made
   * React re-render the whole overlay tree 30 times a second — and before the
   * engine was throttled, 100 times a second — which restarted CSS animations
   * mid-flight and made the bar visibly flicker.
   *
   * The waveform reads this ref from its own animation frame instead, so a level
   * update costs one property write and zero renders. Anything that changes at
   * frame rate does not belong in component state.
   */
  const levelRef = useRef(0);

  useEffect(() => {
    if (!inTauri()) return;
    let stops: Array<() => void> = [];
    let cancelled = false;

    (async () => {
      const { listen, invoke } = await api();

      // Poll status rather than relying only on the one-shot events. The engine
      // can become ready — or fail — before this listener attaches, and a missed
      // failure leaves the UI claiming "Starting…" forever with the real error
      // emitted into the void. Asking cannot be missed.
      const poll = async () => {
        try {
          const s = await invoke<
            | { state: "starting" }
            | ({ state: "ready" } & Ready)
            | { state: "failed"; error: string }
          >("get_status");
          if (cancelled) return true;
          if (s.state === "ready") {
            const { state: _s, ...ready } = s;
            setView((v) => ({ ...v, ready: ready as Ready, error: null }));
            return true;
          }
          if (s.state === "failed") {
            setView((v) => ({ ...v, error: s.error }));
            return true;
          }
        } catch {
          /* command not registered yet */
        }
        return false;
      };

      if (!(await poll())) {
        const id = window.setInterval(async () => {
          if (await poll()) clearInterval(id);
        }, 700);
        stops.push(() => clearInterval(id));
      }

      const un1 = await listen<Ready>("ov://ready", (e) =>
        setView((v) => ({ ...v, ready: e.payload, error: null })),
      );
      const un2 = await listen<string>("ov://error", (e) =>
        setView((v) => ({ ...v, error: e.payload })),
      );
      const un3 = await listen<EngineEvent>("ov://event", (e) => {
        // Level is the only high-frequency event, and it never touches state.
        if (e.payload.type === "Level") {
          levelRef.current = e.payload.rms;
          return;
        }
        setView((v) => reduce(v, e.payload, startedAt));
      });

      if (cancelled) {
        un1();
        un2();
        un3();
      } else {
        stops.push(un1, un2, un3);
      }
    })();

    return () => {
      cancelled = true;
      stops.forEach((s) => s());
    };
  }, []);

  // The engine does not stamp elapsed time on Level events, so the window that is
  // showing a clock keeps its own. One timer, only while listening.
  useEffect(() => {
    if (view.state !== "listening") return;
    const id = window.setInterval(() => {
      setView((v) => ({ ...v, elapsedMs: Date.now() - startedAt.current }));
    }, 200);
    return () => clearInterval(id);
  }, [view.state]);

  const dismissNotice = useCallback(() => setView((v) => ({ ...v, notice: null })), []);

  return { view, levelRef, dismissNotice };
}

function reduce(
  v: LiveView,
  e: EngineEvent,
  startedAt: React.MutableRefObject<number>,
): LiveView {
  switch (e.type) {
    case "Idle":
      return { ...v, state: "idle", elapsedMs: 0 };

    case "Listening":
      startedAt.current = Date.now();
      return {
        ...v,
        state: "listening",
        profile: e.profile,
        elapsedMs: 0,
        notice: null,
      };

    case "Transcribing":
      return { ...v, state: "transcribing" };

    case "Injecting":
      return { ...v, state: "injecting" };

    case "Finished": {
      const failed = e.outcome.kind === "asr_failed";
      return {
        ...v,
        state: failed ? "fault" : v.state,
        lastText: e.text || v.lastText,
        lastLatencyMs: e.latencyMs,
        sessions: v.sessions + 1,
      };
    }

    case "Notice":
      return {
        ...v,
        notice: { level: e.level, message: e.message },
        state: e.level === "error" ? "fault" : v.state,
      };

    default:
      return v;
  }
}

/** Milliseconds to `M:SS`. */
export function elapsed(ms: number): string {
  const total = Math.floor(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}
