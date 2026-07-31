/** Reduces the engine event stream into render state.
 *
 * This is the whole of the UI's "logic": a fold over events the engine already
 * decided. Nothing here infers, guesses, or derives a state the engine did not
 * publish. If a value is wanted that only the engine can know, it goes in an event. */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { MockEngine, HISTORY_SEED } from "./mockEngine";
import type {
  EngineEvent,
  EngineState,
  Stage,
  Utterance,
  NoticeLevel,
} from "./types";

export interface Timing {
  stage: Stage;
  tookMs: number;
}

export interface EngineView {
  state: EngineState;
  profile: string;
  level: number;
  peak: number;
  elapsedMs: number;
  lastText: string;
  lastLatencyMs: number | null;
  notice: { level: NoticeLevel; message: string } | null;
  timings: Timing[];
  history: Utterance[];
  sessionCount: number;
}

const INITIAL: EngineView = {
  state: "idle",
  profile: "editor",
  level: 0,
  peak: 0,
  elapsedMs: 0,
  lastText: "",
  lastLatencyMs: null,
  notice: null,
  timings: [],
  history: HISTORY_SEED,
  sessionCount: HISTORY_SEED.length,
};

export function useEngine() {
  const engine = useMemo(() => new MockEngine(), []);
  const [view, setView] = useState<EngineView>(INITIAL);
  const peakHold = useRef({ value: 0, at: 0 });

  useEffect(() => {
    return engine.subscribe((e: EngineEvent) => {
      setView((v) => reduce(v, e, peakHold));
    });
  }, [engine]);

  // Peak marker falls back on its own after the hold expires. Without this the
  // marker sticks at the loudest syllable of the session and stops being useful.
  useEffect(() => {
    const id = window.setInterval(() => {
      setView((v) => {
        if (v.state !== "listening") return v;
        if (Date.now() - peakHold.current.at < 800) return v;
        const decayed = Math.max(v.level, peakHold.current.value - 0.04);
        peakHold.current.value = decayed;
        return decayed === v.peak ? v : { ...v, peak: decayed };
      });
    }, 60);
    return () => clearInterval(id);
  }, []);

  const dismissNotice = useCallback(
    () => setView((v) => ({ ...v, notice: null })),
    [],
  );

  return { view, engine, dismissNotice };
}

function reduce(
  v: EngineView,
  e: EngineEvent,
  peakHold: React.MutableRefObject<{ value: number; at: number }>,
): EngineView {
  switch (e.type) {
    case "Idle":
      return { ...v, state: "idle", level: 0, peak: 0, elapsedMs: 0 };

    case "Listening":
      return {
        ...v,
        state: "listening",
        profile: e.profile,
        level: 0,
        peak: 0,
        elapsedMs: 0,
        timings: [],
        notice: null,
      };

    case "Level": {
      if (e.peak > peakHold.current.value) {
        peakHold.current = { value: e.peak, at: Date.now() };
      }
      return {
        ...v,
        level: e.rms,
        peak: Math.max(peakHold.current.value, e.peak),
        elapsedMs: e.elapsedMs,
      };
    }

    case "Transcribing":
      return { ...v, state: "transcribing", level: 0, peak: 0 };

    case "Injecting":
      return { ...v, state: "injecting" };

    case "Timing":
      return { ...v, timings: [...v.timings, { stage: e.stage, tookMs: e.tookMs }] };

    case "Finished": {
      const failed =
        e.outcome.kind === "asr_failed" || e.outcome.kind === "cancelled";
      const entry: Utterance = {
        id: e.session,
        createdAt: Date.now(),
        durationMs: v.elapsedMs,
        rawText: e.text,
        finalText: e.text,
        profile: v.profile,
        targetApp: v.profile === "terminal" ? "WindowsTerminal.exe" : "Code.exe",
        model: "faster-whisper/large-v3-turbo@cuda",
        status: e.outcome.kind,
        latencyMs: e.latencyMs,
      };
      // Every session lands in history, including the ones that failed. "Never
      // lose a word" is only true if the failures are recorded too.
      return {
        ...v,
        state: failed ? "fault" : v.state,
        lastText: e.text,
        lastLatencyMs: e.latencyMs,
        history: e.text || failed ? [entry, ...v.history] : v.history,
        sessionCount: v.sessionCount + 1,
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

/** State to lamp colour. The one place this mapping lives. */
export function stateSignal(s: EngineState): "off" | "green" | "amber" | "red" {
  switch (s) {
    case "listening":
      return "green";
    case "transcribing":
    case "injecting":
      return "amber";
    case "fault":
      return "red";
    default:
      return "off";
  }
}

export function stateLegend(s: EngineState): string {
  switch (s) {
    case "listening":
      return "Listening";
    case "transcribing":
      return "Transcribing";
    case "injecting":
      return "Injecting";
    case "fault":
      return "Fault";
    default:
      return "Standby";
  }
}

export function formatElapsed(ms: number): string {
  const total = Math.floor(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}
