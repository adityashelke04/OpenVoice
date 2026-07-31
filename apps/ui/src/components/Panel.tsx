/** Panel primitives — the annunciator vocabulary the whole UI is built from.
 *
 * Deliberately small. If a screen needs a shape that is not here, the right move is
 * usually to express it with these rather than to add a new container: a panel that
 * grows a second kind of box stops reading as one milled ground. */

import type { ReactNode } from "react";
import "./panel.css";

export type Signal = "off" | "green" | "amber" | "red";

/** An illuminated legend capsule. The legend renders lit or unlit, exactly as on a
 *  real panel where the label of a dark lamp is still readable — which is also why
 *  colour never carries meaning alone here. */
export function Capsule({ legend, state = "off" }: { legend: string; state?: Signal }) {
  return (
    <span className="capsule" data-state={state}>
      {legend}
    </span>
  );
}

const SEGMENTS = 20;

/** A 20-segment LED bargraph with peak hold.
 *
 * `level` and `peak` are 0..1 amplitudes straight from the engine's Level events.
 * Zones follow audio convention: green up to about -12 dB, amber to -3, red above. */
export function Meter({ level, peak }: { level: number; peak: number }) {
  const lit = Math.round(level * SEGMENTS);
  const peakSeg = Math.round(peak * SEGMENTS);

  return (
    <div className="meter" role="presentation">
      {Array.from({ length: SEGMENTS }, (_, i) => {
        const zone = i >= SEGMENTS - 2 ? "red" : i >= SEGMENTS - 6 ? "amber" : "green";
        return (
          <div
            key={i}
            className="meter-seg"
            data-zone={zone}
            data-lit={i < lit}
            data-peak={i === peakSeg - 1 && peakSeg > lit}
          />
        );
      })}
    </div>
  );
}

/** A tabular value with its unit. Units are never omitted: a bare number on an
 *  instrument panel is an invitation to misread it. */
export function Readout({
  value,
  unit,
  tone,
  large,
}: {
  value: string | number;
  unit?: string;
  tone?: "green" | "amber" | "red";
  large?: boolean;
}) {
  return (
    <span
      className="readout"
      data-tone={tone}
      style={{ fontSize: large ? 20 : 12 }}
    >
      <span className="readout-value">{value}</span>
      {unit && <span className="readout-unit">{unit}</span>}
    </span>
  );
}

export function Section({
  legend,
  action,
  children,
}: {
  legend: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="section">
      <header className="section-head">
        <h2 className="legend" style={{ margin: 0 }}>
          {legend}
        </h2>
        {action}
      </header>
      {children}
    </section>
  );
}

export function Station({
  legend,
  hint,
  children,
}: {
  legend: string;
  hint?: string;
  children?: ReactNode;
}) {
  return (
    <div className="station">
      <div className="station-label">
        <span className="legend-sm">{legend}</span>
        {hint && <span className="station-hint">{hint}</span>}
      </div>
      <div className="station-control">{children}</div>
    </div>
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
      className="ptoggle"
      data-on={on}
      role="switch"
      aria-checked={on}
      aria-label={label}
      onClick={() => onChange(!on)}
    />
  );
}

export function Button({
  children,
  onClick,
  variant,
  disabled,
}: {
  children: ReactNode;
  onClick?: () => void;
  variant?: "primary";
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className="pbutton"
      data-variant={variant}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </button>
  );
}
