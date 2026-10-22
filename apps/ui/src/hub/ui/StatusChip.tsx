import { ClipboardText, WarningCircle } from "@phosphor-icons/react";

/** History distinguishes text held in OpenVoice from an actual clipboard copy. */
export function StatusChip({ kind }: { kind: "clipboard" | "held" | "failed" }) {
  if (kind === "held") return <span className="warn-chip"><ClipboardText weight="bold" aria-hidden />Held in OpenVoice</span>;
  return kind === "clipboard"
    ? <span className="warn-chip"><ClipboardText weight="bold" aria-hidden />Not pasted, on clipboard</span>
    : <span className="fail-chip"><WarningCircle weight="bold" aria-hidden />Failed</span>;
}
