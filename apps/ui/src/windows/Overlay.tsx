/** The Flow Bar window.
 *
 * Frameless, transparent, always on top, and non-activating (`WS_EX_NOACTIVATE`,
 * applied on the Rust side). If this window ever takes focus, the caret in the
 * user's editor is lost and the dictated text goes nowhere.
 *
 * `WS_EX_NOACTIVATE` prevents *focus*, not *input* — so the bar can be dragged and
 * right-clicked while the editor keeps the caret. That is the only reason an
 * interactive always-on-top overlay is viable for a dictation tool.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { FlowBar, Kbd, flowMode, flowSpeaks, flowText } from "../ui";
import type { FlowEdge, FlowMode, FlowStatus } from "../ui";
import { playCompletionChime, playStartTone } from "../ui/sound";
import { elapsed, useLiveEngine } from "../engine/useLiveEngine";
import { useSettings } from "../screens/Settings";
import {
  checkInvariants,
  mark,
  noteWindowAt,
  probePill,
  reportStuckBox,
  noteViewportContract,
  watchViewport,
  viewportNow,
} from "./overlay-trace";
import { useIdleCollapse } from "./useIdleCollapse";
import "./overlay.css";

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call(cmd: string, args?: Record<string, unknown>) {
  if (!inTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(cmd, args);
}

/**
 * Window space reserved on every side while listening, so the pill's glow has
 * somewhere to land.
 *
 * A `box-shadow` is clipped at the window edge, and the window used to be sized
 * to the pill exactly — which is why this surface had no glow for so long
 * despite DESIGN.md granting it one. It is claimed only while listening, because
 * the window's *region* is the pill plus this margin: everything outside that is
 * clipped, so it neither paints nor takes clicks, and widening the region
 * widens the dead zone punched into whatever is underneath.
 */
const GLOW_MARGIN = 22;

/**
 * The window's fixed width, and where the pill sits inside it.
 *
 * These are the contract with `overlay.rs`, which declares the same constants,
 * and with `tauri.conf.json`, which creates the window at that size. All three must
 * agree: the window exists before this script does, and Rust places it without
 * waiting to be told anything.
 *
 * The width is the widest pill any state can produce — the alert clamp, 360 —
 * plus the glow margin on both sides. Nothing here varies with what the bar is
 * doing; that is the entire point.
 */
const OVERLAY_W = 404;
/**
 * The window's height, in logical pixels. `OVERLAY_H` in `overlay.rs`.
 *
 * This side used to have no name for it, on the grounds that nothing here needed
 * it — the pill is positioned from the top, so its height never entered any sum.
 * It is here now because there is one thing this side has to be able to check:
 * whether the layout viewport it is measuring is *the window*. WebView2 can lay
 * the page out at a different scale from the one the window was created at, and
 * the only way to notice is to compare both axes against what the window is
 * supposed to be. See `noteViewportContract`.
 */
const OVERLAY_H = 640;
/** The pill's top edge, from the window's top. Symmetrical vertical center: 300px headroom above, 300px below. */
const PILL_TOP = 300;

/**
 * The pill's height, in every state but the menu.
 *
 * Duplicated as `--pill-h` in `overlay.css` and as `PILL_H` in `overlay.rs`,
 * deliberately and with a comment at each end. CSS paints the box; Rust clips the
 * window to a region computed from the same number. If they disagree, the bar and
 * its clickable area come apart. Change one and the other two are wrong.
 */
const PILL_H = 40;

/**
 * The compact indicator's short axis.
 *
 * Big enough to hold the 7px status dot and a hairline border with room left to
 * read as a pill rather than as a line.
 */
const MINI_H = 22;

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

/**
 * Pointer travel, in pixels, that turns a press into a drag rather than a click.
 *
 * There has to be a threshold, because the bar is both a handle and a button.
 * Four pixels is below what anyone moves on purpose and above what a hand resting
 * on a mouse produces while clicking.
 */
const DRAG_SLOP = 4;

/** The pill's painted box, in logical pixels. */
type Geo = { w: number; h: number };

/**
 * How big the pill has to be to say what it is currently saying.
 *
 * One function, consulted by the shape sent to Rust and by the pill's own style,
 * so the region the window is clipped to and the box CSS paints cannot disagree.
 * It replaces a nested ternary that knew about four states and a fixed CSS height
 * that knew about none of them — which is what made a compact or a docked form
 * impossible to express.
 */
function geometry(v: {
  mode: FlowMode;
  mini: boolean;
  /** Put away on the idle clock. Outranks every tier below except the menu. */
  collapsed?: boolean;
  edge: FlowEdge;
  menu: boolean;
  hint: string;
  hasAction?: boolean;
  text?: string;
}): Geo {
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
    return { w: short, h: v.mini ? MINI_H : 52 };
  }

  if (v.mini) {
    // Wide enough for the dot plus seven waveform bars while live; a squat pill
    // around the dot alone otherwise.
    return { w: v.mode === "live" ? 78 : 44, h: MINI_H };
  }

  if (v.mode === "live") return { w: 240, h: PILL_H };
  if (v.mode === "working") return { w: 170, h: PILL_H };

  // Measured from their own content rather than fixed. The old 248px alert tier
  // truncated real engine messages at about thirty characters, which is reliably
  // before the part that says what to do about it.
  if (v.text !== undefined) {
    const actionW = v.hasAction ? 96 : 0;
    const w = Math.ceil(MSG_CHROME + textWidth(v.text, "400 12px $sans") + actionW);
    return { w: Math.min(380, Math.max(200, w)), h: PILL_H };
  }

  // Idle. The fixed 150px tier fit exactly one shortcut — the default — and any
  // remap collided with the word "Hold" and was clipped.
  const w = Math.ceil(IDLE_CHROME + textWidth(v.hint, "500 11px $mono"));
  return { w: Math.max(150, w), h: PILL_H };
}

/** One row of the Flow Menu. `sep` renders a divider before the item. */
type MenuRow = { id: string; label: string; run: () => void; sep?: boolean };

const MENU_PAD = 4;
const MENU_ITEM = 28;
const MENU_SEP = 9;

/** The menu's height, computed from its contents rather than pinned to a number
 *  that silently stops matching the moment an item is added. */
function menuHeight(rows: MenuRow[]): number {
  const seps = rows.filter((r) => r.sep).length;
  return PILL_H + MENU_PAD * 2 + rows.length * MENU_ITEM + seps * MENU_SEP + 12;
}

/** How long the bar takes to slide into a snapped position after a drag. */
const SETTLE_MS = 220;

/**
 * How long after the last real movement a drag is still considered to be in
 * progress.
 *
 * Generous, because it is refreshed by every move a drag produces and only has to
 * outlast the pause when someone holds the bar still mid-drag. It exists to
 * expire a drag that ended without saying so — see `dragging`.
 */
const DRAG_TTL = 1500;

/**
 * How many commanded positions to keep so the bar can recognise its own moves
 * coming back.
 *
 * Windows reports every move, including the ones this app asks for, and there is
 * no flag on the report saying which. Telling them apart used to rest entirely on
 * a "the user is dragging" boolean, which is a heuristic about a Windows move
 * loop that eats its own mouse-up — so when it was wrong, an automatic resize was
 * committed as a deliberate drag: snapped, written to disk as the user's chosen
 * position, and then animated there.
 *
 * Comparing against what was actually commanded is not a heuristic. More than one
 * is kept because the hotkey produces resizes 26ms apart and each move is
 * reported over an IPC round trip, so several of ours can be in flight at once.
 */
