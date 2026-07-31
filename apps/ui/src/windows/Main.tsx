/** The main panel window.
 *
 * Opened rarely and deliberately, to inspect or fix something. So it optimises for
 * legibility and sourcing over expression: every value carries its unit, every state
 * carries its legend, and nothing is coloured unless it is reporting machine state. */

import { useState } from "react";
import {
  Button,
  Capsule,
  Meter,
  Readout,
  Section,
  Station,
  Toggle,
} from "../components/Panel";
import {
  formatElapsed,
  stateLegend,
  stateSignal,
  type EngineView,
} from "../engine/useEngine";
import { STAGE_BUDGET_MS, type Stage, type Utterance } from "../engine/types";
import type { MockEngine } from "../engine/mockEngine";
import "./main.css";

const TABS = [
  "dictate",
  "history",
  "dictionary",
  "profiles",
  "models",
  "settings",
  "debug",
] as const;
type Tab = (typeof TABS)[number];

export function Main({
  view,
  engine,
  dismissNotice,
}: {
  view: EngineView;
  engine: MockEngine;
  dismissNotice: () => void;
}) {
  const [tab, setTab] = useState<Tab>("dictate");
  const signal = stateSignal(view.state);

  return (
    <div className="app">
      <Annunciator view={view} />

      <div className="body">
        <nav className="rail" role="tablist" aria-label="Panel sections">
          {TABS.map((t) => (
            <button
              key={t}
              className="rail-tab"
              role="tab"
              aria-selected={tab === t}
              data-lamp={t === "dictate" && signal !== "off" ? signal : undefined}
              onClick={() => setTab(t)}
            >
              <span className="rail-lamp" aria-hidden="true" />
              {t}
            </button>
          ))}
          <div className="rail-foot">
            <span className="legend-sm" style={{ display: "block", marginBottom: 6 }}>
              Local only
            </span>
            <Capsule legend="No network" state="green" />
          </div>
        </nav>

        <main className="surface">
          {view.notice && (
            <div className="notice" role="status">
              <Capsule
                legend={view.notice.level}
                state={view.notice.level === "error" ? "red" : "amber"}
              />
              <span className="notice-text">{view.notice.message}</span>
              <Button onClick={dismissNotice}>Dismiss</Button>
            </div>
          )}

          {tab === "dictate" && <Dictate view={view} engine={engine} />}
          {tab === "history" && <History history={view.history} />}
          {tab === "dictionary" && <Dictionary />}
          {tab === "profiles" && <Profiles />}
          {tab === "models" && <Models />}
          {tab === "settings" && <Settings />}
          {tab === "debug" && <Debug view={view} />}
        </main>
      </div>
    </div>
  );
}

/* -- Annunciator row --------------------------------------------------------- */

function Annunciator({ view }: { view: EngineView }) {
  const signal = stateSignal(view.state);
  return (
    <header className="annunciator">
      <Capsule legend="Standby" state={view.state === "idle" ? "green" : "off"} />
      <Capsule legend="Listening" state={view.state === "listening" ? "green" : "off"} />
      <Capsule
        legend="Decoding"
        state={view.state === "transcribing" ? "amber" : "off"}
      />
      <Capsule legend="Injecting" state={view.state === "injecting" ? "amber" : "off"} />
      <Capsule legend="Fault" state={view.state === "fault" ? "red" : "off"} />

      <span className="annunciator-spacer" />

      <div className="annunciator-meta">
        <div className="meta-item">
          <span className="meta-label">Model</span>
          <span className="data" style={{ fontSize: 11 }}>
            large-v3-turbo
          </span>
        </div>
        <div className="meta-item">
          <span className="meta-label">Device</span>
          <Readout value="CUDA" tone={signal === "red" ? "red" : "green"} />
        </div>
        <div className="meta-item">
          <span className="meta-label">VRAM</span>
          <Readout value="1.6" unit="GB" />
        </div>
      </div>
    </header>
  );
}

/* -- DICTATE ----------------------------------------------------------------- */

