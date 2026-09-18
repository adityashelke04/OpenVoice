/**
 * Fixture data for the screenshots and the twin harness, and the Tauri stub that
 * serves it.
 *
 * WHY THIS EXISTS. Every Hub screen reads its state through a Tauri command. In a
 * plain browser those commands do not exist, so `inTauri()` returns false, the
 * loading skeletons never resolve, and a capture of Settings or Dictionary is an
 * empty grey rectangle. This file answers those commands so the real screens can
 * be photographed, and (since the redesign) so they can be pixel-diffed against
 * `docs/redesign/reference/reference.html` by `twin-check.mjs`.
 *
 * WHAT IT DOES NOT DO. It does not mock a component or hand-build a fake screen.
 * The real `Hub`, the real `SettingsScreen`, the real `DictionaryScreen` render,
 * against the same `invoke` boundary the app uses. Only the far side of that
 * boundary is canned. A screenshot can still go stale against a redesign, but it
 * cannot show a layout the code does not produce.
 *
 * WHY THE CLOCK IS FROZEN. The reference page is a still image of one afternoon:
 * Friday 18 September 2026, 14:43. Every "2 min ago", "Yesterday" and streak on
 * it is computed from that instant, so the app has to believe it is that instant
 * too, or the twin fails on the time labels alone. `tauriStub` shifts `Date` to
 * it before any page script runs; `performance.now` is left alone, so animation
 * timing is unaffected.
 *
 * THE DATA IS TRUE. The dictionary terms and app profiles below are copied from
 * `ov-format/src/dictionary.rs` and `ov-format/src/profile.rs`. The history rows
 * are the ones the reference shows (spec section 9); the `raw_text` ->
 * `final_text` pairs for useEffect, kubectl, git status and the resampler are the
 * transcriptions the README quotes and `ov-format/src/lib.rs` asserts. The totals
 * are invented: they are one plausible person's month, and nothing in the project
 * claims otherwise.
 */

/** 2026-09-18 14:43:00 in the machine's own timezone: every relative time on screen is computed from this. */
export const FROZEN_NOW = new Date(2026, 8, 18, 14, 43, 0).getTime();

/** A wall-clock time `daysAgo` days before the frozen day. Built from calendar
 *  fields rather than by subtracting milliseconds, so a DST change in the
 *  operator's timezone cannot move "yesterday 18:05" to 17:05. */
const at = (daysAgo, h, m) => new Date(2026, 8, 18 - daysAgo, h, m).getTime();

/** Words and speaking time chosen to land on ~152 wpm, the rate the README
 *  quotes for ordinary speech, against the 40 wpm typing baseline in
 *  `stats.ts`. Deriving the milliseconds from the target rate rather than
 *  picking both means the two figures on screen cannot contradict each other. */
const WORDS = 9_540;
const TARGET_WPM = 152;
const SPEAKING_MS = Math.round((WORDS / TARGET_WPM) * 60_000);

/** Home variants the reference renders (`?state=`). */
export const VARIANTS = ["normal", "failed", "first", "loading", "error"];

