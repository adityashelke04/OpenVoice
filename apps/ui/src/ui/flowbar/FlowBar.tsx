import { useRef } from "react";
import type { ReactNode } from "react";
import { Kbd } from "../index";
import { LoadingDots, TickingEllipsis } from "../LoadingDots";
import { Waveform } from "../Waveform";

/**
 * The Flow Bar — the floating overlay, and the only interface visible while
 * dictating.
 *
 * Compact by default and wider while listening. The width change is not decoration:
 * it is the confirmation that the key registered, readable in peripheral vision
 * before any glyph or colour is resolved.
 */
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

export function FlowBar({
  live,
  level,
  levelRef,
  elapsed,
  hint = "Right Ctrl",
  working,
  failed,
  message,
  action,
  confirm,
  publish,
  onCancel,
  status = "ready",
  progress,
  mini = false,
  edge = "bottom",
  latched = false,
  onToggle,
}: {
  live: boolean;
  /** Static level, for the component sheet and previews. The overlay uses
   *  `levelRef` instead; see the note on `Waveform`. */
  level?: number;
  levelRef?: { current: number };
  elapsed: string;
  hint?: string;
  /** Transcribing or injecting — the key is released but text has not landed. */
  working?: boolean;
  /** The dictation did not land. Distinct from `message` alone: this is the
   *  outcome, not the explanation. */
  failed?: boolean;
  /**
   * What happened, when something did.
   *
   * Carries both outcomes that are not silent success: a hard failure when
   * `failed` is set, and otherwise the soft one — text that reached the
   * clipboard instead of the cursor. The second is not an error and must not
   * look like one, or the colour stops meaning anything.
   */
  message?: string;
  /** Action control (e.g. interactive "Paste Now" button for clipboard fallback). */
  action?: ReactNode;
  /** Momentary, on a clean landing. See `.flowbar-mic[data-confirm]`. */
  confirm?: boolean;
  /**
   * The microphone is held open without a key — see `taplatch.rs`.
   *
   * The single most important distinction this bar draws. A held session and a
   * latched one are otherwise identical, and only one of them keeps recording
   * when you take your hand off the keyboard. Someone who cannot tell them
   * apart can walk away from an open microphone believing they closed it.
   */
  latched?: boolean;
  /** Element the live level is published onto; see `Waveform`. */
  publish?: { current: HTMLElement | null };
  /**
   * Discard the dictation in flight. Shown only while listening.
   *
   * Escape has always done this, but nothing ever said so, and push-to-talk
   * with no discard path means an accidental trigger has to be transcribed and
   * injected into someone else's document before it can be undone. The control
   * is what makes the capability real; the key remains the fast way.
   */
  onCancel?: () => void;
  /**
   * Whether the speech engine is up.
   *
   * The bar used to have no opinion about this, which made it dishonest in the
   * one window that is on screen while it matters: through the first-run model
   * download and the several seconds of loading that follow it, and forever
   * after a hard engine failure, it displayed "Hold Right Ctrl" — an invitation
   * to press a key that was not going to do anything.
   */
  status?: FlowStatus;
  /** Download progress, 0..1, when the size is known. */
  progress?: number;
  /**
   * Render as the compact indicator rather than the full pill.
   *
   * superwhisper's mini window, and the reason to copy it is that the honest
   * answer to "this thing is in my way" is to make it smaller rather than to
   * hide it. A bar you have hidden cannot tell you the microphone is open.
   */
  mini?: boolean;
  /** Which edge the bar is docked to, and so which axis it lays out on. */
  edge?: FlowEdge;
  /** Start or stop a dictation by clicking the bar. */
  onToggle?: () => void;
}) {
  /**
   * Where the live level gets published.
   *
   * The overlay passes its own hit target, because that element also reserves
   * the window margin the glow paints into. Everywhere else — the component
   * sheet, the review surface — falls back to the pill itself, so the
   * level-reactive parts still animate outside the app.
   */
  const root = useRef<HTMLDivElement>(null);
  const sink = publish ?? root;

  const mode = flowMode({ live, working, failed, message, status });
  // Docked vertically *and* with nothing to say. See `flowSpeaks`.
  const column = edge !== "bottom" && !flowSpeaks(mode);

  const text = flowText({ mode, message, progress });

  return (
    <div
      className="flowbar"
      ref={root}
      data-live={live}
      data-working={working}
      data-failed={failed}
      data-mode={mode}
      data-mini={mini}
      data-column={column}
      data-latched={latched}
    >
      <span className="flowbar-mic" data-confirm={confirm} />
      {/* Keyed on the mode so React replaces the subtree on every change, which
          is what replays the entrance animation. Without the key the text
          swaps in place and the bar reads as a label that changed rather than a
          state that did. */}
      <div className="flowbar-body" key={`${mode}:${mini}:${column}`}>
        {mode === "live" ? (
          <>
            <div className="flowbar-wave">
              {/* 24, not 32.
                  At 1:1 — the size this is actually seen at, in the corner of an
                  eye — thirty-two bars across ~150px read as a green texture
                  rather than "a shape that travels", which is the whole claim
                  the waveform makes.

                  The compact forms get fewer still: the count has to fall with
                  the space or the bars stop being individually visible and the
                  shape stops travelling, which is the only thing this is for. */}
              <Waveform
                level={level}
                levelRef={levelRef}
                bars={mini ? 7 : column ? 10 : 24}
                publish={sink}
              />
            </div>
            {!mini && !column && <span className="flowbar-time">{elapsed}</span>}
            {onCancel && !mini && !column ? (
              <button
                type="button"
                className="flowbar-cancel"
                aria-label="Discard this dictation"
                title="Discard (Esc)"
                // The pill is a drag handle: without this, pressing the button
                // hands the pointer to the Windows move loop and the click never
                // lands.
                onMouseDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  onCancel();
                }}
              >
                <svg viewBox="0 0 12 12" width="11" height="11" aria-hidden focusable="false">
                  <path
                    d="M3 3l6 6M9 3l-6 6"
                    stroke="currentColor"
                    strokeWidth="1.5"
                    strokeLinecap="round"
                  />
                </svg>
              </button>
            ) : null}
          </>
        ) : mode === "loading" ? (
          mini || column ? (
            <LoadingDots size="sm" tone="warn" variant="wave" />
          ) : (
            <span className="flowbar-msg" title={text}>
              {progress != null ? (
                <TickingEllipsis
                  text="Getting the speech model"
                  suffix={` ${Math.round(progress * 100)}%`}
                  tone="warn"
                />
              ) : (
                <TickingEllipsis text="Starting the speech engine" tone="warn" />
              )}
            </span>
          )
        ) : text !== undefined ? (
          <>
            <span className="flowbar-msg" title={text}>
              {text}
            </span>
            {action && !mini && !column ? (
              <div className="flowbar-action" onMouseDown={(e) => e.stopPropagation()}>
                {action}
              </div>
            ) : null}
          </>
        ) : mode === "working" ? (
          mini || column ? (
            <LoadingDots size="sm" tone="body" variant="wave" />
          ) : (
            <span className="flowbar-working-text t-caption">
              <TickingEllipsis text="Writing" tone="body" />
            </span>
          )
        ) : mini || column ? // Nothing but the dot. In the compact forms the dot *is* the bar, and
        // the shortcut it would otherwise name is one the user already knows —
        // which is why they made it small. Hovering brings the words back.
        null : (
          <div className="flowbar-idle">
            <span className="t-caption" style={{ color: "var(--mute)" }}>
              Hold
            </span>
            <Kbd>{hint}</Kbd>
            {onToggle ? (
              <button
                type="button"
                className="flowbar-go"
                aria-label="Start dictating"
                title="Click to dictate"
                onMouseDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  onToggle();
                }}
              >
                {/* A microphone, because this is the one control that opens one.
                    Drawn rather than pulled from an icon font: two shapes and an
                    arc is less code than a dependency. */}
                <svg viewBox="0 0 12 16" width="10" height="13" aria-hidden focusable="false">
                  <rect x="4" y="1" width="4" height="7" rx="2" fill="currentColor" />
                  <path
                    d="M2 7a4 4 0 0 0 8 0M6 11v3"
                    stroke="currentColor"
                    strokeWidth="1.3"
                    strokeLinecap="round"
                    fill="none"
                  />
                </svg>
              </button>
            ) : null}
          </div>
        )}
      </div>
    </div>
  );
}
