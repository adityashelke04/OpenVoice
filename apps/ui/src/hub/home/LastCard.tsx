/** The Last dictation card: what you said most recently, first, with the three
 *  things you are most likely to want to do with it (spec 6.2, reference
 *  lines 457-485).
 *
 *  "failed" is the same card when the text did not land: amber, a sentence
 *  saying where the text is now, and Paste again promoted to the first action,
 *  because recovering the paste is the only thing that matters in that moment.
 *
 *  "first" and "error" are the other two faces of the same slot (spec 6.3):
 *  nothing dictated yet, and the speech engine down. */
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  ArrowClockwise,
  CheckCircle,
  ClipboardText,
  ClockCounterClockwise,
  FolderOpen,
  Sparkle,
  WarningCircle,
  WarningOctagon,
} from "@phosphor-icons/react";
import { AppChip, Button, Keycap } from "../ui";
import { appDisplay } from "../apps";
import { outcomeInfo, rowKey, wordCount } from "../history";
import { relativeTime } from "../time";
import { openDataDir, retryEngine, type Settings } from "../../engine/settings";
import type { Row } from "../../engine/stats";
import { Actions } from "./Actions";
import { useCopyPaste } from "./useCopyPaste";
import { FixPanel } from "./FixPanel";

export type LastVariant = "normal" | "failed" | "first" | "error";

type Patch = (fn: (s: Settings) => void) => void;

export function LastCard({ variant, row, now, shortcut, error, patch }: {
  variant: LastVariant;
  row?: Row;
  now: number;
  shortcut: string;
  error?: string | null;
  patch: Patch;
}) {
  if (variant === "error") return <ErrorCard error={error ?? ""} />;
  if (variant === "first" || !row) return <FirstCard shortcut={shortcut} />;
  // Keyed by the dictation, so a new one arrives with Fix closed and the clamp back on.
  return <DictationCard key={rowKey(row)} row={row} failed={variant === "failed"} now={now} patch={patch} />;
}

/** True when focus is somewhere that owns Ctrl+C itself. */
function inEditable(el: Element | null): boolean {
  if (!el) return false;
  return el.matches("input, textarea, select, [contenteditable=''], [contenteditable='true']");
}

function DictationCard({ row, failed, now, patch }: { row: Row; failed: boolean; now: number; patch: Patch }) {
  const act = useCopyPaste(row.final_text);
  const [fixing, setFixing] = useState(false);
  const [clamped, setClamped] = useState(false);
  const [full, setFull] = useState(false);
  const text = useRef<HTMLParagraphElement>(null);
  const { copy } = act;

  // Ctrl+C with nothing selected and no field focused copies the card: on Home
  // the last dictation is the obvious thing to mean. A selection, or a field
  // with focus, keeps the platform's own copy.
  const onKey = useCallback((e: KeyboardEvent) => {
    if (!e.ctrlKey || e.shiftKey || e.altKey || e.metaKey || e.code !== "KeyC" || e.repeat) return;
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed && sel.toString().length > 0) return;
    if (inEditable(document.activeElement)) return;
    // Focus on an Earlier row means that row is what the user is on, not the card.
    if (document.activeElement?.closest(".rows")) return;
    e.preventDefault();
    void copy();
  }, [copy]);
  useEffect(() => {
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onKey]);

  // "Show all" only when the four-line clamp actually cut something off.
  useLayoutEffect(() => {
    const p = text.current;
    if (!p || full) return;
    const measure = () => setClamped(p.scrollHeight > p.clientHeight + 1);
    measure();
    let ro: ResizeObserver | undefined;
    try { ro = new ResizeObserver(measure); ro.observe(p); } catch { /* measured once */ }
    return () => ro?.disconnect();
  }, [row.final_text, full]);

  const kind = outcomeInfo(row);
  const app = appDisplay(row.target_app, row.profile).name;
  const Tag = failed ? WarningCircle : ClockCounterClockwise;

  return (
    <article className={failed ? "last glass fail" : "last glass"} aria-label="Your last dictation">
      <div className="last-head">
        <span className="tag"><Tag weight="bold" aria-hidden />Last dictation</span>
        <span className="meta"><AppChip exe={row.target_app} profile={row.profile} />{relativeTime(row.created_at, now)}</span>
        {kind === "delivered" && <span className="status-ok"><CheckCircle weight="bold" aria-hidden />Pasted</span>}
      </div>
      <p ref={text} className={full ? "last-text full" : "last-text"}>{row.final_text}</p>
      {clamped && !full && <button type="button" className="show-all" onClick={() => setFull(true)}>Show all</button>}
      {failed && (
        <div className="fail-note">
          <ClipboardText weight="fill" aria-hidden />
          {kind === "clipboard"
            ? `This didn’t paste into ${app}, so it’s on your clipboard.`
            : `This didn’t reach ${app}. Copy it or paste it again.`}
        </div>
      )}
      {fixing && <FixPanel row={row} patch={patch} onDone={() => setFixing(false)} />}
      <div className="last-actions">
        <Actions act={act} kind={failed ? "failed" : "normal"} fixOpen={fixing} onFix={() => setFixing((f) => !f)} />
        <span className="words">
          {failed ? `${wordCount(row.final_text)} words` : `${wordCount(row.final_text)} words, ${Math.round(row.latency_ms)} ms`}
        </span>
      </div>
    </article>
  );
}

