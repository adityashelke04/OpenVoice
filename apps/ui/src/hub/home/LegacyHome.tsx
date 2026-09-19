/** The old Home body, moved verbatim out of windows/Hub.tsx so the new shell
 *  (Task 7) can wrap it. Temporary: Task 8 replaces it with hub/home/HomeScreen
 *  and deletes this file together with windows/hub.css.
 *
 *  The engine-failure notice lives here now too; the sidebar's engine card says
 *  "Not running" on every screen, and Home is where the fix (Try again) is. */

import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Card, Empty, Input, Kbd, Notice, Stat, Waveform, useCountUp, useLiveTimeAgo } from "../../ui";
import type { LiveView } from "../../engine/useLiveEngine";
import {
  computeStats,
  humanDuration,
  speedContext,
  SPEAKING_WPM,
  statsFromTotals,
  TYPING_WPM,
  wordsInPerspective,
  type Row,
  type Totals,
} from "../../engine/stats";
import {
  addDictionaryTerm,
  retryEngine,
  type Settings as SettingsDoc,
} from "../../engine/settings";
import "../../windows/hub.css";

/** A horizontal bar placing one rate against the others.
 *
 * Three bars, one scale. A number on its own ("152 wpm") is inert; the same number
 * shown beside the typing baseline is an argument. */
function ScaleBar({
  label,
  value,
  max,
  highlight,
}: {
  label: string;
  value: number;
  max: number;
  highlight?: boolean;
}) {
  return (
    <div className="scale-row">
      <span className="scale-label">{label}</span>
      <div className="scale-track">
        <div
          className="scale-fill"
          data-highlight={highlight}
          style={{ width: `${Math.min(100, (value / max) * 100)}%` }}
        />
      </div>
      <span className="scale-value">{value || "—"}</span>
    </div>
  );
}

/** `chrome.exe` reads as a file path; "Chrome" reads as a place you were. */
function prettyApp(exe: string): string {
  const base = exe.replace(/\.exe$/i, "");
  const known: Record<string, string> = {
    chrome: "Chrome",
    msedge: "Edge",
    firefox: "Firefox",
    Code: "VS Code",
    Cursor: "Cursor",
    slack: "Slack",
    Discord: "Discord",
    Notion: "Notion",
    explorer: "File Explorer",
    WindowsTerminal: "Terminal",
    powershell: "PowerShell",
    notepad: "Notepad",
  };
  return known[base] ?? base;
}

