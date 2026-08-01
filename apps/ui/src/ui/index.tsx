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
}) {
  const host = useRef<HTMLDivElement>(null);
  const latest = useRef(level ?? 0);
  latest.current = level ?? 0;
  const isIdle = useRef(idle);
  isIdle.current = idle;
  const external = useRef(levelRef);
  external.current = levelRef;

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

    /** How often the wave advances one bar. ~18 steps/sec reads as a flowing
     *  ribbon; faster becomes a blur, slower becomes a stutter. */
    const SHIFT_MS = 55;
    /** Per-frame chase. 0.28 settles ~90% within one shift interval, so each bar
     *  is always slightly behind its neighbour — which is precisely what makes the
     *  motion look like a wave travelling rather than a row of independent bars. */
    const CHASE = 0.28;
    const MIN = 0.05;

    const tick = (now: number) => {
      raf = requestAnimationFrame(tick);

      if (now - lastShift >= SHIFT_MS) {
        lastShift = now;
        const raw = external.current ? external.current.current : latest.current;
        target.copyWithin(0, 1);
        target[n - 1] = isIdle.current ? 0 : Math.min(1, Math.max(0, raw));
      }

      // Interpolate on EVERY frame, not on every sample. This is the whole
      // difference between smooth and mushy: a CSS transition chasing a 30 Hz
      // signal never arrives, but 60 fps interpolation toward a moving target is
      // continuous by construction.
      for (let i = 0; i < n; i++) {
        current[i] += (target[i] - current[i]) * CHASE;
        // sqrt approximates perceived loudness, so quiet speech still moves the
        // bar visibly while loud speech does not slam into the ceiling.
        const v = MIN + Math.sqrt(current[i]) * (1 - MIN);
        nodes[i].style.transform = `scaleY(${v.toFixed(4)})`;
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
export function FlowBar({
  live,
  level,
  levelRef,
  elapsed,
  hint = "Right Ctrl",
  working,
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
}) {
  return (
    <div className="flowbar" data-live={live} data-working={working}>
      <span className="flowbar-mic" />
      {live ? (
        <>
          <div className="flowbar-wave">
            <Waveform level={level} levelRef={levelRef} bars={32} />
          </div>
          <span className="flowbar-time">{elapsed}</span>
        </>
      ) : (
        <div className="flowbar-idle">
          {working ? (
            // No spinner. The bar itself is already the indicator, and this
            // window exists to be glanced at, not watched.
            <span className="t-caption" style={{ color: "var(--body)" }}>
              Writing…
            </span>
          ) : (
            <>
              <span className="t-caption" style={{ color: "var(--mute)" }}>
                Hold
              </span>
              <Kbd>{hint}</Kbd>
            </>
          )}
        </div>
      )}
    </div>
  );
}
