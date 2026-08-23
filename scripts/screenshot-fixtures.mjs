/**
 * Fixture data for the README screenshots, and the Tauri stub that serves it.
 *
 * WHY THIS EXISTS. Every screen except Home, the design sheet and the Flow Bar
 * reads its state through a Tauri command. In a plain browser those commands do
 * not exist, so `inTauri()` returns false, the loading skeletons never resolve,
 * and a capture of Settings or Dictionary is an empty grey rectangle. Before
 * this file, `screenshots.mjs` handled that by declining to photograph them —
 * which left the README showing three screens out of eight, none of them the
 * ones a person is deciding about.
 *
 * WHAT IT DOES NOT DO. It does not mock a component or hand-build a fake screen.
 * The real `Hub`, the real `SettingsScreen`, the real `DictionaryScreen` render,
 * against the same `invoke` boundary the app uses. Only the far side of that
 * boundary is canned. A screenshot can still go stale against a redesign, but it
 * cannot show a layout the code does not produce.
 *
 * WHY CANNED DATA IS BETTER THAN A LIVE MACHINE. The old `hub-home.png` was
 * captured from a real install, which means every recapture published whatever
 * its operator had last dictated. Fixtures are the fix for that, and they are
 * also the only way the numbers stay the same between two runs a month apart.
 *
 * THE DATA IS TRUE. The dictionary terms, app profiles and model catalogue below
 * are copied from `ov-format/src/dictionary.rs`, `ov-format/src/profile.rs` and
 * `ov-asr/src/catalog.rs`. The two history rows are the transcriptions the README
 * quotes, which are asserted as tests in `ov-format/src/lib.rs`. The totals are
 * invented — they are one plausible person's month — and nothing in the project
 * claims otherwise.
 */

const DAY = 86_400_000;

/** Words and speaking time chosen to land on ~152 wpm, the rate the README
 *  quotes for ordinary speech, against the 40 wpm typing baseline in
 *  `stats.ts`. Deriving the milliseconds from the target rate rather than
 *  picking both means the two figures on screen cannot contradict each other. */
const WORDS = 9_540;
const TARGET_WPM = 152;
const SPEAKING_MS = Math.round((WORDS / TARGET_WPM) * 60_000);

/** A six-day streak ending today. Relative to the capture, because
 *  `streakFromTimestamps` compares against the local calendar — a hard-coded
 *  date would render "0 days" the morning after it was written. */
function activeDays(now) {
  return Array.from({ length: 6 }, (_, i) => now - i * DAY);
}

/** History rows.
 *
 *  Both `raw_text` -> `final_text` pairs are the ones README quotes and
 *  `ov-format` asserts, so the screenshot of the history list is showing real
 *  formatter output rather than an illustration of it.
 */
