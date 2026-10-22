/** Home (spec 6.2, 6.3): the last dictation first, then everything before it,
 *  then the numbers.
 *
 *  Two history reads, not one. `all` is the unfiltered newest 200: the Last
 *  dictation card is always the newest dictation whatever the Earlier filter
 *  says. `filtered` is the same query with a profile, asked of the backend
 *  (spec 7.2) because filtering a fetched page in JavaScript would make older
 *  rows of a rarely used profile unreachable. Both refetch whenever the engine
 *  reports a new session.
 *
 *  The state is chosen, in order: the engine is down (error), the first answers
 *  have not arrived (loading), there is nothing yet (first), the last dictation
 *  did not land (failed), otherwise normal. */
import { useEffect, useRef, useState } from "react";
import { ClockCounterClockwise } from "@phosphor-icons/react";
import { getHistory, getTotals, type ProfileFilter } from "../api";
import { outcomeInfo, rowKey } from "../history";
import { pushToast } from "../toast";
import type { LiveView } from "../../engine/useLiveEngine";
import { getLastText, getLastTranscript, getStorageStatus, getStorageWarning, type Settings } from "../../engine/settings";
import type { Row, Totals } from "../../engine/stats";
import { Button } from "../ui";
import { useCopyPaste } from "./useCopyPaste";
import { Recovery } from "./Recovery";
import { Earlier } from "./Earlier";
import { LastCard } from "./LastCard";
import { HomeSkeleton } from "./Skeleton";
import { StatsColumn } from "./StatsColumn";
import "./home.css";

/** See HistoryView: a fresh `[]` per render would defeat every Row's memo. */
const EMPTY_DICT: Settings["dictionary"] = [];

export type HomeState = "loading" | "error" | "first" | "failed" | "normal";

const NO_TOTALS: Totals = { sessions: 0, words: 0, speakingMs: 0, topApp: null, activeDays: [] };

export function HomeScreen({ view, settings, patch, now, onOpenHistory }: {
  view: LiveView;
  settings: Settings | null;
  patch: (fn: (s: Settings) => void) => void;
  now: number;
  onOpenHistory: () => void;
}) {
  const [memoryText, setMemoryText] = useState("");
  const [partialText, setPartialText] = useState(false);
  const [storageWarning, setStorageWarning] = useState<string | null>(null);
  const [persistent, setPersistent] = useState<boolean | null>(null);
  const [all, setAll] = useState<Row[] | null>(null);
  const [totals, setTotals] = useState<Totals | null>(null);
  const [filter, setFilter] = useState<ProfileFilter>("all");
  const [filtered, setFiltered] = useState<{ filter: ProfileFilter; rows: Row[] } | null>(null);

  useEffect(() => {
    let live = true;
    const read = () => {
      void (async () => {
        const transcript = await getLastTranscript();
        const text = transcript?.text ?? await getLastText();
        if (live) {
          setMemoryText(text || view.lastText);
          setPartialText(transcript?.partial ?? false);
        }
      })().catch(() => { if (live) { setMemoryText(view.lastText); setPartialText(false); } });
      void getStorageWarning().then(w => { if (live) setStorageWarning(w); }, () => {});
      void getStorageStatus().then(p => { if (live) setPersistent(p); }, () => {});
    };
    read();
    const timer = window.setInterval(read, 5000);
    return () => { live = false; clearInterval(timer); };
  }, [view.sessions, view.lastText]);

  useEffect(() => {
    let live = true;
    // A read that fails leaves the screen showing what it can rather than a
    // skeleton forever: history not readable yet reads as "nothing yet".
    getHistory({ limit: 200 }).then((r) => live && setAll(r ?? []), () => live && setAll((p) => p ?? []));
    getTotals().then((t) => live && setTotals(t ?? NO_TOTALS), () => live && setTotals((p) => p ?? NO_TOTALS));
    return () => { live = false; };
  }, [view.sessions]);

  // The filter last shown successfully. A filtered read that fails puts the
  // tab back on it and keeps its rows on screen, with a toast saying why,
  // rather than showing an empty list that reads as "nothing here".
  const shown = useRef<ProfileFilter>("all");
  useEffect(() => {
    if (filter === "all") { shown.current = "all"; return; }
    let live = true;
    getHistory({ limit: 200, profile: filter }).then(
      (rows) => { if (!live) return; shown.current = filter; setFiltered({ filter, rows: rows ?? [] }); },
      (e) => { if (!live) return; pushToast({ tone: "danger", message: String(e) }); setFilter(shown.current); },
    );
    return () => { live = false; };
  }, [filter, view.sessions]);

  const rows = all ?? [];
  const last = rows[0];
  const state: HomeState = view.error
    ? "error"
    : all === null || totals === null
      ? "loading"
      : !last
        ? "first"
        : outcomeInfo(last) !== "delivered" ? "failed" : "normal";

  if (state === "loading") return <HomeSkeleton />;

  const lastKey = last ? rowKey(last) : null;
  // While a new filter's rows are on their way, the previous list stays up.
  const source = filter === "all"
    ? rows
    : filtered?.filter === filter
      ? filtered.rows
      : shown.current !== "all" && filtered?.filter === shown.current ? filtered.rows : rows;
  const earlier = source.filter((r) => rowKey(r) !== lastKey);
  const first = state === "first" || (state === "error" && !last);

  return (
    <section className="scroll">
      {persistent === false && <p role="alert">History is not being saved to disk. This session's history will be lost when OpenVoice closes. Copy any text you need, free disk space or fix data-folder permissions, then restart OpenVoice.</p>}
      {storageWarning && <p role="alert">{storageWarning} Legacy history has been preserved. Check disk space and data-folder permissions, then restart to retry the import.</p>}
      {memoryText && (memoryText !== last?.final_text || partialText) && <MemoryTranscript text={memoryText} partial={partialText} />}
      <Recovery sessions={view.sessions} notice={view.notice} />
      <LastCard
        variant={state === "error" ? "error" : state === "first" ? "first" : state}
        row={last}
        now={now}
        shortcut={view.ready?.shortcut ?? "Right Ctrl"}
        error={view.error}
        patch={patch}
      />
      <div className="lower">
        {first ? (
          <section className="recent glass empty">
            <ClockCounterClockwise aria-hidden className="empty-ic" />
            <div className="empty-title">Your dictations will collect here</div>
            <div className="cap empty-body">Nothing leaves this PC. History is a plain file you can open, copy or delete.</div>
          </section>
        ) : (
          <Earlier
            rows={earlier}
            filter={filter}
            onFilter={setFilter}
            total={totals?.sessions ?? 0}
            onOpenHistory={onOpenHistory}
            dict={settings?.dictionary ?? EMPTY_DICT}
            now={now}
            patch={patch}
          />
        )}
        <StatsColumn totals={totals ?? NO_TOTALS} first={first} now={now} />
      </div>
    </section>
  );
}

function MemoryTranscript({ text, partial }: { text: string; partial: boolean }) {
  const action = useCopyPaste(text);
  return <article className="card glass" aria-label="Recovered transcript">
    <h2>{partial ? "Incomplete transcript" : "Transcript kept in OpenVoice"}</h2>
    {partial && <p role="alert">Incomplete transcript: later speech could not be transcribed. Retry audio for the full recording.</p>}
    <p>{text}</p>
    <p className="cap">Use Copy, then Ctrl+V where you want it. This recovery copy is held in memory and may disappear when OpenVoice closes.</p>
    <Button aria-label="Copy recovered text" onClick={() => void action.copy()}>{action.copied ? "Copied" : "Copy"}</Button>
  </article>;
}
