/** The Hub window.
 *
 * Phase 3 ships the shell and a working Home screen driven by the live engine.
 * Dictionary, Profiles, Models and Settings arrive in Phase 5; their nav items are
 * present but marked, because a sidebar that grows items later is more disorienting
 * than one that shows what is coming.
 */

import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Card, Empty, Kbd, Notice, Stat, Waveform } from "../ui";
import { elapsed, useLiveEngine } from "../engine/useLiveEngine";
import type { Download } from "../engine/useLiveEngine";
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
} from "../engine/stats";
import { DictionaryScreen } from "../screens/Dictionary";
import { AdvancedScreen, ProfilesScreen } from "../screens/Profiles";
import { ModelsScreen, SettingsScreen, useSettings } from "../screens/Settings";
import "./hub.css";

const NAV = [
  { id: "home", label: "Home", ready: true },
  { id: "dictionary", label: "Dictionary", ready: true },
  { id: "style", label: "Writing style", ready: true },
  { id: "models", label: "Speech model", ready: true },
  { id: "settings", label: "Settings", ready: true },
  { id: "advanced", label: "Advanced", ready: true },
];

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

export function Hub() {
  const { view, levelRef, dismissNotice } = useLiveEngine();
  const { settings, patch, error: settingsError } = useSettings();
  const [tab, setTab] = useState("home");
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

  // Empty sessions are already excluded by the query — a key brushed by accident
  // is not history. They stay in the database, where a run of them is the clearest
  // evidence of a dead microphone.
  const shown = rows;

  return (
    <div className="hub">
      <a className="skip-link" href="#main">
        Skip to content
      </a>

      <nav className="hub-nav" aria-label="Sections">
        <div className="hub-brand">
          {/* Decorative: the same waveform as the overlay, doubling as the mark.
              Its meaning is announced by the overlay's live region, not here. */}
          <Waveform levelRef={levelRef} bars={5} idle={!live} />
          <span className="t-body-strong">OpenVoice</span>
        </div>

        {NAV.map((n) => (
          <button
            key={n.id}
            className="nav-item"
            aria-current={tab === n.id ? "page" : undefined}
            onClick={() => n.ready && setTab(n.id)}
            disabled={!n.ready}
            title={n.ready ? undefined : "Coming in the next update"}
          >
            {n.label}
            {!n.ready && <span className="nav-soon">soon</span>}
          </button>
        ))}

        <div className="hub-nav-foot">
          {/* Three states, not two. Showing "Starting…" while a fatal error is on
              screen tells the user the opposite of the truth. */}
          <Badge dot tone={view.error ? "danger" : view.ready ? "live" : "neutral"}>
            {view.error ? "Not running" : view.ready ? "Ready" : "Starting…"}
          </Badge>
          <p className="t-caption" style={{ marginTop: 8 }}>
            Runs entirely on this machine.
          </p>
        </div>
      </nav>

      <main className="hub-main" id="main" tabIndex={-1}>
        {view.error && <StartupError error={view.error} />}

        {/* Takes over the whole pane rather than sitting above the normal screens.
            On a first run there is no history, no statistics and nothing to
            configure that will survive the model changing — showing empty states
            behind a download makes the app look broken at the worst moment. */}
        {!view.error && view.download && <FirstRun download={view.download} />}

        {view.notice && (
          <Notice
            tone={view.notice.level === "error" ? "danger" : view.notice.level === "warn" ? "warn" : "neutral"}
            action={
              <Button size="sm" variant="ghost" onClick={dismissNotice}>
                Dismiss
              </Button>
            }
          >
            {view.notice.message}
          </Notice>
        )}

        {/* Settings arrive asynchronously. Rendering nothing until they do left
            every screen but Home blank for a beat, which reads as a broken app
            rather than a loading one. */}
        {!view.download && tab !== "home" && !settings && (
          <div className="screen">
            <div className="skeleton skeleton-title" />
            <div className="skeleton skeleton-card" />
            <div className="skeleton skeleton-card" />
          </div>
        )}

        {!view.download && tab !== "home" && settings && (
          <>
            {tab === "dictionary" && <DictionaryScreen settings={settings} patch={patch} />}
            {tab === "style" && <ProfilesScreen settings={settings} patch={patch} />}
            {tab === "models" && <ModelsScreen settings={settings} patch={patch} />}
            {tab === "settings" && (
              <SettingsScreen settings={settings} patch={patch} error={settingsError} />
            )}
            {tab === "advanced" && <AdvancedScreen settings={settings} />}
          </>
        )}

        {!view.download && tab === "home" && (
        <>
        <header className="hub-head">
          <div>
            <h1 className="t-display">
              {stats.words > 0
                ? `${stats.words.toLocaleString()} words dictated`
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
              <span className="hero-number">{stats.wpm ?? "—"}</span>
              <span className="hero-unit">words per minute</span>
            </div>
            {stats.wpm && <div className="t-caption">{speedContext(stats.wpm)}</div>}
          </div>

          {/* A rate means nothing without something to compare it against. */}
          <div className="hero-scale" aria-hidden="true">
            <ScaleBar label="Typing" value={TYPING_WPM} max={200} />
            <ScaleBar label="You" value={stats.wpm ?? 0} max={200} highlight />
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
              shown.map((r, i) => <HistoryRow key={`${r.created_at}-${i}`} row={r} />)
            )}
          </div>
        </Card>
        </>
        )}
      </main>
    </div>
  );
}