function Dictate({ view, engine }: { view: EngineView; engine: MockEngine }) {
  const signal = stateSignal(view.state);
  const total = view.timings.find((t) => t.stage === "total");

  return (
    <>
      <div className="live">
        <div className="live-meter">
          <Meter level={view.level} peak={view.peak} />
        </div>
        <div className="live-main">
          <div>
            <div className="live-state" data-signal={signal}>
              {stateLegend(view.state)}
            </div>
            <div className="legend-sm" style={{ marginTop: 6 }}>
              Profile · {view.profile}
            </div>
          </div>

          <div className="live-transcript" data-empty={!view.lastText}>
            {view.lastText || "Hold Right Ctrl and speak. Text lands at your caret."}
          </div>

          <div className="live-stats">
            <Stat label="Elapsed" value={formatElapsed(view.elapsedMs)} />
            <Stat
              label="Last latency"
              value={view.lastLatencyMs ?? "—"}
              unit={view.lastLatencyMs ? "ms" : undefined}
              tone={
                view.lastLatencyMs && view.lastLatencyMs > STAGE_BUDGET_MS.total
                  ? "amber"
                  : "green"
              }
            />
            <Stat label="Sessions" value={view.sessionCount} />
            <Stat label="Budget" value={total ? "met" : "—"} />
          </div>
        </div>
      </div>

      <Section legend="Simulate">
        <Station
          legend="Dictation session"
          hint="The Rust engine is not wired up yet. These replay the real event stream with measured timings so the panel can be reviewed."
        >
          <div style={{ display: "flex", gap: 4 }}>
            <Button variant="primary" onClick={() => engine.simulate()}>
              Run
            </Button>
            <Button onClick={() => engine.simulateClipboardFallback()}>
              Fallback
            </Button>
            <Button onClick={() => engine.simulateSilent()}>Muted</Button>
            <Button onClick={() => engine.stop()}>Reset</Button>
          </div>
        </Station>
      </Section>

      <Section legend="Recent">
        {view.history.slice(0, 3).map((u) => (
          <UtteranceRow key={u.id} u={u} />
        ))}
      </Section>
    </>
  );
}

function Stat({
  label,
  value,
  unit,
  tone,
}: {
  label: string;
  value: string | number;
  unit?: string;
  tone?: "green" | "amber" | "red";
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
      <span className="meta-label">{label}</span>
      <Readout value={value} unit={unit} tone={tone} />
    </div>
  );
}

/* -- HISTORY ----------------------------------------------------------------- */

function History({ history }: { history: Utterance[] }) {
  const [q, setQ] = useState("");
  const shown = history.filter(
    (u) =>
      !q ||
      u.finalText.toLowerCase().includes(q.toLowerCase()) ||
      u.targetApp.toLowerCase().includes(q.toLowerCase()),
  );

  return (
    <Section
      legend="History"
      action={
        <input
          className="pinput"
          placeholder="Search transcripts"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          style={{ width: 200 }}
        />
      }
    >
      {shown.length === 0 ? (
        <p className="empty">
          {q ? `Nothing matches “${q}”.` : "No dictation yet."}
        </p>
      ) : (
        shown.map((u) => <UtteranceRow key={u.id} u={u} showRaw />)
      )}
    </Section>
  );
}

function UtteranceRow({ u, showRaw }: { u: Utterance; showRaw?: boolean }) {
  const failed = u.status !== "delivered";
  return (
    <div className="utt">
      <div style={{ minWidth: 0 }}>
        <div className="utt-text">{u.finalText}</div>
        {showRaw && u.rawText !== u.finalText && (
          <div className="utt-raw">raw · {u.rawText}</div>
        )}
        <div className="utt-meta">
          <Capsule
            legend={failed ? u.status.replace(/_/g, " ") : "delivered"}
            state={failed ? "amber" : "green"}
          />
          <span className="spoken">{u.targetApp}</span>
          <span className="spoken">{u.profile}</span>
          <Readout value={u.latencyMs} unit="ms" />
          <Readout value={(u.durationMs / 1000).toFixed(1)} unit="s" />
        </div>
      </div>
      <div className="utt-actions">
        <Button>Insert</Button>
        <Button>Copy</Button>
      </div>
    </div>
  );
}

/* -- DICTIONARY -------------------------------------------------------------- */

