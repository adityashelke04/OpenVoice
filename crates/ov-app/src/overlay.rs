//! Flow Bar window behaviour: placement, persistence, and visibility policy.
//!
//! # Why clicks work on a non-activating window
//!
//! `WS_EX_NOACTIVATE` stops a window *taking focus*; it does not stop it receiving
//! input. So the bar can be dragged and right-clicked while the user's editor keeps
//! the caret — which is the only reason an interactive always-on-top overlay is
//! possible for a dictation tool at all.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, LogicalPosition, Manager, WebviewWindow};

/// Distance from a screen edge within which the bar snaps flush to it.
const SNAP_PX: f64 = 28.0;
/// Clearance left below the bar when it is auto-placed, enough for a taskbar.
const BOTTOM_GAP: f64 = 96.0;

// The window is a fixed size, and the pill moves inside it. See ADR 0007.
//
// The bar used to be sized exactly to its pill, which meant the window changed
// size on every state change — and to keep the pill visually still while the
// window grew, the window was moved in the opposite direction by half the growth.
// That bargain requires the window move and the pill's internal re-centring to
// land in the same composited frame. They cannot: the move is applied by the
// compositor synchronously, while the re-centring is CSS derived from the layout
// viewport, which reaches the renderer asynchronously, in another process. In
// the gap the pill is painted at its old size, centred in its old viewport,
// inside the window's new rectangle — displaced by exactly half the size delta.
// Measured in production on a real hotkey press: (-67, -22) for idle to
// listening, which is the artefact users reported as the bar jumping up-left.
//
// Fixing the window's size removes the variable. The pill is centred in a
// viewport that never changes, so a late viewport cannot displace it, and the
// window's position stops being a function of the bar's state.

/// The window's width. The widest pill (the alert clamp, 360) plus the glow
/// margin on both sides, so every state fits without the window ever changing.
pub const OVERLAY_W: f64 = 404.0;
/// The window's height: the glow margin above the pill, the pill, and the room
/// the right-click menu needs below it.
pub const OVERLAY_H: f64 = 248.0;
/// The pill's top edge, measured from the window's top. Constant by construction:
/// the space above the pill is the glow's, and the menu hangs below.
pub const PILL_TOP: f64 = 22.0;
/// The pill's height. Must equal `PILL_H` in `Overlay.tsx` and `--pill-h` in
/// `overlay.css`; the three are one identity split across three languages.
pub const PILL_H: f64 = 40.0;

/// The window origin that puts the pill's anchor where it belongs.
///
/// The anchor is the pill's centre-x and its top edge — where the *bar* is, as
/// distinct from where its window is. With a fixed window the two differ by a
/// constant, which is the entire point: no term here depends on the bar's state.
fn origin_for(cx: f64, top: f64) -> (f64, f64) {
    (cx - OVERLAY_W / 2.0, top - PILL_TOP)
}

/// The pill's anchor for a given window origin. Inverse of [`origin_for`].
fn anchor_for(x: f64, y: f64) -> (f64, f64) {
    (x + OVERLAY_W / 2.0, y + PILL_TOP)
}

/// Persisted overlay state.
///
/// What is stored is the *pill's* anchor, not the window's origin. The window is
/// a fixed rectangle around the pill and its position is derived, so persisting
/// the window origin would tie the saved file to the window's dimensions and
/// silently move every user's bar the next time those changed. The anchor is
/// what the user actually chose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placement {
    /// The pill's centre-x, in logical pixels.
    #[serde(default = "nan")]
    pub cx: f64,
    /// The pill's top edge, in logical pixels.
    #[serde(default = "nan")]
    pub top: f64,
    /// Whether the bar is shown when nothing is being dictated.
    #[serde(default = "yes")]
    pub always_visible: bool,
    /// Unix millis until which the user has asked for quiet.
    #[serde(default)]
    pub hidden_until: u64,

    /// Legacy window origin, from before the window was a fixed size.
    ///
    /// Read once and converted in [`Placement::load`], never written back. A user
    /// upgrading has a saved `x`/`y` describing the top-left of a window that was
    /// sized to the idle pill; dropping it would silently move their bar back to
    /// the default position.
    #[serde(default, skip_serializing)]
    x: Option<f64>,
    #[serde(default, skip_serializing)]
    y: Option<f64>,
}

fn yes() -> bool {
    true
}

