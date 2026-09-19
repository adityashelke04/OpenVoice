/** The three actions as buttons; the behaviour is in useCopyPaste.ts. */
import { ArrowUDownLeft, Check, Copy, PencilSimpleLine } from "@phosphor-icons/react";
import { Button } from "../ui";
import type { CopyPaste } from "./useCopyPaste";

/** `kind`: "normal" leads with Copy (primary, with the Ctrl C hint), "failed"
 *  leads with Paste again in amber, "row" is three quiet small buttons. */
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
      variant={kind === "normal" ? "primary" : "default"}
      kbd={kind === "normal" && !act.copied ? "Ctrl C" : undefined}
      icon={act.copied ? <Check weight="bold" aria-hidden /> : <Copy weight={kind === "normal" ? "bold" : "regular"} aria-hidden />}
      onClick={() => void act.copy()}
    >
      {act.copied ? "Copied" : "Copy"}
    </Button>
  );
  const pasteBtn = (
    <Button
      key="paste"
      size={size}
      variant={kind === "failed" ? "warn" : "default"}
      icon={act.pasted ? <Check weight="bold" aria-hidden /> : <ArrowUDownLeft weight={kind === "failed" ? "bold" : "regular"} aria-hidden />}
      onClick={() => void act.paste()}
    >
      {act.pasted ? "Pasted" : "Paste again"}
    </Button>
  );
  const fixBtn = (
    <Button key="fix" size={size} icon={<PencilSimpleLine aria-hidden />} aria-expanded={fixOpen} onClick={onFix}>
      Fix a word
    </Button>
  );
  return <>{kind === "failed" ? [pasteBtn, copyBtn, fixBtn] : [copyBtn, pasteBtn, fixBtn]}</>;
}
