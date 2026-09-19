/** The actions as buttons; the behaviour is in useCopyPaste.ts.
 *
 *  Paste again was dropped by the owner (2026-09-19): Copy is the one way to
 *  get text back out, so on a failed paste it becomes the amber primary. */
import { Check, Copy, PencilSimpleLine } from "@phosphor-icons/react";
import { Button } from "../ui";
import type { CopyPaste } from "./useCopyPaste";

/** `kind`: "normal" leads with Copy (primary, with the Ctrl C hint), "failed"
 *  leads with Copy in amber, "row" is quiet small buttons. */
export function Actions({ act, kind, fixOpen, onFix }: {
  act: CopyPaste;
  kind: "normal" | "failed" | "row";
  fixOpen: boolean;
  onFix: () => void;
}) {
  const size = kind === "row" ? "sm" : undefined;
  const copyBtn = (
    <Button
      key="copy"
      size={size}
      variant={kind === "normal" ? "primary" : kind === "failed" ? "warn" : "default"}
      kbd={kind !== "row" && !act.copied ? "Ctrl C" : undefined}
      icon={act.copied ? <Check weight="bold" aria-hidden /> : <Copy weight={kind === "row" ? "regular" : "bold"} aria-hidden />}
      onClick={() => void act.copy()}
    >
      {act.copied ? "Copied" : "Copy"}
    </Button>
  );
  const fixBtn = (
    <Button key="fix" className="fix-toggle" size={size} icon={<PencilSimpleLine aria-hidden />} aria-expanded={fixOpen} onClick={onFix}>
      Fix a word
    </Button>
  );
  return <>{[copyBtn, fixBtn]}</>;
}