fn nan() -> f64 {
    f64::NAN
}

/// The idle window's width before the window became a fixed size. Used only to
/// convert a legacy saved position into an anchor.
const LEGACY_IDLE_W: f64 = 150.0;

impl Default for Placement {
    fn default() -> Self {
        Self {
            cx: f64::NAN,
            top: f64::NAN,
            always_visible: true,
            hidden_until: 0,
            x: None,
            y: None,
        }
    }
}

impl Placement {
    fn path() -> PathBuf {
        crate::history::data_dir().join("overlay.json")
    }

    pub fn load() -> Self {
        let mut p: Self = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        p.migrate();
        p
    }

    /// Convert a legacy window origin into a pill anchor, once.
    ///
    /// The old file stored the top-left of a window sized to the idle pill, whose
    /// margin was zero — so the pill's top edge *was* the window's, and its
    /// centre-x was half the idle width along. Any later state had a different
    /// window size, but a saved position only ever came from a drag, and the bar
    /// is idle whenever the user is dragging it.
    fn migrate(&mut self) {
        if self.cx.is_finite() && self.top.is_finite() {
            return;
        }
        let (Some(x), Some(y)) = (self.x, self.y) else {
            return;
        };
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        self.cx = x + LEGACY_IDLE_W / 2.0;
        self.top = y;
        tracing::info!(
            x,
            y,
            cx = self.cx,
            top = self.top,
            "overlay placement migrated from a window origin to a pill anchor"
        );
        self.save();
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    fn snoozed(&self) -> bool {
        now_ms() < self.hidden_until
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Owns the placement and decides when the bar is on screen.
pub struct Overlay {
    placement: Mutex<Placement>,
    /// True while a dictation session is running.
    active: Mutex<bool>,
    /// The last few positions this side told the window to go to.
    ///
    /// The frontend reports every move the OS makes, including the ones caused by
    /// [`Self::park`], and filters them with a "the user is dragging" flag. That
    /// flag is not trustworthy: `startDragging` hands the pointer to the Windows
    /// move loop, which swallows the mouse-up, so a press on the bar that moved
    /// nothing leaves it set — and the next auto-placement then comes back as a
    /// drag, gets snapped, and is written to disk as a position the user chose.
    ///
    /// A short history rather than a single value, because one is defeated by
    /// ordinary use. Every resize overwrites it, the frontend reports moves over
    /// an IPC round trip, and the log shows resizes landing 26ms apart while the
    /// hotkey is spammed — so by the time a move is reported, the position it
    /// echoes has routinely been overwritten by a newer one and is no longer
    /// recognised as ours. Remembering the last few closes that window.
    commanded: Mutex<VecDeque<(f64, f64)>>,
    /// The exact region the window should currently be clipped to, in logical
    /// window coordinates. `None` until the first shape arrives.
    shape: Mutex<Option<Rect>>,
    /// Bumped by every shape. A deferred shrink that wakes to find this changed
    /// has been superseded and does nothing.
    shape_gen: Arc<AtomicU64>,
}

/// A rectangle in logical window coordinates: left, top, right, bottom.
type Rect = (f64, f64, f64, f64);

/// How long to leave the window clipped to the union of the old and new shapes
/// before shrinking to the new one. Long enough for the webview to have
/// repainted — the layout viewport was measured settling in 6-20ms — and short
/// enough that the extra dead zone is never noticed.
const SHAPE_SETTLE_MS: u64 = 120;

/// The pill's rectangle within the window, including the margin its glow paints
/// into. Horizontally centred; the top edge is fixed by construction.
fn shape_rect(pill_w: f64, pill_h: f64, margin: f64) -> Rect {
    let left = (OVERLAY_W - pill_w) / 2.0 - margin;
    let top = PILL_TOP - margin;
    let right = left + pill_w + margin * 2.0;
    let bottom = top + pill_h + margin * 2.0;
    // Clamped to the window, so a pill that somehow outgrew it produces a region
    // that is merely the whole window rather than one hanging off the edge. The
    // tests assert no real state gets near this.
    (
        left.max(0.0),
        top.max(0.0),
        right.min(OVERLAY_W),
        bottom.min(OVERLAY_H),
    )
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

/// How many commanded positions to keep for echo detection. Enough to cover a
/// burst of resizes arriving inside one round trip; short enough that a position
/// the user genuinely dragged to later is not mistaken for one of ours.
const COMMANDED_HISTORY: usize = 8;

impl Overlay {
    pub fn new() -> Self {
        Self {
            placement: Mutex::new(Placement::load()),
            active: Mutex::new(false),
            commanded: Mutex::new(VecDeque::new()),
            shape: Mutex::new(None),
            shape_gen: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn placement(&self) -> Placement {
        self.placement.lock().expect("placement").clone()
    }

    /// Record a new position after a drag, snapping to nearby screen edges.
    ///
    /// Returns where the bar should end up, and deliberately does *not* put it
    /// there. Teleporting to the snapped point was the least pleasant moment in
    /// handling this window: the drag itself is smooth, because Windows owns it,
    /// and then release jumped the bar up to 28px sideways in a single frame.
    /// The frontend eases it over that distance instead — see `settleTo` in
    /// `Overlay.tsx`. The rules stay here; only the last few hundred milliseconds
    /// of travel happen where there is already an animation loop to do it in.
    /// `x`/`y` are the *window's* origin, as the OS reports it; `pill_w`/`pill_h`
    /// are the pill actually painted inside it. The snap lines are computed
    /// against the pill, not the window, because the window is now a fixed
    /// rectangle much larger than the bar — snapping it flush to the bottom of
    /// the screen would leave the pill floating 186px above the edge.
    pub fn move_to(
        &self,
        win: &WebviewWindow,
        x: f64,
        y: f64,
        pill_w: f64,
        pill_h: f64,
    ) -> (f64, f64) {
        // Logged unconditionally, including the echoes.
        //
        // This function writes the user's saved position to disk, and it had no
        // instrumentation at all. That is how a spurious commit — an automatic
        // resize mistaken for a drag — could rewrite the placement with nothing
        // to show for it, and how days were spent reading a log that recorded
        // every resize perfectly while saying nothing about the one call that
        // was moving the bar.
        if self.is_echo(x, y) {
            tracing::debug!(x, y, "overlay move_to ignored as echo");
            return (x, y);
        }

        let (cx, top) = anchor_for(x, y);
        let pill = (pill_w, pill_h);
        let (sx, sy) = snap_box(win, cx - pill_w / 2.0, top, pill);
        let (ncx, ntop) = (sx + pill_w / 2.0, sy);
        let (wx, wy) = origin_for(ncx, ntop);

        tracing::debug!(
            from = ?(x, y),
            to = ?(wx, wy),
            anchor = ?(ncx, ntop),
            pill = ?pill,
            "overlay move_to committed"
        );
        {
            let mut p = self.placement.lock().expect("placement");
            p.cx = ncx;
            p.top = ntop;
            p.save();
        }
        // Recorded as commanded even though nothing was commanded: the frontend
        // is about to move the window here, and those moves coming back must not
        // be mistaken for a fresh drag.
        self.remember(wx, wy);
        (wx, wy)
    }

    /// Clip the window to the pill, so the rest of the fixed rectangle is neither
    /// painted nor clickable.
    ///
    /// This replaces `set_box`. The window no longer changes size or position when
    /// the bar changes state — only its *shape* does, and a shape can only ever
    /// hide painting, never move it. That is the property the old design lacked:
    /// a late region is invisible, whereas a late viewport moved the pill.
    ///
    /// **Grow now, shrink late.** The region is applied as the union of the old
    /// and the new immediately, and the exact new region only after the webview
    /// has had time to repaint. Growing early costs nothing, because nothing is
    /// painted in the area being uncovered; shrinking early would clip a pill that
    /// is still painted at its previous, larger size. The delay is cancelled by
    /// any newer shape, so a burst of state changes settles once.
    pub fn set_shape(&self, win: &WebviewWindow, pill_w: f64, pill_h: f64, margin: f64) {
        let next = shape_rect(pill_w, pill_h, margin);
        let now = {
            let mut cur = self.shape.lock().expect("shape");
            let union = cur.map_or(next, |c| union_rect(c, next));
            *cur = Some(next);
            union
        };

        apply_region(win, now);

        if now == next {
            return;
        }

        // Cancelled by identity rather than by a handle: any newer shape bumps the
        // generation, and the task that wakes to find it changed does nothing. The
        // window's state is never read back to decide, so two settles racing cannot
        // apply each other's rectangles.
        let generation = self.shape_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let win = win.clone();
        let seen = Arc::clone(&self.shape_gen);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(SHAPE_SETTLE_MS));
            if seen.load(Ordering::SeqCst) != generation {
                return;
            }
            // Back onto the thread that owns the message loop before touching the
            // window. Checked again once there, because the wait to be scheduled
            // is itself a window in which a newer shape can arrive.
            let target = win.clone();
            let _ = win.run_on_main_thread(move || {
                if seen.load(Ordering::SeqCst) != generation {
                    return;
                }
                apply_region(&target, next);
            });
        });
    }

    /// Where the bar *would* land if it were released at this position.
    ///
    /// Pure: reads nothing, writes nothing, moves nothing. It exists so the bar
    /// can show the user where it is about to snap while they are still
    /// dragging, without the frontend having to reimplement `snap` — which would
    /// split one rule across two languages and guarantee they drift.
    pub fn snap_preview(
        &self,
        win: &WebviewWindow,
        x: f64,
        y: f64,
        pill_w: f64,
        pill_h: f64,
    ) -> (f64, f64) {
        let (cx, top) = anchor_for(x, y);
        let (sx, sy) = snap_box(win, cx - pill_w / 2.0, top, (pill_w, pill_h));
        origin_for(sx + pill_w / 2.0, sy)
    }

    pub fn set_always_visible(&self, win: &WebviewWindow, on: bool) {
        {
            let mut p = self.placement.lock().expect("placement");
            p.always_visible = on;
            p.hidden_until = 0;
            p.save();
        }
        self.apply(win);
    }

    /// Hide the bar for a while. The one relief valve for an always-on-top window:
    /// without it, an overlay that cannot be dismissed becomes something the user
    /// resents rather than relies on.
    pub fn snooze(&self, win: &WebviewWindow, minutes: u64) {
        {
            let mut p = self.placement.lock().expect("placement");
            p.hidden_until = now_ms() + minutes * 60_000;
            p.save();
        }
        self.apply(win);
    }

    /// Called by the engine as sessions start and finish.
    pub fn set_active(&self, win: &WebviewWindow, active: bool) {
        *self.active.lock().expect("active") = active;
        self.apply(win);
    }

    /// Reconcile the window with the current policy.
    pub fn apply(&self, win: &WebviewWindow) {
        let p = self.placement();
        let active = *self.active.lock().expect("active");

        // A running session always wins over a snooze: seeing that the microphone
        // is open matters more than the user's earlier wish for quiet.
        let visible = active || (p.always_visible && !p.snoozed());

        if visible {
            // Park only on the way back from hidden.
            //
            // This runs on every session start and every session end. Parking
            // each time made Rust a second authority on where the window is,
            // fighting the frontend, which moves the window itself to keep the
            // pill still while the window changes size — and the two disagree
            // by design: `park` pins the window's *bottom* edge, so when the
            // window grows by the glow margin the pill gets shoved upward, and
            // pressing the hotkey repeatedly walked the bar around the screen.
            //
            // Placement is still owned here, and still applied whenever the bar
            // actually has to be put somewhere: at startup, and when coming back
            // from a snooze or from "only show while dictating".
            if !win.is_visible().unwrap_or(false) {
                self.park(win);
            }
            // Before the show as well as after, and the "before" is the one that
            // matters.
            //
            // tao's `apply_diff` issues `ShowWindow(SW_SHOW)` at the top and only
            // rewrites `GWL_EXSTYLE` further down, so the window is already on
            // screen by the time the styles are applied. The preceding `hide()`
            // went through the same path and cleared these bits, which means a
            // show-from-hidden was activating the overlay *before* anything could
            // put `WS_EX_NOACTIVATE` back — and an overlay that activates costs
            // the user the caret their dictation was aimed at. Restoring the bits
            // afterwards was shutting the door behind the horse.
            //
            // `focusable: false` in `tauri.conf.json` is what actually fixes this:
            // tao derives `WS_EX_NOACTIVATE` from its own `FOCUSABLE` flag, so the
            // bit is now part of the state `apply_diff` applies rather than
            // something bolted on behind it. These two calls stay as belt and
            // braces, cheap because they return immediately when the bits are
            // already right.
            ensure_noactivate(win);
            let _ = win.show();
            // Re-asserted after every show, not once at startup.
            //
            // `configure_overlay` sets these bits during setup by OR-ing them into
            // `GWL_EXSTYLE` behind tao's back — and tao keeps its own idea of the
            // window's flags, which it writes over the whole word whenever it
            // touches visibility or z-order. On a running build both bits were
            // gone: the live window read `0x00040118`, with neither
            // `WS_EX_NOACTIVATE` nor `WS_EX_TOOLWINDOW` and with `WS_EX_APPWINDOW`
            // set instead, so the bar could take focus — and taking focus costs
            // the user the caret their dictation was aimed at, which is the one
            // thing this window exists not to do.
            ensure_noactivate(win);
        } else {
            let _ = win.hide();
        }
        tracing::debug!(visible, active, "overlay policy");
    }

    /// Position the window: remembered spot if it is still on a screen, otherwise
    /// bottom-centre.
    pub fn park(&self, win: &WebviewWindow) {
        let p = self.placement();
        if p.cx.is_finite() && p.top.is_finite() && on_screen(win, p.cx, p.top) {
            self.command(win, p.cx, p.top);
            return;
        }

        let Some((origin, area)) = monitor_logical(win) else {
            return;
        };
        // Placed by where the *pill* goes, not the window. Both terms are
        // independent of the pill's width, so auto-placement no longer depends on
        // which state the bar happens to be in when it runs — which is what made
        // a bar parked during startup and a bar parked later land tens of pixels
        // apart.
        let cx = origin.0 + area.0 / 2.0;
        let top = origin.1 + area.1 - BOTTOM_GAP - PILL_H;
        self.command(win, cx, top);
        tracing::debug!(cx, top, "overlay auto-placed");
    }

    /// Move the window, remembering that this side asked for it.
    ///
    /// Also tells the frontend, which keeps its own idea of where the bar
    /// belongs so it can resize the window around a stationary pill. This side
    /// places the bar at startup and whenever it comes back from hidden, and
    /// without being told, the frontend would go on sizing around wherever the
    /// bar used to be.
    /// Takes the pill's anchor, not the window's origin. The frontend is told the
    /// anchor directly, so it no longer has to reconstruct one from a window
    /// position — the conversion that, done against a stale box, displaced the bar
    /// by the difference between two states' half-widths.
    fn command(&self, win: &WebviewWindow, cx: f64, top: f64) {
        let (x, y) = origin_for(cx, top);
        self.remember(x, y);
        let _ = win.set_position(LogicalPosition::new(x, y));
        let _ = win.emit("overlay-parked", (cx, top));
    }

    /// Whether a reported position is one of our own moves coming back.
    ///
    /// Compared with a tolerance rather than for equality: the round trip is
    /// logical -> physical -> logical, and on a fractional display scale a
    /// coordinate does not survive it intact. Two logical pixels is far below what
    /// a deliberate drag covers and far above the rounding error.
    fn is_echo(&self, x: f64, y: f64) -> bool {
        self.commanded
            .lock()
            .expect("commanded")
            .iter()
            .any(|&(cx, cy)| (cx - x).abs() < 2.0 && (cy - y).abs() < 2.0)
    }

    /// Record a position this side commanded, for [`Self::is_echo`].
    fn remember(&self, x: f64, y: f64) {
        let mut q = self.commanded.lock().expect("commanded");
        if q.len() == COMMANDED_HISTORY {
            q.pop_front();
        }
        q.push_back((x, y));
    }
}

fn monitor_logical(win: &WebviewWindow) -> Option<((f64, f64), (f64, f64))> {
    let m = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten())?;
    let scale = m.scale_factor();
    let size = m.size().to_logical::<f64>(scale);
    let pos = m.position().to_logical::<f64>(scale);
    Some(((pos.x, pos.y), (size.width, size.height)))
}

/// Whether a remembered position still lands on a connected display. Monitors get
/// unplugged; a bar restored onto a screen that no longer exists is gone forever.
fn on_screen(win: &WebviewWindow, x: f64, y: f64) -> bool {
    let Ok(monitors) = win.available_monitors() else {
        return false;
    };
    monitors.iter().any(|m| {
        let scale = m.scale_factor();
        let p = m.position().to_logical::<f64>(scale);
        let s = m.size().to_logical::<f64>(scale);
        x >= p.x - 8.0 && y >= p.y - 8.0 && x < p.x + s.width - 40.0 && y < p.y + s.height - 20.0
    })
}

/// The snap rules, for a stated box rather than for whatever size the window
/// happens to be at this instant.
///
/// The size matters more than it looks. Every snap line is computed from it —
/// `right`, `centre` and `bottom` are all the screen less the window — so the
/// same released position snaps to different places depending on which state the
/// bar is in when the question is asked. Between the idle box and the listening
/// box the horizontal centre line moves 67px and the bottom line moves 44px, and
/// a commit that arrived while the bar was momentarily the wrong size therefore
/// snapped it, saved it to disk, and slid it there.
///
/// Taking the box as an argument does not by itself make the caller pass the
/// right one, but it makes the dependency impossible to miss.
fn snap_box(win: &WebviewWindow, x: f64, y: f64, (w, h): (f64, f64)) -> (f64, f64) {
    let Some((origin, area)) = monitor_logical(win) else {
        return (x, y);
    };
    let (mut nx, mut ny) = (x, y);

    let left = origin.0;
    let right = origin.0 + area.0 - w;
    let top = origin.1;
    let bottom = origin.1 + area.1 - h;
    let centre = origin.0 + (area.0 - w) / 2.0;

    if (nx - left).abs() < SNAP_PX {
        nx = left;
    } else if (nx - right).abs() < SNAP_PX {
        nx = right;
    } else if (nx - centre).abs() < SNAP_PX {
        // Snapping to the horizontal centre as well as the edges, because the
        // bottom-centre is where this thing wants to live and freehand dragging
        // never quite lands there.
        nx = centre;
    }

    if (ny - top).abs() < SNAP_PX {
        ny = top;
    } else if (ny - (bottom - BOTTOM_GAP)).abs() < SNAP_PX {
        ny = bottom - BOTTOM_GAP;
    } else if (ny - bottom).abs() < SNAP_PX {
        ny = bottom;
    }

    (nx, ny)
}

/// A logical region rectangle in physical pixels, rounded *outward*.
///
/// Outward, not nearest. The region clips both painting and input, so a half
/// pixel lost off an edge is a half pixel off the pill's 1px border — visible,
/// and permanent at 125%. Rounding out can only ever leave a sliver of the
/// window unclipped, which paints nothing and swallows nothing anyone notices.
fn region_box((l, t, r, b): Rect, scale: f64) -> (i32, i32, i32, i32) {
    (
        (l * scale).floor() as i32,
        (t * scale).floor() as i32,
        (r * scale).ceil() as i32,
        (b * scale).ceil() as i32,
    )
}

/// Clip the window to a rectangle, so the rest of it neither paints nor takes
/// clicks.
///
/// This is what makes a window permanently larger than its content acceptable. A
/// transparent window still swallows OS-level clicks across its whole rectangle —
/// `pointer-events: none` governs the webview, not the window — so without this
/// the fixed 404x248 box would punch a dead zone into whatever is underneath.
///
/// `SetWindowRgn` was rejected once on the grounds that it clips painting. It
/// does; that is only fatal if the region is the pill. The region here is the
/// pill *plus the margin its glow paints into*, which is byte-for-byte the
/// rectangle the window used to occupy in each state, so nothing that was painted
/// before is clipped now and the dead zone does not grow.
///
/// Verified against the running app before this was written: with a region
/// applied, the pixels inside it were identical, input inside it still reached
/// the overlay, and input outside it reached the window underneath. Tauri itself
/// relies on the same behaviour in `undecorated_resizing.rs`.
///
/// The system takes ownership of the region on success and must not be asked to
/// free it; on failure it is ours to delete.
#[cfg(windows)]
fn apply_region(win: &WebviewWindow, rect: Rect) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, SetWindowRgn};

