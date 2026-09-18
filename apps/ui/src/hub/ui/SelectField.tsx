import { CaretDown } from "@phosphor-icons/react";
import type { ReactNode } from "react";

/** A `.field` that looks like the reference's static dropdown but is a real native
 *  `<select>`: the visible text sits in the flow, and the select is stretched over
 *  the whole field at opacity 0 (see controls.css). Clicks, keyboard, screen readers
 *  and the OS dropdown all go to the native control; nothing is re-implemented. */
export function SelectField({ value, options, onChange, display, width, label, disabled }: {
  value: string;
  options: { value: string; label: string }[];
  onChange: (v: string) => void;
  display?: ReactNode;
  width?: number;
  label: string;
  disabled?: boolean;
}) {
  const shown = display ?? options.find((o) => o.value === value)?.label ?? "";
  return (
    <label className="field" style={width ? { width } : undefined}>
      <span className="sel-display">{shown}</span>
      <CaretDown className="caret" aria-hidden />
      <select aria-label={label} value={value} disabled={disabled} onChange={(e) => onChange(e.target.value)}>
        {options.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
      </select>
    </label>
  );
}
