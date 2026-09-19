import { ClipboardText, WarningCircle } from "@phosphor-icons/react";

/** The two ways a dictation can fail to land. Amber = not pasted (the text is safe
 *  on the clipboard); red = failed. Colour never carries it alone: the words do. */
export function StatusChip({ kind }: { kind: "clipboard" | "failed" }) {
  return kind === "clipboard"
    ? <span className="warn-chip"><ClipboardText weight="bold" aria-hidden />Not pasted, on clipboard</span>
    : <span className="fail-chip"><WarningCircle weight="bold" aria-hidden />Failed</span>;
}