    let (Ok(scale), Ok(handle)) = (win.scale_factor(), win.hwnd()) else {
        return;
    };
    let (l, t, r, b) = region_box(rect, scale);
    let hwnd = HWND(handle.0 as _);

    // SAFETY: `hwnd` is a live window handle owned by this process. The region is
    // handed to the system on success and deleted by us only when it was refused.
    unsafe {
        let rgn = CreateRectRgn(l, t, r, b);
        if rgn.is_invalid() {
            tracing::warn!(l, t, r, b, "overlay could not create a window region");
            return;
        }
        if SetWindowRgn(hwnd, rgn, true) == 0 {
            let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(rgn.0));
            tracing::warn!(l, t, r, b, "overlay SetWindowRgn was refused");
            return;
        }
    }
    tracing::debug!(
        scale,
        logical = ?rect,
        physical = ?(l, t, r, b),
        "overlay set_shape"
    );
}

#[cfg(not(windows))]
fn apply_region(_win: &WebviewWindow, _rect: Rect) {}

/// Put `WS_EX_NOACTIVATE` and `WS_EX_TOOLWINDOW` back on the overlay.
///
/// Idempotent, and called after every show rather than once at startup, because
/// something puts them back the way they were. They are applied by OR-ing into
/// `GWL_EXSTYLE` directly, which is behind tao's back: tao maintains its own
/// `WindowFlags` and rewrites the entire extended-style word from it whenever it
/// changes visibility, z-order or decorations, discarding anything it did not put
/// there. A build that had been running for a few minutes read `0x00040118` — no
/// `WS_EX_NOACTIVATE`, no `WS_EX_TOOLWINDOW`, and `WS_EX_APPWINDOW` set — meaning
/// the bar could take focus and was eligible for the taskbar and Alt-Tab.
///
/// Losing `WS_EX_NOACTIVATE` is the most expensive bug this window can have. It
/// is the reason the bar can be clicked without the user's editor losing its
/// caret, and without it a dictation aims at the wrong place.
///
/// Logged when it actually had to change something, so the next occurrence names
/// whatever is clearing them instead of being invisible for another few days.
#[cfg(windows)]
pub fn ensure_noactivate(win: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW,
    };

    let Ok(raw) = win.hwnd() else {
        tracing::error!("overlay has no HWND");
        return;
    };
    let hwnd = HWND(raw.0);
    let wanted = (WS_EX_NOACTIVATE.0 as isize) | (WS_EX_TOOLWINDOW.0 as isize);
    // `WS_EX_APPWINDOW` forces a top-level window onto the taskbar and outranks
    // `WS_EX_TOOLWINDOW`, so setting the tool-window bit while leaving this one
    // standing achieves nothing. tao sets it from its own `ON_TASKBAR` flag, which
    // is why it returns despite `skipTaskbar` being configured. The Flow Bar has
    // no business in the taskbar or in Alt-Tab.
    let unwanted = WS_EX_APPWINDOW.0 as isize;

    // SAFETY: `hwnd` is a live window handle owned by this process, and the
    // existing style is read back before being modified so no other flag is lost.
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if current & wanted == wanted && current & unwanted == 0 {
            return;
        }
        let fixed = (current | wanted) & !unwanted;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, fixed);
        tracing::warn!(
            was = format!("0x{current:08X}"),
            now = format!("0x{fixed:08X}"),
            "overlay extended style was wrong; restored"
        );
    }
}