function history(now) {
  return [
    {
      created_at: now - 4 * 60_000,
      outcome: "delivered",
      raw_text: "um so we need to call use effect here comma then return null",
      final_text: "So we need to call useEffect here, then return null",
      profile: "editor",
      target_app: "Code.exe",
      audio_ms: 4_200,
      latency_ms: 512,
    },
    {
      created_at: now - 31 * 60_000,
      outcome: "delivered",
      raw_text: "cube control get pods",
      final_text: "kubectl get pods",
      profile: "terminal",
      target_app: "WindowsTerminal.exe",
      audio_ms: 1_600,
      latency_ms: 288,
    },
    {
      created_at: now - 2 * 3_600_000,
      outcome: "delivered",
      raw_text:
        "i think we should ship the formatter changes on friday and hold the rest",
      final_text:
        "I think we should ship the formatter changes on Friday and hold the rest.",
      profile: "prose",
      target_app: "slack.exe",
      audio_ms: 5_100,
      latency_ms: 604,
    },
    {
      created_at: now - 5 * 3_600_000,
      outcome: "clipboard_fallback",
      raw_text: "get status",
      final_text: "git status",
      profile: "terminal",
      target_app: "pwsh.exe",
      audio_ms: 1_100,
      latency_ms: 241,
    },
    {
      created_at: now - DAY - 3_600_000,
      outcome: "delivered",
      raw_text:
        "the resampler runs at sixteen kilohertz mono so the sidecar never has to guess",
      final_text:
        "The resampler runs at 16 kHz mono, so the sidecar never has to guess.",
      profile: "prose",
      target_app: "Notion.exe",
      audio_ms: 6_300,
      latency_ms: 688,
    },
  ];
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

/** Copied from `ov-asr/src/catalog.rs`. `base.en` is installed and in use,
 *  matching what a first launch actually downloads. */
const MODELS = [
  {
    id: "large-v3-turbo",
    repo: "deepdml/faster-whisper-large-v3-turbo-ct2",
    computeType: "float16",
    fallbackCompute: "int8_float16",
    sizeMb: 1600,
    vramMb: 1600,
    englishOnly: false,
    installed: false,
    installedBytes: 0,
    inUse: false,
  },
  {
    id: "small.en",
    repo: "Systran/faster-whisper-small.en",
    computeType: "float16",
    fallbackCompute: "int8_float16",
    sizeMb: 250,
    vramMb: 600,
    englishOnly: true,
    installed: true,
    installedBytes: 249_000_000,
    inUse: false,
  },
  {
    id: "base.en",
    repo: "Systran/faster-whisper-base.en",
    computeType: "int8",
    fallbackCompute: null,
    sizeMb: 75,
    vramMb: 0,
    englishOnly: true,
    installed: true,
    installedBytes: 74_500_000,
    inUse: true,
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

/** Every command the UI can issue, and what the stub answers.
 *
 *  Commands that mutate echo their input back, so a screenshot taken after a
 *  toggle shows the toggle in its new position rather than snapping back. */
export function responses(now = Date.now()) {
  return {
    get_status: {
      state: "ready",
      model: "base.en",
      device: "CPU · int8",
      shortcut: "Right Ctrl",
      mic: "Microphone Array (Realtek Audio)",
    },
    get_settings: {
      config: CONFIG,
      model: "base.en",
      dictionary: DICTIONARY,
      profiles: PROFILES,
    },
    get_history: history(now),
    get_totals: {
      sessions: 128,
      words: WORDS,
      speakingMs: SPEAKING_MS,
      topApp: { name: "Code.exe", count: 61 },
      activeDays: activeDays(now),
    },
    list_models: MODELS,
    list_microphones: [
      "Microphone Array (Realtek Audio)",
      "Headset (WH-1000XM4 Hands-Free)",
      "Yeti Nano",
    ],
    check_for_update: {
      available: false,
      version: null,
      notes: null,
      currentVersion: "0.3.0",
    },
    get_log_path: "C:\\Users\\you\\AppData\\Roaming\\OpenVoice\\openvoice.log",
    preview_format: [
      ["raw", "um so we need to call use effect here comma then return null"],
      ["fillers", "so we need to call use effect here comma then return null"],
      ["dictionary", "so we need to call useEffect here comma then return null"],
      ["commands", "so we need to call useEffect here, then return null"],
      ["capitalize", "So we need to call useEffect here, then return null"],
      // The Dictionary preview asks for the `prose` profile, and prose sets
      // `end_period`. Without this stage the box would show a sentence the
      // profile it named would not actually produce.
      ["end_period", "So we need to call useEffect here, then return null."],
    ],
  };
}

/**
 * The script installed into every page before its own JavaScript runs.
 *
 * `@tauri-apps/api` is not stubbed — the real module is imported and left alone.
 * It routes everything through `window.__TAURI_INTERNALS__`, so replacing that
 * one object is enough, and it means the code under test still exercises its own
 * argument marshalling rather than a shortcut around it.
 *
 * `plugin:event|listen` resolves without ever firing. The screens photographed
 * here are driven by `get_status` and their own commands; the event stream only
 * matters to the Flow Bar, which has `?window=flowbar` for exactly this reason
 * and needs no stub at all.
 */
export function tauriStub(now = Date.now()) {
  return `(() => {
  const RESPONSES = ${JSON.stringify(responses(now), null, 2)};
  let nextId = 1;

  window.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { windowLabel: "main", label: "main" },
    },
    plugins: {},
    // Tauri's own implementation, kept faithful: the handler is parked on
    // \`window\` under a numeric key that the Rust side calls back into. Nothing
    // calls back here, but \`listen\` reads the id it returns.
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
      if (cmd === "plugin:event|listen") return Promise.resolve(nextId++);
      if (cmd === "plugin:event|unlisten") return Promise.resolve();
      // Mutations echo, so the UI keeps whatever the capture just set.
      if (cmd === "save_settings") return Promise.resolve(args && args.settings);
      if (cmd in RESPONSES) return Promise.resolve(RESPONSES[cmd]);
      // Anything unlisted resolves empty rather than rejecting. A rejection
      // surfaces as an error banner across the screenshot, which is a worse
      // failure mode than a component rendering its own empty state.
      return Promise.resolve(null);
    },
  };
})();`;
}
