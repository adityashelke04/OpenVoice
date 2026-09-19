import type { InputHTMLAttributes, ReactNode, Ref } from "react";

/** Reference `.field`: the whole 36px well is a `<label>`, so a click anywhere on it
 *  (icon included) focuses the input. The field has no visible label of its own, so
 *  callers pass `aria-label` (or `aria-labelledby`) to give the input its name.
 *  `ref` (a plain prop in React 19) reaches the input itself. */
export function Field({ icon, mono, className, ref, ...input }: InputHTMLAttributes<HTMLInputElement> & { icon?: ReactNode; mono?: boolean; ref?: Ref<HTMLInputElement> }) {
  const cls = ["field", mono && "mono", className].filter(Boolean).join(" ");
  return (
    <label className={cls}>
      {icon}
      <input ref={ref} {...input} />
    </label>
  );
}
