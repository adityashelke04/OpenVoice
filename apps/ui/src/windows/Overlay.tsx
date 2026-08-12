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
import { FlowBar } from "../ui";
import { playCompletionChime, playStartTone } from "../ui/sound";
import { elapsed, useLiveEngine } from "../engine/useLiveEngine";
import { useSettings } from "../screens/Settings";
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
 * A `box-shadow` is clipped at the window edge, and the window is sized to the
 * pill exactly — which is why this surface had no glow for so long despite
 * DESIGN.md granting it one. Reserving the margin is the same trick the
 * right-click menu already uses for its own shadow. It is claimed only while
 * listening: the window swallows OS clicks across its whole rectangle, so the
 * dead zone this creates is only acceptable during the seconds the user is
 * holding a key and talking rather than clicking.
 */
const GLOW_MARGIN = 22;

/** How long the bar takes to slide into a snapped position after a drag. */
const SETTLE_MS = 220;

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
const IDLE_CHROME = 85;

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
 * Keep the Tauri window exactly the size of what is painted in it, and keep the
 * pill still while that changes.
 *
 * NO SPARE AREA, EVER. This is the invariant the whole window depends on, and
 * breaking it is what made the bar flash a rectangle on key release. Webview
 * transparency on Windows is not reliable enough to lean on, so any window area
 * with nothing painted in it can show up as a translucent box. An earlier
 * version here held the window at the larger size for a quarter of a second so
 * the pill could ease between widths — and on release, live → working, that left
 * a 284×84 window around a 170×40 pill with the glow already switched off. The
 * pill therefore does not animate its width: it is always exactly the window's
 * content box, and the window is resized once per state change. The one
 * deliberate exception is the glow margin while listening, which the glow itself
 * paints into.
 *
 * ANCHORED, ABSOLUTELY. The window grows from its top-left, so without
 * compensation, reserving the glow margin shoves the pill down and to the right
 * by exactly that margin. The fix is an anchor — where the *pill* belongs, as
 * its centre-x and top edge — from which each window position is computed
 * outright.
 *
 * Computing it outright is the part that matters. An earlier version adjusted
 * the previous position by the change in size, which is correct once and wrong
 * forever: every logical coordinate makes a lossy round trip through physical
 * pixels, the measured pill widths are frequently odd so the half-difference is
 * fractional, and each hotkey press folded that residue into the next position.
 * Spamming the key walked the bar across the screen. An absolute anchor cannot
 * drift, because nothing it produces is ever fed back into it.
 *
 * The anchor moves only when the user moves the bar, and is re-derived from the
 * window itself whenever that happens.
 */
type Anchor = { cx: number; top: number };
type Box = { w: number; h: number; m: number };
function useWindowGeometry(
  pillW: number,
  pillH: number,
  margin: number,
  anchor: React.MutableRefObject<Anchor | null>,
  want: React.MutableRefObject<Box>,
  ready: boolean,
) {
  const boxW = pillW + margin * 2;
  const boxH = pillH + margin * 2;

  // Whether a command is already in flight. With `want`, this makes the sender
  // single-flight: bursts collapse to the latest desired box instead of
  // queueing, so a stale size can never land last — which is what left the
  // window bigger than the pill, showing as a rectangle, when the hotkey was
  // spammed.
  const sending = useRef(false);

  const flush = useCallback(async () => {
    if (sending.current) return;
    sending.current = true;
    try {
      // Loop rather than recurse: while awaiting, the state may have changed
      // again, and only the newest target is worth sending.
      for (;;) {
        const a = anchor.current;
        if (!a) return;
        const target = want.current;
        // Integer logical pixels. A fractional coordinate is rounded on the way
        // to physical pixels and rounded differently on the way back, and with
        // the odd pill widths that come out of measuring text, that residue is
        // what walked the bar sideways a fraction at a time.
        const x = Math.round(a.cx - target.w / 2);
        const y = Math.round(a.top - target.m);
        await call("overlay_set_box", { x, y, w: target.w, h: target.h });
        if (want.current === target) return;
      }
    } finally {
      sending.current = false;
    }
  }, [anchor, want]);

  useLayoutEffect(() => {
    if (!inTauri()) return;
    if (want.current.w === boxW && want.current.h === boxH && want.current.m === margin) return;
    want.current = { w: boxW, h: boxH, m: margin };
    void flush();
  }, [boxW, boxH, margin, ready, flush, want]);
}

