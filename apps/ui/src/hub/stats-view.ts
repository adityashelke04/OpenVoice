import { TYPING_WPM } from "../engine/stats";
import { daysBetween } from "./time";
export const SCALE_MAX_WPM = 200;
const clamp = (n: number) => Math.min(100, Math.max(0, n));
export function speedBar(wpm: number) {
  return { fillPct: clamp((wpm / SCALE_MAX_WPM) * 100), typingPct: (TYPING_WPM / SCALE_MAX_WPM) * 100 };
}
export const multiple = (wpm: number) => (wpm / TYPING_WPM).toFixed(1);
export function weekBars(activeDays: number[], now: number): boolean[] {
  const back = new Set(activeDays.map((ms) => daysBetween(ms, now)));
  return Array.from({ length: 7 }, (_, i) => back.has(6 - i));
}
export function savedParts(minutes: number): { big: string; small: string } {
  if (minutes < 1) return { big: "<1", small: "min" };
  if (minutes < 90) return { big: String(Math.round(minutes)), small: "min" };
  const hours = minutes / 60;
  if (hours < 24) {
    let h = Math.floor(hours), m = Math.round((hours - h) * 60);
    if (m === 60) { h += 1; m = 0; }
    return m > 0 ? { big: `${h}h`, small: `${m}m` } : { big: String(h), small: h === 1 ? "hour" : "hours" };
  }
  return { big: (hours / 24).toFixed(1), small: "days" };
}
export const sentenceCase = (s: string) => (s ? s[0].toUpperCase() + s.slice(1) : s);