/** History rows, newest first, exactly as the reference lists them (spec 9). */
function history(now, variant) {
  const rows = [
    { created_at: now - 2 * 60_000, outcome: "delivered", profile: "prose", target_app: "slack.exe",
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
  if (variant === "first") return [];
  // The "not pasted" Home: the last dictation landed on the clipboard in Outlook
  // twenty seconds ago, which is the state the Fix panel exists for.
  if (variant === "failed") rows[0] = { ...rows[0], created_at: now - 20_000, outcome: "clipboard_fallback", target_app: "olk.exe",
    final_text: "Can you check whether the terminal profile still drops the trailing period, and send me the JSON export from this morning so I can compare the before and after?",
    raw_text: "can you check whether the terminal profile still drops the trailing period and send me the json export from this morning so i can compare the before and after" };
  return rows;
}

/** Totals for the stats column. A six-day streak ending on the frozen day
 *  (Sun 13 to Fri 18 Sep); `first` is a fresh install with nothing yet. */
function totals(variant) {
  if (variant === "first") return { sessions: 0, words: 0, speakingMs: 0, topApp: null, activeDays: [] };
  return {
    sessions: 128,
    words: WORDS,
    speakingMs: SPEAKING_MS,
    topApp: { name: "Code.exe", count: 61 },
    activeDays: [0, 1, 2, 3, 4, 5].map((i) => at(i, 12, 0)),
  };
}

/** Copied from `ov-format/src/dictionary.rs`. */
const DICTIONARY = [
  { written: "useEffect", spoken: ["use effect", "you seffect"], group: "code" },
  { written: "useState", spoken: ["use state"], group: "code" },
  { written: "TypeScript", spoken: ["type script"], group: "code" },
  { written: "Node.js", spoken: ["node j s", "node js"], group: "code" },
  { written: "JSON", spoken: ["jason", "j son"], group: "code" },
  { written: "async", spoken: ["a sync", "ay sink"], group: "code" },
  { written: "npm", spoken: ["n p m", "enpiem"], group: "code" },
  { written: "kubectl", spoken: ["cube control", "cube cuttle"], group: "shell" },
  { written: "nginx", spoken: ["engine x", "n g inx"], group: "shell" },
  { written: "PostgreSQL", spoken: ["postgres q l", "post gres"], group: "shell" },
  { written: "Kubernetes", spoken: ["kubernetes", "cuber netties"], group: "shell" },
  { written: "ssh", spoken: ["s s h"], group: "shell" },
];

/** Copied from `ov-format/src/profile.rs`. `default` has no `matches`, which is
 *  what makes it the fallback. */
const PROFILES = [
  {
    name: "default",
    matches: [],
    capitalize: "sentence",
    end_period: false,
    fillers: "light",
    voice_commands: true,
    case_transforms: true,
    dictionaries: ["code"],
  },
  {
    name: "terminal",
    matches: [
      "WindowsTerminal.exe",
      "powershell.exe",
      "pwsh.exe",
      "cmd.exe",
      "wt.exe",
      "alacritty.exe",
    ],
    capitalize: "force_lower",
    end_period: false,
    fillers: "light",
    voice_commands: true,
    case_transforms: true,
    dictionaries: ["shell", "code"],
  },
  {
    name: "editor",
    matches: [
      "Code.exe",
      "Cursor.exe",
      "idea64.exe",
      "devenv.exe",
      "zed.exe",
      "sublime_text.exe",
    ],
    capitalize: "sentence",
    end_period: false,
    fillers: "light",
    voice_commands: true,
    case_transforms: true,
    dictionaries: ["code"],
  },
  {
    name: "prose",
    matches: [
      "slack.exe",
      "Discord.exe",
      "Notion.exe",
      "chrome.exe",
      "msedge.exe",
      "firefox.exe",
      "olk.exe",
    ],
    capitalize: "sentence",
    end_period: true,
    fillers: "aggressive",
    voice_commands: true,
    case_transforms: false,
    dictionaries: ["code"],
  },
];


/** Mirrors `ov_core::config::Config`'s defaults. */
const CONFIG = {
  version: 1,
  chord: { key: "right_ctrl", exclusive: true },
  activation: "hold",
  limits: {
    min_duration_ms: 300,
    max_duration_ms: 120_000,
    preroll_ms: 250,
    silence_rms: 0.004,
  },
  privacy: {
    retain_audio: false,
    audio_days: 7,
    history_days: 0,
    redact_patterns: ["sk-[A-Za-z0-9]{20,}", "ghp_[A-Za-z0-9]{36}"],
  },
  updates: { check_on_launch: true },
  language: null,
  input_device: null,
  paste_threshold_chars: 120,
  sound_enabled: true,
};

/** Mirrors `ov_asr::catalog`: Standard is bundled and active, the other two are
 *  offered for download. Sizes are the catalogue's. */
const MODELS = [
  { id: "parakeet-tdt-0.6b-v2", kind: "transducer", downloadMb: 631, diskMb: 631, bundled: true, englishOnly: true, installed: true, selected: true },
  { id: "parakeet-tdt-0.6b-v3", kind: "transducer", downloadMb: 465, diskMb: 640, bundled: false, englishOnly: false, installed: false, selected: false },
  { id: "whisper-tiny.en", kind: "whisper", downloadMb: 112, diskMb: 112, bundled: false, englishOnly: true, installed: false, selected: false },
];

/** Every command the UI can issue, and what the stub answers.
 *
 *  `get_history` here is the full list; the stub filters it by `profile`,
 *  `query` and `limit` in the page, the way the Rust side does. Commands that
 *  mutate echo their input back, so a screenshot taken after a toggle shows the
 *  toggle in its new position rather than snapping back. */
export function responses(now = FROZEN_NOW, variant = "normal") {
  return {
    get_status:
      variant === "error"
        ? { state: "failed", error: "sherpa-onnx: failed to allocate 786432000 bytes" }
        : {
            state: "ready",
            model: "parakeet-tdt-0.6b-v2",
            device: "CPU · int8",
            shortcut: "Right Ctrl",
            mic: "Microphone Array (Realtek Audio)",
          },
    // The Flow Bar asks for this on launch to restore its compact/docked form.
    overlay_placement: {
      x: 0,
      y: 0,
      always_visible: true,
      hidden_until: 0,
      mini: false,
      edge: "bottom",
    },
    get_settings: {
      config: CONFIG,
      model: "parakeet-tdt-0.6b-v2",
      dictionary: DICTIONARY,
      profiles: PROFILES,
    },
    get_history: history(now, variant),
    get_totals: totals(variant),
    get_user_name: "Aditya",
    window_material: "none",
    windows_transparency: true,
    restart_reasons: [],
    list_models: MODELS,
    models_on_disk: 0,
    get_download: null,
    paste_again: "pasted",
    list_microphones: [
      "Microphone Array (Realtek Audio)",
      "Headset (WH-1000XM4 Hands-Free)",
      "Yeti Nano",
    ],
    check_for_update: {
      available: false,
      version: null,
      notes: null,
      currentVersion: "0.4.1",
    },
    get_log_path: "C:\\Users\\you\\AppData\\Roaming\\OpenVoice\\openvoice.log",
  };
}

/**
 * The formatter trace, as `preview_format` returns it, for the Dictionary /
 * Advanced sentence.
 *
 * Profile-dependent, because the two screens that ask for it ask for different
 * profiles: Dictionary requests `prose`, Advanced requests `editor`. Prose sets
 * `end_period` and editor does not, so a single fixed answer would have put a
 * full stop on the Advanced screen that the profile named directly above it
 * would never add, a screenshot contradicting the rules table beside it.
 *
 * The stages are the real ones for this sentence; it is the phrase README
 * quotes and `ov-format` asserts.
 */
function previewFormat(profile) {
  const stages = [
    ["raw", "um so we need to call use effect here comma then return null"],
    ["fillers", "so we need to call use effect here comma then return null"],
    ["dictionary", "so we need to call useEffect here comma then return null"],
    ["commands", "so we need to call useEffect here, then return null"],
    ["capitalize", "So we need to call useEffect here, then return null"],
  ];
  const endPeriod = (PROFILES.find((p) => p.name === profile) ?? PROFILES[0]).end_period;
  if (endPeriod) {
    stages.push(["end_period", "So we need to call useEffect here, then return null."]);
  }
  return stages;
}

/** The Writing style screen's sample sentence and its trace, as the reference
 *  shows it: fillers stripped, then capitalised, then the full stop. */
const STYLE_SAMPLE = "um so basically the deploy is done and you know we should ship it";
const STYLE_TRACE = [
  ["raw", STYLE_SAMPLE],
  ["fillers", "the deploy is done and we should ship it"],
  ["capitalize", "The deploy is done and we should ship it"],
  ["end_period", "The deploy is done and we should ship it."],
];

/** Traces keyed by the exact input text. Anything else falls back to the
 *  per-profile Dictionary trace, which is what the screens asked for before the
 *  Writing style sample existed. */
const TRACES_BY_TEXT = { [STYLE_SAMPLE]: STYLE_TRACE };

/**
 * The script installed into every page before its own JavaScript runs.
 *
 * `@tauri-apps/api` is not stubbed: the real module is imported and left alone.
 * It routes everything through `window.__TAURI_INTERNALS__`, so replacing that
 * one object is enough, and it means the code under test still exercises its own
 * argument marshalling rather than a shortcut around it.
 *
 * `now`     the instant the page's clock reads at load (default: the frozen one).
 * `variant` which Home state to serve: normal | failed | first | loading | error.
 * `prefs`   { theme, mode, solid } written to localStorage before the app reads it,
 *           so the first paint is already in the right theme.
 * `level`   the one microphone level the event bridge emits, so the Settings mic
 *           meter shows the lit bars the reference shows without any test hook
 *           in product code.
 */
export function tauriStub({ now = FROZEN_NOW, variant = "normal", prefs, level = 0.44 } = {}) {
  // One trace per profile, resolved here rather than in the page, so the stub
  // stays a lookup table and the rule that decides the shape lives with the
  // profiles it reads.
  const previews = Object.fromEntries(PROFILES.map((p) => [p.name, previewFormat(p.name)]));

  return `(() => {
  // 1. Freeze the clock by offset. Time still advances from the frozen instant,
  //    so timers and "just now" behave; performance.now is untouched.
  const OFFSET = ${Number(now)} - Date.now(), RealDate = Date;
  class FrozenDate extends RealDate {
    constructor(...a) { if (a.length === 0) super(RealDate.now() + OFFSET); else super(...a); }
    static now() { return RealDate.now() + OFFSET; }
  }
  window.Date = FrozenDate;

  // 2. Preferences, before the app's pre-paint read of them.
  ${
    prefs
      ? `try {
    localStorage.setItem("ov.theme", ${JSON.stringify(prefs.theme ?? "glacier")});
    localStorage.setItem("ov.mode", ${JSON.stringify(prefs.mode ?? "dark")});
    localStorage.setItem("ov.solid", ${JSON.stringify(prefs.solid ? "1" : "0")});
  } catch {}`
      : ""
  }

  // 3. The bridge.
  const RESPONSES = ${JSON.stringify(responses(now, variant), null, 2)};
  const PREVIEWS = ${JSON.stringify(previews, null, 2)};
  const TRACES_BY_TEXT = ${JSON.stringify(TRACES_BY_TEXT, null, 2)};
  const VARIANT = ${JSON.stringify(variant)};
  const LEVEL = ${Number(level)};
  const never = () => new Promise(() => {});
  const listeners = [];
  let nextId = 1;

  window.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { windowLabel: "main", label: "main" },
    },
    plugins: {},
    // Tauri's own implementation, kept faithful: the handler is parked on
    // \`window\` under a numeric key that the Rust side calls back into.
    transformCallback(callback, once) {
      const id = nextId++;
      Object.defineProperty(window, "_" + id, {
        value: (result) => {
          if (once) Reflect.deleteProperty(window, "_" + id);
          return callback && callback(result);
        },
        writable: false,
        configurable: true,
      });
      return id;
    },
    convertFileSrc(path) {
      return path;
    },
    invoke(cmd, args) {
      if (cmd === "plugin:event|listen") {
        listeners.push({ event: args && args.event, handler: args && args.handler });
        // The engine stream gets one microphone level, on the next macrotask so
        // the listener is registered by the time it arrives.
        if (args && args.event === "ov://event") {
          setTimeout(() => {
            const fn = window["_" + args.handler];
            if (fn) fn({ event: "ov://event", id: 1, payload: { type: "Level", rms: LEVEL, peak: LEVEL, elapsedMs: 0 } });
          }, 0);
        }
        return Promise.resolve(nextId++);
      }
      if (cmd === "plugin:event|unlisten") return Promise.resolve();
      // Mutations echo, so the UI keeps whatever the capture just set.
      if (cmd === "save_settings") return Promise.resolve(args && args.settings);
      if (cmd === "preview_format") {
        const byText = args && TRACES_BY_TEXT[args.text];
        if (byText) return Promise.resolve(byText);
        return Promise.resolve(PREVIEWS[(args && args.profile) || "default"] ?? PREVIEWS.default);
      }
      if (VARIANT === "loading" && (cmd === "get_history" || cmd === "get_totals")) return never();
      if (cmd === "get_history") {
        let rows = RESPONSES.get_history;
        if (args && args.profile) rows = rows.filter((r) => r.profile === args.profile);
        if (args && args.query) {
          const q = String(args.query).toLowerCase();
          rows = rows.filter((r) => r.final_text.toLowerCase().includes(q));
        }
        if (args && args.limit) rows = rows.slice(0, args.limit);
        return Promise.resolve(rows);
      }
      if (cmd in RESPONSES) return Promise.resolve(RESPONSES[cmd]);
      // Anything unlisted resolves empty rather than rejecting. A rejection
      // surfaces as an error banner across the screenshot, which is a worse
      // failure mode than a component rendering its own empty state.
      return Promise.resolve(null);
    },
  };
})();`;
}
