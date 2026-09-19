/** The stats column (reference lines 529-545): speaking speed against the
 *  typing baseline, time saved, the streak, and words dictated. Every figure is
 *  measured over the whole history (`get_totals`), never over the page of rows
 *  the screen happens to hold. `first` is the fresh install, where there is
 *  nothing to measure yet and the column teaches instead. */
import { Flame, Gauge, HourglassMedium, Lightbulb, TextAa } from "@phosphor-icons/react";
import { statsFromTotals, wordsInPerspective, type Totals } from "../../engine/stats";
import { multiple, savedParts, sentenceCase, speedBar, weekBars } from "../stats-view";

export function StatsColumn({ totals, first, now }: { totals: Totals; first: boolean; now: number }) {
  if (first) {
    return (
      <aside className="stats">
        <div className="stat glass">
          <div className="l"><Lightbulb weight="bold" aria-hidden />Try saying</div>
          <div className="s try">"call use effect here comma then return null"</div>
          <div className="s" style={{ marginTop: 8 }}>In VS Code that becomes useEffect, with the comma.</div>
        </div>
        <div className="stat glass">
          <div className="l"><Gauge weight="bold" aria-hidden />Speaking speed</div>
          <div className="v none">-</div>
          <div className="s">Appears after a few dictations</div>
        </div>
      </aside>
    );
  }

  const stats = statsFromTotals(totals);
  const bar = stats.wpm ? speedBar(stats.wpm) : null;
  const saved = savedParts(stats.savedMinutes);
  const week = weekBars(totals.activeDays, now);
  const caption = !stats.wpm
    ? "Appears after a few dictations"
    : !stats.meaningful && stats.words > 0
      ? "Settles after a few more dictations"
      : `${multiple(stats.wpm)}× faster than typing (40 wpm)`;

  return (
    <aside className="stats">
      <div className="stat glass">
        <div className="l"><Gauge weight="bold" aria-hidden />Speaking speed</div>
        {stats.wpm ? <div className="v">{stats.wpm}<small>wpm</small></div> : <div className="v none">-</div>}
        {bar && (
          <div className="speedbar" aria-hidden="true">
            <div className="axis" />
            <div className="fill" style={{ width: `${bar.fillPct}%` }} />
            <span className="mk" style={{ left: `${bar.typingPct}%` }} />
            <span className="mk you" style={{ left: `${bar.fillPct}%` }} />
          </div>
        )}
        <div className="s">{caption}</div>
      </div>
      <div className="two">
        <div className="stat glass">
          <div className="l"><HourglassMedium weight="bold" aria-hidden />Saved</div>
          <div className="v">{saved.big}<small>{saved.small}</small></div>
        </div>
        <div className="stat glass">
          <div className="l"><Flame weight="bold" aria-hidden />Streak</div>
          <div className="v">{stats.streak}<small>{stats.streak === 1 ? "day" : "days"}</small></div>
          <div className="week" aria-label={`Active on ${week.filter(Boolean).length} of the last 7 days`} role="img">
            {week.map((on, i) => <i key={i} className={on ? "on" : ""} />)}
          </div>
        </div>
      </div>
      <div className="stat glass">
        <div className="l"><TextAa weight="bold" aria-hidden />Words dictated</div>
        <div className="v">{stats.words.toLocaleString("en-US")}</div>
        <div className="s">{sentenceCase(wordsInPerspective(stats.words))}</div>
      </div>
    </aside>
  );
}
