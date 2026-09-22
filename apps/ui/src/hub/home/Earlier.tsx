/** Earlier dictations (reference lines 507-519): everything before the last
 *  one, grouped by day, filtered by writing style in the backend (spec 7.2).
 *  As many rows as fit; the "N total" link opens History for the rest. */
import { useCallback, useMemo, useRef, useState } from "react";
import { CaretRight } from "@phosphor-icons/react";
import { Segmented } from "../ui";
import { FILTERS, groupByDay, rowKey } from "../history";
import type { ProfileFilter } from "../api";
import type { DictEntry, Settings } from "../../engine/settings";
import type { Row as RowData } from "../../engine/stats";
import { Row } from "./Row";
import { useFit } from "./useFit";
import { useRowCap } from "./useRowCap";

export function Earlier({ rows, filter, onFilter, total, onOpenHistory, dict, now, patch }: {
  rows: RowData[];
  filter: ProfileFilter;
  onFilter: (f: ProfileFilter) => void;
  total: number;
  onOpenHistory: () => void;
  dict: DictEntry[];
  now: number;
  patch: (fn: (s: Settings) => void) => void;
}) {
  const [picked, setOpen] = useState<string | null>(null);
  // Stable, and keyed by the row: an inline `() => setOpen(...)` per row is a
  // new function on every render, which defeats Row's memo entirely.
  const toggle = useCallback((k: string) => setOpen((o) => (o === k ? null : k)), []);
  // A row that left the list (new filter, new dictation) is no longer open.
  const open = picked && rows.some((r) => rowKey(r) === picked) ? picked : null;
  const box = useRef<HTMLDivElement>(null);
  // Only as many rows as the box could possibly show: rendering all two hundred
  // and hiding the ones that overflow put 2,403 nodes and 398 icons in the DOM
  // for fifteen visible rows. An open row is exempt — the list scrolls then
  // (see useFit), so every row it holds has to stay reachable.
  const cap = useRowCap(box);
  const shown = useMemo(() => (open ? rows : rows.slice(0, cap)), [rows, open, cap]);
  const groups = useMemo(() => groupByDay(shown, now), [shown, now]);
  // `now` is deliberately not a dependency: it ticks every minute, and a row's
  // height does not depend on it. Only the day labels read it, and their text
  // changing cannot make a row taller.
  useFit(box, [shown, open]);

  return (
    <section className="recent glass" aria-label="Earlier dictations">
      <div className="recent-head">
        <h2>Earlier</h2>
        <Segmented value={filter} options={FILTERS} onChange={onFilter} label="Filter by kind of app" />
        <button type="button" className="all" onClick={onOpenHistory}>
          {total.toLocaleString("en-US")} total <CaretRight aria-hidden />
        </button>
      </div>
      <div className={open ? "rows scrolling" : "rows"} ref={box}>
        {groups.flatMap((g) => [
          <div key={`d${g.key}`} className="day">{g.label}</div>,
          ...g.rows.map((r) => {
            const k = rowKey(r);
            // No `now`: Row never read it, and it changes every minute, which
            // would re-render every row for nothing.
            return <Row key={k} rowId={k} row={r} dict={dict} open={open === k} onToggle={toggle} patch={patch} />;
          }),
        ])}
      </div>
    </section>
  );
}
