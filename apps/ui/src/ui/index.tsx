/** Primitive components.
 *
 * Deliberately small and unopinionated about layout. Rules and reasoning live in
 * DESIGN.md; the two that erode fastest and matter most:
 *
 *   1. There are no shadows anywhere. Depth is the surface ladder plus hairlines.
 *   2. Green appears only on live state, the record action, and focus rings.
 */

import { useEffect, useRef } from "react";
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from "react";
import "./ui.css";

export type Tone = "neutral" | "live" | "warn" | "danger";

/* -------------------------------------------------------------------------- */

export function Button({
  children,
  variant = "secondary",
  size,
  ...rest
}: {
  children: ReactNode;
  variant?: "primary" | "secondary" | "ghost" | "danger" | "record";
  size?: "sm";
} & ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button type="button" className="btn" data-variant={variant} data-size={size} {...rest}>
      {children}
    </button>
  );
}

export function Input({
  label,
  ...rest
}: { label?: string } & InputHTMLAttributes<HTMLInputElement>) {
  if (!label) return <input className="input" {...rest} />;
  return (
    <label className="field">
      <span className="t-label">{label}</span>
      <input className="input" {...rest} />
    </label>
  );
}

export function Select({
  label,
  options,
  ...rest
}: {
  label?: string;
  options: string[];
} & React.SelectHTMLAttributes<HTMLSelectElement>) {
  const el = (
    <select className="select" {...rest}>
      {options.map((o) => (
        <option key={o} value={o}>
          {o}
        </option>
      ))}
    </select>
  );
  if (!label) return el;
  return (
    <label className="field">
      <span className="t-label">{label}</span>
      {el}
    </label>
  );
}

export function Toggle({
  on,
  onChange,
  label,
}: {
  on: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      className="toggle"
      data-on={on}
      role="switch"
      aria-checked={on}
      aria-label={label}
      onClick={() => onChange(!on)}
    />
  );
}

export function Badge({
  children,
  tone = "neutral",
  dot,
  pulse,
}: {
  children: ReactNode;
  tone?: Tone;
  dot?: boolean;
  pulse?: boolean;
}) {
  return (
    <span className="badge" data-tone={tone === "neutral" ? undefined : tone}>
      {dot && <span className="dot" data-pulse={pulse} />}
      {children}
    </span>
  );
}

export function Card({
  title,
  action,
  children,
}: {
  title?: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="card">
      {(title || action) && (
        <header className="card-head">
          {title && <h2 className="t-heading">{title}</h2>}
          {action}
        </header>
      )}
      {children}
    </section>
  );
}

/** A headline number with its unit. Units are never omitted — a bare number on a
 *  dashboard is an invitation to misread it. */
export function Stat({
  label,
  value,
  unit,
  tone,
}: {
  label: string;
  value: string | number;
  unit?: string;
  tone?: Tone;
}) {
  return (
    <div className="stat">
      <span className="t-label">{label}</span>
      <span className="stat-value">
        <span
          className="t-mono-lg"
          style={tone === "live" ? { color: "var(--live)" } : undefined}
        >
          {value}
        </span>
        {unit && <span className="stat-unit">{unit}</span>}
      </span>
    </div>
  );
}

