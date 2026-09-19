/** One earlier dictation (reference lines 514-518). Shared by Home's Earlier
 *  list and the History view.
 *
 *  The row is one line: time, text, then what is worth knowing about it (what
 *  the dictionary corrected, whether it failed to paste, which app) and a copy
 *  button. It is a disclosure: the time and text are one real button
 *  (`.row-toggle`, spanning the first two grid columns through a subgrid) that
 *  opens the row in place, with the whole text and the same three actions as
 *  the Last dictation card. The copy button is its sibling, never nested in it.
 *  A click anywhere else on the row toggles too, as a mouse convenience.
 *
 *  `<time>` stays inside `.row`: the twin harness hovers the row whose time
 *  reads "2:39 PM". */
import { useId, useLayoutEffect, useRef, useState, type MouseEvent } from "react";
import { Check, Copy } from "@phosphor-icons/react";
import { AppChip, StatusChip } from "../ui";
import { dictionaryHits, outcomeInfo } from "../history";
import { clockTime } from "../time";
import type { DictEntry, Settings } from "../../engine/settings";
import type { Row as RowData } from "../../engine/stats";
import { Actions } from "./Actions";
import { useCopyPaste } from "./useCopyPaste";
import { FixPanel } from "./FixPanel";
import { revealInList } from "./useFit";

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
  const moreId = useId();
  const el = useRef<HTMLDivElement>(null);

  // Closing the row also closes its Fix panel, so it reopens as it first did.
  const toggle = () => {
    if (open) setFixing(false);
    onToggle();
  };
  // Controls inside the row do their own thing; a click on the row's bare
  // background opens or closes it like the toggle does.
  const onClick = (e: MouseEvent<HTMLDivElement>) => {
    if ((e.target as Element).closest("button, input, textarea, a, .row-more")) return;
    toggle();
  };

  // An open row near the bottom of the list scrolls into view, and again
  // whenever it grows (Fix a word opening, the text rewrapping).
  useLayoutEffect(() => {
    const node = el.current;
    if (!open || !node) return;
    revealInList(node);
    let ro: ResizeObserver | undefined;
    try { ro = new ResizeObserver(() => revealInList(node)); ro.observe(node); } catch { /* revealed once */ }
    return () => ro?.disconnect();
  }, [open, fixing]);

  return (
    <div ref={el} className={open ? "row open" : "row"} onClick={onClick}>
      <button type="button" className="row-toggle" aria-expanded={open} aria-controls={moreId} onClick={toggle}>
        <time dateTime={new Date(row.created_at).toISOString()}>{clockTime(row.created_at)}</time>
        <span className={row.profile === "terminal" ? "t code" : "t"}>{row.final_text}</span>
      </button>
      <span className="r">
        {hit && <span className="heard">heard <s>{hit.spoken}</s></span>}
        {kind !== "delivered" && <StatusChip kind={kind} />}
        <AppChip exe={row.target_app} profile={row.profile} />
        <button type="button" className="copy" aria-label="Copy" onClick={() => void act.copy()}>
          {act.copied ? <Check weight="bold" aria-hidden /> : <Copy aria-hidden />}
        </button>
      </span>
      <div className="row-more" id={moreId} hidden={!open}>
        {open && (
          <>
            <p className={row.profile === "terminal" ? "full code" : "full"}>{row.final_text}</p>
            {fixing && <FixPanel row={row} patch={patch} onDone={() => setFixing(false)} />}
            <div className="acts">
              <Actions act={act} kind="row" fixOpen={fixing} onFix={() => setFixing((f) => !f)} />
            </div>
          </>
        )}
      </div>
    </div>
  );
}
