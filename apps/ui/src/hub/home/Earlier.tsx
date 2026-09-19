/** Earlier dictations (reference lines 507-519): everything before the last
 *  one, grouped by day, filtered by writing style in the backend (spec 7.2).
 *  As many rows as fit; the "N total" link opens History for the rest. */
import { useRef, useState } from "react";
import { CaretRight } from "@phosphor-icons/react";
import { Segmented } from "../ui";
import { groupByDay, rowKey } from "../history";
import type { ProfileFilter } from "../api";
import type { DictEntry, Settings } from "../../engine/settings";
import type { Row as RowData } from "../../engine/stats";
import { Row } from "./Row";
import { useFit } from "./useFit";

const FILTERS: { value: ProfileFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "editor", label: "Code" },
  { value: "terminal", label: "Terminal" },
  { value: "prose", label: "Messages" },
];

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
  // A row that left the list (new filter, new dictation) is no longer open.
  const open = picked && rows.some((r) => rowKey(r) === picked) ? picked : null;
  const box = useRef<HTMLDivElement>(null);
  useFit(box, [rows, open, now]);

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
        {groupByDay(rows, now).flatMap((g) => [
          <div key={`d${g.key}`} className="day">{g.label}</div>,
          ...g.rows.map((r) => {
            const k = rowKey(r);
            return <Row key={k} row={r} now={now} dict={dict} open={open === k} onToggle={() => setOpen((o) => (o === k ? null : k))} patch={patch} />;
          }),
        ])}
      </div>
    </section>
  );
}