export function Tabs({
  value,
  onChange,
  options,
}: {
  value: string;
  onChange: (v: string) => void;
  options: string[];
}) {
  return (
    <div className="tabs" role="tablist">
      {options.map((o) => (
        <button
          key={o}
          type="button"
          className="tab"
          role="tab"
          aria-selected={value === o}
          onClick={() => onChange(o)}
        >
          {o}
        </button>
      ))}
    </div>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}

export function Notice({
  children,
  tone = "neutral",
  action,
}: {
  children: ReactNode;
  tone?: Tone;
  action?: ReactNode;
}) {
  return (
    <div className="notice" data-tone={tone === "neutral" ? undefined : tone} role="status">
      <span className="dot" />
      <span style={{ flex: 1 }}>{children}</span>
      {action}
    </div>
  );
}

export function Empty({ title, hint, action }: { title: string; hint?: string; action?: ReactNode }) {
  return (
    <div className="empty">
      <p className="t-subheading">{title}</p>
      {hint && <p className="t-caption" style={{ maxWidth: "46ch" }}>{hint}</p>}
      {action}
    </div>
  );
}

/**
 * Scrolling live waveform.
 *
 * Holds a rolling buffer of the last ~1.5 s of loudness. Each frame the newest
 * sample enters at the right and every older one shifts left, so a spoken syllable
 * becomes **a shape that travels** — you watch the sentence you just said move
 * across the bar. An equaliser, where bars jitter in place, communicates nothing to
 * a person and is the thing this deliberately is not.
 *
 * Heights are written straight to the DOM rather than through React state. At 30 Hz
 * a re-render per frame would make this the most expensive thing in an app that is
 * supposed to be invisible when idle.
 */
export function Waveform({
  level,
  levelRef,
  bars = 32,
  idle,
  publish,
}: {
  /** Static level. Used by the component sheet and previews. */
  level?: number;
  /**
   * Live level, read every animation frame.
   *
   * Passing a ref rather than a value is the point: the microphone level changes
   * ~30 times a second, and routing that through React state re-rendered the whole
   * overlay at frame rate, restarting CSS animations mid-flight and making the bar
   * flicker. A ref costs zero renders.
   */
  levelRef?: { current: number };
  bars?: number;
  idle?: boolean;
  /**
   * Element to publish the metered level onto, as a `--level` custom property.
   *
   * The Flow Bar's glow tracks the voice, and the metered value it needs is
   * computed here, once, in a loop that already runs every frame. Writing it
   * onto an ancestor element rather than returning it keeps that value out of
   * React entirely — the alternative is a state update per frame, which is the
   * exact thing the `levelRef` note above exists to prevent. Custom properties
   * inherit downward, so this has to be the element that *owns* the glow.
   */
  publish?: { current: HTMLElement | null };
}) {
  const host = useRef<HTMLDivElement>(null);
  const latest = useRef(level ?? 0);
  latest.current = level ?? 0;
  const isIdle = useRef(idle);
  isIdle.current = idle;
  const external = useRef(levelRef);
  external.current = levelRef;
  const sink = useRef(publish);
  sink.current = publish;

  useEffect(() => {
    const el = host.current;
    if (!el) return;

    const n = bars;
    /** Where each bar is heading — the scrolled history of loudness. */
    const target = new Float32Array(n);
    /** Where each bar actually is. Chases `target` every frame. */
    const current = new Float32Array(n);
    const nodes = Array.from(el.children) as HTMLElement[];

    let raf = 0;
    let lastShift = 0;
    let lastFrame = 0;

    /** How often the wave advances one bar. ~18 steps/sec reads as a flowing
     *  ribbon; faster becomes a blur, slower becomes a stutter. */
    const SHIFT_MS = 55;
    /** Per-bar chase, as a time constant rather than a per-frame fraction, so the
     *  wave travels at the same speed on a 60Hz and a 144Hz display. 50ms is the
     *  constant the old fixed 0.28-per-frame worked out to at 60fps — each bar
     *  stays slightly behind its neighbour, which is what makes this read as a
     *  wave travelling rather than a row of independent bars. */
    const CHASE_MS = 50;
    const MIN = 0.05;

    /* VU ballistics.
     *
     * A meter that rises and falls at the same rate reads as a progress bar. A
     * real one snaps up and eases down, and holds its peak long enough for the
     * eye to catch it — that asymmetry is the whole reason this looks like an
     * instrument instead of a graph, and it is what DESIGN.md specifies.
     *
     * The wave samples `meter`, not the raw level: the raw signal is a noisy
     * 30Hz stream, and feeding it straight in was making quiet consonants
     * flicker between bars. */
    const ATTACK_MS = 60;
    const RELEASE_MS = 380;
    const HOLD_MS = 800;

    let meter = 0;
    let peak = 0;
    let peakAt = 0;

    const tick = (now: number) => {
      raf = requestAnimationFrame(tick);

      // Clamped so a backgrounded window returning after seconds away does not
      // jump the meter to its target in one frame.
      const dt = lastFrame === 0 ? 16.7 : Math.min(64, now - lastFrame);
      lastFrame = now;

      const raw = isIdle.current
        ? 0
        : Math.min(1, Math.max(0, external.current ? external.current.current : latest.current));

      // Rise fast, fall slow. Exponential toward the target with a time constant
      // per direction, so both are frame-rate independent.
      const tau = raw >= meter ? ATTACK_MS : RELEASE_MS;
      meter += (raw - meter) * (1 - Math.exp(-dt / tau));

      // Peak hold: the ceiling stays put for HOLD_MS after a syllable, then
      // releases. Published to the glow so a loud word leaves a visible trace
      // rather than vanishing the instant the speaker stops.
      if (meter >= peak) {
        peak = meter;
        peakAt = now;
      } else if (now - peakAt > HOLD_MS) {
        peak += (meter - peak) * (1 - Math.exp(-dt / RELEASE_MS));
      }

      const out = sink.current?.current;
      if (out) out.style.setProperty("--level", peak.toFixed(3));

      if (now - lastShift >= SHIFT_MS) {
        lastShift = now;
        target.copyWithin(0, 1);
        target[n - 1] = meter;
      }

      // Interpolate on EVERY frame, not on every sample. This is the whole
      // difference between smooth and mushy: a CSS transition chasing a 30 Hz
      // signal never arrives, but 60 fps interpolation toward a moving target is
      // continuous by construction.
      const chase = 1 - Math.exp(-dt / CHASE_MS);
      for (let i = 0; i < n; i++) {
        current[i] += (target[i] - current[i]) * chase;
        // sqrt approximates perceived loudness, so quiet speech still moves the
        // bar visibly while loud speech does not slam into the ceiling.
        const v = MIN + Math.sqrt(current[i]) * (1 - MIN);
        // A custom property rather than the transform itself, so the stylesheet
        // owns which axis this scales. Docked to a side edge the bar runs
        // vertically and the wave has to run with it, and the alternative — a
        // quarter-turn `rotate` on the container — needs the container's own
        // length in advance, which is a number this loop has no business
        // knowing. The write is the same cost either way.
        nodes[i].style.setProperty("--v", v.toFixed(4));
      }
    };

    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [bars]);

  return (
    <div className="wave" data-idle={idle} ref={host} role="presentation">
      {Array.from({ length: bars }, (_, i) => (
        <span key={i} className="wave-bar" />
      ))}
    </div>
  );
}

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
  confirm,
  publish,
  onCancel,
  status = "ready",
  progress,
  mini = false,
  edge = "bottom",
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
  /** Momentary, on a clean landing. See `.flowbar-mic[data-confirm]`. */
  confirm?: boolean;
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
        ) : text !== undefined ? (
          <span className="flowbar-msg" title={text}>
            {text}
          </span>
        ) : mode === "working" ? (
          // No spinner. The bar itself is already the indicator, and this
          // window exists to be glanced at, not watched. The motion it does get
          // is the text's own — see `.flowbar-working-text`.
          <span className="flowbar-working-text t-caption">
            {mini || column ? "···" : "Writing…"}
          </span>
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
