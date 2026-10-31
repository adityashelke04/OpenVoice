import { useRef } from "react";
import type { CSSProperties, MouseEvent, ReactNode } from "react";
import { FlowWave } from "./FlowWave";
import { FlowIcon } from "./icons";
import { flowMode, flowSpeaks, flowText } from "./mode";
import type { FlowEdge, FlowStatus } from "./mode";
import "./flowbar.css";

/** Measured Wispr capsule, with OpenVoice's local model and recovery states. */
export function FlowBar({ live, level, levelRef, elapsed, hint = "Right Ctrl", working,
  workingPhase, failed, message, action, confirm, publish, onCancel, status = "ready",
  progress, mini = false, edge = "bottom", latched = false, onToggle,
}: {
  live: boolean; level?: number; levelRef?: { current: number }; elapsed: string;
  hint?: string; working?: boolean; workingPhase?: "transcribing" | "injecting";
  failed?: boolean; message?: string; action?: ReactNode; confirm?: boolean;
  publish?: { current: HTMLElement | null }; onCancel?: () => void;
  status?: FlowStatus; progress?: number; mini?: boolean; edge?: FlowEdge;
  latched?: boolean; onToggle?: () => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  const mode = flowMode({ live, working, failed, message, status });
  const column = edge !== "bottom" && !flowSpeaks(mode);
  const text = flowText({ mode, message, progress });
  const pct = progress != null && Number.isFinite(progress)
    ? Math.round(Math.min(1, Math.max(0, progress)) * 100) : undefined;
  const stop = (e: MouseEvent) => e.stopPropagation();
  const run = (fn?: () => void) => (e: MouseEvent) => { e.stopPropagation(); fn?.(); };
  const phase = workingPhase === "transcribing" ? "Transcribing" : workingPhase === "injecting" ? "Pasting" : "Writing";
  const icon = mode === "failed" || mode === "enginefail" ? "warn" : /clipboard/i.test(text ?? "") ? "clipboard" : "info";
  return <div className="flowbar" ref={root} data-live={live} data-working={working}
    data-failed={failed} data-mode={mode} data-mini={mini} data-column={column}
    data-latched={latched} data-confirm={confirm}>
    <div className="flowbar-body">
      {mode === "live" || mode === "working" ? <>
        {mode === "live" && latched && <>
          <span className="sr-only" role="img" aria-label="Hands-free" />
          <button type="button" className="flowbar-cancel" aria-label="Discard this dictation"
            title="Discard (Esc)" onMouseDown={stop} onClick={run(onCancel)} disabled={!onCancel}>
            <FlowIcon name="close" />
          </button>
        </>}
        <div className="flowbar-wave">
          <FlowWave active={mode === "live"} level={level} levelRef={levelRef} publish={publish ?? root} />
        </div>
        {mode === "live" && !latched && onToggle && <button type="button"
          className="flowbar-stop-area" aria-label="Stop dictation" title={`Listening · ${elapsed} · click to stop · Esc to discard`}
          onMouseDown={stop} onClick={run(onToggle)} />}
        {mode === "live" && latched && <button type="button" className="flowbar-stop"
          aria-label="Stop dictation" title={`Hands-free · ${elapsed} · stop dictation`}
          onMouseDown={stop} onClick={run(onToggle)} disabled={!onToggle}><FlowIcon name="stop" /></button>}
        {mode === "working" && <span className="flowbar-spinner" role="status" aria-label={phase}>
          {Array.from({ length: 8 }, (_, i) => <i key={i} style={{ "--tick": i } as CSSProperties} />)}
        </span>}
      </> : mode === "loading" ? <>
        <span className="flowbar-icon" data-icon="model"><FlowIcon name="model" /></span>
        <span className="flowbar-msg" title={text}>{pct != null ? "Getting the speech model" : "Starting the speech engine"}</span>
        {pct != null && <span className="flowbar-pct">{pct}%</span>}
      </> : text !== undefined ? <>
        <span className="flowbar-icon" data-icon={icon}><FlowIcon name={icon} /></span>
        <span className="flowbar-msg" title={text}>{text}</span>
        {action && <div className="flowbar-action" onMouseDown={stop}>{action}</div>}
      </> : onToggle ? <button type="button" className="flowbar-go" aria-label="Start dictating"
        title={`Hold ${hint}, or click to dictate`} onMouseDown={stop} onClick={run(onToggle)}>
        <FlowIcon name="mic" />
      </button> : <span className="flowbar-ready" aria-hidden><FlowIcon name="mic" /></span>}
    </div>
    {mode === "loading" && <span className="flowbar-rail" role="progressbar" aria-label="Speech model"
      aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} data-indeterminate={pct == null}
      style={{ "--p": `${pct ?? 0}%` } as CSSProperties}><span className="flowbar-rail-fill" /></span>}
  </div>;
}
