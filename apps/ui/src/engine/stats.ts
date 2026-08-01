/** What the numbers on Home actually mean.
 *
 * The first version showed sessions, median latency and the model name. Those are
 * *engineering* metrics — they measure how the software is doing, not how the
 * person is doing. Nobody dictates in order to lower a latency figure.
 *
 * These are the metrics the category converged on, and they hold up: how fast you
 * speak, how much time that saved you, how much you have written, and whether you
 * kept it up. Every one of them is about the user.
 *
 * Baselines are published research, not guesses:
 *   - Average conversational speech ≈ 150 wpm (NCVS / NIH).
 *   - Average typing ≈ 40 wpm.
 * Dictation is therefore roughly 3.75× faster than typing for most people, which is
 * what makes "time saved" a real number rather than marketing.
 */

/** Words per minute a typical person types. The baseline "time saved" is against. */
export const TYPING_WPM = 40;
/** Average conversational speaking rate, for context on the user's own figure. */
export const SPEAKING_WPM = 150;

export interface Row {
  created_at: number;
  outcome: string;
  raw_text: string;
  final_text: string;
  profile: string;
  target_app: string;
  audio_ms: number;
  latency_ms: number;
}

export interface Stats {
  /** Total words actually delivered. */
  words: number;
  /** The user's speaking rate, from audio duration — not wall-clock time. */
  wpm: number | null;
  /** Minutes saved versus typing the same words at [`TYPING_WPM`]. */
  savedMinutes: number;
  /** Consecutive calendar days with at least one dictation, counting back. */
  streak: number;
  /** Where they dictate most, and how many times. */
  topApp: { name: string; count: number } | null;
  /** Whether there is enough data for the figures to mean anything. */
  meaningful: boolean;
}

/** Aggregates computed in SQL over the *whole* history. */
export interface Totals {
  sessions: number;
  words: number;
  speakingMs: number;
  topApp: { name: string; count: number } | null;
  /** Timestamps of sessions, newest first — the streak is derived here because it
   *  depends on the user's timezone, which the database cannot know. */
  activeDays: number[];
}

/**
 * Build stats from server-side totals.
 *
 * Preferred over [`computeStats`], which sums whatever page of rows the UI happens
 * to be holding. That silently ignored everything older than the last 200 sessions,
 * so the word count and speaking rate quietly became wrong the moment history grew
 * past a page — with no symptom other than numbers that were too small.
 */
export function statsFromTotals(t: Totals): Stats {
  const wpm = t.speakingMs > 2000 ? Math.round(t.words / (t.speakingMs / 60_000)) : null;
  const savedMinutes =
    wpm && wpm > TYPING_WPM ? t.words / TYPING_WPM - t.words / wpm : 0;

  return {
    words: t.words,
    wpm,
    savedMinutes,
    streak: streakFromTimestamps(t.activeDays),
    topApp: t.topApp,
    meaningful: t.sessions >= 3,
  };
}

const words = (s: string) => s.trim().split(/\s+/).filter(Boolean).length;

export function computeStats(rows: Row[]): Stats {
  // Only delivered sessions count. Including cancelled or silent ones would
  // deflate the speaking rate with recordings that contain no speech.
  const good = rows.filter((r) => r.outcome === "delivered" || r.outcome === "clipboard_fallback");

  const totalWords = good.reduce((n, r) => n + words(r.final_text), 0);
  const totalAudioMs = good.reduce((n, r) => n + (r.audio_ms || 0), 0);

  // Rate is words over *speaking* time. Measuring against wall-clock would punish
  // the user for pausing to think, which is not a typing speed anyone would
  // recognise as theirs.
  const wpm =
    totalAudioMs > 2000 ? Math.round(totalWords / (totalAudioMs / 60_000)) : null;

  // Saved against typing the same words. Deliberately compares to the typing
  // baseline rather than to zero: the alternative to dictating is typing, not
  // silence.
  const savedMinutes =
    wpm && wpm > TYPING_WPM ? totalWords / TYPING_WPM - totalWords / wpm : 0;

  return {
    words: totalWords,
    wpm,
    savedMinutes,
    streak: computeStreak(good),
    topApp: computeTopApp(good),
    meaningful: good.length >= 3,
  };
}

function computeStreak(rows: Row[]): number {
  return streakFromTimestamps(rows.map((r) => r.created_at));
}

/** Consecutive calendar days ending today or yesterday.
 *
 * Yesterday still counts, so the streak does not appear broken first thing in the
 * morning before you have dictated anything. Local dates, because a streak is
 * about your days, not UTC's — which is exactly why this is computed here rather
 * than in SQL. */
export function streakFromTimestamps(timestamps: number[]): number {
  if (timestamps.length === 0) return 0;

  const dayKey = (ms: number) => {
    const d = new Date(ms);
    return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  };

  const days = new Set(timestamps.map(dayKey));
  const today = new Date();

  // Allow the streak to be "alive" if the last activity was yesterday.
  let cursor = new Date(today);
  if (!days.has(dayKey(cursor.getTime()))) {
    cursor.setDate(cursor.getDate() - 1);
    if (!days.has(dayKey(cursor.getTime()))) return 0;
  }

  let streak = 0;
  while (days.has(dayKey(cursor.getTime()))) {
    streak += 1;
    cursor.setDate(cursor.getDate() - 1);
  }
  return streak;
}

function computeTopApp(rows: Row[]): { name: string; count: number } | null {
  const counts = new Map<string, number>();
  for (const r of rows) {
    if (!r.target_app) continue;
    counts.set(r.target_app, (counts.get(r.target_app) ?? 0) + 1);
  }
  const top = [...counts.entries()].sort((a, b) => b[1] - a[1])[0];
  return top ? { name: top[0], count: top[1] } : null;
}

/** Put a word count in human terms.
 *
 * A bare "174 words" means nothing to most people. Anchoring it to something they
 * have written before is what turns a number into a sense of scale — and it is the
 * one thing people actually screenshot and share. */
export function wordsInPerspective(n: number): string {
  if (n < 50) return "a couple of messages";
  if (n < 200) return "about a short email";
  if (n < 600) return "about a long email";
  if (n < 1500) return "about a blog post";
  if (n < 5000) return "about a long article";
  if (n < 20_000) return "about a dissertation chapter";
  if (n < 60_000) return "about a novella";
  if (n < 150_000) return "about a novel";
  return "more than most novels";
}

/** Minutes as something readable: "3 min", "1 h 20 m", "2 days". */
export function humanDuration(minutes: number): { value: string; unit: string } {
  if (minutes < 1) return { value: "<1", unit: "min" };
  if (minutes < 90) return { value: String(Math.round(minutes)), unit: "min" };
  const hours = minutes / 60;
  if (hours < 24) {
    const h = Math.floor(hours);
    const m = Math.round((hours - h) * 60);
    return { value: m > 0 ? `${h}h ${m}` : String(h), unit: m > 0 ? "m" : "hours" };
  }
  return { value: (hours / 24).toFixed(1), unit: "days" };
}

/** How the user's rate compares to the conversational average. */
export function speedContext(wpm: number): string {
  const ratio = wpm / TYPING_WPM;
  if (ratio >= 1.2) return `${ratio.toFixed(1)}× faster than typing`;
  if (ratio >= 1) return "about as fast as typing";
  return "slower than typing — try speaking in longer runs";
}