#[cfg(not(windows))]
pub fn ensure_noactivate(_win: &WebviewWindow) {}

/// Convenience for command handlers.
pub fn window(app: &tauri::AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("overlay")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The anchor conversion is exactly invertible. If it were not, every park
    /// would walk the bar a little further, which is how the old design drifted.
    #[test]
    fn anchor_and_origin_round_trip() {
        for (cx, top) in [(960.0, 944.0), (0.0, 0.0), (-1720.0, 300.5)] {
            let (x, y) = origin_for(cx, top);
            assert_eq!(anchor_for(x, y), (cx, top));
        }
    }

    /// The pill is centred horizontally and its top edge never moves, whatever
    /// the state. This is the property the whole fix rests on: no term in the
    /// pill's position depends on its width.
    #[test]
    fn pill_top_is_constant_across_states() {
        let idle = shape_rect(150.0, PILL_H, 0.0);
        let listening = shape_rect(240.0, PILL_H, 22.0);
        let working = shape_rect(170.0, PILL_H, 0.0);
        let alert = shape_rect(360.0, PILL_H, 0.0);

        assert_eq!(idle.1, PILL_TOP);
        assert_eq!(working.1, PILL_TOP);
        assert_eq!(alert.1, PILL_TOP);
        // Listening reserves the glow margin, so its rectangle starts a margin
        // higher — but the pill inside it is still at PILL_TOP.
        assert_eq!(listening.1, PILL_TOP - 22.0);
        assert_eq!(listening.1 + 22.0, PILL_TOP);

        // Every state is centred on the same vertical line.
        for r in [idle, listening, working, alert] {
            assert_eq!((r.0 + r.2) / 2.0, OVERLAY_W / 2.0);
        }
    }

    /// Every state's rectangle fits inside the window. If one did not, the region
    /// would clip the pill permanently rather than transiently.
    #[test]
    fn every_state_fits_the_window() {
        for (w, h, m) in [
            (150.0, PILL_H, 0.0),
            (240.0, PILL_H, 22.0),
            (170.0, PILL_H, 0.0),
            (360.0, PILL_H, 0.0),
            (280.0, 226.0, 0.0),
        ] {
            let (l, t, r, b) = shape_rect(w, h, m);
            assert!(l >= 0.0, "{w}x{h}+{m} overflows the left edge: {l}");
            assert!(t >= 0.0, "{w}x{h}+{m} overflows the top edge: {t}");
            assert!(r <= OVERLAY_W, "{w}x{h}+{m} overflows the right edge: {r}");
            assert!(b <= OVERLAY_H, "{w}x{h}+{m} overflows the bottom edge: {b}");
        }
    }

    /// The union covers both rectangles, in both directions. This is what makes
    /// grow-now/shrink-late safe: whichever way the bar changes size, the region
    /// applied immediately contains the pill that is still painted.
    #[test]
    fn union_is_a_superset_both_ways() {
        let states = [
            shape_rect(150.0, PILL_H, 0.0),
            shape_rect(240.0, PILL_H, 22.0),
            shape_rect(170.0, PILL_H, 0.0),
            shape_rect(360.0, PILL_H, 0.0),
            shape_rect(280.0, 226.0, 0.0),
        ];
        for a in states {
            for b in states {
                let u = union_rect(a, b);
                for r in [a, b] {
                    assert!(u.0 <= r.0 && u.1 <= r.1 && u.2 >= r.2 && u.3 >= r.3);
                }
            }
        }
    }

    /// Snap lines are computed against the pill, so a bar released at the bottom
    /// of the screen puts the *pill* on the edge, not the window.
    #[test]
    fn shape_rect_is_the_pill_plus_its_margin() {
        let (l, t, r, b) = shape_rect(240.0, PILL_H, 22.0);
        assert_eq!(r - l, 240.0 + 44.0);
        assert_eq!(b - t, PILL_H + 44.0);
    }
}

