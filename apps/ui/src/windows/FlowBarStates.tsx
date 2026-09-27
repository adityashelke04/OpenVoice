/** Flow Bar review surface — every state, over every kind of backdrop.
 *
 * `?window=flowbar`. Not part of the app; the sibling of `?window=sheet`, and it
 * exists for the same reason: several Flow Bar states cannot be reached without
 * reproducing a real failure, a real completion, or a real microphone.
 *
 * WHY THE BACKDROPS. The Flow Bar is the one surface that leaves this design
 * system's world — it floats over a white document, a dark editor, a photo,
 * whatever the user happens to be looking at. A review that only ever shows it
 * on the app's own black canvas cannot answer the only question that matters:
 * does it read there? So each state is rendered over all four plates at once,
 * and a border or glow that works on black and vanishes on white is visible as
 * a defect rather than passing unnoticed.
 *
 * The waveform is driven by a synthetic speech envelope rather than a fixed
 * level, because a flat level renders a flat wave and a screenshot of that
 * misrepresents the component. See `useSpokenEnvelope`.
 */

import { useEffect, useRef } from "react";
import { FlowBar } from "../ui";
import { FlowIcon } from "../ui/flowbar/icons";
import { geometry } from "./geometry";
import type { GeometryInput } from "./geometry";
import { flowMenuRows } from "./useFlowMenu";
import { useFontsReady } from "./useFontsReady";
// The right-click menu's styles live with the overlay window. Imported so the
// edge-case section below can show the menu as it actually renders rather than
// as an approximation of it.
import "./overlay.css";
import "./flowbar-states.css";

/**
 * A level ref that moves the way speech does.
 *
 * Syllables, not noise: a carrier around 3.6 Hz gated by a slower phrase
 * envelope, so the wave shows bursts separated by breaths. A random walk reads
 * as static and a sine reads as a test tone; neither looks like a person
 * talking, which is what this component is for.
 */
