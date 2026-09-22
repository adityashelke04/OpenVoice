/** The full history (spec 6.4, reference lines 550-571): Home's Earlier list,
 *  opened to the whole height of the window and to every dictation.
 *
 *  Everything is asked of the backend: the filter is a `profile`, the search a
 *  `query`, and the page a `limit`. Filtering or searching a fetched page in
 *  JavaScript would make the older rows unreachable, which is the one thing a
 *  history must not do. Search waits 180 ms after the last key, so typing a
 *  word is one read rather than one per letter. Scrolling near the bottom of a
 *  full page asks for 200 more; a page that came back short is the end.
 *
 *  While a new answer is on its way the previous rows stay up, so the list
 *  never flashes empty between keystrokes. Escape clears a search that has
 *  text in it, and otherwise returns to Home. */
import { useCallback, useEffect, useRef, useState, type UIEvent } from "react";
import { MagnifyingGlass } from "@phosphor-icons/react";
import { Button, Field, Segmented } from "../ui";
import { getHistory, getTotals, type ProfileFilter } from "../api";
import { FILTERS, groupByDay, newestFirst, rowKey } from "../history";
import { pushToast } from "../toast";
import type { LiveView } from "../../engine/useLiveEngine";
import type { Settings } from "../../engine/settings";
import type { Row as RowData } from "../../engine/stats";
import { Row } from "./Row";
import "./home.css";

/** A single frozen empty dictionary, so a render with no settings yet does not
 *  hand every Row a brand-new array and break their memo. */
const EMPTY_DICT: Settings["dictionary"] = [];

const PAGE = 200;
const DEBOUNCE_MS = 180;
/** How close to the bottom (px) counts as "the bottom": the next page is asked
 *  for a little before the last row, so a steady scroll does not stall on it. */
const NEAR_BOTTOM = 160;

interface Request { filter: ProfileFilter; query: string; limit: number }
const keyOf = (r: Request) => JSON.stringify([r.filter, r.query, r.limit]);

export function HistoryView({ view, settings, patch, now, onClose }: {
  view: LiveView;
  settings: Settings | null;
  patch: (fn: (s: Settings) => void) => void;
  now: number;
  onClose: () => void;
}) {
  const [text, setText] = useState("");
  const [req, setReq] = useState<Request>({ filter: "all", query: "", limit: PAGE });
  const [page, setPage] = useState<{ key: string; req: Request; rows: RowData[] } | null>(null);
  const [total, setTotal] = useState<number | null>(null);
  const [picked, setOpen] = useState<string | null>(null);
  // One stable handler for every row, so Row's memo holds (see Earlier.tsx).
  const toggle = useCallback((k: string) => setOpen((o) => (o === k ? null : k)), []);

  useEffect(() => {
    let live = true;
    getTotals().then((t) => live && setTotal(t?.sessions ?? 0), () => {});
    return () => { live = false; };
  }, [view.sessions]);

  // The search field is instant; the query it sends waits for a pause.
  useEffect(() => {
    const q = text.trim();
    const id = window.setTimeout(() => setReq((r) => (r.query === q ? r : { ...r, query: q, limit: PAGE })), DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [text]);

  useEffect(() => {
    let live = true;
    getHistory({ limit: req.limit, query: req.query, profile: req.filter }).then(
      (rows) => live && setPage({ key: keyOf(req), req, rows: newestFirst(rows ?? []) }),
      (e) => live && pushToast({ tone: "danger", message: String(e) }),
    );
    return () => { live = false; };
  }, [req, view.sessions]);

  const onFilter = (filter: ProfileFilter) => setReq((r) => ({ ...r, filter, limit: PAGE }));

  // Escape: a search with text in it is cleared first (at once, not after the
  // debounce), then the next Escape goes back to Home. A handler that already
  // dealt with the key (a dialog on top) marks it handled and this leaves it.
  // Any other field (a row's Fix a word inputs, anything later) owns its own
  // Escape: leaving the screen would throw away what was being typed there.
  const state = useRef({ text, onClose });
  state.current = { text, onClose };
  const search = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || e.isComposing) return;
      const t = e.target instanceof Element ? e.target : null;
      if (t && t !== search.current && t.closest("input, textarea, select, [contenteditable]:not([contenteditable='false'])")) return;
      if (state.current.text) {
        setText("");
        setReq((r) => (r.query === "" ? r : { ...r, query: "", limit: PAGE }));
        return;
      }
      state.current.onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const rows = page?.rows ?? [];
  const settled = page?.key === keyOf(req);
  // A page that came back as long as it was asked to be may have more behind it.
  const more = settled && page!.rows.length >= req.limit;
  const shownLimit = page?.req.limit;
  const onScroll = useCallback((e: UIEvent<HTMLDivElement>) => {
    if (!more) return;
    const el = e.currentTarget;
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - NEAR_BOTTOM) {
      // Only from the page on screen: a burst of scroll events asks once.
      setReq((r) => (r.limit === shownLimit ? { ...r, limit: r.limit + PAGE } : r));
    }
  }, [more, shownLimit]);

  // A new filter or search is a new list, read from its top.
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (list.current) list.current.scrollTop = 0;
  }, [req.filter, req.query]);

  const open = picked && rows.some((r) => rowKey(r) === picked) ? picked : null;
  // A stable array identity: `settings?.dictionary ?? []` allocates a new empty
  // array on every render while settings are still arriving.
  const dict = settings?.dictionary ?? EMPTY_DICT;
  const shownQuery = page?.req.query ?? "";
  const label = FILTERS.find((f) => f.value === (page?.req.filter ?? "all"))?.label ?? "";
  const placeholder = total === null ? "Search dictations" : `Search ${total.toLocaleString("en-US")} dictations`;

  return (
    <section className="scroll">
      <section className="hist glass" aria-label="All dictations">
        <div className="recent-head">
          <Segmented value={req.filter} options={FILTERS} onChange={onFilter} label="Filter by kind of app" />
          <Field
            ref={search}
            icon={<MagnifyingGlass aria-hidden />}
            type="search"
            placeholder={placeholder}
            aria-label="Search dictations"
            value={text}
            onChange={(e) => setText(e.target.value)}
            spellCheck={false}
            autoComplete="off"
          />
        </div>
        <div className="rows" ref={list} onScroll={onScroll}>
          {page && rows.length === 0 ? (
            <div className="hist-empty" role="status">
              {shownQuery ? (
                <>
                  <div className="hist-empty-title">Nothing matches “{shownQuery}”</div>
                  <div className="cap">Search looks for the words as they were written, not as they were heard.</div>
                  <Button size="sm" onClick={() => { setText(""); setReq((r) => ({ ...r, query: "", limit: PAGE })); }}>Clear search</Button>
                </>
              ) : page.req.filter !== "all" ? (
                <div className="hist-empty-title">No dictations in {label} yet</div>
              ) : (
                <>
                  <div className="hist-empty-title">Your dictations will collect here</div>
                  <div className="cap">Nothing leaves this PC. History is a plain file you can open, copy or delete.</div>
                </>
              )}
            </div>
          ) : (
            groupByDay(rows, now).flatMap((g) => [
              <div key={`d${g.key}`} className="day">{g.label}</div>,
              ...g.rows.map((r) => {
                const k = rowKey(r);
                return <Row key={k} rowId={k} row={r} dict={dict} open={open === k} onToggle={toggle} patch={patch} />;
              }),
            ])
          )}
        </div>
      </section>
    </section>
  );
}