const COMMANDED_HISTORY = 8;

/** Whether a reported position is one of ours coming back. Two logical pixels:
 *  far below what a deliberate drag covers, far above the rounding error of a
 *  logical -> physical -> logical round trip on a fractional display scale. */
function isEcho(history: Array<[number, number]>, x: number, y: number): boolean {
  return history.some(([cx, cy]) => Math.abs(cx - x) < 2 && Math.abs(cy - y) < 2);
}

/**
 * Everything in the idle pill that is not the shortcut itself: padding, border,
 * the status dot, two gaps, the word "Hold", and the key cap's own chrome.
 *
 * Measured off the rendered component rather than derived from the stylesheet,
 * because it is the sum of seven values that live in four rules. With the
 * default "Right Ctrl" it reproduces the 150px this tier was hardcoded to; the
 * point of computing it is every *other* shortcut, which previously collided
 * with the word "Hold" and got clipped by the pill's own `overflow: hidden`.
 */
const IDLE_CHROME = 85 + 22;

/** Padding, border, dot and gap around a message. Same method as above. */
const MSG_CHROME = 49;

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
  const root = getComputedStyle(document.documentElement);
  ctx.font = font.replace("$mono", root.getPropertyValue("--font-mono").trim() || "monospace")
    .replace("$sans", root.getPropertyValue("--font-sans").trim() || "sans-serif");
  return ctx.measureText(text).width;
}

/** How long a failure or a clipboard fallback stays on the bar before it packs
 *  itself away. Long enough to read at a glance, short enough that an
 *  always-on-top window never becomes a notification that has to be dismissed. */
const ALERT_MS = 6000;

/** A discard needs acknowledging, not reading. The user already knows why. */
const DISCARD_MS = 1600;

/** Outcomes where the user ended up with nothing. See `failed` below. */
const FAILED_OUTCOMES = new Set(["asr_failed", "capture_failed"]);

/**
 * The window is a fixed rectangle. The pill moves inside it. See ADR 0007.
 *
 * THE WINDOW NEVER MOVES OR RESIZES ON A STATE CHANGE. That is the whole design,
 * and it replaces the one this file used to implement.
 *
 * The old rule was "no spare area, ever": the window was sized exactly to the
 * pill, so it changed size five times a dictation, and to keep the pill visually
 * still while it grew, the window was moved in the opposite direction by half the
 * growth. Holding the pill still therefore required the window move and the
 * pill's internal re-centring to be presented in the same composited frame. They
 * cannot be. The move is applied by the compositor synchronously; the re-centring
 * is CSS derived from the layout viewport, which reaches the renderer
 * asynchronously, in another process. In the gap the pill is painted at its old
 * size, centred in its old viewport, inside the window's new rectangle.
 *
 * Measured in production, on a real hotkey press, with the window commanded to
 * (626, 706, 284, 84) while the viewport still read 151x40:
 *
 *     pill displaced from anchor  dx=-67  dy=-22  viewportBehind=true
 *
 * which is exactly half the size delta in each axis, and exactly the "bar jumps
 * up and to the left" users reported. Four rounds of fixes tightened that
 * bargain. It is not winnable, so it had to be deleted rather than narrowed.
 *
 * The window is now a constant `OVERLAY_W` x `OVERLAY_H` whose position is a pure
 * function of the anchor, and the pill is centred in a viewport that never
 * changes. A late viewport cannot displace anything, because there is nothing for
 * it to be late about. What this side sends on a state change is not a box but a
 * *shape* — the rectangle to clip the window to, so the parts painting nothing
 * also swallow no clicks. A shape can only ever hide painting, never move it.
 *
 * The "no spare area" rule it replaces rested on a premise that turned out to be
 * false: bare transparent window area does not paint as a rectangle on this
 * stack. The rectangle people saw came from an element painting a background
 * before script ran, which `global.css` fixed, and from a pill cropped by a
 * window smaller than itself. Both are gone.
 *
 * The anchor still moves when the user moves the bar, and only then.
 */
type Anchor = { cx: number; top: number };
/**
 * The pill, not the window: its painted size, glow margin, and whether menu opens
 * above — plus the layout viewport it is centred in.
 *
 * The viewport is part of the *shape*, not context alongside it, and that is the
 * fix. Rust clips the window to a rectangle it computes by centring the pill in
 * the viewport; if the viewport changes and no shape follows, the clip describes
 * a rectangle that no longer exists and the bar is cut off — in the reported case,
 * cut off entirely, in every state, until the app was restarted.
 *
 * Carrying it here means a viewport change *is* a shape change, so the
 * convergence loop below re-sends it for the same reason it re-sends a width. No
 * new listener has to remember to; the loop's "are we there yet?" comparison
 * covers it, which is the only mechanism in this file that has never dropped one.
 */
type Box = { w: number; h: number; m: number; above?: boolean; vw: number; vh: number };

/**
 * Where the pill is, given where its window is.
 *
 * Constant arithmetic, and that is the point. Every previous version of this
 * conversion multiplied the window position by something that could be a state
 * ahead — first `box.current`, the box this side *intends* the window to become,
 * and then the layout viewport, which is the quantity that turned out to be
 * lagging. Both produced an anchor wrong by the difference between two states,
 * which is how re-deriving the anchor from a move corrupted it.
 *
 * With a fixed window there is nothing to read. The window's width and the pill's
 * offset within it are constants shared with `overlay.rs`, so this cannot be
 * stale, cannot drift, and reproduces the anchor exactly when applied to a move
 * this side commanded itself.
 */
function anchorFrom(x: number, y: number): Anchor {
  return { cx: x + OVERLAY_W / 2, top: y + PILL_TOP };
}
/**
 * Whether two shapes are the same request.
 *
 * One definition, because there were two and they disagreed: the convergence loop
 * and the stuck-box alarm each spelled the comparison out by hand, so a field
 * added to `Box` had to be remembered in both or the loop would keep sending
 * while the alarm insisted everything had arrived.
 */
function sameBox(a: Box, b: Box): boolean {
  return (
    a.w === b.w && a.h === b.h && a.m === b.m && a.above === b.above && a.vw === b.vw && a.vh === b.vh
  );
}

