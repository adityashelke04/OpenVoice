/** The Flow Bar's sizes: the one place that decides how big the pill is.
 *
 * Its own module so the numbers can be tested without mounting the window, and
 * so `Overlay.tsx` stays a component file. */
import { flowSpeaks } from "../ui";
import type { FlowEdge, FlowMode } from "../ui";
import { SANS_12, resolveFont } from "./useFontsReady";

/**
 * The pill's height, in every state but the menu.
 *
 * Duplicated as `--pill-h` in `overlay.css` and as `PILL_H` in `overlay.rs`,
 * deliberately and with a comment at each end. CSS paints the box; Rust clips the
 * window to a region computed from the same number. If they disagree, the bar and
 * its clickable area come apart. Change one and the other two are wrong.
 */
export const PILL_H = 40;

/**
 * The compact indicator's short axis.
 *
 * Big enough to hold the 7px status dot and a hairline border with room left to
 * read as a pill rather than as a line.
 */
export const MINI_H = 22;

/**
 * The collapsed bar's width. Long enough to find, short enough to ignore.
 *
 * Was 64. Widened because "short enough to ignore" turned out to be the easy
 * half of that sentence and "long enough to find" the hard one: at 64x5 the
 * first person to use it could not locate the bar on their own screen.
 *
 * Its *height* is not here. The stroke is 4px, and that number lives only in
 * `overlay.css` as `--line-h`, because only CSS paints it — this file's business
 * is the box the window is clipped to, which is `LINE_HIT`. Putting the stroke
 * height here too would create a third opinion about a size, which is the exact
 * failure mode `geometry()` exists to prevent.
 */
const LINE_W = 96;

/**
 * The collapsed bar's *clickable* height, which is not the height it paints.
 *
 * This one lives here alone. `PILL_H` is triplicated because Rust genuinely
 * computes with it in `shape_rect`; this number only ever travels to Rust as
 * part of the box `set_shape` is handed, so a mirrored constant over there
 * would be a second opinion with no reader — the very thing `geometry()` exists
 * to prevent. Clippy caught it as dead code, and clippy was right.
 *
 * The stroke is 8px (`--line-h` in `overlay.css`), and the window is clipped to
 * a band three times that, so the extra is invisible target.
 *
 * This still sits under the 44px minimum-target guideline, and the reason is the
 * one in `useWindowBox`: on a transparent, click-through-less window every pixel
 * of hit area is a dead zone punched into whatever is underneath, so a 44px band
 * would be a 96x44 hole in the user's screen for a bar they deliberately put
 * away.
 *
 * But 16 was too far the other way. The dead-zone argument justifies being under
 * 44; it does not justify being as small as possible, and treating it as though
 * it did produced a control that was hard to find and hard to hit. 24 keeps the
 * hole modest while giving the pointer something real to land on.
 */
const LINE_HIT = 24;

/** The pill's painted box, in logical pixels. */
export type Geo = { w: number; h: number };

/**
 * How big the pill has to be to say what it is currently saying.
 *
 * One function, consulted by the shape sent to Rust and by the pill's own style,
 * so the region the window is clipped to and the box CSS paints cannot disagree.
 * It replaces a nested ternary that knew about four states and a fixed CSS height
 * that knew about none of them — which is what made a compact or a docked form
 * impossible to express.
 */
export type GeometryInput = {
  mode: FlowMode;
  mini: boolean;
  /** Put away on the idle clock. Outranks every tier below except the menu. */
  collapsed?: boolean;
  edge: FlowEdge;
  menu: boolean;
  hasAction?: boolean;
  text?: string;
};