export function Overlay() {
  const { view, levelRef } = useLiveEngine();
  const { settings } = useSettings();
  const [menu, setMenu] = useState(false);
  const [snapping, setSnapping] = useState(false);
  const dragging = useRef(false);
  const hit = useRef<HTMLDivElement>(null);
  /** Where the pill belongs on screen. See `useWindowGeometry`. */
  const anchor = useRef<Anchor | null>(null);
  /** The box the window has been told to be. Also how a window position is
   *  converted back into an anchor after the user drags. */
  const box = useRef<Box>({ w: 150, h: 40, m: 0 });
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
      un = await listen<[number, number]>("overlay-parked", ({ payload }) => {
        anchor.current = {
          cx: payload[0] + box.current.w / 2,
          top: payload[1] + box.current.m,
        };
      });
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
      const [pos, size] = await Promise.all([win.outerPosition(), win.outerSize()]);
      if (!live) return;
      box.current = { w: size.width / scale, h: size.height / scale, m: 0 };
      anchor.current = {
        cx: pos.x / scale + box.current.w / 2,
        top: pos.y / scale,
      };
      setGeomReady(true);
    })();
    return () => {
      live = false;
    };
  }, []);

  const live = view.state === "listening";
  const working = view.state === "transcribing" || view.state === "injecting";
  const soundEnabled = settings?.config.sound_enabled ?? true;

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
  const settleTo = useCallback(async (tx: number, ty: number) => {
    const { getCurrentWindow, LogicalPosition } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    const scale = await win.scaleFactor();
    const at = await win.outerPosition();
    const fx = at.x / scale;
    const fy = at.y / scale;
    const dx = tx - fx;
    const dy = ty - fy;
    // Nothing to travel: released away from every snap line.
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) return;

    const t0 = performance.now();
    const step = (now: number) => {
      const t = Math.min(1, (now - t0) / SETTLE_MS);
      // Exponential out, matching `--ease`: most of the distance early, so it
      // reads as the bar being pulled rather than pushed.
      const e = 1 - Math.pow(1 - t, 3);
      const nx = fx + dx * e;
      const ny = fy + dy * e;
      void win.setPosition(new LogicalPosition(nx, ny));
      // Re-derived throughout, not just at the end: a hotkey press landing
      // mid-settle must size around wherever the bar has actually got to.
      anchor.current = { cx: nx + box.current.w / 2, top: ny + box.current.m };
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
        // A first filter, not the only one. The OS move loop eats the mouse-up, so
        // this flag can be left standing by a press that moved nothing; Rust drops
        // positions it just commanded itself. See `Overlay::commanded`.
        if (!dragging.current) return;

        // The user is moving the bar, so the anchor follows it. Without this the
        // next resize would size around where the bar used to be.
        void (async () => {
          const scale = await win.scaleFactor();
          anchor.current = {
            cx: payload.x / scale + box.current.w / 2,
            top: payload.y / scale + box.current.m,
          };
        })();

        // While the drag is running, ask Rust — the one place that owns the snap
        // rules — whether releasing here would move the bar, and say so on the
        // pill. Throttled, because onMoved fires far faster than anyone can read.
        const now = performance.now();
        if (now - cueAt > 90) {
          cueAt = now;
          void (async () => {
            const scale = await win.scaleFactor();
            const x = payload.x / scale;
            const y = payload.y / scale;
            const to = (await call("overlay_snap_preview", { x, y })) as
              | [number, number]
              | undefined;
            if (!to) return;
            setSnapping(Math.abs(to[0] - x) > 0.5 || Math.abs(to[1] - y) > 0.5);
          })();
        }

        // Debounced: onMoved fires continuously through a drag, and only the
        // resting place is worth committing. Shorter than it was — the settle
        // below is now visible, so any wait in front of it reads as lag.
        window.clearTimeout(timer);
        timer = window.setTimeout(async () => {
          const scale = await win.scaleFactor();
          // Cleared before the commit, not after: the settle moves the window
          // too, and those events must not be read as more dragging.
          dragging.current = false;
          setSnapping(false);
          const to = (await call("overlay_move", {
            x: payload.x / scale,
            y: payload.y / scale,
          })) as [number, number] | undefined;
          if (to) void settleTo(to[0], to[1]);
        }, 120);
      });
    })();

    return () => {
      window.clearTimeout(timer);
      un?.();
    };
  }, [settleTo]);

  const startDrag = useCallback(async (e: React.MouseEvent) => {
    if (e.button !== 0 || !inTauri()) return;
    setMenu(false);
    dragging.current = true;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().startDragging();
  }, []);

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
  const idleWidth = Math.max(150, Math.ceil(IDLE_CHROME + textWidth(hint, "500 11px $mono")));
  const alertWidth = alertText
    ? Math.min(360, Math.max(200, Math.ceil(MSG_CHROME + textWidth(alertText, "400 12px $sans"))))
    : 248;

  const pillWidth = menu
    ? 280
    : alerting
      ? alertWidth
      : live
        ? 240
        : working
          ? 170
          : idleWidth;
  const pillHeight = menu ? 226 : 40;

  // Never both: the menu already claims a much larger box for its own purposes,
  // and stacking the glow margin on top would overshoot it. The menu also closes
  // itself the moment a session starts (see below), so this is belt and braces.
  const glowing = live && !menu;
  const margin = glowing ? GLOW_MARGIN : 0;

  useWindowGeometry(pillWidth, pillHeight, margin, anchor, box, geomReady);

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
    ? "Listening. Press Escape to discard."
    : working
      ? "Writing your words"
      : alerting
        ? alertText
        : "Ready. Hold the shortcut to dictate.";

  return (
    <div className="overlay-root" data-menu={menu}>
      {/* `assertive` when something went wrong: a failure announced politely
          waits for the screen reader to finish whatever it was saying, which
          for a message about text that did not land is too late to be useful. */}
      <span className="sr-only" role="status" aria-live={failed ? "assertive" : "polite"}>
        {spoken}
      </span>
      <div
        className="overlay-hit"
        ref={hit}
        data-snapping={snapping}
        style={{ width: pillWidth }}
        onMouseDown={startDrag}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu((m) => !m);
        }}
        title="Drag to move · right-click for options"
      >
        <FlowBar
          live={live}
          levelRef={levelRef}
          elapsed={elapsed(view.elapsedMs)}
          hint={hint}
          working={working}
          failed={failed && alerting}
          message={alerting ? alertText : undefined}
          confirm={confirm}
          publish={hit}
          onCancel={() => void call("cancel_session")}
        />
      </div>

      {menu && (
        <div className="overlay-menu" role="menu">
          <button role="menuitem" onClick={() => { call("show_hub_cmd"); setMenu(false); }}>
            Open OpenVoice
          </button>
          <button role="menuitem" onClick={() => { call("paste_last"); setMenu(false); }}>
            Paste last transcript
          </button>
          <div className="overlay-menu-sep" />
          <button role="menuitem" onClick={() => { call("overlay_snooze", { minutes: 60 }); setMenu(false); }}>
            Hide for an hour
          </button>
          <button role="menuitem" onClick={() => { call("overlay_always_visible", { on: false }); setMenu(false); }}>
            Only show while dictating
          </button>
        </div>
      )}
    </div>
  );
}