export function LegacyHome({
  view,
  levelRef,
  patch,
}: {
  view: LiveView;
  levelRef: { readonly current: number };
  patch: (fn: (s: SettingsDoc) => void) => void;
}) {
  const [rows, setRows] = useState<Row[]>([]);
  const [query, setQuery] = useState("");
  const [totals, setTotals] = useState<Totals | null>(null);

  const load = useCallback(async (search: string) => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const { invoke } = await import("@tauri-apps/api/core");
    try {
      // Search runs in SQLite. Filtering a fetched page in JavaScript made
      // anything older than the last 200 sessions permanently unfindable.
      const [h, t] = await Promise.all([
        invoke<Row[]>("get_history", { limit: 200, query: search || null }),
        invoke<Totals>("get_totals"),
      ]);
      setRows(h);
      setTotals(t);
    } catch {
      /* history not readable yet */
    }
  }, []);

  // Debounced so typing does not issue a query per keystroke.
  useEffect(() => {
    const id = window.setTimeout(() => load(query), query ? 180 : 0);
    return () => clearTimeout(id);
  }, [load, query, view.sessions]);

  const live = view.state === "listening";
  // Server totals cover the whole history; the row-based fallback only exists for
  // the moment before the first response arrives.
  const stats = totals ? statsFromTotals(totals) : computeStats(rows);
  const saved = humanDuration(stats.savedMinutes);
  const animatedWpm = useCountUp(stats.wpm);
  const animatedWords = useCountUp(stats.words);

  // Empty sessions are already excluded by the query, a key brushed by accident
  // is not history. They stay in the database, where a run of them is the clearest
  // evidence of a dead microphone.
  const shown = rows;

  return (
    <div className="legacy-home">
      {view.error && <StartupError error={view.error} />}
      <header className="hub-head">
        <div>
          <h1 className="t-display">
            {stats.words > 0
              ? `${(animatedWords ?? stats.words).toLocaleString()} words dictated`
              : "Ready when you are"}
          </h1>
          <p className="t-body" style={{ marginTop: 6 }}>
            {stats.words > 0 ? (
              <>
                That is {wordsInPerspective(stats.words)}. Hold{" "}
                <Kbd>{view.ready?.shortcut ?? "Right Ctrl"}</Kbd> anywhere to keep going.
              </>
            ) : (
              <>
                Hold <Kbd>{view.ready?.shortcut ?? "Right Ctrl"}</Kbd> anywhere, speak, then
                let go.
              </>
            )}
          </p>
        </div>
        <div className="hub-live" data-live={live}>
          <Waveform levelRef={levelRef} bars={24} idle={!live} />
        </div>
      </header>

      {/* Your speaking rate is the headline because it is the only number here
          that is about you rather than about the software, and it is the one that
          makes the case for dictating at all. */}
      <div className="hub-hero">
        <div className="hero-main">
          <div className="t-label">Your speaking speed</div>
          <div className="hero-value">
            <span className="hero-number">{animatedWpm ?? "—"}</span>
            <span className="hero-unit">words per minute</span>
          </div>
          {stats.wpm && <div className="t-caption">{speedContext(stats.wpm)}</div>}
        </div>

        {/* A rate means nothing without something to compare it against. */}
        <div className="hero-scale" aria-hidden="true">
          <ScaleBar label="Typing" value={TYPING_WPM} max={200} />
          <ScaleBar label="You" value={animatedWpm ?? 0} max={200} highlight />
          <ScaleBar label="Average speech" value={SPEAKING_WPM} max={200} />
        </div>
      </div>

      <div className="hub-stats">
        <Stat
          label="Time saved"
          value={saved.value}
          unit={saved.unit}
        />
        <Stat label="Day streak" value={stats.streak} unit={stats.streak === 1 ? "day" : "days"} />
        <Stat
          label="Used most in"
          value={stats.topApp ? prettyApp(stats.topApp.name) : "—"}
        />
      </div>

      {!stats.meaningful && stats.words > 0 && (
        <p className="t-caption" style={{ maxWidth: "62ch" }}>
          These settle down after a few more dictations — a handful of short ones
          skews the average.
        </p>
      )}

      <Card
        title="Recent"
        action={
          <input
            className="input"
            placeholder="Search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            style={{ width: 200 }}
          />
        }
      >
        <div className="hub-rows">
          {shown.length === 0 ? (
            <Empty
              title={query ? `Nothing matches “${query}”` : "Nothing dictated yet"}
              hint={
                query
                  ? undefined
                  : "Click into any text box — an email, a document, a chat — then hold the shortcut and start talking. What you say lands where your cursor is, and shows up here."
              }
            />
          ) : (
            shown.map((r, i) => (
              <HistoryRow key={`${r.created_at}-${i}`} row={r} patch={patch} />
            ))
          )}
        </div>
      </Card>
    </div>
  );
}

/** Turns an engine failure into something a person can act on.
 *
 * The raw text is kept — it is what you would search for — but the common causes
 * get a plain-language explanation above it. An error message that only names the
 * failing C function tells the user nothing they can do. */
function StartupError({ error }: { error: string }) {
  const memory = /malloc|out of memory|allocat|cuda error|cublas/i.test(error);
  const missing = /speech model|no speech model|not found|incomplete/i.test(error);
  const [retrying, setRetrying] = useState(false);
  const retry = async () => {
    setRetrying(true);
    try {
      await retryEngine();
    } finally {
      // The button stays disabled until the poller sees the outcome; releasing it
      // here would let someone queue a second attempt behind the first.
      setTimeout(() => setRetrying(false), 4000);
    }
  };

  return (
    <Notice
      tone="danger"
      action={
        <Button variant="primary" onClick={retry} disabled={retrying}>
          {retrying ? "Trying again…" : "Try again"}
        </Button>
      }
    >
      <div>
        <div className="t-body-strong" style={{ color: "var(--ink)" }}>
          {memory
            ? "Not enough memory to load the speech model"
            : missing
              ? "The speech model could not be found"
              : "The speech engine could not start"}
        </div>
        <div className="t-caption" style={{ marginTop: 4, maxWidth: "70ch" }}>
          {memory ? (
            <>
              The speech model needs about 750 MB of memory. Close something
              large — a game, a browser with many tabs, another AI tool — and
              reopen OpenVoice.
            </>
          ) : missing ? (
            <>
              The speech model is missing from the installation. Reinstalling
              OpenVoice will restore it; it ships inside the installer, so this
              needs no download.
            </>
          ) : (
            <>The full details are in the log file.</>
          )}
        </div>
        <div className="t-mono" style={{ marginTop: 8, color: "var(--faint)", fontSize: 11 }}>
          {error}
        </div>
      </div>
    </Notice>
  );
}

