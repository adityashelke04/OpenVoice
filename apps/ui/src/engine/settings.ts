/** Settings shared with the Rust store.
 *
 * Mirrors `ov_app::settings::Settings`, which in turn embeds `ov_core::config::Config`.
 * The whole document is sent back on save — for a single-user desktop app that is
 * simpler than patching and has no lost-update problem worth the machinery.
 */

export interface DictEntry {
  written: string;
  spoken: string[];
  group: string;
}

export interface UpdateConfig {
  /** Ask once per launch whether a newer release exists. Off means no request
   *  is made at all — not that one is made with an opt-out flag. */
  check_on_launch: boolean;
}

export interface Config {
  version: number;
  chord: { key: string; exclusive: boolean };
  activation: string;
  limits: {
    min_duration_ms: number;
    max_duration_ms: number;
    preroll_ms: number;
    silence_rms: number;
  };
  privacy: {
    retain_audio: boolean;
    /** Days of recordings to keep. 0 keeps them indefinitely. Independent of
     *  `history_days` — turning recordings off never touches your transcripts. */
    audio_days: number;
    history_days: number;
    redact_patterns: string[];
  };
  updates: UpdateConfig;
  /** Forced ISO 639-1 code (`"en"`, `"es"`, ...), or `null` to auto-detect. */
  language: string | null;
  input_device: string | null;
  paste_threshold_chars: number;
  /** Whether the UI plays a short tone on start and on a successful finish. */
  sound_enabled: boolean;
}

/** Filler-removal level. Mirrors `ov_format::profile::FillerLevel`, which
 *  serialises snake_case. */
export type FillerLevel = "off" | "light" | "aggressive";

/** Sentence capitalisation policy. Mirrors `ov_format::profile::Capitalization`.
 *  `force_lower` is what keeps `git status` from becoming `Git status` in a
 *  terminal. */
export type Capitalization = "off" | "sentence" | "force_lower";

/** A named set of formatting rules bound to a set of executables.
 *  Mirrors `ov_format::profile::Profile`. */
export interface Profile {
  name: string;
  /** Executable names this profile applies to, matched case-insensitively.
   *  Empty means "the fallback for everything not matched elsewhere". */
  matches: string[];
  capitalize: Capitalization;
  end_period: boolean;
  fillers: FillerLevel;
  voice_commands: boolean;
  case_transforms: boolean;
  dictionaries: string[];
}

/** Teach the dictionary one correction, in place.
 *
 * Shared by the Dictionary screen and the Fix action on a history row, so both
 * behave identically — and so the matching rule below lives in exactly one file.
 *
 * Written forms are matched **case-insensitively** when deciding whether an
 * entry already exists. `useEffect` and `useeffect` are the same word to a
 * person, and letting both exist produced two entries competing for the same
 * spoken phrase, of which the formatter silently keeps whichever was compiled
 * first. The user's own capitalisation is kept — it is the thing they are
 * teaching — but a second spelling extends the existing entry instead of
 * starting a rival one.
 *
 * Returns what happened, so the caller can say so rather than leaving the user
 * unsure whether anything was saved.
 */
export function addDictionaryTerm(
  s: Settings,
  spoken: string,
  written: string,
): "added" | "extended" | "already-known" {
  const heard = spoken.trim().toLowerCase();
  const write = written.trim();
  if (!heard || !write) return "already-known";

  const existing = s.dictionary.find((t) => t.written.toLowerCase() === write.toLowerCase());
  if (!existing) {
    s.dictionary.unshift({ written: write, spoken: [heard], group: "code" });
    return "added";
  }
  if (existing.spoken.includes(heard)) return "already-known";
  existing.spoken.push(heard);
  return "extended";
}

export interface Settings {
  config: Config;
  model: string;
  dictionary: DictEntry[];
  /** Per-application writing rules. Never empty: the Rust store reseeds it from
   *  the builtins if the file contains none. */
  profiles: Profile[];
}

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!inTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export const loadSettings = () => call<Settings>("get_settings");
export const saveSettings = (settings: Settings) => call<Settings>("save_settings", { settings });
export const listMicrophones = () => call<string[]>("list_microphones");
export const previewFormat = (text: string, profile: string) =>
  call<[string, string][]>("preview_format", { text, profile });
export const openDataDir = () => call<void>("open_data_dir");
export const getLogPath = () => call<string>("get_log_path");
export const restartApp = () => call<void>("restart_app");

/** Try starting the engine again after a failure, without relaunching the app.
 *  Resolves false when an attempt is already running. */
export const retryEngine = () => call<boolean>("retry_engine");

