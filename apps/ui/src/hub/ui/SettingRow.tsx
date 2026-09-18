import type { ReactNode } from "react";

/** Reference `.srow`: label and hint on the left, the control on the right.
 *  `title` is a native tooltip for rows whose hint is shortened. */
export function SettingRow({ label, hint, title, children }: { label: ReactNode; hint?: ReactNode; title?: string; children?: ReactNode }) {
  return (
    <div className="srow" title={title}>
      <div>
        <div className="lab">{label}</div>
        {hint && <div className="hint">{hint}</div>}
      </div>
      {children}
    </div>
  );
}
