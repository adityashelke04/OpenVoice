/** One earlier dictation (reference lines 514-518). Shared by Home's Earlier
 *  list and the History view.
 *
 *  The row is one line: time, text, then what is worth knowing about it (what
 *  the dictionary corrected, whether it failed to paste, which app) and a copy
 *  button. Clicking anywhere else opens it in place, with the whole text and the
 *  same three actions as the Last dictation card. The markup keeps
 *  `<time>` inside `.row`: the twin harness hovers the row whose time reads
 *  "2:39 PM". */
import { useState, type KeyboardEvent, type MouseEvent } from "react";
import { Check, Copy } from "@phosphor-icons/react";
import { AppChip, StatusChip } from "../ui";
import { dictionaryHits, outcomeInfo } from "../history";
import { clockTime } from "../time";
import type { DictEntry, Settings } from "../../engine/settings";
import type { Row as RowData } from "../../engine/stats";
import { Actions } from "./Actions";
import { useCopyPaste } from "./useCopyPaste";
import { FixPanel } from "./FixPanel";

export function Row({ row, dict, open, onToggle, patch }: {
  row: RowData;
  now?: number;
  dict: DictEntry[];
  open: boolean;
  onToggle: () => void;
  patch: (fn: (s: Settings) => void) => void;
}) {
  const act = useCopyPaste(row.final_text);
  const [fixing, setFixing] = useState(false);
  const kind = outcomeInfo(row);
  const hit = dictionaryHits(row, dict)[0];

  // Controls inside the row (its copy button, the open panel) do their own
  // thing; only a click on the row itself opens or closes it.
  const onClick = (e: MouseEvent<HTMLDivElement>) => {
    if ((e.target as Element).closest("button, input, textarea, a, .row-more")) return;
    onToggle();
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.target !== e.currentTarget || (e.key !== "Enter" && e.key !== " ")) return;
    e.preventDefault();
    onToggle();
  };

  return (
    <div className="row" tabIndex={0} aria-expanded={open} onClick={onClick} onKeyDown={onKeyDown}>
      <time dateTime={new Date(row.created_at).toISOString()}>{clockTime(row.created_at)}</time>
      <span className={row.profile === "terminal" ? "t code" : "t"}>{row.final_text}</span>
      <span className="r">
        {hit && <span className="heard">heard <s>{hit.spoken}</s></span>}
        {kind !== "delivered" && <StatusChip kind={kind} />}
        <AppChip exe={row.target_app} profile={row.profile} />
        <button type="button" className="copy" aria-label="Copy" onClick={() => void act.copy()}>
          {act.copied ? <Check weight="bold" aria-hidden /> : <Copy aria-hidden />}
        </button>
      </span>
      {open && (
        <div className="row-more">
          <p className={row.profile === "terminal" ? "full code" : "full"}>{row.final_text}</p>
          {fixing && <FixPanel row={row} patch={patch} onDone={() => setFixing(false)} />}
          <div className="acts">
            <Actions act={act} kind="row" fixOpen={fixing} onFix={() => setFixing((f) => !f)} />
          </div>
        </div>
      )}
    </div>
  );
}