function FirstCard({ shortcut }: { shortcut: string }) {
  return (
    <article className="last glass" aria-label="Get started">
      <div className="last-head"><span className="tag"><Sparkle weight="bold" aria-hidden />Ready when you are</span></div>
      <h2 className="first-title">Click into any text box, hold the key and talk.</h2>
      <p className="first-body">Let go and your words appear where your cursor is. They also land here, so you can copy or paste them again.</p>
      <div className="bigkey"><Keycap>{shortcut}</Keycap><span className="cap">hold while you speak</span></div>
    </article>
  );
}

/** The engine failed to start. The raw error is kept (it is what you would
 *  search for) under a plain explanation of the common causes; copy from the
 *  old Hub's StartupError, without its em dashes. */
function ErrorCard({ error }: { error: string }) {
  const memory = /malloc|out of memory|allocat|cuda error|cublas/i.test(error);
  // "not installed" is what ov-asr actually says when the model folder is
  // missing ("the parakeet-tdt-0.6b-v2 model is not installed. Expected it
  // in …", crates/ov-asr/src/locate.rs); the older phrases stay for the
  // sidecar's own wording.
  const missing = /speech model|no speech model|not found|incomplete|not installed/i.test(error);
  const [retrying, setRetrying] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const retry = async () => {
    setRetrying(true);
    try {
      await retryEngine();
    } catch { /* the engine card and this one stay up; the poller reports the outcome */ } finally {
      // Held until the poller sees the outcome; releasing it at once would let
      // someone queue a second attempt behind the first.
      timer.current = window.setTimeout(() => setRetrying(false), 4000);
    }
  };
  const [title, body] = memory
    ? ["Not enough memory to load the speech model", "The speech model needs about 750 MB of memory. Close something large, a game, a browser with many tabs, another AI tool, and try again."]
    : missing
      ? ["The speech model could not be found", "The speech model is missing from the installation. Reinstalling OpenVoice will restore it; it ships inside the installer, so this needs no download."]
      : ["The speech engine could not start", "The full details are in the log file."];
  return (
    <article className="last glass err" aria-label="Speech engine problem">
      <div className="last-head"><span className="tag"><WarningOctagon weight="bold" aria-hidden />Speech engine</span></div>
      <h2 className="err-title">{title}</h2>
      <p className="err-body">{body}</p>
      <p className="err-raw">{error}</p>
      <div className="last-actions">
        <Button variant="primary" icon={<ArrowClockwise weight="bold" aria-hidden />} disabled={retrying} onClick={() => void retry()}>
          {retrying ? "Trying again…" : "Try again"}
        </Button>
        <Button icon={<FolderOpen aria-hidden />} onClick={() => void openDataDir().catch(() => {})}>Open log folder</Button>
      </div>
    </article>
  );
}
