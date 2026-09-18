import type { ReactNode } from "react";

/** Reference `.badge`; `tone="ok"` adds the green dot, for "active / ready" only. */
export function Badge({ tone, children }: { tone?: "ok"; children: ReactNode }) {
  return <span className={tone ? `badge ${tone}` : "badge"}>{children}</span>;
}