/** Bytes as a person reads them. Two significant figures is enough at every size
 *  and stops the number changing width several times a second. */
function mb(bytes: number): string {
  // The threshold is 995 MB, not 1 GB, because the megabyte branch *rounds*:
  // 999,999,999 bytes rendered as "1000 MB" while the total beside it rendered
  // as "1.0 GB", so the progress line read "1000 MB of 1.0 GB". Switching units
  // at the point where rounding would first produce a four-digit number keeps
  // the two halves of that sentence in the same units.
  if (bytes >= 995e6) return `${(bytes / 1e9).toFixed(1)} GB`;
  return `${Math.round(bytes / 1e6)} MB`;
}

/** First run: the speech model is being fetched.
 *
 * This is the only moment OpenVoice uses the network, and it is the first thing a
 * new user ever sees — so it says plainly what is being downloaded, how far along
 * it is, and that it happens once. A silent multi-minute wait on first launch is
 * the single most likely reason someone deletes the app before using it.
 *
 * No spinner: a real byte count is both more honest and more reassuring than an
 * animation that conveys nothing.
 */
function FirstRun({ download }: { download: Download }) {
  const { done, total, model } = download;
  // `total` is 0 when the size lookup failed. Showing "NaN%" or a bar stuck at
  // zero would read as a hang, so fall back to counting up the bytes we do know.
  const known = total > 0;
  const pct = known ? Math.min(100, Math.round((done / total) * 100)) : 0;

  return (
    <div className="first-run">
      <h1 className="t-title">Setting up OpenVoice</h1>
      <p className="t-body first-run-lede">
        Downloading the <strong>{model}</strong> speech model. This happens once —
        afterwards OpenVoice works offline, and your voice never leaves this
        machine.
      </p>

      <div
        className="first-run-track"
        role="progressbar"
        aria-label={`Downloading the ${model} speech model`}
        aria-valuenow={known ? pct : undefined}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuetext={known ? `${pct} percent` : `${mb(done)} downloaded`}
      >
        <div
          className={known ? "first-run-fill" : "first-run-fill is-unknown"}
          style={known ? { width: `${pct}%` } : undefined}
        />
      </div>

      <div className="first-run-figures t-mono">
        <span>{known ? `${mb(done)} of ${mb(total)}` : mb(done)}</span>
        {known && <span>{pct}%</span>}
      </div>

      <p className="t-caption first-run-note">
        Leave this window open. You can carry on working — OpenVoice will be ready
        when the download finishes.
      </p>
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
  const missing = /sidecar|python|faster-whisper|not found/i.test(error);

  return (
    <Notice tone="danger">
      <div>
        <div className="t-body-strong" style={{ color: "var(--ink)" }}>
          {memory
            ? "Not enough memory to load the speech model"
            : missing
              ? "The speech engine could not be found"
              : "The speech engine could not start"}
        </div>
        <div className="t-caption" style={{ marginTop: 4, maxWidth: "70ch" }}>
          {memory ? (
            <>
              Close anything using the graphics card — a game, a video call, or
              another AI tool — and reopen OpenVoice. On a 4 GB card the model needs
              roughly 1.6 GB to itself. A smaller model is available in Models.
            </>
          ) : missing ? (
            <>Reinstall, or set OPENVOICE_ROOT to the OpenVoice folder.</>
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

function HistoryRow({ row }: { row: Row }) {
  // A badge on every row was noise, not signal: "delivered" is the outcome for
  // almost every dictation, so repeating it down the whole list said nothing a
  // reader didn't already assume. Green is reserved elsewhere in this app for
  // exactly one meaning -- the microphone is open -- and stacking it down every
  // history row diluted that into decoration. Silence now means "this worked
  // as expected"; a badge appears only when something didn't.
  const failed = row.outcome !== "delivered";
  return (
    <div className="row">
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
          <span className="t-caption">{when(row.created_at)}</span>
        </div>
      </div>
      <div className="row-actions">
        <Button size="sm" variant="ghost" onClick={() => navigator.clipboard?.writeText(row.final_text)}>
          Copy
        </Button>
      </div>
    </div>
  );
}

function when(ms: number): string {
  const mins = Math.round((Date.now() - ms) / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const h = Math.round(mins / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.round(h / 24)}d ago`;
}

export { elapsed };
