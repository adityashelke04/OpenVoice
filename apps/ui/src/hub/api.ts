/** Typed wrappers over the Hub's Tauri commands. One place names each command and
 *  its argument shape, so a rename on the Rust side breaks one file, not six. */
import type { Row, Totals } from "../engine/stats";

export type ProfileFilter = "all" | "editor" | "terminal" | "prose";

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Outside Tauri (plain browser) every call resolves to `fallback`, so Home renders its first-run state. */
async function call<T>(cmd: string, args: Record<string, unknown> | undefined, fallback: T): Promise<T> {
  if (!inTauri()) return fallback;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export const getHistory = (o: { limit?: number; query?: string; profile?: ProfileFilter } = {}) =>
  call<Row[]>("get_history", { limit: o.limit ?? 200, query: o.query?.trim() || null, profile: o.profile && o.profile !== "all" ? o.profile : null }, []);
export const getTotals = () => call<Totals>("get_totals", undefined, { sessions: 0, words: 0, speakingMs: 0, topApp: null, activeDays: [] });
export const getUserName = () => call<string | null>("get_user_name", undefined, null);
export const windowMaterial = () => call<"mica" | "none">("window_material", undefined, "none");
export const windowsTransparency = () => call<boolean>("windows_transparency", undefined, true);
