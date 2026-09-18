const WEEKDAY = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MIN = 60_000, HOUR = 3_600_000, DAY = 86_400_000;

/** Formatted by hand: Chrome's en-US formatter puts a narrow no-break space before
 *  "PM", which renders narrower than the reference and breaks the pixel twin. */
export function clockTime(ms: number): string {
  const d = new Date(ms), h = d.getHours();
  return `${h % 12 || 12}:${String(d.getMinutes()).padStart(2, "0")} ${h < 12 ? "AM" : "PM"}`;
}
const startOfDay = (ms: number) => { const d = new Date(ms); d.setHours(0, 0, 0, 0); return d.getTime(); };
export function daysBetween(ms: number, now: number): number {
  return Math.round((startOfDay(now) - startOfDay(ms)) / DAY); // round absorbs a 23 h or 25 h DST day
}
export function dayLabel(ms: number, now: number): string {
  const n = daysBetween(ms, now);
  if (n <= 0) return "Today";
  if (n === 1) return "Yesterday";
  const d = new Date(ms);
  return `${WEEKDAY[d.getDay()]} ${d.getDate()} ${MONTH[d.getMonth()]}`;
}
export function relativeTime(ms: number, now: number): string {
  const diff = Math.max(0, now - ms);
  if (diff < MIN) return "just now";
  if (diff < HOUR) return `${Math.floor(diff / MIN)} min ago`;
  if (diff < DAY) return `${Math.floor(diff / HOUR)} h ago`;
  return `${dayLabel(ms, now)}, ${clockTime(ms)}`;
}
