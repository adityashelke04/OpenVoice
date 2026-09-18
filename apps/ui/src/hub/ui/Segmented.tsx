import { useRef, type KeyboardEvent, type ReactNode } from "react";

/** Reference `.seg`: a tablist with a roving tabindex. Only the selected tab is in
 *  the Tab order; the arrow keys (wrapping), Home and End move the selection and
 *  focus follows it, which is the WAI-ARIA tabs pattern with automatic activation. */
export type SegOption<T extends string> = { value: T; label: ReactNode; icon?: ReactNode };

export function Segmented<T extends string>({ value, options, onChange, label, size }: {
  value: T; options: SegOption<T>[]; onChange: (v: T) => void; label: string; size?: "lg";
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const current = Math.max(0, options.findIndex((o) => o.value === value));

  const onKey = (e: KeyboardEvent) => {
    const n = options.length;
    const next = e.key === "ArrowRight" ? (current + 1) % n
      : e.key === "ArrowLeft" ? (current - 1 + n) % n
      : e.key === "Home" ? 0
      : e.key === "End" ? n - 1
      : -1;
    if (next < 0) return;
    e.preventDefault();
    onChange(options[next].value);
    refs.current[next]?.focus();
  };

  return (
    <div role="tablist" aria-label={label} className={size ? `seg ${size}` : "seg"} onKeyDown={onKey}>
      {options.map((o, i) => {
        const on = i === current;
        return (
          <button
            key={o.value}
            ref={(el) => { refs.current[i] = el; }}
            type="button"
            role="tab"
            aria-selected={on}
            tabIndex={on ? 0 : -1}
            className={on ? "on" : undefined}
            onClick={() => onChange(o.value)}
          >
            {o.icon}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