/** Teach the dictionary from a dictation that came out wrong.
 *
 * The whole point is the first line: it shows what the model actually *heard*,
 * which is information the user has never had access to. Knowing that "kubectl"
 * arrived as "cube control" is most of the work — once you can see the
 * mishearing, correcting it is obvious. Guessing at it from the formatted output
 * is not.
 *
 * The heard words are buttons because the spoken form is almost always a couple
 * of adjacent words lifted straight out of that line, and retyping them by hand
 * to teach a tool that a word was misheard is exactly the kind of friction that
 * stops people bothering.
 */
function FixRow({
  row,
  patch,
  onDone,
}: {
  row: Row;
  patch: (fn: (s: SettingsDoc) => void) => void;
  onDone: () => void;
}) {
  const [heard, setHeard] = useState("");
  const [written, setWritten] = useState("");

  const words = row.raw_text.split(/\s+/).filter(Boolean);
  const canSave = heard.trim().length > 0 && written.trim().length > 0;

  const save = () => {
    if (!canSave) return;
    patch((s) => addDictionaryTerm(s, heard, written));
    onDone();
  };

  return (
    <div className="fix">
      <div className="t-caption">OpenVoice heard — click the words it got wrong</div>
      <div className="fix-words">
        {words.map((w, i) => (
          <button
            key={`${w}-${i}`}
            type="button"
            className="fix-word"
            onClick={() => setHeard((h) => (h ? `${h} ${w}` : w))}
          >
            {w}
          </button>
        ))}
      </div>
      <div className="fix-form">
        <Input
          label="You said"
          placeholder="cube control"
          value={heard}
          onChange={(e) => setHeard(e.target.value)}
        />
        <Input
          label="Write it as"
          placeholder="kubectl"
          value={written}
          onChange={(e) => setWritten(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && save()}
        />
        <Button variant="primary" onClick={save} disabled={!canSave}>
          Save
        </Button>
        <Button variant="ghost" onClick={onDone}>
          Cancel
        </Button>
      </div>
      <p className="t-caption">
        Applies to the next thing you dictate. Nothing already written changes.
      </p>
    </div>
  );
}

function HistoryRow({
  row,
  patch,
}: {
  row: Row;
  patch: (fn: (s: SettingsDoc) => void) => void;
}) {
  // A badge on every row was noise, not signal: "delivered" is the outcome for
  // almost every dictation, so repeating it down the whole list said nothing a
  // reader didn't already assume. Green is reserved elsewhere in this app for
  // exactly one meaning -- the microphone is open -- and stacking it down every
  // history row diluted that into decoration. Silence now means "this worked
  // as expected"; a badge appears only when something didn't.
  const failed = row.outcome !== "delivered";
  const [fixing, setFixing] = useState(false);
  const [copied, setCopied] = useState(false);
  const timeAgo = useLiveTimeAgo(row.created_at);

  // A copy that says nothing is indistinguishable from a copy that failed, and
  // the failure is silent: `navigator.clipboard` is undefined without a secure
  // context and `writeText` rejects when the window is not focused.
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(row.final_text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="row row-stack">
      <div className="row-main">
        <div style={{ minWidth: 0 }}>
          <div className="t-body" style={{ color: "var(--ink)" }}>
            {row.final_text}
          </div>
          <div className="hstack" style={{ marginTop: 6 }}>
            {failed && (
              <Badge dot tone="warn">
                {row.outcome.replace(/_/g, " ")}
              </Badge>
            )}
            <span className="t-caption">{row.target_app || "unknown app"}</span>
            <span className="t-caption">{Math.round(row.latency_ms)} ms</span>
            <span className="t-caption">{timeAgo}</span>
          </div>
        </div>
        <div className="row-actions">
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setFixing((f) => !f)}
            aria-expanded={fixing}
          >
            {fixing ? "Close" : "Fix a word"}
          </Button>
          <Button size="sm" variant="ghost" onClick={copy}>
            {copied ? "Copied" : "Copy"}
          </Button>
        </div>
      </div>
      {fixing && <FixRow row={row} patch={patch} onDone={() => setFixing(false)} />}
    </div>
  );
}

