import type { Row } from "../engine/stats";
import { dayLabel, daysBetween } from "./time";
export type OutcomeKind = "delivered" | "clipboard" | "failed";
export function outcomeInfo(row: Pick<Row, "outcome">): OutcomeKind {
  if (row.outcome === "delivered") return "delivered";
  return row.outcome === "clipboard_fallback" ? "clipboard" : "failed";
}
export interface DayGroup { key: string; label: string; rows: Row[] }
export function groupByDay(rows: Row[], now: number): DayGroup[] {
  const out: DayGroup[] = [];
  for (const r of rows) {
    const key = String(daysBetween(r.created_at, now));
    const last = out[out.length - 1];
    if (last?.key === key) last.rows.push(r);
    else out.push({ key, label: dayLabel(r.created_at, now), rows: [r] });
  }
  return out;
}
const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const wholeWord = (needle: string, flags: string) => new RegExp(`(?<![\\p{L}\\p{N}])${escape(needle)}(?![\\p{L}\\p{N}])`, flags);
export interface DictHit { spoken: string; written: string }
export function dictionaryHits(row: Pick<Row, "raw_text" | "final_text">, dict: { written: string; spoken: string[] }[]): DictHit[] {
  const hits: DictHit[] = [];
  for (const e of dict) {
    if (!wholeWord(e.written, "u").test(row.final_text)) continue;
    const spoken = e.spoken.find((s) => s.trim() && wholeWord(s.trim(), "iu").test(row.raw_text));
    if (spoken) hits.push({ spoken, written: e.written });
  }
  return hits;
}
export const wordCount = (text: string) => text.trim().split(/\s+/).filter(Boolean).length;