/** What has been saved but cannot reach the running engine, in the user's words.
 *
 *  Answered by the Rust side, which diffs what the live engine booted with against
 *  what is on disk. Deliberately not computed here: the UI has no way of knowing
 *  which fields the engine can adopt in place, and a hardcoded list in the front
 *  end is exactly how a screen ends up telling people to restart for a setting
 *  that already took effect. */
export const restartReasons = () => call<string[]>("restart_reasons");

/** Keys that can be bound, and how to name them on screen.
 *
 *  Mirrors `ov_core::config::Key::ALL` and `Key::label()`. The serde name is the
 *  snake_case value stored in settings.toml; the label is what a person reads.
 *  Ordered by how likely the key is to be free, not alphabetically.
 */
export const HOTKEYS: readonly [value: string, label: string][] = [
  ["right_ctrl", "Right Ctrl"],
  ["right_alt", "Right Alt"],
  ["right_shift", "Right Shift"],
  ["caps_lock", "Caps Lock"],
  ["scroll_lock", "Scroll Lock"],
  ["pause", "Pause"],
  ["f13", "F13"],
  ["f14", "F14"],
  ["f15", "F15"],
  ["f16", "F16"],
  ["f17", "F17"],
  ["f18", "F18"],
  ["f19", "F19"],
  ["f20", "F20"],
  ["f21", "F21"],
  ["f22", "F22"],
  ["f23", "F23"],
  ["f24", "F24"],
];

/** What an update check found. Mirrors `ov_app::update::UpdateStatus`. */
export interface UpdateStatus {
  available: boolean;
  version: string | null;
  notes: string | null;
  currentVersion: string;
}

/** Ask whether a newer version exists. Makes one request, carries nothing. */
export const checkForUpdate = () => call<UpdateStatus>("check_for_update");

/** Download, verify and apply an update, then restart. Only ever from a button —
 *  nothing is downloaded as a side effect of checking. */
export const installUpdate = () => call<void>("install_update");

/* -- Speech models ---------------------------------------------------------- */

/** A model as the Rust side describes it.
 *
 *  Mirrors `ov_app::models::ModelRow`, which flattens `ov_asr::catalog::ModelSpec`.
 *  Every fact here comes from the catalogue; this file adds only the words used
 *  to describe them. */
export interface ModelSpec {
  id: string;
  kind: "transducer" | "whisper";
  downloadMb: number;
  diskMb: number;
  bundled: boolean;
  englishOnly: boolean;
  /** Every file present. A part-finished download is not installed. */
  installed: boolean;
  /** What the app will load at the next start. */
  selected: boolean;
}

export const listModels = () => call<ModelSpec[]>("list_models");
export const downloadModel = (id: string) => call<void>("download_model", { id });
export const deleteModel = (id: string) => call<void>("delete_model", { id });
export const modelsOnDisk = () => call<number>("models_on_disk");

/** Bytes fetched so far by whatever download is running, or null if none is.
 *
 *  Polled rather than pushed: a 465 MB transfer can begin before the webview has
 *  finished loading, so an event would be published to nobody. */
export const getDownload = () =>
  call<{ model: string; done: number; total: number } | null>("get_download");

/** Bytes as a person would write them. */
export function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${Math.round(bytes / 1e6)} MB`;
  if (bytes > 0) return "under 1 MB";
  return "nothing";
}

/** Megabytes as a person would write them. */
export function formatSize(mb: number): string {
  return mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${mb} MB`;
}

/** How a model is described to someone who has never heard of Parakeet.
 *
 *  Deliberately *not* in the Rust catalogue: "Multilingual" is copy and "~0.6 s"
 *  is a measurement taken on one particular laptop, and neither is a property of
 *  the model. A model with no entry here still renders, labelled with its id.
 *
 *  No accuracy figure appears in any of these. The measured one came from audio
 *  that is in-domain for Parakeet (see ADR 0008), so it is a ceiling rather than
 *  a forecast, and a number on screen would outlive that caveat. */
export const MODEL_COPY: Record<string, { name: string; detail: string; speed: string }> = {
  "parakeet-tdt-0.6b-v2": {
    name: "Standard",
    detail: "English. Included with OpenVoice, so it is always available — even offline.",
    speed: "~0.5 s",
  },
  "parakeet-tdt-0.6b-v3": {
    name: "Multilingual",
    detail:
      "25 languages, detected as you speak with nothing to configure. Same speed and size as Standard.",
    speed: "~0.6 s",
  },
  "whisper-tiny.en": {
    name: "Light",
    detail:
      "English. A sixth of the disk and far less memory, and noticeably less accurate — for machines that cannot spare the room.",
    speed: "~0.5 s",
  },
};