function useWindowShape(
  pillW: number,
  pillH: number,
  margin: number,
  above: boolean,
  /** The layout viewport, measured. See `Box`. */
  view: { w: number; h: number },
  want: React.MutableRefObject<Box>,
  ready: boolean,
) {
  // Whether a command is already in flight. With `want`, this makes the sender
  // single-flight: bursts collapse to the latest desired shape instead of
  // queueing, so a stale one can never land last.
  const sending = useRef(false);

  /** The shape Rust has actually been told about, as opposed to the one this
   *  side currently wants. Null until the first command completes. */
  const sent = useRef<Box | null>(null);

  /**
   * Drive the window to `want`, and keep going until it is there.
   *
   * A convergence loop, and it has to be one. Two earlier versions of this were
   * *edge*-triggered — they decided at the top whether a send was needed and then
   * relied on the loop noticing later changes — and each dropped a different
   * resize:
   *
   *   - Comparing the desired box against `want` dropped the very first one. The
   *     mount pass sets `want` and calls this, which bails at `if (!a)` because
   *     the anchor is still being read off the window; when the anchor lands and
   *     the effect re-runs, `want` already holds the target, so it returned early
   *     and the box was never sent at all.
   *   - Comparing it against `sent` dropped the *last* one. Cancelling a dictation
   *     returns the bar to a box it was already at, so if that happened while the
   *     listening box was still in flight, the effect saw `sent` already matching
   *     and returned without touching `want` — and the loop below, terminating on
   *     `want.current === target` by identity, then agreed it was finished. The
   *     window stayed at the listening width with an idle pill in it: the bar
   *     stuck visibly elongated after Escape.
   *
   * Both holes come from the same mistake, which is asking "has something
   * changed?" instead of "are we there yet?". The loop now terminates only when
   * the window has actually been told the box this side wants, compared by value,
   * and the effect below unconditionally records what it wants and calls this. No
   * interleaving can leave the two disagreeing, because disagreeing is the loop's
   * continuation condition.
   */
  const flush = useCallback(async () => {
    if (sending.current) return;
    sending.current = true;
    try {
      for (;;) {
        const target = want.current;
        const s = sent.current;
        if (s && sameBox(s, target)) return;

        // No position and no anchor in this call, deliberately.
        //
        // Everything that used to be computed here — the window's x and y from
        // the anchor and the box — is what made the bar's position a function of
        // its state, and therefore of a value that could be stale. The window is
        // a constant now. All that crosses the boundary is how big the pill is.
        mark("flush", { w: target.w, h: target.h, m: target.m, above: target.above });
        await call("overlay_set_shape", {
          pillW: target.w,
          pillH: target.h,
          margin: target.m,
          above: target.above ?? false,
          viewW: target.vw,
          viewH: target.vh,
        });
        sent.current = target;
        mark("flushed", { w: target.w, h: target.h, m: target.m, above: target.above });
      }
    } finally {
      sending.current = false;
    }
  }, [want]);

  // Records what is wanted and asks for it. No conditions, deliberately.
  //
  // Every early return that has ever been added here has dropped a resize, in
  // both directions — see the two named in `flush`. This effect's only job is to
  // state the target; deciding whether anything needs sending is the loop's job,
  // and the loop decides it by comparing where the window is with where it should
  // be rather than by trying to detect a change. `flush` is single-flight, so
  // calling it on a pass that has nothing to do costs one comparison.
  useLayoutEffect(() => {
    if (!inTauri()) return;
    want.current = { w: pillW, h: pillH, m: margin, above, vw: view.w, vh: view.h };
    void flush();
  }, [pillW, pillH, margin, above, view.w, view.h, ready, flush, want]);

  // A resize that never lands is silent, and that is what made this expensive.
  //
  // Both dropped-resize bugs looked to the user like the bar rendering wrong —
  // stuck wide, or the wrong shape — and neither left any trace, because from
  // this side everything had been asked for correctly. What was missing was
  // anybody checking that it *arrived*. Convergence is normally a frame or two;
  // half a second means a request was dropped, and the loop above is now built so
  // that cannot happen, which is precisely why it is worth an alarm if it does.
  useEffect(() => {
    if (!inTauri()) return;
    const id = window.setInterval(() => {
      const s = sent.current;
      const w = want.current;
      if (!s || sameBox(s, w)) return;
      reportStuckBox(s, w, sending.current);
    }, 500);
    return () => window.clearInterval(id);
  }, [want]);
}

