/** The twin's fixture data (scripts/screenshot-fixtures.mjs, spec 9), copied for
 *  component tests so they read the same afternoon the reference shows:
 *  Friday 18 September 2026, 14:43, nine dictations, 9,540 words at 152 wpm. */
import type { Row, Totals } from "../engine/stats";
import type { DictEntry } from "../engine/settings";

export const NOW = new Date(2026, 8, 18, 14, 43).getTime();

/** A wall-clock time `daysAgo` days before the frozen day (calendar fields, so DST cannot move it). */
const at = (daysAgo: number, h: number, m: number) => new Date(2026, 8, 18 - daysAgo, h, m).getTime();

export const ROWS: Row[] = [
  { created_at: NOW - 2 * 60_000, outcome: "delivered", profile: "prose", target_app: "slack.exe",
    final_text: "I think we should ship the formatter changes on Friday and hold the rest until the dictionary sync is reviewed. Can you check whether the terminal profile still drops the trailing period, and send me the JSON export from this morning so I can compare the before and after?",
    raw_text: "i think we should ship the formatter changes on friday and hold the rest until the dictionary sync is reviewed can you check whether the terminal profile still drops the trailing period and send me the json export from this morning so i can compare the before and after",
    audio_ms: 19_000, latency_ms: 604 },
  { created_at: at(0, 14, 39), outcome: "delivered", profile: "editor", target_app: "Code.exe", final_text: "So we need to call useEffect here, then return null", raw_text: "um so we need to call use effect here comma then return null", audio_ms: 4_200, latency_ms: 512 },
  { created_at: at(0, 14, 12), outcome: "delivered", profile: "terminal", target_app: "WindowsTerminal.exe", final_text: "kubectl get pods", raw_text: "cube control get pods", audio_ms: 1_600, latency_ms: 288 },
  { created_at: at(0, 11, 8), outcome: "delivered", profile: "prose", target_app: "chrome.exe", final_text: "Moving the standup to 10:30 tomorrow, same link as always.", raw_text: "moving the standup to 10:30 tomorrow same link as always", audio_ms: 3_900, latency_ms: 455 },
  { created_at: at(1, 18, 5), outcome: "clipboard_fallback", profile: "terminal", target_app: "pwsh.exe", final_text: "git status", raw_text: "get status", audio_ms: 1_100, latency_ms: 241 },
  { created_at: at(1, 16, 30), outcome: "delivered", profile: "prose", target_app: "Notion.exe", final_text: "The resampler runs at 16 kHz mono, so the sidecar never has to guess.", raw_text: "the resampler runs at sixteen kilohertz mono so the sidecar never has to guess", audio_ms: 6_300, latency_ms: 688 },
  { created_at: at(2, 17, 52), outcome: "delivered", profile: "prose", target_app: "slack.exe", final_text: "Thanks, that fixed it. I'll push the branch after lunch.", raw_text: "thanks that fixed it i'll push the branch after lunch", audio_ms: 3_000, latency_ms: 402 },
  { created_at: at(2, 15, 17), outcome: "delivered", profile: "terminal", target_app: "WindowsTerminal.exe", final_text: "npm run build", raw_text: "npm run build", audio_ms: 1_200, latency_ms: 250 },
  { created_at: at(2, 9, 44), outcome: "delivered", profile: "default", target_app: "notepad.exe", final_text: "Draft the release notes for 1.0.1 and keep them under a hundred words.", raw_text: "draft the release notes for one point zero point one and keep them under a hundred words", audio_ms: 4_400, latency_ms: 530 },
];

/** The "not pasted" Home: the last dictation landed on the clipboard in Outlook twenty seconds ago. */
export const FAILED_ROW: Row = {
  ...ROWS[0], created_at: NOW - 20_000, outcome: "clipboard_fallback", target_app: "olk.exe",
  final_text: "Can you check whether the terminal profile still drops the trailing period, and send me the JSON export from this morning so I can compare the before and after?",
  raw_text: "can you check whether the terminal profile still drops the trailing period and send me the json export from this morning so i can compare the before and after",
};

const WORDS = 9_540;
export const TOTALS: Totals = {
  sessions: 128,
  words: WORDS,
  speakingMs: Math.round((WORDS / 152) * 60_000),
  topApp: { name: "Code.exe", count: 61 },
  activeDays: [0, 1, 2, 3, 4, 5].map((i) => at(i, 12, 0)),
};

/** The dictionary terms the fixture rows exercise (from ov-format/src/dictionary.rs). */
export const DICTIONARY: DictEntry[] = [
  { written: "useEffect", spoken: ["use effect", "you seffect"], group: "code" },
  { written: "kubectl", spoken: ["cube control", "cube cuttle"], group: "shell" },
  { written: "JSON", spoken: ["jason", "j son"], group: "code" },
];

/** All twelve terms the twin's fixture dictionary holds, in the order the
 *  reference's Dictionary screen lists them (scripts/screenshot-fixtures.mjs
 *  stores them in this order too). */
export const DICTIONARY_12: DictEntry[] = [
  { written: "useEffect", spoken: ["use effect", "you seffect"], group: "code" },
  { written: "kubectl", spoken: ["cube control", "cube cuttle"], group: "shell" },
  { written: "useState", spoken: ["use state"], group: "code" },
  { written: "nginx", spoken: ["engine x", "n g inx"], group: "shell" },
  { written: "TypeScript", spoken: ["type script"], group: "code" },
  { written: "PostgreSQL", spoken: ["postgres q l", "post gres"], group: "shell" },
  { written: "Node.js", spoken: ["node j s", "node js"], group: "code" },
  { written: "Kubernetes", spoken: ["kubernetes", "cuber netties"], group: "shell" },
  { written: "JSON", spoken: ["jason", "j son"], group: "code" },
  { written: "ssh", spoken: ["s s h"], group: "shell" },
  { written: "async", spoken: ["a sync", "ay sink"], group: "code" },
  { written: "npm", spoken: ["n p m", "enpiem"], group: "code" },
];