const DICT = [
  { spoken: "use effect", written: "useEffect", group: "code" },
  { spoken: "cube control · cube cuddle", written: "kubectl", group: "shell" },
  { spoken: "engine x", written: "nginx", group: "shell" },
  { spoken: "sir day", written: "serde", group: "code" },
  { spoken: "tokyo", written: "tokio", group: "code" },
  { spoken: "sequel", written: "SQL", group: "code" },
];

function Dictionary() {
  return (
    <>
      <Section
        legend="Vocabulary"
        action={<Button variant="primary">Add term</Button>}
      >
        <div className="dict-row" style={{ borderBottomColor: "var(--seam-lit)" }}>
          <span className="meta-label">Heard as</span>
          <span className="meta-label">Written</span>
          <span className="meta-label">Group</span>
        </div>
        {DICT.map((d) => (
          <div className="dict-row" key={d.written}>
            <span className="spoken">{d.spoken}</span>
            <span className="written">{d.written}</span>
            <Capsule legend={d.group} />
          </div>
        ))}
      </Section>

      <Section legend="How this works">
        <div className="section-body body" style={{ maxWidth: "68ch" }}>
          Terms are used twice. They are fed into the model's initial prompt so it
          gets them right while decoding, and they repair the transcript afterwards
          if it still got them wrong. The first pass is the one that matters —
          the decoder still has the audio, which post-processing has thrown away.
        </div>
      </Section>
    </>
  );
}

/* -- PROFILES ---------------------------------------------------------------- */

const PROFILES = [
  {
    name: "terminal",
    apps: "WindowsTerminal.exe, powershell.exe, cmd.exe",
    rules: "lowercase first · no trailing period · shell + code vocabulary",
  },
  {
    name: "editor",
    apps: "Code.exe, Cursor.exe, idea64.exe",
    rules: "sentence case · no trailing period · code vocabulary",
  },
  {
    name: "prose",
    apps: "slack.exe, chrome.exe, Notion.exe",
    rules: "sentence case · trailing period · aggressive filler removal",
  },
];

function Profiles() {
  return (
    <Section legend="Profiles" action={<Button variant="primary">New profile</Button>}>
      {PROFILES.map((p) => (
        <Station key={p.name} legend={p.name} hint={`${p.apps} — ${p.rules}`}>
          <Button>Edit</Button>
        </Station>
      ))}
    </Section>
  );
}

/* -- MODELS ------------------------------------------------------------------ */

const MODELS = [
  { id: "large-v3-turbo", size: "1.6 GB", vram: "1.6 GB", note: "Default", on: true },
  { id: "small.en", size: "250 MB", vram: "0.6 GB", note: "Battery", on: false },
  { id: "base.en", size: "75 MB", vram: "CPU ok", note: "Fallback", on: false },
];

function Models() {
  return (
    <>
      <Section legend="Models">
        {MODELS.map((m) => (
          <Station key={m.id} legend={m.id} hint={`${m.size} on disk · ${m.vram} VRAM`}>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <Capsule legend={m.note} state={m.on ? "green" : "off"} />
              <Button disabled={m.on}>{m.on ? "Loaded" : "Load"}</Button>
            </div>
          </Station>
        ))}
      </Section>
      <Section legend="Measured on this machine">
        <Station legend="Model load, warm cache">
          <Readout value="1.4" unit="s" tone="green" />
        </Station>
        <Station
          legend="Model load, network reachable"
          hint="huggingface_hub revalidates a cached model over the network. Offline mode is on by default; downloads go through the model manager."
        >
          <Readout value="171" unit="s" tone="amber" />
        </Station>
        <Station legend="Decode, 5 s utterance">
          <Readout value="188" unit="ms" tone="green" />
        </Station>
      </Section>
    </>
  );
}

/* -- SETTINGS ---------------------------------------------------------------- */