export function geometry(v: GeometryInput): Geo {
  // When the menu is open, the pill is 280px wide to match the menu, but maintains
  // its standard height (PILL_H or MINI_H). The window's overall shape is expanded
  // to menuHeight(rows) separately in useWindowShape.
  if (v.menu) return { w: 280, h: v.mini ? MINI_H : PILL_H };

  // Put away. Above every tier below it and below the menu, because opening the
  // menu is a deliberate act and a bar that stayed a stroke under its own open
  // menu would be a panel hanging off nothing.
  //
  // No mode test here on purpose: every mode worth reading — live, working, and
  // all four that `flowSpeaks` covers — blocks the clock in `useIdleCollapse`,
  // so by the time this is reached the bar has nothing to say. Testing the mode
  // again would be a second copy of that rule, drifting from the first.
  if (v.collapsed) {
    // The box is `LINE_HIT`, not `LINE_H`. What this function returns is the
    // region the window is clipped to, and the 4px stroke is painted centred
    // inside it by `overlay.css` — so the invisible margin that makes a 4px bar
    // clickable lives in one place, and this stays the single sizing authority
    // rather than growing a second rule about hit areas.
    //
    // On a side edge the stroke stands up with the bar. A horizontal line on a
    // vertical edge reads as a scrap of some other window.
    const column = v.edge !== "bottom";
    return column ? { w: LINE_HIT, h: LINE_W } : { w: LINE_W, h: LINE_HIT };
  }

  // Docked to a side edge, with nothing that needs words: a column. Anything
  // with a sentence to deliver unfurls back to horizontal — see `flowSpeaks`.
  const column = v.edge !== "bottom" && !flowSpeaks(v.mode);
  if (column) {
    const short = v.mini ? MINI_H : 34;
    if (v.mode === "live") return { w: short, h: v.mini ? 74 : 132 };
    // Tall enough for the mark stood on end (39px, or 26 compact) plus the
    // column's padding: a shorter box would shave its top and bottom bars.
    return { w: short, h: v.mini ? 36 : 60 };
  }

  if (v.mini) {
    // Nine waveform bars while live; the mark alone otherwise.
    return { w: v.mode === "live" ? 78 : 48, h: MINI_H };
  }

  if (v.mode === "live") return { w: 240, h: PILL_H };
  if (v.mode === "working") return { w: 156, h: PILL_H };

  // Measured from their own content rather than fixed. The old 248px alert tier
  // truncated real engine messages at about thirty characters, which is reliably
  // before the part that says what to do about it.
  if (v.text !== undefined) {
    // The Paste chip (24px tall, "Paste" and "Ctrl V") and the gap before it.
    const actionW = v.hasAction ? 104 : 0;
    const w = Math.ceil(MSG_CHROME + textWidth(v.text, SANS_12) + actionW);
    return { w: Math.min(380, Math.max(200, w)), h: PILL_H };
  }

  // Idle: a capsule around the seven-bar mark. Fixed, because nothing in it
  // varies — the shortcut used to be written here and sized the bar, and is now
  // only in the tooltip and the Flow Menu.
  return { w: 88, h: PILL_H };
}

/** Everything in a message pill that is not the words: 12px padding each side,
 *  the 1px border each side, the 16px glyph and the 9px gap after it. Taken from
 *  `flowbar.css`; change them together. */
const MSG_CHROME = 12 * 2 + 1 * 2 + 16 + 9;

/**
 * Width of a string as the bar will actually draw it.
 *
 * A canvas rather than a hidden DOM node: this has to be answered before the
 * window is sized, and a measuring element would need a layout pass inside a
 * window that is still the wrong size to hold it.
 */
function textWidth(text: string, font: string): number {
  const canvas = (textWidth as { c?: HTMLCanvasElement }).c ??
    ((textWidth as { c?: HTMLCanvasElement }).c = document.createElement("canvas"));
  const ctx = canvas.getContext("2d");
  if (!ctx) return text.length * 7;
  // The same substitution the preload in `useFontsReady` applies, so the face
  // that was loaded is by construction the face being measured. When it was
  // written out twice, only one of the two knew what a `$mono` was.
  ctx.font = resolveFont(font);
  return ctx.measureText(text).width;
}