export function Overlay() {
  const { view, levelRef } = useLiveEngine();
  const { settings } = useSettings();
  const [menu, setMenu] = useState(false);
  /**
   * The layout viewport, measured rather than assumed to be `OVERLAY_W`x`OVERLAY_H`.
   *
   * State, not a ref, because it has to re-render: the shape sent to Rust is
   * derived from it, and Rust clips the window to that shape. A viewport change
   * that produced no render produced no shape, and the window went on being
   * clipped to a rectangle computed from a viewport that no longer existed —
   * which is how the bar came to be invisible in every state at once.
   */
  const [viewport, setViewport] = useState(viewportNow);
  const [snapping, setSnapping] = useState(false);
  /** Compact rather than full. Persisted in Rust; see `overlay_set_mini`. */
  const [mini, setMini] = useState(false);
  /**
   * Whether the bar is allowed to put itself away on the idle clock. Persisted
   * in Rust; see `overlay_set_auto_collapse`.
   *
   * Defaults to `true` here as well as in Rust, so the first frame — before the
   * persisted value has made the round trip — is the same shape as every frame
   * after it for the overwhelming majority of users.
   */
  const [autoCollapse, setAutoCollapse] = useState(true);
  /** How long the quiet has to last. Persisted alongside `autoCollapse`. */
  const [collapseDelayMs, setCollapseDelayMs] = useState(5000);
  /** Bumped by a click on the put-away bar. See `useIdleCollapse`. */
  const [wake, setWake] = useState(0);
  /**
   * The microphone is open with no key held — a double-tap latch.
   *
   * Pushed from Rust rather than derived here, because the gesture that opens it
   * is recognised in `taplatch.rs` and nothing about it reaches the view state.
   */
  const [latched, setLatched] = useState(false);
  /**
   * Whether the bar is currently put away, readable from the mouse handlers.
   *
   * A ref because `collapsed` is derived far below these callbacks — it needs
   * `mode`, which needs the whole view — and a `const` cannot be read above its
   * own declaration. Assigned once per render, right where it is computed.
   */
  const collapsedRef = useRef(false);
  /** Whether the pointer is over the bar, which is what expands a compact one. */
  const [hovering, setHovering] = useState(false);
  /** Which edge the bar is docked to, and so which axis it lays out on. */
  const [edge, setEdge] = useState<FlowEdge>("bottom");
  /**
   * When the current drag was last known to be real, as a `performance.now()`
   * stamp, or 0 for "not dragging".
   *
   * A timestamp rather than a boolean because the boolean had no reliable way
   * back down. It was raised on mouse-down and lowered only by a debounce that is
   * armed inside the `onMoved` handler — so a click on the bar that moved it zero
   * pixels produced no `onMoved`, never armed the debounce, and left the flag
   * raised for the rest of the session. `startDragging` cannot lower it either:
   * it dispatches `WM_NCLBUTTONDOWN` and returns immediately rather than when the
   * Windows move loop ends, and the move loop swallows the mouse-up.
   *
   * A real drag refreshes this on every move it produces. Anything arriving more
   * than `DRAG_TTL` after the last genuine movement is not part of a drag,
   * whatever the flag says.
   */
  const dragging = useRef(0);
  const hit = useRef<HTMLDivElement>(null);
  /** Where the pill belongs on screen. See `useWindowGeometry`. */
  const anchor = useRef<Anchor | null>(null);
  /** The box the window has been told to be. Read only by `useWindowGeometry`:
   *  converting a window position back into an anchor uses `anchorFrom`, which
   *  reads the window rather than this. See the note there. */
  const box = useRef<Box>({ w: 150, h: 40, m: 0, vw: OVERLAY_W, vh: OVERLAY_H });
  /**
   * Positions this side has commanded, so moves reported back can be recognised
   * as our own rather than guessed at. See `COMMANDED_HISTORY`.
   */
  const commanded = useRef<Array<[number, number]>>([]);
  /**
   * Which settle is allowed to run. Bumped to start one and bumped again by
   * anything that re-places the window, which abandons it at its next frame.
   */
  const settleGen = useRef(0);
  /**
   * The display scale, read once.
   *
   * It is a constant for a monitor, and every place that awaited it was paying a
   * round trip for a fixed number — while opening a gap in which the window could
   * change size, so a position from one instant was combined with a viewport from
   * another. Held here so the conversion is synchronous everywhere.
   */
  const scaleRef = useRef(1);
  const [geomReady, setGeomReady] = useState(false);

  // Rust places the bar at startup and whenever it returns from hidden, and
  // says so. Re-deriving the anchor from that is what keeps "only show while
  // dictating" working: without it, the first resize after the bar reappears
  // would size it around wherever it sat before it was hidden.
  useEffect(() => {
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      // The payload is the anchor itself now, not the window's origin. Rust owns
      // the conversion, so there is no longer a second implementation of it here
      // to disagree with the first.
      un = await listen<[number, number, FlowEdge]>("overlay-parked", ({ payload }) => {
        anchor.current = { cx: payload[0], top: payload[1] };
        noteWindowAt(payload[0] - OVERLAY_W / 2, payload[1] - PILL_TOP);
        // The edge rides along with the anchor, because a bar restored onto a
        // side edge has to come back as a column rather than as a horizontal
        // pill hanging off the screen.
        if (payload[2]) setEdge(payload[2]);
        mark("parked", { anchor: anchor.current, edge: payload[2] });
      });
    })();
    return () => un?.();
  }, []);

  // The compact/full choice is persisted in Rust and can be changed from either
  // the bar's own menu or the Hub, so it arrives as an event rather than being
  // owned here.
  useEffect(() => {
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      un = await listen<boolean>("overlay-mini", ({ payload }) => setMini(payload));
    })();
    return () => un?.();
  }, []);

  // Same arrangement as `overlay-mini`, and for the same reason: the collapse
  // preference can be changed from the bar's own menu or from the Hub, and
  // whichever window did not send it still has to re-render.
  useEffect(() => {
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      un = await listen<boolean>("overlay-auto-collapse", ({ payload }) =>
        setAutoCollapse(payload),
      );
    })();
    return () => un?.();
  }, []);

  // Whether the microphone is latched open. Rust clears this on every route out
  // of a session -- a tap, the bar's own control, Escape, a fault -- so the bar
  // cannot be left claiming an open microphone after one has closed.
  useEffect(() => {
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      un = await listen<boolean>("overlay-latched", ({ payload }) => setLatched(payload));
    })();
    return () => un?.();
  }, []);

  // Derive the anchor from the window once. Rust places the bar before this
  // window's script runs, so the starting point has to come from the window
  // itself rather than from a constant.
  useEffect(() => {
    if (!inTauri()) return;
    let live = true;
    void (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const win = getCurrentWindow();
      const scale = await win.scaleFactor();
      const pos = await win.outerPosition();
      if (!live) return;
      scaleRef.current = scale;
      // The window's size is not read, because it is a constant. Reading it was
      // the last place a window dimension could reach the anchor, and every such
      // path has been a bug at least once.
      anchor.current = anchorFrom(pos.x / scale, pos.y / scale);
      // The persisted compact and docked choices come back on the same trip, so
      // the bar is never briefly the wrong shape on launch.
      void (async () => {
        const saved = (await call("overlay_placement")) as
          | {
              mini?: boolean;
              edge?: FlowEdge;
              auto_collapse?: boolean;
              collapse_delay_ms?: number;
            }
          | undefined;
        if (!live || !saved) return;
        if (saved.mini) setMini(true);
        if (saved.edge) setEdge(saved.edge);
        // Tested against `undefined` rather than for truthiness, because `false`
        // is the value that matters here: someone who turned the collapse off
        // must not have it turned back on by a falsy check.
        if (saved.auto_collapse !== undefined) setAutoCollapse(saved.auto_collapse);
        if (saved.collapse_delay_ms !== undefined) setCollapseDelayMs(saved.collapse_delay_ms);
      })();
      // Seeds the trace's idea of where the window is. It is maintained after
      // this by parking and by the settle, the only two things that move it.
      noteWindowAt(pos.x / scale, pos.y / scale);
      setGeomReady(true);
    })();
    return () => {
      live = false;
    };
  }, []);

  const live = view.state === "listening";
  const working = view.state === "transcribing" || view.state === "injecting";
  const soundEnabled = settings?.config.sound_enabled ?? true;

  /**
   * Whether the speech engine is actually up, which this window used to have no
   * opinion about.
   *
   * `main.rs` says of showing the bar at startup that it "is the only evidence
   * the user has that anything is happening during those nine seconds" — and then
   * this file read neither `view.download` nor `view.error`, so it spent those
   * nine seconds, and the 1.6 GB download before them, displaying "Hold Right
   * Ctrl". A hard engine failure displayed it forever. The invitation was to
   * press a key that was not going to do anything, on the one surface that is on
   * screen while it matters.
   */
  const status: FlowStatus = view.error ? "error" : view.ready ? "ready" : "loading";
  const progress =
    view.download && view.download.total > 0
      ? view.download.done / view.download.total
      : undefined;

  // Two tones: one when the hotkey engages, one when a dictation finishes and
  // actually landed. Tracked off the raw state transition rather than the
  // `live`/`working` booleans above so a clipboard-fallback completion --
  // which settles to "idle" exactly the same way a real success does, per
  // `useLiveEngine`'s reducer -- doesn't get the success chime just because it
  // isn't a hard failure. `view.notice` is set the moment that fallback
  // happens and nothing in this window clears it, so its presence at the
  // instant of the idle transition is a reliable signal the completion wasn't
  // clean, independent of exactly when the Notice and Finished events arrive
  // relative to each other.
  const prevState = useRef(view.state);
  const [confirm, setConfirm] = useState(false);
  useEffect(() => {
    const prev = prevState.current;
    prevState.current = view.state;

    // "Delivered" specifically, not "reached idle without complaining". A
    // dictation cancelled mid-transcribe settles to idle exactly like a
    // successful one, so without the outcome this congratulated the user on
    // landing text they had just thrown away.
    // The outcome alone decides this now. It used to also require no notice,
    // as a proxy for "nothing went wrong" — but a notice from a *previous*
    // queued session arrives after this one started and is never cleared, so a
    // clean dictation following a clipboard fallback silently lost both its
    // ring and its chime. `lastOutcome` only ever reflects the current session.
    const landed =
      (prev === "transcribing" || prev === "injecting") &&
      view.state === "idle" &&
      view.lastOutcome === "delivered";

    // The visual half of the same moment the chime already marks. It runs even
    // with sound off — muting the app should not cost the confirmation.
    if (landed) {
      setConfirm(true);
      // Outlives the 420ms ring, so the animation is never cut off mid-flight.
      const t = window.setTimeout(() => setConfirm(false), 480);
      return () => window.clearTimeout(t);
    }

    if (!soundEnabled) return;
    if (prev !== "listening" && view.state === "listening") playStartTone();
  }, [view.state, view.notice, view.lastOutcome, soundEnabled]);

  // The chime, kept on its own so the effect above can return a cleanup for the
  // confirm timer without swallowing it.
  useEffect(() => {
    if (confirm && soundEnabled) playCompletionChime();
  }, [confirm, soundEnabled]);

  /**
   * The two endings that are not silent success, and the one thing this window
   * has never shown.
   *
   * `view.notice` and `state === "fault"` were both read only by the Hub — which
   * is deliberately not open mid-dictation. So a failed injection, or text
   * diverted to the clipboard, looked from here exactly like nothing having
   * happened. That is the failure mode the "never lose a word" promise cannot
   * survive.
   */
  // Read from the outcome, not from `state === "fault"`.
  //
  // The core emits `Finished` and `Idle` in the same effect batch, so React
  // applies both before it paints and the fault frame never exists. Every ASR
  // and capture failure was therefore completely silent on this window — the
  // one surface that is on screen when it happens. The outcome is what
  // survives the batch, so the outcome is what this reads.
  const failed = view.state === "fault" || FAILED_OUTCOMES.has(view.lastOutcome ?? "");
  // A discard is an outcome the user chose, so it is acknowledged rather than
  // reported: no colour, no six-second dwell, just long enough to confirm the
  // key did something. Without it, pressing Escape looked identical to the
  // dictation having quietly failed.
  const discarded = view.lastOutcome === "cancelled" && !view.notice && !failed;
  const alertText =
    view.notice?.message ??
    (failed ? "Dictation failed — nothing was written" : discarded ? "Discarded" : undefined);
  // Identity of the current alert, so a second one re-arms the timer rather
  // than inheriting the first one's remaining time. The session count is in the
  // key because without it two *identical* consecutive messages — two clipboard
  // fallbacks in a row, say — produced the same key, which the timer had
  // already retired, so the second one never appeared at all.
  const alertKey =
    alertText && !live && !working ? `${view.sessions}:${view.state}:${alertText}` : "";
  const [packedAway, setPackedAway] = useState("");

  useEffect(() => {
    if (!alertKey) return;
    const t = window.setTimeout(() => setPackedAway(alertKey), discarded ? DISCARD_MS : ALERT_MS);
    return () => window.clearTimeout(t);
  }, [alertKey, discarded]);

  const alerting = alertKey !== "" && packedAway !== alertKey;

  /**
   * Slide the window to a resting position.
   *
   * The drag itself is smooth because Windows owns it. The old ending was not:
   * Rust snapped and called `set_position`, so release teleported the bar by up
   * to 28px in a single frame. Rust now returns where the bar belongs and this
   * covers the distance over `SETTLE_MS`, which turns the snap from a glitch
   * into the thing that makes the bar feel magnetic.
   */
  const settleTo = useCallback(async (fx: number, fy: number, tx: number, ty: number) => {
    const dx = tx - fx;
    const dy = ty - fy;
    // Nothing to travel: released away from every snap line.
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) return;

    // Claim this settle. Any settle already running is abandoned at its next
    // frame, and any state change abandons this one — see the layout effect that
    // bumps the token.
    //
    // Without that, this loop wrote a window position sixty times a second for
    // 220ms with nothing able to stop it, against an `overlay_set_box` that
    // writes once. A settle beat every resize it overlapped, and two settles
    // could run at once, each convinced it knew where the bar was.
    const mine = ++settleGen.current;

    const { getCurrentWindow, LogicalPosition } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    if (settleGen.current !== mine) return;

    mark("settle-start", { from: [fx, fy], to: [tx, ty] });
    const t0 = performance.now();
    const step = (now: number) => {
      // Superseded: a newer settle, or a state change that has re-placed the
      // window. Continuing would drag it back off the position the resize just
      // gave it.
      if (settleGen.current !== mine) {
        mark("settle-cancelled", { at: Math.round(now - t0) });
        return;
      }
      const t = Math.min(1, (now - t0) / SETTLE_MS);
      // Exponential out, matching `--ease`: most of the distance early, so it
      // reads as the bar being pulled rather than pushed.
      const e = 1 - Math.pow(1 - t, 3);
      const nx = fx + dx * e;
      const ny = fy + dy * e;
      // Recorded as ours before it is sent, exactly as `flush` does, so the moves
      // this produces are recognised as echoes rather than read back as the user
      // dragging — which is how a settle used to feed itself.
      commanded.current.push([nx, ny]);
      if (commanded.current.length > COMMANDED_HISTORY) commanded.current.shift();
      // The trace needs this too, and for the same reason: it reconstructs the
      // pill's screen rectangle from the last position the window was commanded
      // to, and a settle moves the window without going through `flush`.
      noteWindowAt(nx, ny);
      void win.setPosition(new LogicalPosition(nx, ny));
      // Only at the end, and from the destination rather than from the frame.
      //
      // Writing it every frame from a track computed before the round trip is
      // what left the anchor permanently wrong: any resize landing mid-slide
      // changed the window under a loop that was still interpolating from the
      // old one, and the last frame to run won. The destination is known up
      // front and cannot go stale.
      if (t >= 1) {
        anchor.current = anchorFrom(tx, ty);
        mark("settle-end", { anchor: anchor.current });
      }
      if (t < 1) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }, []);

  // Persist the position after a native drag. Snapping happens in Rust so the
  // rules live in one place rather than being split across the boundary.
  useEffect(() => {
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    let timer = 0;
    let cueAt = 0;

    (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const win = getCurrentWindow();
      un = await win.onMoved(({ payload }) => {
        // Read before anything is awaited.
        //
        // `scaleFactor` is a constant for a monitor, and awaiting it here bought
        // nothing while costing correctness: by the time it resolved, the window
        // could already be a different size, so the position from *this* move was
        // combined with the viewport from the *next* one. That is the same
        // one-value-two-instants mistake ADR 0006 was written about, relocated
        // into an await. The viewport is now sampled in the same turn as the
        // payload it belongs to.
        const scale = scaleRef.current;
        const lx = payload.x / scale;
        const ly = payload.y / scale;

        // Our own move coming back, not the user's hand.
        //
        // This is the gate that matters, and it replaces a heuristic with a fact.
        // Every position this side commands is recorded before it is sent, so a
        // report that matches one is provably ours. The old filter was the
        // "dragging" flag alone — a guess about a Windows move loop that eats its
        // own mouse-up — and when it guessed wrong, the resize that follows every
        // hotkey press was committed as a drag: snapped against whatever size the
        // bar happened to be, written to disk as the user's chosen position, and
        // then animated there by `settleTo`. Up and to the left, for as long as
        // the slide took, and permanently wrong afterwards.
        if (isEcho(commanded.current, lx, ly)) {
          mark("moved-echo", { x: lx, y: ly });
          return;
        }

        // Expired as well as checked. A drag produces a continuous stream of
        // moves, so a genuine one keeps refreshing the stamp; a move arriving
        // long after the last real movement is not part of a drag, however the
        // flag was left.
        if (!dragging.current || performance.now() - dragging.current > DRAG_TTL) return;
        dragging.current = performance.now();

        // The user is moving the bar, so the anchor follows it. Without this the
        // next resize would size around where the bar used to be.
        anchor.current = anchorFrom(lx, ly);
        mark("dragged", { x: lx, y: ly, anchor: anchor.current });

        // While the drag is running, ask Rust — the one place that owns the snap
        // rules — whether releasing here would move the bar, and say so on the
        // pill. Throttled, because onMoved fires far faster than anyone can read.
        const now = performance.now();
        if (now - cueAt > 90) {
          cueAt = now;
          void (async () => {
            const to = (await call("overlay_snap_preview", {
              x: lx,
              y: ly,
              pillW: box.current.w,
              pillH: box.current.h,
            })) as
              | [number, number]
              | undefined;
            if (!to) return;
            setSnapping(Math.abs(to[0] - lx) > 0.5 || Math.abs(to[1] - ly) > 0.5);
          })();
        }

        // Debounced: onMoved fires continuously through a drag, and only the
        // resting place is worth committing. Shorter than it was — the settle
        // below is now visible, so any wait in front of it reads as lag.
        window.clearTimeout(timer);
        timer = window.setTimeout(async () => {
          // Cleared before the commit, not after: the settle moves the window
          // too, and those events must not be read as more dragging.
          dragging.current = 0;
          setSnapping(false);
          const to = (await call("overlay_move", {
            x: lx,
            y: ly,
            pillW: box.current.w,
            pillH: box.current.h,
          })) as
            | [number, number, FlowEdge]
            | undefined;
          // `lx, ly` is where the drag actually ended, captured in the same turn
          // as the move that reported it. Passing it to `settleTo` as the origin
          // removes the three awaits that used to stand between the commit and
          // reading the window's position back — during which a resize could land
          // and leave the slide travelling from somewhere the bar no longer was.
          if (!to) return;
          // The edge comes back with the position, so docking to a side and
          // reorienting into a column are one event rather than two.
          setEdge(to[2] ?? "bottom");
          void settleTo(lx, ly, to[0], to[1]);
        }, 120);
      });
    })();

    return () => {
      window.clearTimeout(timer);
      un?.();
    };
  }, [settleTo]);

  /**
   * Press, then either a drag or a click.
   *
   * The bar is both a handle and a button, so the press cannot commit to either
   * until the pointer says which. `startDragging` is therefore deferred until the
   * pointer has travelled `DRAG_SLOP`: hand off to the Windows move loop any
   * earlier and there is no click at all, because that loop dispatches
   * `WM_NCLBUTTONDOWN` and swallows the mouse-up — the same property the
   * `dragging` timestamp above exists to work around.
   */
  const press = useRef<{ x: number; y: number; moved: boolean; dismissed: boolean } | null>(
    null,
  );

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (e.button !== 0 || !inTauri()) return;
      // A click that closes the menu has already done its job. Without
      // remembering that, dismissing the menu by clicking the bar behind it also
      // opened the microphone — two outcomes from one click, and the one the user
      // did not ask for is the one that starts recording them.
      press.current = { x: e.screenX, y: e.screenY, moved: false, dismissed: menu };
      setMenu(false);
    },
    [menu],
  );

  const onMouseMove = useCallback(async (e: React.MouseEvent) => {
    const p = press.current;
    if (!p || p.moved) return;
    if (Math.abs(e.screenX - p.x) < DRAG_SLOP && Math.abs(e.screenY - p.y) < DRAG_SLOP) return;
    p.moved = true;
    dragging.current = performance.now();
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().startDragging();
  }, []);

  const onMouseUp = useCallback(() => {
    const p = press.current;
    press.current = null;
    // A drag ends inside the OS move loop, which never returns a mouse-up here.
    // Reaching this line at all means the pointer stayed put: a click.
    if (!p || p.moved || p.dismissed) return;
    // A click on the put-away bar brings it back, and does nothing else.
    //
    // It deliberately does not also start dictating. The two gestures would be
    // indistinguishable — the bar is 4px tall, so anyone clicking it is aiming
    // at a stroke they can barely see — and of the two possible mistakes,
    // opening the microphone by accident is much the worse one. Hovering
    // already reveals the bar without a click at all; this is for the times the
    // pointer arrives by tap rather than by travel.
    if (collapsedRef.current) {
      setWake((w) => w + 1);
      return;
    }
    // Nothing to start yet. The bar says so in this state — see `status` — and a
    // click that silently did nothing would contradict what it is displaying.
    if (status !== "ready") return;
    void call("toggle_session");
  }, [status]);

  // The window is sized to exactly what is painted, and resized whenever that
  // changes.
  //
  // Two reasons, and both were bugs before:
  //
  //  1. Any window area not covered by the pill shows as a translucent rectangle.
  //     Webview transparency on Windows is unreliable, so rather than depending on
  //     it, there is simply no spare area to reveal.
  //  2. A transparent window still swallows OS-level clicks across its whole
  //     rectangle — `pointer-events: none` governs the webview, not the window. An
  //     oversized window would punch a dead zone into whatever is underneath.
  //
  // The idle case, 150x40, is also what `tauri.conf.json` declares the window at.
  // They have to agree: the window exists before this code does, Rust shows and
  // auto-places it from `overlay.rs` without waiting to be told a size, and when
  // the two disagreed the bar was briefly a rectangle and then moved.
  //
  // These are the *pill's* tiers. The window adds the glow margin on top; see
  // `useWindowBox`.
  //
  // Idle and alert are measured from their own content rather than fixed. The
  // fixed 150px idle tier fit exactly one shortcut — the default — and any
  // remap collided with the word "Hold" and was clipped; the fixed 248px alert
  // tier truncated real engine messages at about thirty characters, which is
  // reliably before the part that says what to do about it.
  const hint = view.ready?.shortcut ?? "Right Ctrl";

  // Hovering a compact bar shows the full one. superwhisper's mini window does
  // the same, and the reason is that a compact bar has deliberately given up its
  // labels — hover is how you get them back without giving up the space
  // permanently. A live session always renders full: the whole point of the
  // window is being readable while the microphone is open.
  const compact = mini && !hovering && !live && !menu;

  const messageText = alerting ? alertText : undefined;
  const mode = flowMode({
    live,
    working,
    failed: failed && alerting,
    message: messageText,
    status,
  });
  // The same words the component will render. Sizing the window from a private
  // copy of this logic is what clipped "Starting the speech engine…" down to
  // "Starting the speech en…": the shape was measured for the idle pill because
  // only the component knew there was a sentence coming.
  const barText = flowText({ mode, message: messageText, progress });

  // The idle clock.
  //
  // Composed with `mini` rather than merged into it, because they answer
  // different questions and both answers are worth keeping. `mini` is what size
  // the bar rests at when you are looking at it; this is whether it stays that
  // size when you are not. Hovering a collapsed bar reveals whichever of the two
  // resting forms the user chose.
  //
  // `snapping` stands in for the whole of dragging: a bar being dragged is under
  // the pointer, so `hovering` already holds it open, and `snapping` covers the
  // moment after release while it settles.
  // Whether the collapse transition is in flight. See the invariant check below.
  const morphing = useRef(false);
  /** The height `geometry()` last asked for, for the settled-size assertion. */
  const expectedPillH = useRef(PILL_H);

  const collapsed = useIdleCollapse(
    {
      live,
      working,
      menu,
      hovering,
      moving: snapping,
      speaking: flowSpeaks(mode),
    },
    collapseDelayMs,
    autoCollapse,
    wake,
  );
  collapsedRef.current = collapsed;

  // Raised when the shape changes and lowered by the element that actually
  // finishes moving, rather than by a timer guessing the duration. A timer here
  // would be a third copy of the CSS durations, drifting the first time one of
  // them was tuned.
  useLayoutEffect(() => {
    if (!inTauri()) return;
    const el = hit.current;
    if (!el) return;
    morphing.current = true;
    const done = (e: TransitionEvent) => {
      if (e.target !== el) return;
      if (e.propertyName !== "width" && e.propertyName !== "height") return;
      // The two axes are deliberately staggered, so this fires twice and the
      // first one arrives while the other is still travelling. Clearing the flag
      // there would re-create the false alarm this whole effect exists to stop.
      //
      // The settled height is the signal, not the event count: if the box is not
      // yet the size it is heading for, another transition is still running and
      // there is nothing to assert.
      if (Math.abs(el.getBoundingClientRect().height - expectedPillH.current) > 1) return;
      morphing.current = false;
      // Assert the settled size. This is the reading that matters: a box still
      // at the wrong height once the animation has finished is the squished pill
      // this check exists to catch.
      //
      // Read from a ref rather than recomputed. Calling `geometry()` again here
      // would be a second opinion about a size -- the precise failure that
      // function exists to prevent -- and it would be a stale one, since this
      // handler closes over the render that armed it.
      checkInvariants(el, PILL_TOP, expectedPillH.current);
    };
    el.addEventListener("transitionend", done);
    return () => {
      el.removeEventListener("transitionend", done);
      // A shape change that lands while an earlier one is still travelling must
      // not leave the flag raised forever.
      morphing.current = false;
    };
    // Keyed on the shape alone: this arms on a size change, and re-arming it on
    // every unrelated re-render would keep the flag raised.
  }, [collapsed, compact, menu, edge]);

  const rows = useFlowMenu({
    mini,
    live,
    working,
    autoCollapse,
    setMenu,
    setMini,
    setAutoCollapse,
  });

  const isClipboardFallback =
    alerting &&
    (view.lastOutcome === "clipboard_fallback" ||
      (view.notice != null && view.notice.message.toLowerCase().includes("clipboard")));

  const pasteAction = isClipboardFallback ? (
    <button
      type="button"
      className="flowbar-btn"
      title="Paste now (Ctrl+V)"
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        void call("paste_last");
      }}
    >
      Paste Now <Kbd>Ctrl+V</Kbd>
    </button>
  ) : undefined;

  // These are the *window's* target, not the pill's painted size.
  //
  // The pill used to be given this number directly, as an inline width, while
  // the window was told the same number over IPC and got it a round trip later.
  // For that round trip the pill was wider than the window it lives in, and a
  // pill wider than its window is cropped: rounded ends gone, side borders gone,
  // the timer cut mid-glyph — a black rectangle with the bar trapped inside it.
  // The pill now reads its width off the window instead (`.overlay-hit` in
  // overlay.css), so it can be briefly the previous size but never the wrong
  // shape, and this stays what it always meant: where the window is going.
  const geo = geometry({
    mode,
    mini: compact,
    collapsed,
    edge,
    menu,
    hint,
    hasAction: Boolean(pasteAction),
    text: barText,
  });
  const pillWidth = geo.w;
  const pillHeight = geo.h;
  expectedPillH.current = pillHeight;
  // If the bar is near the top of the screen (top < 340), open menu below; otherwise open above.
  const menuAbove = (anchor.current?.top ?? 900) >= 340;
  const menuPlacement = menuAbove ? "above" : "below";
  const shapeHeight = menu ? menuHeight(rows) : pillHeight;

  // Never both: the menu already claims a much larger box for its own purposes,
  // and stacking the glow margin on top would overshoot it. The menu also closes
  // itself the moment a session starts (see below), so this is belt and braces.
  const glowing = live && !menu;
  const margin = glowing ? GLOW_MARGIN : 0;

  useWindowShape(pillWidth, shapeHeight, margin, menu && menuAbove, viewport, box, geomReady);

  // Measure what was actually painted against the window it was painted in.
  //
  // Silent unless the two disagree, and the two disagreeing is the entire bug:
  // it is the difference between "the bar looks wrong" and a line in the console
  // saying the pill is 240px wide in a 150px window. Runs after layout on every
  // pass rather than being gated behind a debug flag, because the frames this
  // catches are the ones nobody is watching for. See overlay-trace.ts.
  //
  // The height assertion is suspended while the collapse is animating, and only
  // then. `checkInvariants` measures the painted box the instant layout settles,
  // but the box is on a 200-260ms transition between the two sizes, so mid-flight
  // it is legitimately neither of them -- and asserting against it there produced
  // an INVARIANT error on every single collapse and expand. That is worse than no
  // check at all: an alarm that cries wolf twice a minute is one nobody reads the
  // day it is right.
  //
  // Suspended, not weakened. The width-versus-window check still runs throughout,
  // and the height is asserted again the moment the transition ends -- which is
  // when a box that settled at the wrong size would actually be a bug.
  useLayoutEffect(() => {
    if (!inTauri()) return;
    checkInvariants(hit.current, PILL_TOP, morphing.current ? undefined : pillHeight);
    // And separately: where the pill actually landed *on screen*, against where
    // it belongs. `checkInvariants` can pass in full while the bar is visibly in
    // the wrong place, because a pill centred in a viewport that has not caught
    // up with its window is still perfectly centred. See `probePill`.
    probePill(hit.current, anchor.current);
  });

  useEffect(() => {
    // One place learns the viewport, and everything downstream follows from the
    // state it sets: the shape sent to Rust, and therefore the region the window
    // is clipped to. Nothing else may send a shape derived from a viewport, or
    // there are two answers to how big the bar is again.
    //
    // Setting the *same* size returns the previous object, so React bails out and
    // the poll below costs one comparison rather than a render every half second.
    const sync = (v: { w: number; h: number }) => {
      setViewport((prev) => (prev.w === v.w && prev.h === v.h ? prev : v));
      noteViewportContract(v, OVERLAY_W, OVERLAY_H);
    };

    const stop = watchViewport(sync);

    // The listeners attach after the first paint, so anything that moved between
    // the initial read and here happened unobserved. Read once more now that
    // something is watching, rather than waiting for the next change to reveal a
    // size that has already been wrong for a frame.
    sync(viewportNow());

    // And a poll, because the whole failure is that the window ends up clipped to
    // a region derived from a viewport that no longer exists — and every part of
    // detecting that is event-driven.
    //
    // All three listeners above did fire for the incident this was written for,
    // so this is not the mechanism; it is the answer to "what if one day none of
    // them does". The cost of being wrong is not a lost frame, it is a bar that
    // is invisible until the app is restarted, and half a second is a much better
    // worst case than that.
    const id = window.setInterval(() => sync(viewportNow()), 500);

    return () => {
      window.clearInterval(id);
      stop();
    };
  }, []);

  // Keep the cached scale honest.
  //
  // Reading it synchronously is what removed the await that used to let a
  // position from one instant meet a viewport from another — but a display scale
  // is only constant for one monitor. Dragging the bar to a screen at a different
  // DPI, or changing scaling while the app runs, would otherwise leave every
  // conversion wrong for the rest of the session. Tauri reports the change; the
  // read stays synchronous and the cache is refreshed out of band.
  useEffect(() => {
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    void (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      un = await getCurrentWindow().onScaleChanged(({ payload }) => {
        scaleRef.current = payload.scaleFactor;
        mark("scale-changed", { scale: payload.scaleFactor });
      });
    })();
    return () => un?.();
  }, []);

  // A state change no longer re-places the window, so a settle running across
  // one has nothing to fight and is left alone.
  //
  // This used to bump `settleGen` on every state change, abandoning the slide.
  // It had to, because a resize moved the window and the slide would drag it
  // back off the position the resize had just given it — two authorities on one
  // number, sixty writes a second against one. With a fixed window the resize
  // does not move anything, so the slide is now the only writer and cancelling
  // it mid-flight would just make a snap look broken if the user happened to
  // start dictating while it travelled.


  // Close the menu when a session starts.
  //
  // `blur` cannot do this job here, however much it looks like it should: this
  // window never takes focus (`WS_EX_NOACTIVATE`), so it never loses it either,
  // and the listener below only ever fires in the component sheet, where the same
  // component runs in an ordinary browser window. Without the state check, right-
  // clicking the bar and then dictating left the 226px menu panel open behind the
  // pill — an opaque rectangle around a window whose entire job is to be a pill,
  // and one the user cannot dismiss by clicking away from, because there is
  // nowhere to click that this window can see.
  useEffect(() => {
    if (!menu) return;
    if (live || working) {
      setMenu(false);
      return;
    }
    const close = () => setMenu(false);
    window.addEventListener("blur", close);
    return () => window.removeEventListener("blur", close);
  }, [menu, live, working]);

  // State changes are announced. A person using a screen reader gets no benefit
  // from a waveform, and the whole point of this window is knowing whether the
  // microphone is open. Lost in an earlier rewrite; restored here.
  const spoken = live
    ? latched
      ? "Microphone open, hands-free. Tap the shortcut or click the bar to stop. Escape discards."
      : "Listening. Press Escape to discard."
    : working
      ? "Writing your words"
      : mode === "enginefail"
        ? "The speech engine is unavailable. Open OpenVoice for details."
        : mode === "loading"
          ? progress != null
            ? `Getting the speech model, ${Math.round(progress * 100)} percent`
            : "Starting the speech engine"
          : alerting
            ? alertText
            : `Ready. Hold ${hint} to dictate, or click the bar.`;

  return (
    <div
      className="overlay-root"
      data-menu={menu}
      data-placement={menuPlacement}
      data-edge={edge}
      // On the root rather than on the pill, because the menu hangs *beside* the
      // pill in the DOM and a custom property only inherits downward — set on
      // `.overlay-hit`, the menu could not read it and sized itself to the whole
      // 404px window. The height is here for the same reason the width is: the
      // compact and docked forms are not 40px tall, and a CSS constant cannot
      // know which one is on screen. Both are the numbers `geometry()` produced,
      // which is also what Rust clips the window's region to.
      style={
        {
          "--pill-w": `${pillWidth}px`,
          "--pill-h": `${pillHeight}px`,
          "--pill-top": `${PILL_TOP}px`,
        } as React.CSSProperties
      }
    >
      {/* `assertive` when something went wrong: a failure announced politely
          waits for the screen reader to finish whatever it was saying, which
          for a message about text that did not land is too late to be useful. */}
      <span className="sr-only" role="status" aria-live={failed ? "assertive" : "polite"}>
        {spoken}
      </span>

      {menu && menuPlacement === "above" && (
        <div className="overlay-menu overlay-menu--above" role="menu">
          {rows.map((r) => (
            <div key={r.id}>
              {r.sep && <div className="overlay-menu-sep" />}
              <button role="menuitem" onClick={r.run}>
                {r.label}
              </button>
            </div>
          ))}
        </div>
      )}

      <div
        className="overlay-hit"
        ref={hit}
        data-snapping={snapping}
        // A custom property rather than an inline `width`, so the stylesheet
        // still decides how it is used and the component sheet can override it.
        // The number itself is safe to own here again: the window is bigger than
        // the widest pill in every state, so a pill painted at the previous width
        // for a frame is the previous width rather than a cropped one.
        data-mini={compact}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
        onMouseEnter={() => setHovering(true)}
        onMouseLeave={() => {
          setHovering(false);
          press.current = null;
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu((m) => !m);
        }}
        data-collapsed={collapsed}
        title={
          collapsed
            ? "Click to bring the bar back · drag to move · right-click for options"
            : "Click to dictate · drag to move · right-click for options"
        }
      >
        {/* Always mounted, never conditional.
            Rendering it only while collapsed was the seam behind the "pop":
            the stroke disappeared in one frame and the pill arrived in another,
            with no state in between for any easing to act on. Mounted, the two
            cross-fade and the bar reads as one object changing shape.

            Decorative, and `aria-hidden` for that reason: the `sr-only` live
            region already says what the bar is doing, and it says the same thing
            whatever size the bar happens to be. Collapsing is a change of shape,
            not of state; announcing it would interrupt someone with news that
            nothing had happened. */}
        <span className="flowbar-line" aria-hidden />
        <FlowBar
          live={live}
          levelRef={levelRef}
          elapsed={elapsed(view.elapsedMs)}
          hint={hint}
          working={working}
          failed={failed && alerting}
          message={messageText}
          action={pasteAction}
          confirm={confirm}
          publish={hit}
          status={status}
          progress={progress}
          mini={compact}
          edge={edge}
          latched={latched}
          onCancel={() => void call("cancel_session")}
          onToggle={status === "ready" ? () => void call("toggle_session") : undefined}
        />
      </div>

      {menu && menuPlacement === "below" && (
        <div className="overlay-menu overlay-menu--below" role="menu">
          {rows.map((r) => (
            <div key={r.id}>
              {r.sep && <div className="overlay-menu-sep" />}
              <button role="menuitem" onClick={r.run}>
                {r.label}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * The Flow Menu.
 *
 * Modelled on Wispr Flow's, which offers Hide for 1 hour, Settings, Microphone,
 * transcript history and Paste last transcript — and is the part of their bar
 * that makes it a control surface rather than a status light. The two items this
 * menu used to have could open the Hub and hide the bar, which meant every other
 * thing a person might want mid-dictation required finding the Hub first.
 *
 * Destinations route to a named Hub section (see `show_hub_cmd`), so the labels
 * name where they actually go.
 */
function useFlowMenu(v: {
  mini: boolean;
  live: boolean;
  working: boolean;
  autoCollapse: boolean;
  setMenu: (b: boolean) => void;
  setMini: (b: boolean) => void;
  setAutoCollapse: (b: boolean) => void;
}): MenuRow[] {
  const { mini, live, working, autoCollapse, setMenu, setMini, setAutoCollapse } = v;
  const close = useCallback(() => setMenu(false), [setMenu]);

  return [
    {
      id: "dictate",
      label: live ? "Stop dictating" : "Start dictating",
      run: () => {
        void call("toggle_session");
        close();
      },
    },
    {
      id: "paste",
      label: "Paste last transcript",
      run: () => {
        void call("paste_last");
        close();
      },
    },
    {
      id: "history",
      label: "Transcript history",
      sep: true,
      run: () => {
        void call("show_hub_cmd", { tab: "home" });
        close();
      },
    },
    {
      id: "mic",
      label: "Microphone",
      run: () => {
        void call("show_hub_cmd", { tab: "settings" });
        close();
      },
    },
    {
      id: "settings",
      label: "Settings",
      run: () => {
        void call("show_hub_cmd", { tab: "settings" });
        close();
      },
    },
    {
      id: "mini",
      label: mini ? "Full bar" : "Compact bar",
      sep: true,
      run: () => {
        setMini(!mini);
        void call("overlay_set_mini", { on: !mini });
        close();
      },
    },
    {
      // Named for what the bar does, not for the mechanism. "Auto-collapse" is
      // a description of an implementation; "get out of the way" is the thing
      // the user actually wants, and the label has to survive being read once,
      // in a hurry, over somebody else's window.
      id: "auto-collapse",
      label: autoCollapse ? "Stay full size" : "Shrink when idle",
      run: () => {
        setAutoCollapse(!autoCollapse);
        void call("overlay_set_auto_collapse", { on: !autoCollapse });
        close();
      },
    },
    {
      // Named for what it does rather than for how long, because an hour is a
      // detail and "you will not see this again today" is the decision.
      id: "snooze",
      label: "Hide for an hour",
      run: () => {
        void call("overlay_snooze", { minutes: 60 });
        close();
      },
    },
    {
      id: "dictate-only",
      label: "Only show while dictating",
      run: () => {
        void call("overlay_always_visible", { on: false });
        close();
      },
    },
  ].filter((r) => !(working && r.id === "dictate"));
}