function Settings() {
  const [retain, setRetain] = useState(false);
  const [launch, setLaunch] = useState(true);
  const [sound, setSound] = useState(false);

  return (
    <>
      <Section legend="Activation">
        <Station legend="Hotkey" hint="Hold to speak, release to transcribe.">
          <span className="data">Right Ctrl</span>
        </Station>
        <Station
          legend="Minimum press"
          hint="Shorter presses are discarded as accidental taps, without a notification."
        >
          <Readout value="300" unit="ms" />
        </Station>
        <Station
          legend="Maximum recording"
          hint="A stuck key cannot record indefinitely. The audio is kept, not discarded."
        >
          <Readout value="120" unit="s" />
        </Station>
        <Station
          legend="Pre-roll"
          hint="Audio retained from before the key registered, which prevents clipped first syllables."
        >
          <Readout value="200" unit="ms" />
        </Station>
      </Section>

      <Section legend="Privacy">
        <Station
          legend="Retain audio"
          hint="Off: audio is held in RAM and dropped after transcription. For debugging only."
        >
          <Toggle on={retain} onChange={setRetain} label="Retain audio on disk" />
        </Station>
        <Station legend="History retention">
          <Readout value="90" unit="days" />
        </Station>
        <Station
          legend="Telemetry"
          hint="There is none. Not disabled — absent from the codebase, kept absent by a CI job."
        >
          <Capsule legend="None" state="green" />
        </Station>
      </Section>

      <Section legend="Behaviour">
        <Station legend="Launch at login">
          <Toggle on={launch} onChange={setLaunch} label="Launch at login" />
        </Station>
        <Station legend="Sound cue">
          <Toggle on={sound} onChange={setSound} label="Sound cue" />
        </Station>
      </Section>
    </>
  );
}

/* -- DEBUG ------------------------------------------------------------------- */

const FORMAT_TRACE = [
  { stage: "parse", text: "um so we need to call use effect here comma then return null" },
  { stage: "fillers", text: "so we need to call use effect here comma then return null" },
  { stage: "commands", text: "so we need to call use effect here, then return null" },
  { stage: "dictionary", text: "so we need to call useEffect here, then return null" },
  { stage: "case", text: "so we need to call useEffect here, then return null" },
  { stage: "capitalize", text: "So we need to call useEffect here, then return null" },
  { stage: "profile", text: "So we need to call useEffect here, then return null" },
];

function Debug({ view }: { view: EngineView }) {
  const timings = view.timings.length
    ? view.timings
    : ([
        { stage: "finalize", tookMs: 8 },
        { stage: "vad", tookMs: 18 },
        { stage: "decode", tookMs: 441 },
        { stage: "format", tookMs: 3 },
        { stage: "inject", tookMs: 96 },
        { stage: "total", tookMs: 566 },
      ] as { stage: Stage; tookMs: number }[]);

  const scale = Math.max(
    ...timings.map((t) => Math.max(t.tookMs, STAGE_BUDGET_MS[t.stage])),
  );

  return (
    <>
      <Section legend="Latency waterfall">
        {timings.map((t) => {
          const budget = STAGE_BUDGET_MS[t.stage];
          const over = t.tookMs > budget;
          return (
            <div className="wf" key={t.stage}>
              <span className="legend-sm">{t.stage}</span>
              <div className="wf-track">
                <div
                  className="wf-bar"
                  data-over={over}
                  style={{ width: `${(t.tookMs / scale) * 100}%` }}
                />
                <div
                  className="wf-budget"
                  style={{ left: `${(budget / scale) * 100}%` }}
                  title={`budget ${budget} ms`}
                />
              </div>
              <div style={{ textAlign: "right" }}>
                <Readout value={t.tookMs} unit="ms" tone={over ? "amber" : undefined} />
              </div>
            </div>
          );
        })}
        <div className="section-body">
          <span className="station-hint">
            The thin vertical line is the stage's budget from the architecture doc.
            Every session is measured, not only slow ones — a regression is only
            visible if the baseline was recorded too.
          </span>
        </div>
      </Section>

      <Section legend="Formatter trace">
        {FORMAT_TRACE.map((s, i) => {
          const changed = i > 0 && FORMAT_TRACE[i - 1].text !== s.text;
          return (
            <div className="dict-row" key={s.stage}>
              <span className="legend-sm">{s.stage}</span>
              <span className={changed ? "written" : "spoken"}>{s.text}</span>
              <Capsule legend={changed ? "changed" : "no-op"} state={changed ? "green" : "off"} />
            </div>
          );
        })}
        <div className="section-body">
          <span className="station-hint">
            Output after every rule. “The formatter did something weird” is otherwise
            an unfalsifiable bug report; with this the offending rule is obvious in
            about ten seconds.
          </span>
        </div>
      </Section>
    </>
  );
}
