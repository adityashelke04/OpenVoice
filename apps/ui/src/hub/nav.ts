/** The Hub's screens and the six sidebar sections (spec 6.1).
 *
 *  The ids double as the payloads of the `hub-navigate` event the Flow Bar sends,
 *  so they are part of a contract with Rust and must not be renamed. "history" is
 *  a screen but not a section: it is reached from Home and keeps Home current. */
import { BookOpenText, Flask, HouseSimple, PenNib, SlidersHorizontal, Waveform, type Icon } from "@phosphor-icons/react";

export type ScreenId = "home" | "history" | "dictionary" | "style" | "models" | "settings" | "advanced";

export const NAV: { id: Exclude<ScreenId, "history">; label: string; Icon: Icon; key: string }[] = [
  { id: "home", label: "Home", Icon: HouseSimple, key: "1" },
  { id: "dictionary", label: "Dictionary", Icon: BookOpenText, key: "2" },
  { id: "style", label: "Writing style", Icon: PenNib, key: "3" },
  { id: "models", label: "Speech model", Icon: Waveform, key: "4" },
  { id: "settings", label: "Settings", Icon: SlidersHorizontal, key: "5" },
  { id: "advanced", label: "Advanced", Icon: Flask, key: "6" },
];

const IDS: readonly string[] = ["history", ...NAV.map((n) => n.id)];

export function isScreenId(x: unknown): x is ScreenId {
  return typeof x === "string" && IDS.includes(x);
}
