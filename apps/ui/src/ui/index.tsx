/** Primitive components.
 *
 * Deliberately small and unopinionated about layout. The design sheet
 * (`?window=sheet`) shows every one. The two rules that erode fastest and
 * matter most:
 *
 *   1. There are no shadows anywhere. Depth is the surface ladder plus hairlines.
 *   2. Green appears only on live state, the record action, and focus rings.
 */

import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from "react";
import { LoadingDots, TickingEllipsis } from "./LoadingDots";
import { MicTestMeter } from "./MicTestMeter";
import { useCountUp } from "./useCountUp";
import { useLiveTimeAgo, formatTimeAgo } from "./useLiveTimeAgo";
import "./ui.css";
import "./loading-dots.css";
import "./mic-test-meter.css";

export { LoadingDots, TickingEllipsis, MicTestMeter, useCountUp, useLiveTimeAgo, formatTimeAgo };
export type {
  LoadingDotsProps,
  TickingEllipsisProps,
  LoadingTone,
  LoadingSize,
  LoadingVariant,
} from "./LoadingDots";
export type { MicTestMeterProps } from "./MicTestMeter";
export type { CountUpOptions } from "./useCountUp";

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

export { Waveform } from "./Waveform";
export { FlowBar } from "./flowbar/FlowBar";
export { flowMode, flowSpeaks, flowText } from "./flowbar/mode";
export type { FlowEdge, FlowStatus, FlowMode } from "./flowbar/mode";
