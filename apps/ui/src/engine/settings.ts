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
    history_days: number;
    redact_patterns: string[];
  };
  model: string;
  /** Forced ISO 639-1 code (`"en"`, `"es"`, ...), or `null` to auto-detect. */
  language: string | null;
  input_device: string | null;
  paste_threshold_chars: number;
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

/** Models the sidecar knows about, with the numbers measured on this machine. */
export const MODELS = [
  {
    id: "large-v3-turbo",
    name: "Accurate",
    detail: "Best quality. Needs about 1.6 GB of graphics memory.",
    size: "1.6 GB",
    speed: "~650 ms",
  },
  {
    id: "small.en",
    name: "Light",
    detail: "Good quality, English only. Runs alongside a game.",
    size: "250 MB",
    speed: "~300 ms",
  },
  {
    id: "base.en",
    name: "Fastest",
    detail: "Lowest quality. Works without a graphics card.",
    size: "75 MB",
    speed: "~190 ms",
  },
] as const;