#[cfg(all(test, windows))]
mod region_tests {
    use super::*;

    /// At 100% the logical rectangle survives untouched. This is the case every
    /// developer sees and the reason the scaled cases below went unnoticed.
    #[test]
    fn unscaled_region_is_the_identity() {
        assert_eq!(region_box((82.0, 0.0, 322.0, 40.0), 1.0), (82, 0, 322, 40));
    }

    /// 125%, the scale a 1080p laptop panel ships at — and the scale the machine
    /// that reported this bug is actually running.
    #[test]
    fn scales_the_region() {
        assert_eq!(
            region_box((80.0, 0.0, 320.0, 40.0), 1.25),
            (100, 0, 400, 50)
        );
    }

    /// Outward, not nearest. A rectangle that does not land on a physical pixel
    /// must grow to the next one, never shrink to the previous: the pill's border
    /// is 1px and the region clips it.
    #[test]
    fn rounds_outward_never_inward() {
        let (l, t, r, b) = region_box((10.4, 10.6, 20.4, 20.6), 1.0);
        assert_eq!((l, t, r, b), (10, 10, 21, 21));

        let (l, t, r, b) = region_box((82.0, 0.0, 322.0, 40.0), 1.5);
        assert!(l <= (82.0f64 * 1.5) as i32);
        assert!(r >= (322.0f64 * 1.5).ceil() as i32);
        assert_eq!((l, t, r, b), (123, 0, 483, 60));
    }

    /// A rectangle is never inverted by rounding, at any scale.
    #[test]
    fn stays_well_formed_at_every_scale() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            for (w, m) in [(150.0, 0.0), (240.0, 22.0), (170.0, 0.0), (360.0, 0.0)] {
                let (l, t, r, b) = region_box(shape_rect(w, PILL_H, m), scale);
                assert!(r > l && b > t, "inverted at {scale} for {w}+{m}");
            }
        }
    }
}
