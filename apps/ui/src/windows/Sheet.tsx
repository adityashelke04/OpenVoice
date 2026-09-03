/** Component sheet — the Phase 2 deliverable.
 *
 * Every primitive, every state, on one page, so the system can be judged before
 * any screen is built on it. Not shipped in the app; reachable at `?window=sheet`.
 */

import { useEffect, useRef, useState } from "react";
import {
  Badge,
  Button,
  Card,
  Empty,
  FlowBar,
  Input,
  Kbd,
  LoadingDots,
  MicTestMeter,
  Notice,
  Select,
  Stat,
  Tabs,
  TickingEllipsis,
  Toggle,
  Waveform,
} from "../ui";
import "./sheet.css";

const SWATCHES: [string, string, string][] = [
  ["--canvas", "#000000", "page floor"],
  ["--surface-1", "#0a0a0b", "cards, panels"],
  ["--surface-2", "#101012", "hover, featured"],
  ["--surface-3", "#16161a", "inputs, menus"],
  ["--surface-4", "#1c1c21", "deepest lift"],
  ["--hairline", "#1f1f24", "1px divider"],
  ["--hairline-strong", "#2c2c33", "input edge"],
];

const INK: [string, string, string][] = [
  ["--ink", "#f5f5f5", "19.0:1"],
  ["--body", "#c9c9c9", "11.3:1"],
  ["--mute", "#8f8f8f", "5.6:1 · AA"],
  ["--faint", "#7a7a7a", "4.6:1 · AA · tertiary text"],
  // Split out from --faint when that token was found on placeholders, rule names
  // and raw error text — all of which a person has to read. WCAG exempts genuinely
  // disabled controls from the contrast floor; nothing carrying information may
  // use this.
  ["--disabled", "#5c5c5c", "disabled controls only"],
];

const TYPE: [string, string][] = [
  ["t-display", "32 / 600 / −0.96px"],
  ["t-title", "22 / 600 / −0.44px"],
  ["t-heading", "17 / 600 / −0.17px"],
  ["t-subheading", "15 / 500 / −0.1px"],
  ["t-body", "14 / 400 / 0"],
  ["t-body-strong", "14 / 500 / 0"],
  ["t-caption", "12 / 400 / 0"],
  ["t-label", "11 / 500 / +0.3px"],
  ["t-mono", "13 / 400 mono"],
  ["t-mono-lg", "20 / 500 / −0.2px mono"],
];