function useSpokenEnvelope(active: boolean) {
  const level = useRef(0);

  useEffect(() => {
    if (!active) {
      level.current = 0;
      return;
    }
    let raf = 0;
    const t0 = performance.now();

    const tick = (now: number) => {
      raf = requestAnimationFrame(tick);
      const t = (now - t0) / 1000;
      // Phrase: ~5s of speech, then a pause. Syllables: ~3.6 Hz within it.
      const phrase = Math.max(0, Math.sin(t * 0.42) * 0.7 + 0.45);
      const syllable = 0.5 + 0.5 * Math.sin(t * 3.6 * Math.PI * 2 * 0.5);
      const jitter = 0.85 + 0.15 * Math.sin(t * 11.3);
      level.current = Math.min(1, phrase * syllable * jitter);
    };

    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [active]);

  return level;
}

/** The plates the bar has to survive. */
const PLATES = [
  { id: "document", label: "White document", hint: "Word, a browser, a PDF" },
  { id: "editor", label: "Dark editor", hint: "VS Code, a terminal" },
  { id: "photo", label: "Busy image", hint: "a video call, a photo" },
  { id: "canvas", label: "App canvas", hint: "the system's own black" },
] as const;

/**
 * One state, at the width the window actually gives it.
 *
 * The width tiers are not incidental — the bar growing is the confirmation that
 * the key registered, and it is meant to be readable before any glyph or colour
 * resolves. A review surface that rendered every state at one width would hide
 * the single most important signal this component has, so the tiers here mirror
 * `Overlay.tsx`'s own numbers exactly.
 */
function Row({
  label,
  width,
  height = 40,
  freeze,
  hover,
  children,
}: {
  label: string;
  width: number;
  /** The pill fills its window, and on this surface the row is the window. */
  height?: number;
  /** Hold a one-shot animation open so it can be seen and captured. */
  freeze?: boolean;
  /** Hold the bar in its hover look. */
  hover?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className="fbs-row" data-freeze={freeze} data-hover={hover}>
      <span className="fbs-row-label">{label}</span>
      <span className="fbs-row-width">
        {width}x{height}
      </span>
      <div className="fbs-row-bar" style={{ width, height }}>
        {children}
      </div>
    </div>
  );
}

/** The box `geometry()` gives a state, so every row here is the size the real
 *  window would be clipped to — never a second opinion typed in by hand. */
const box = (v: Partial<GeometryInput> & Pick<GeometryInput, "mode">) =>
  geometry({ mini: false, edge: "bottom", menu: false, ...v });

function size(g: { w: number; h: number }) {
  return { width: g.w, height: g.h };
}

/** The Paste chip the overlay hands the bar on a clipboard fallback. */
const paste = (
  <button type="button" className="flowbar-btn">
    Paste <kbd>Ctrl V</kbd>
  </button>
);

const noop = () => undefined;

export function FlowBarStates() {
  const live = useSpokenEnvelope(true);
  // Re-render once the bar's faces have loaded, so the boxes below are measured
  // with the font that paints them — the overlay waits for the same signal.
  // Measured with the fallback, a message box comes out a few pixels short and
  // truncates here while fitting in the app.
  useFontsReady();
  const rows = flowMenuRows(
    { mini: false, live: false, working: false, autoCollapse: true },
    { call: noop, close: noop, setMini: noop, setAutoCollapse: noop },
  );
  const clip = "Copied to clipboard";
  const failed = "No text was produced";

  return (
    <div className="fbs-root">
      <header className="fbs-head">
        <h1 className="fbs-title">The Flow Bar</h1>
        <p className="fbs-deck">
          Every state, over the surfaces it actually floats above, each at the size the real
          window is clipped to. The bar is the same in every Hub theme: colour appears only
          while the microphone is open.
        </p>
      </header>

      {PLATES.map((plate) => (
        <section key={plate.id} className="fbs-plate-section">
          <div className="fbs-plate-head">
            <span className="fbs-plate-label">{plate.label}</span>
            <span className="fbs-plate-hint">{plate.hint}</span>
          </div>
          <div className="fbs-plate" data-plate={plate.id}>
            <Row label="Idle" {...size(box({ mode: "idle" }))}>
              <FlowBar live={false} elapsed="0:00" onToggle={noop} />
            </Row>
            {/* Held in its hover look: the mark warms into the spectrum to say
                the bar can be clicked. */}
            <Row label="Idle, hover" hover {...size(box({ mode: "idle" }))}>
              <FlowBar live={false} elapsed="0:00" onToggle={noop} />
            </Row>
            <Row label="Listening" {...size(box({ mode: "live" }))}>
              <FlowBar live levelRef={live} elapsed="0:04" onCancel={noop} />
            </Row>
            {/* Hands-free and held must never read the same: only one of them
                keeps recording when you let go. */}
            <Row label="Hands-free" {...size(box({ mode: "live" }))}>
              <FlowBar live latched levelRef={live} elapsed="1:12" onCancel={noop} />
            </Row>
            <Row label="Writing" {...size(box({ mode: "working" }))}>
              <FlowBar live={false} working elapsed="0:00" />
            </Row>
            <Row label="Landed" freeze {...size(box({ mode: "idle" }))}>
              <FlowBar live={false} confirm elapsed="0:00" onToggle={noop} />
            </Row>
            <Row label="Clipboard" {...size(box({ mode: "notice", text: clip, hasAction: true }))}>
              <FlowBar live={false} message={clip} action={paste} elapsed="0:00" />
            </Row>
            <Row label="Failed" {...size(box({ mode: "failed", text: failed }))}>
              <FlowBar live={false} failed message={failed} elapsed="0:00" />
            </Row>
            <Row label="Discarded" {...size(box({ mode: "notice", text: "Discarded" }))}>
              <FlowBar live={false} message="Discarded" elapsed="0:00" />
            </Row>
            <Row
              label="Starting"
              {...size(box({ mode: "loading", text: "Starting the speech engine…" }))}
            >
              <FlowBar live={false} status="loading" elapsed="0:00" />
            </Row>
            <Row
              label="Downloading"
              {...size(box({ mode: "loading", text: "Getting the speech model… 43%" }))}
            >
              <FlowBar live={false} status="loading" progress={0.43} elapsed="0:00" />
            </Row>
            <Row
              label="Engine down"
              {...size(
                box({ mode: "enginefail", text: "Speech engine unavailable — open OpenVoice" }),
              )}
            >
              <FlowBar live={false} status="error" elapsed="0:00" />
            </Row>
            <Row label="Compact" {...size(box({ mode: "idle", mini: true }))}>
              <FlowBar live={false} mini elapsed="0:00" />
            </Row>
            <Row label="Compact, live" {...size(box({ mode: "live", mini: true }))}>
              <FlowBar live mini levelRef={live} elapsed="0:04" />
            </Row>
            <Row label="Compact, writing" {...size(box({ mode: "working", mini: true }))}>
              <FlowBar live={false} mini working elapsed="0:00" />
            </Row>
            {/* Not a `FlowBar`: the collapsed bar is a stroke painted by
                `overlay.css`, so this reproduces the overlay's own markup. */}
            <Row label="Put away" {...size(box({ mode: "idle", collapsed: true }))}>
              <div
                className="overlay-hit"
                data-collapsed="true"
                style={{ width: "100%", height: "100%" }}
              >
                <span className="flowbar-line" aria-hidden />
              </div>
            </Row>
            <Row label="Docked" {...size(box({ mode: "idle", edge: "left" }))}>
              <FlowBar live={false} edge="left" elapsed="0:00" />
            </Row>
            <Row label="Docked, live" {...size(box({ mode: "live", edge: "left" }))}>
              <FlowBar live edge="left" levelRef={live} elapsed="0:04" />
            </Row>
            <Row label="Docked, writing" {...size(box({ mode: "working", edge: "left" }))}>
              <FlowBar live={false} edge="left" working elapsed="0:00" />
            </Row>
          </div>
        </section>
      ))}

      {/* ------------------------------------------------------------ menu -- */}
      <section className="fbs-plate-section">
        <div className="fbs-plate-head">
          <span className="fbs-plate-label">The Flow Menu</span>
          <span className="fbs-plate-hint">right-click the bar; these are the real rows</span>
        </div>
        <div className="fbs-plate" data-plate="editor">
          <div className="fbs-menu">
            <div className="overlay-menu" role="presentation" style={{ width: 280 }}>
              {rows.map((r) => (
                <div key={r.id}>
                  {r.sep && <div className="overlay-menu-sep" />}
                  <button type="button" data-row={r.id}>
                    <FlowIcon name={r.icon} />
                    <span>{r.label}</span>
                    {r.id === "dictate" && <kbd>Right Ctrl</kbd>}
                  </button>
                </div>
              ))}
            </div>
            <div style={{ width: 280, height: 40 }}>
              <FlowBar live={false} elapsed="0:00" onToggle={noop} />
            </div>
          </div>
        </div>
      </section>

      {/* ------------------------------------------------------- real size -- */}
      <section className="fbs-plate-section">
        <div className="fbs-plate-head">
          <span className="fbs-plate-label">Actual size</span>
          <span className="fbs-plate-hint">
            everything above is magnified; this is what you glance at
          </span>
        </div>
        <div className="fbs-truth">
          <p className="fbs-truth-copy">
            The quarterly review is attached. I have summarised the three points that came up
            in the meeting and flagged the one that needs a decision before Friday. Let me
            know if you would like the underlying figures as well.
          </p>
          <div className="fbs-truth-bar">
            <div style={{ width: 240, height: 40 }}>
              <FlowBar live levelRef={live} elapsed="0:04" onCancel={noop} />
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
