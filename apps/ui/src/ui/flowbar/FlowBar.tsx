import { useRef } from "react";
import type { CSSProperties, MouseEvent, ReactNode } from "react";
import { Waveform } from "../Waveform";
import { FlowMark } from "./FlowMark";
import { FlowIcon } from "./icons";
import type { FlowIconName } from "./icons";
import { flowMode, flowSpeaks, flowText } from "./mode";
import type { FlowEdge, FlowMode, FlowStatus } from "./mode";
import "./flowbar.css";

/**
 * The Flow Bar — the floating overlay, and the only interface visible while
 * dictating.
 *
 * Compact by default and wider while listening. The width change is not decoration:
 * it is the confirmation that the key registered, readable in peripheral vision
 * before any glyph or colour is resolved.
 */

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
  /** Momentary, on a clean landing: one pass of light around the rim. See `.flowbar[data-confirm]`. */
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
   * The overlay passes its own hit target, which is also where the bar reads
   * `--level` from. Everywhere else — the component sheet, the review surface —
   * falls back to the pill itself, so the voice light still moves outside the app.
   */
  const root = useRef<HTMLDivElement>(null);
  const sink = publish ?? root;

  const mode = flowMode({ live, working, failed, message, status });
  // Docked vertically *and* with nothing to say. See `flowSpeaks`.
  const column = edge !== "bottom" && !flowSpeaks(mode);
  // The compact and docked forms carry the mark or the wave and nothing else.
  const bare = mini || column;

  const text = flowText({ mode, message, progress });
  // A percentage a screen reader can trust: clamped, and absent rather than
  // wrong when the size of the download is not known.
  const pct =
    progress != null && Number.isFinite(progress)
      ? Math.round(Math.min(1, Math.max(0, progress)) * 100)
      : undefined;

  const stop = (e: MouseEvent) => e.stopPropagation();

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
      data-confirm={confirm}
    >
      {/* Keyed on the mode so React replaces the subtree on every change, which
          is what replays the entrance animation. Without the key the text swaps
          in place and the bar reads as a label that changed rather than a state
          that did. */}
      <div className="flowbar-body" key={`${mode}:${mini}:${column}`}>
        {mode === "live" ? (
          <>
            {latched && !bare ? (
              // The single most important distinction this bar draws: a held
              // session ends when you let go, a latched one does not.
              <span className="flowbar-lock" role="img" aria-label="Hands-free">
                <FlowIcon name="lock" />
              </span>
            ) : null}
            <div className="flowbar-wave">
              {/* 26 across the full bar: enough to read as a travelling shape at
                  1:1, few enough that each bar stays individually visible. The
                  compact forms need fewer still, or the bars fuse into texture. */}
              <Waveform
                level={level}
                levelRef={levelRef}
                bars={mini ? 9 : column ? 10 : 26}
                publish={sink}
                shaped
              />
            </div>
            {!bare && <span className="flowbar-time">{elapsed}</span>}
            {onCancel && !bare ? (
              <button
                type="button"
                className="flowbar-cancel"
                aria-label="Discard this dictation"
                title="Discard (Esc)"
                // The pill is a drag handle: without this, pressing the button
                // hands the pointer to the Windows move loop and the click never
                // lands.
                onMouseDown={stop}
                onClick={(e) => {
                  e.stopPropagation();
                  onCancel();
                }}
              >
                <FlowIcon name="close" />
              </button>
            ) : null}
          </>
        ) : mode === "loading" ? (
          <>
            <span className="flowbar-icon" data-icon="model">
              <FlowIcon name="model" />
            </span>
            {!bare && (
              <span className="flowbar-msg" title={text}>
                {pct != null ? "Getting the speech model" : "Starting the speech engine"}
              </span>
            )}
            {!bare && pct != null && <span className="flowbar-pct">{pct}%</span>}
          </>
        ) : text !== undefined ? (
          <>
            <span className="flowbar-icon" data-icon={iconFor(mode, text)}>
              <FlowIcon name={iconFor(mode, text)} />
            </span>
            <span className="flowbar-msg" title={text}>
              {text}
            </span>
            {action && !bare ? (
              <div className="flowbar-action" onMouseDown={stop}>
                {action}
              </div>
            ) : null}
          </>
        ) : mode === "working" ? (
          bare ? (
            <FlowMark />
          ) : (
            <span className="flowbar-thread-wrap">
              <span className="flowbar-label">Writing</span>
              <span className="flowbar-thread" aria-hidden />
            </span>
          )
        ) : onToggle ? (
          <button
            type="button"
            className="flowbar-go"
            aria-label="Start dictating"
            title={`Hold ${hint}, or click to dictate`}
            onMouseDown={stop}
            onClick={(e) => {
              e.stopPropagation();
              onToggle();
            }}
          >
            <FlowMark />
          </button>
        ) : (
          <FlowMark />
        )}
      </div>
      {/* On the bar, not in the body: the body's entrance animation makes it the
          containing block for anything absolutely positioned inside it, and the
          rail belongs on the glass's bottom edge, not under the words. */}
      {mode === "loading" ? (
        <span
          className="flowbar-rail"
          role="progressbar"
          aria-label="Speech model"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={pct}
          data-indeterminate={pct == null}
          style={{ "--p": `${pct ?? 0}%` } as CSSProperties}
        >
          <span className="flowbar-rail-fill" />
        </span>
      ) : null}
    </div>
  );
}

/**
 * Which glyph names what happened.
 *
 * Colour no longer does this job — amber for a clipboard fallback made a normal
 * outcome look like a warning. Only a real failure is marked, and it is marked on
 * the glyph; everything else is said in words beside a neutral one.
 */
function iconFor(mode: FlowMode, text: string): FlowIconName {
  if (mode === "failed" || mode === "enginefail") return "warn";
  return /clipboard/i.test(text) ? "clipboard" : "info";
}
