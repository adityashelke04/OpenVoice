import type { ReactNode } from "react";

/** A key name set as a keycap ("Right Ctrl"). Styled in controls.css. */
export function Keycap({ children }: { children: ReactNode }) {
  return <span className="keycap">{children}</span>;
}