export function Sheet() {
  const [tab, setTab] = useState("All");
  const [on, setOn] = useState(true);
  const [live, setLive] = useState(false);
  const [level, setLevel] = useState(0);
  const raf = useRef(0);

  // Drive the waveform with speech-shaped noise so the ballistics are visible.
  useEffect(() => {
    if (!live) {
      setLevel(0);
      return;
    }
    const start = performance.now();
    const tick = () => {
      const t = performance.now() - start;
      // Speech-shaped, not a sine. Real dictation is bursts separated by pauses:
      // a slow phrase envelope that actually reaches zero, syllabic modulation
      // inside it, and per-sample jitter. A smooth sine makes every bar the same
      // height, which is why the first version looked like a row of pills.
      const phrase = Math.sin(t / 1700) * 0.5 + 0.5;
      const gate = phrase > 0.3 ? 1 : 0;
      const syllable = Math.abs(Math.sin(t / 105)) ** 0.7;
      const jitter = 0.7 + Math.random() * 0.3;
      setLevel(Math.min(1, gate * syllable * (0.3 + 0.7 * phrase) * jitter));
      raf.current = requestAnimationFrame(tick);
    };
    raf.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf.current);
  }, [live]);

  return (
    <div className="sheet">
      <header className="sheet-head">
        <div>
          <h1 className="t-display">OpenVoice design system</h1>
          <p className="t-body" style={{ marginTop: 6 }}>
            Phase 2 · every primitive and state. Palette pinned to{" "}
            <code className="t-mono" style={{ color: "var(--live)" }}>
              #44D62C
            </code>{" "}
            on{" "}
            <code className="t-mono">#000000</code>.
          </p>
        </div>
        <Badge tone="live" dot pulse={live}>
          {live ? "LISTENING" : "IDLE"}
        </Badge>
      </header>

      <Section title="Colour" note="Steps are tiny on purpose — the whole ladder spans 28 hex points. Dark UIs that jump #111→#222→#333 are the clearest marker of an amateur system.">
        <div className="sw-grid">
          {SWATCHES.map(([token, hex, use]) => (
            <div className="sw" key={token}>
              <span className="sw-chip" style={{ background: hex }} />
              <div>
                <div className="t-body-strong">{token}</div>
                <div className="t-caption">
                  {hex} · {use}
                </div>
              </div>
            </div>
          ))}
        </div>

        <div className="sw-grid" style={{ marginTop: 16 }}>
          {INK.map(([token, hex, ratio]) => (
            <div className="sw" key={token}>
              <span className="sw-chip" style={{ background: hex }} />
              <div>
                <div className="t-body-strong">{token}</div>
                <div className="t-caption">
                  {hex} · {ratio}
                </div>
              </div>
            </div>
          ))}
        </div>

        <div className="live-note">
          <span className="sw-chip" style={{ background: "var(--live)" }} />
          <div>
            <div className="t-body-strong" style={{ color: "var(--live)" }}>
              --live · #44D62C
            </div>
            <div className="t-caption">
              The only colour in the system. Means the microphone is open, or an
              action that opens it. Never success, never links, never emphasis —
              and never a large flat fill, because at full chroma on pure black it
              vibrates above button size.
            </div>
          </div>
        </div>
      </Section>

      <Section title="Type" note="Tracking follows the universal ≈ −0.03em rule and reaches exactly zero at body size. Weight never exceeds 600.">
        <div className="type-list">
          {TYPE.map(([cls, spec]) => (
            <div className="type-row" key={cls}>
              <span className={cls}>Dictate at the speed of thought</span>
              <span className="t-caption type-spec">
                .{cls} · {spec}
              </span>
            </div>
          ))}
        </div>
      </Section>

      <Section title="Buttons">
        <div className="hstack" style={{ flexWrap: "wrap" }}>
          <Button variant="primary">Primary</Button>
          <Button variant="secondary">Secondary</Button>
          <Button variant="ghost">Ghost</Button>
          <Button variant="danger">Delete</Button>
          <Button variant="record">
            <span className="dot" /> Record
          </Button>
          <Button variant="secondary" disabled>
            Disabled
          </Button>
          <Button variant="secondary" size="sm">
            Small
          </Button>
        </div>
        <p className="t-caption" style={{ marginTop: 12 }}>
          Primary is monochrome — the Raycast and Warp move. Spending the accent on
          ordinary buttons would waste the only signal that matters. Exactly one
          button is allowed to be green: the one that opens the microphone.
        </p>
      </Section>

      <Section title="Form controls">
        <div className="hstack" style={{ alignItems: "flex-end", flexWrap: "wrap" }}>
          <Input label="Search" placeholder="Search transcripts…" style={{ width: 240 }} />
          <Select
            label="Microphone"
            options={["Microphone Array (Realtek)", "External Microphone"]}
            style={{ width: 240 }}
          />
          <div className="field">
            <span className="t-label">Mic test meter</span>
            <MicTestMeter />
          </div>
          <div className="field">
            <span className="t-label">Launch at login</span>
            <Toggle on={on} onChange={setOn} label="Launch at login" />
          </div>
          <div className="field">
            <span className="t-label">Shortcut</span>
            <div className="hstack" style={{ gap: 4 }}>
              <Kbd>Right</Kbd>
              <Kbd>Ctrl</Kbd>
            </div>
          </div>
        </div>
      </Section>

      <Section title="Status">
        <div className="hstack" style={{ flexWrap: "wrap" }}>
          <Badge>default</Badge>
          <Badge tone="live" dot>
            delivered
          </Badge>
          <Badge tone="warn" dot>
            clipboard
          </Badge>
          <Badge tone="danger" dot>
            failed
          </Badge>
        </div>
        <div className="vstack" style={{ marginTop: 16 }}>
          <Notice action={<Button size="sm" variant="ghost">Dismiss</Button>}>
            Model <code className="t-mono">parakeet-tdt-0.6b-v2</code> loaded in 2.8 s.
          </Notice>
          <Notice tone="warn" action={<Button size="sm" variant="ghost">Paste</Button>}>
            Copied to clipboard — press <Kbd>Ctrl</Kbd> <Kbd>V</Kbd>
          </Notice>
          <Notice tone="danger">
            No speech detected — is your microphone muted?
          </Notice>
        </div>
      </Section>

      <Section title="Stats" note="Units are never omitted. A bare number on a dashboard invites misreading.">
        <div className="stat-grid">
          <Stat label="Total words" value="12,480" />
          <Stat label="Average speed" value="164" unit="wpm" />
          <Stat label="Median latency" value="664" unit="ms" tone="live" />
          <Stat label="Streak" value="9" unit="days" />
        </div>
      </Section>

      <Section
        title="Waveform"
        note="2px bars on a 4px pitch, driven by transform: scaleY() and interpolated every frame in JavaScript. No CSS transition — a transition chasing a 30 Hz signal never arrives, which is what made the first attempt feel mushy."
      >
        <div className="wave-demo">
          <Button variant={live ? "secondary" : "record"} onClick={() => setLive(!live)}>
            {live ? "Stop" : <><span className="dot" /> Start</>}
          </Button>
          <span className="t-caption">Real size, as it appears in the Flow Bar</span>
        </div>

        <div className="wave-row">
          <div className="wave-actual">
            <Waveform level={level} idle={!live} bars={32} />
          </div>
        </div>

        <p className="t-caption wave-cap">Magnified 3× for inspection</p>
        <div className="wave-row">
          <div className="wave-zoom">
            <Waveform level={level} idle={!live} bars={32} />
          </div>
        </div>
      </Section>

      <Section title="Rows and tabs">
        <Card
          title="History"
          action={<Tabs value={tab} onChange={setTab} options={["All", "Editor", "Terminal"]} />}
        >
          <div style={{ margin: "0 -16px -16px" }}>
            {[
              ["Thanks for sending that over — I'll take a look this afternoon.", "Slack", "612 ms", "live"],
              ["kubectl get pods --all-namespaces", "Terminal", "448 ms", "live"],
              ["Remind me to book the dentist before the end of the month", "Notes", "1104 ms", "warn"],
            ].map(([text, app, ms, tone]) => (
              <div className="row" key={text}>
                <div>
                  <div className="t-mono" style={{ color: "var(--ink)" }}>
                    {text}
                  </div>
                  <div className="hstack" style={{ marginTop: 6 }}>
                    <Badge tone={tone as "live" | "warn"} dot>
                      {tone === "live" ? "delivered" : "clipboard"}
                    </Badge>
                    <span className="t-caption">{app}</span>
                    <span className="t-caption">{ms}</span>
                  </div>
                </div>
                <div className="row-actions">
                  <Button size="sm" variant="ghost">Insert</Button>
                  <Button size="sm" variant="ghost">Copy</Button>
                </div>
              </div>
            ))}
          </div>
        </Card>
      </Section>

      <Section title="Empty state">
        <Card>
          <Empty
            title="No dictation yet"
            hint="Hold Right Ctrl anywhere and speak. Your words land at the cursor, and every session shows up here."
            action={<Button variant="record"><span className="dot" /> Try it now</Button>}
          />
        </Card>
      </Section>

      <Section
        title="Kinetic Loading"
        note="GPU-accelerated 3-dot harmonic wave bounce and kinetic ticking ellipsis. Transforms and opacity only — zero layout reflows."
      >
        <div className="vstack" style={{ gap: 20 }}>
          <div>
            <div className="t-label" style={{ marginBottom: 8 }}>LoadingDots Sizes & Variants</div>
            <div className="hstack" style={{ gap: 24, flexWrap: "wrap", alignItems: "center" }}>
              <div className="hstack" style={{ gap: 8 }}>
                <span className="t-caption">xs</span>
                <LoadingDots size="xs" tone="body" variant="wave" />
              </div>
              <div className="hstack" style={{ gap: 8 }}>
                <span className="t-caption">sm</span>
                <LoadingDots size="sm" tone="body" variant="wave" />
              </div>
              <div className="hstack" style={{ gap: 8 }}>
                <span className="t-caption">md</span>
                <LoadingDots size="md" tone="live" variant="wave" />
              </div>
              <div className="hstack" style={{ gap: 8 }}>
                <span className="t-caption">lg</span>
                <LoadingDots size="lg" tone="warn" variant="wave" />
              </div>
              <div className="hstack" style={{ gap: 8 }}>
                <span className="t-caption">pulse</span>
                <LoadingDots size="md" tone="danger" variant="pulse" />
              </div>
            </div>
          </div>

          <div>
            <div className="t-label" style={{ marginBottom: 8 }}>Ticking Ellipsis</div>
            <div className="vstack" style={{ gap: 10 }}>
              <div className="t-body">
                <TickingEllipsis text="Writing" tone="body" />
              </div>
              <div className="t-body">
                <TickingEllipsis text="Starting the speech engine" tone="warn" />
              </div>
              <div className="t-body">
                <TickingEllipsis text="Getting the speech model" suffix=" 43%" tone="warn" />
              </div>
              <div className="t-body">
                <TickingEllipsis text="Transcribing audio" tone="live" />
              </div>
            </div>
          </div>

          <div>
            <div className="t-label" style={{ marginBottom: 8 }}>Flow Bar Loading States</div>
            <div className="hstack" style={{ gap: 16, flexWrap: "wrap", alignItems: "center" }}>
              <div style={{ width: 170, height: 40 }}>
                <FlowBar live={false} working elapsed="0:00" />
              </div>
              <div style={{ width: 224, height: 40 }}>
                <FlowBar live={false} status="loading" elapsed="0:00" />
              </div>
              <div style={{ width: 252, height: 40 }}>
                <FlowBar live={false} status="loading" progress={0.68} elapsed="0:00" />
              </div>
              <div style={{ width: 44, height: 22 }}>
                <FlowBar live={false} mini working elapsed="0:00" />
              </div>
            </div>
          </div>
        </div>
      </Section>

      <Section
        title="The Flow Bar"
        note="The only interface visible while you dictate, so everything else was cut. It sits at 148px idle and grows to 216px while listening — the growth itself confirms the key registered, readable before any glyph is. Press Start above to watch it."
      >
        <div className="bar-demo">
          <FlowBar live={live} level={level} elapsed={live ? "0:04" : "0:00"} />
        </div>
        <p className="t-caption" style={{ marginTop: 12, maxWidth: "68ch" }}>
          The wave is a scrolling seismograph, not an equaliser. The newest sample
          enters at the right and the rest shift left, so a syllable becomes a shape
          that travels — you can watch the sentence you just said move across the
          bar. Equaliser bars jitter in place and tell a person nothing.
        </p>
      </Section>
    </div>
  );
}

function Section({
  title,
  note,
  children,
}: {
  title: string;
  note?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="sheet-section">
      <div className="sheet-section-head">
        <h2 className="t-title">{title}</h2>
        {note && <p className="t-caption sheet-note">{note}</p>}
      </div>
      {children}
    </section>
  );
}
