import { Warning } from "@phosphor-icons/react";
import type { ReactNode } from "react";

/** An inline warning strip (not in the reference; styled from its warn tokens).
 *  `role="status"` so a screen reader hears it without it stealing focus. */
export function Notice({ tone, action, children }: { tone: "warn"; action?: ReactNode; children: ReactNode }) {
  return (
    <div role="status" className={`notice glass ${tone}`}>
      <Warning weight="bold" aria-hidden className="notice-ic" />
      <div className="notice-body">{children}</div>
      {action}
    </div>
  );
}
