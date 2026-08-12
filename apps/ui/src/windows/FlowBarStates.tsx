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
  freeze,
  children,
}: {
  label: string;
  width: number;
  /** Hold a one-shot animation open so it can be seen and captured. */
  freeze?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className="fbs-row" data-freeze={freeze}>
      <span className="fbs-row-label">{label}</span>
      <span className="fbs-row-width">{width}px</span>
      <div className="fbs-row-bar" style={{ width }}>
        {children}
      </div>
    </div>
  );
}

export function FlowBarStates() {
  const live = useSpokenEnvelope(true);

  return (
    <div className="fbs-root">
      <header className="fbs-head">
        <h1 className="fbs-title">The Flow Bar</h1>
        <p className="fbs-deck">
          Every state, over the surfaces it actually floats above. This window is a review
          surface, not part of the app — the states below are rendered directly, because
          reaching them for real needs a microphone, a failed injection, and a completed
          dictation respectively.
        </p>
      </header>

      {PLATES.map((plate) => (
        <section key={plate.id} className="fbs-plate-section">
          <div className="fbs-plate-head">
            <span className="fbs-plate-label">{plate.label}</span>
            <span className="fbs-plate-hint">{plate.hint}</span>
          </div>

          <div className="fbs-plate" data-plate={plate.id}>
            <Row label="Idle" width={150}>
              <FlowBar live={false} elapsed="0:00" />
            </Row>
            <Row label="Listening" width={240}>
              <FlowBar live levelRef={live} elapsed="0:04" onCancel={() => undefined} />
            </Row>
            <Row label="Working" width={170}>
              <FlowBar live={false} working elapsed="0:00" />
            </Row>
            {/* The landing. Momentary in the app — a single ring, 340ms — so the
                review surface holds it open at the point the ring is widest;
                otherwise every screenshot of it would be of the frame after it
                finished. The row below it runs at real speed. */}
            <Row label="Landed" width={150} freeze>
              <FlowBar live={false} confirm elapsed="0:00" />
            </Row>
            <Row label="Clipboard" width={248}>
              <FlowBar live={false} elapsed="0:00" message="Copied to clipboard — press Ctrl+V" />
            </Row>
            <Row label="Failed" width={248}>
              <FlowBar live={false} failed elapsed="0:00" message="No text was produced" />
            </Row>
          </div>
        </section>
      ))}

      {/* ---------------------------------------------------------- motion -- */}
      <section className="fbs-plate-section" id="fbs-landing">
        <div className="fbs-plate-head">
          <span className="fbs-plate-label">The landing, frame by frame</span>
          <span className="fbs-plate-hint">420ms, held open at six points</span>
        </div>
        <div className="fbs-plate" data-plate="canvas">
          <div className="fbs-film">
            {[0, 60, 120, 180, 260, 400].map((ms) => (
              <div key={ms} className="fbs-frame">
                <div
                  className="fbs-frame-bar"
                  style={{ ["--freeze" as string]: `-${ms.toString()}ms` }}
                >
                  <FlowBar live={false} confirm elapsed="0:00" />
                </div>
                <span className="fbs-frame-t">{ms}ms</span>
              </div>
            ))}
          </div>
          <p className="fbs-note">
            A single ripple leaving the dot. Held here because it is over in under half a
            second — the point of the state is that you catch it without looking at it.
          </p>
        </div>
      </section>

      {/* ------------------------------------------------------ edge cases -- */}
      <section className="fbs-plate-section">
        <div className="fbs-plate-head">
          <span className="fbs-plate-label">Edge cases</span>
          <span className="fbs-plate-hint">where a fixed-width pill breaks</span>
        </div>
        <div className="fbs-plate" data-plate="editor">
          {/* Same message at the old fixed tier and at the measured one, which
              is capped at 360px — past that the pill stops being a pill. */}
          <Row label="Msg, was" width={248}>
            <FlowBar
              live={false}
              elapsed="0:00"
              message="Could not reach the speech model — check Settings"
            />
          </Row>
          <Row label="Msg, now" width={360}>
            <FlowBar
              live={false}
              elapsed="0:00"
              message="Could not reach the speech model — check Settings"
            />
          </Row>
          <Row label="Discarded" width={200}>
            <FlowBar live={false} elapsed="0:00" message="Discarded" />
          </Row>
          {/* The same shortcut at the old fixed tier and at the measured one.
              The first is what shipped: "Hold" and the key cap collide and the
              cap is clipped by the pill's own overflow. */}
          <Row label="Key, was" width={150}>
            <FlowBar live={false} elapsed="0:00" hint="Ctrl+Alt+Space" />
          </Row>
          <Row label="Key, now" width={176}>
            <FlowBar live={false} elapsed="0:00" hint="Ctrl+Alt+Space" />
          </Row>
          <Row label="Long take" width={240}>
            <FlowBar live levelRef={live} elapsed="12:34" onCancel={() => undefined} />
          </Row>
          <div className="fbs-row">
            <span className="fbs-row-label">Menu</span>
            <span className="fbs-row-width">280px</span>
            <div className="fbs-row-bar" style={{ width: 280 }}>
              <FlowBar live={false} elapsed="0:00" />
              <div className="overlay-menu" role="presentation">
                <button type="button">Open OpenVoice</button>
                <button type="button">Paste last transcript</button>
                <div className="overlay-menu-sep" />
                <button type="button">Hide for an hour</button>
                <button type="button">Only show while dictating</button>
              </div>
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
            <div style={{ width: 240 }}>
              <FlowBar live levelRef={live} elapsed="0:04" onCancel={() => undefined} />
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
