/** Mirror of the event vocabulary published by `ov-core::event::Event`.
 *
 * The UI is a pure projection of this stream. It holds no business logic and makes
 * no decisions — it renders whatever the last event said. That constraint is what
 * lets the whole engine be tested headlessly and what will let a CLI reuse the same
 * core. If you find yourself wanting to compute something here that the engine
 * could compute, add it to the event instead. */

export type Outcome =
  | { kind: "delivered" }
  | { kind: "clipboard_fallback"; detail: string }
  | { kind: "cancelled" }
  | { kind: "too_short" }
  | { kind: "silent" }
  | { kind: "asr_failed"; detail: string }
  | { kind: "capture_failed"; detail: string };

export type NoticeLevel = "info" | "warn" | "error";

export type Stage =
  | "finalize"
  | "vad"
  | "decode"
  | "format"
  | "inject"
  | "total";

export type EngineEvent =
  | { type: "Idle" }
  | { type: "Listening"; session: number; profile: string }
  | { type: "Level"; rms: number; peak: number; elapsedMs: number }
  | { type: "Transcribing"; session: number; audioMs: number }
  | { type: "Injecting"; session: number; chars: number }
  | {
      type: "Finished";
      session: number;
      outcome: Outcome;
      text: string;
      latencyMs: number;
    }
  | { type: "Notice"; level: NoticeLevel; message: string }
  | { type: "Timing"; session: number; stage: Stage; tookMs: number };

/** What the panel lamps report. Derived from the event stream, never invented. */
export type EngineState =
  | "idle"
  | "listening"
  | "transcribing"
  | "injecting"
  | "fault";

/** Latency budgets from `ov-core::event::Stage::budget_ms`, for a ten-second
 *  utterance on the reference machine. Over budget is a regression worth seeing. */
export const STAGE_BUDGET_MS: Record<Stage, number> = {
  finalize: 10,
  vad: 25,
  decode: 600,
  format: 5,
  inject: 120,
  total: 800,
};

export interface Utterance {
  id: number;
  createdAt: number;
  durationMs: number;
  rawText: string;
  finalText: string;
  profile: string;
  targetApp: string;
  model: string;
  status: string;
  latencyMs: number;
}

export interface FormatStage {
  stage: string;
  text: string;
}
