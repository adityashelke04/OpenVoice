/** What the Flow Bar is saying, decided once. Pure, so the window can size
 *  itself from the same rules the component renders by. */

/** Which way the bar is laid out, mirroring `Edge` in `overlay.rs`. */
export type FlowEdge = "bottom" | "left" | "right";

/** Engine health, independent of whatever a session is doing. */
export type FlowStatus = "loading" | "ready" | "error";

/**
 * The states the bar can be in, in precedence order.
 *
 * One name for what the bar is currently saying, so the class hooks, the
 * content, the window size and the entrance animation cannot disagree about it.
 */
export type FlowMode =
  | "failed"
  | "enginefail"
  | "notice"
  | "live"
  | "working"
  | "loading"
  | "idle";

/**
 * What the bar is saying, resolved once.
 *
 * Exported because the window has to size itself to this before it renders it,
 * and a second copy of the precedence rules in `Overlay.tsx` would drift from
 * this one the first time either changed.
 */
export function flowMode(v: {
  live?: boolean;
  working?: boolean;
  failed?: boolean;
  message?: string;
  status?: FlowStatus;
}): FlowMode {
  if (v.failed) return "failed";
  if (v.live) return "live";
  if (v.working) return "working";
  // Everything below here is a resting state, and they are ordered by how much
  // the user needs to know. A dead engine outranks a stale notice: the notice
  // describes one dictation that went sideways, the engine describes every
  // dictation that is not going to happen.
  if (v.status === "error") return "enginefail";
  if (v.message) return "notice";
  if (v.status === "loading") return "loading";
  return "idle";
}

/**
 * Whether this mode has something to say in words.
 *
 * Docked vertically, the bar is a narrow column — too narrow for a sentence, and
 * rotating the text would make a failure message something you have to tilt your
 * head to read. So the modes that carry words unfurl back to horizontal even
 * while docked; the column is for the resting states, which say everything they
 * need to with a colour and a shape.
 */
export function flowSpeaks(mode: FlowMode): boolean {
  return mode === "failed" || mode === "notice" || mode === "enginefail" || mode === "loading";
}

/**
 * The words this mode puts on the bar, or `undefined` when it has none.
 *
 * Exported for the same reason `flowMode` is, and it was a real bug before it
 * was: the window is sized from the text *before* the component renders it, so
 * when the component generated the loading and engine-failure sentences
 * privately, the window sized itself for the idle pill and the sentence was
 * clipped at "Starting the speech en…". One function, two callers, no drift.
 */
export function flowText(v: {
  mode: FlowMode;
  message?: string;
  progress?: number;
}): string | undefined {
  switch (v.mode) {
    case "failed":
      return v.message ?? "Dictation failed";
    case "notice":
      return v.message;
    case "enginefail":
      return v.message ?? "Speech engine unavailable — open OpenVoice";
    case "loading":
      return v.progress != null
        ? `Getting the speech model… ${Math.round(v.progress * 100)}%`
        : "Starting the speech engine…";
    default:
      return undefined;
  }
}
