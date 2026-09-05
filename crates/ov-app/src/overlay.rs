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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// The window's height: symmetrical headroom for the menu to open above or below.
pub const OVERLAY_H: f64 = 640.0;
/// The pill's top edge, measured from the window's top. Symmetrical vertical center:
/// 300px headroom above for upward menu, 40px pill, and 300px room below for downward menu.
pub const PILL_TOP: f64 = 300.0;
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

/// Which screen edge the bar is docked against, and therefore which way it is
/// laid out.
///
/// Wispr Flow's bar reorients vertically when dragged to a side edge, and it is
/// right for the same reason it is there: a horizontal pill on a left or right
/// edge either hangs off the screen or eats a strip of whatever is maximised
/// underneath. A vertical bar on a vertical edge occupies the margin people
/// already leave empty.
///
/// Bottom is the default and the only non-docked state — "bottom" here means
/// "free-floating, laid out horizontally", not "flush with the bottom edge".
///
/// This does not change the *window*, which stays the fixed rectangle ADR 0007
/// specifies. It changes the shape the pill paints inside it, which reaches this
/// side as `set_shape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    #[default]
    Bottom,
    Left,
    Right,
}

/// What the Flow Bar is doing right now, for the Hub to display.
#[derive(Debug, Clone, Serialize)]
pub struct OverlayState {
    pub visible: bool,
    pub always_visible: bool,
    /// Unix millis the snooze runs until, or `None` when not snoozed.
    pub snoozed_until: Option<u64>,
    pub mini: bool,
    pub edge: Edge,
    /// Whether the OS currently agrees the bar is on top.
    pub topmost: bool,
    /// How many times it has had to be put back. See `topmost.rs`.
    pub topmost_recoveries: u64,
    /// Physical pixels per webview CSS pixel, as last measured. Equals the
    /// monitor's scale factor when the webview is behaving. See [`css_to_physical`].
    pub webview_scale: f64,
    /// The monitor's scale factor, for comparison against `webview_scale`.
    pub monitor_scale: f64,
    /// The layout viewport the webview last reported, in CSS pixels. Equals
    /// `OVERLAY_W` x `OVERLAY_H` when the webview is behaving.
    pub viewport: (f64, f64),
    /// How many times the two above have parted company since launch.
    ///
    /// Reported for the same reason `topmost_recoveries` is: the user experiences
    /// this as "the bar disappeared and I had to restart", and until there was a
    /// number for it there was no way to tell that from a snooze, a lost z-order,
    /// or a crash. A non-zero count names it.
    pub scale_desyncs: u64,
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
    ///
    /// `NAN` means "never placed". It is skipped rather than written, because
    /// JSON has no NaN: `serde_json` renders it as `null`, and `null` will not
    /// deserialise back into an `f64`. Since [`load`](Self::load) discards a
    /// parse error and falls back to `default()`, writing one `null` silently
    /// reset *every* preference in this file on the next launch — the snooze,
    /// the docked edge, `always_visible`, `mini`. Skipping the field leaves it
    /// absent, and absent is exactly what `default = "nan"` already restores.
    #[serde(default = "nan", skip_serializing_if = "is_nan")]
    pub cx: f64,
    /// The pill's top edge, in logical pixels. Skipped when unset, as `cx` is.
    #[serde(default = "nan", skip_serializing_if = "is_nan")]
    pub top: f64,
    /// Whether the bar is shown when nothing is being dictated.
    #[serde(default = "yes")]
    pub always_visible: bool,
    /// Unix millis until which the user has asked for quiet.
    #[serde(default)]
    pub hidden_until: u64,
    /// Whether the bar renders as the compact indicator rather than the full pill.
    ///
    /// superwhisper persists its mini state across restarts, and the reason is
    /// worth copying: the choice between "I want to see the shortcut" and "I know
    /// the shortcut, just tell me the microphone is alive" is a standing
    /// preference about how much screen someone wants spent on a status light,
    /// not a per-session decision.
    #[serde(default)]
    pub mini: bool,
    /// Which edge the bar is docked to. See [`Edge`].
    #[serde(default)]
    pub edge: Edge,
    /// Whether the bar collapses to a line on its own after a spell of quiet.
    ///
    /// Distinct from [`mini`](Self::mini), which pins it collapsed and ignores
    /// the clock. This is the default behaviour rather than an opt-in: a bar
    /// that floats over someone else's window should not hold full width while
    /// saying nothing.
    #[serde(default = "yes")]
    pub auto_collapse: bool,
    /// How long the bar must be idle before it collapses, in milliseconds.
    ///
    /// Persisted rather than constant because the right value depends on how
    /// fast someone reads a finished transcript, which is not something this
    /// code can know.
    #[serde(default = "default_collapse_delay_ms")]
    pub collapse_delay_ms: u64,

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

/// `serde`'s `skip_serializing_if` hands over a reference; `f64::is_nan` takes
/// `self` by value, so it cannot be named directly.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_nan(v: &f64) -> bool {
    v.is_nan()
}

/// Five seconds: long enough to read a short transcript that has just landed,
/// short enough that the bar is out of the way before you have started typing
/// again.
fn default_collapse_delay_ms() -> u64 {
    5_000
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
            mini: false,
            edge: Edge::Bottom,
            auto_collapse: true,
            collapse_delay_ms: default_collapse_delay_ms(),
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
    /// The last exact region that did **not** include the Flow Menu.
    ///
    /// The base for the grow-now union whenever the menu is not on screen. Kept
    /// separately from `shape` because the menu is the one thing in this window
    /// that vanishes rather than morphs: unioning a new pill shape against a
    /// region that still spanned the menu left half a second of clipped-but-
    /// unpainted window every time the menu closed, which is a white panel.
    pill_shape: Mutex<Option<Rect>>,
    /// Bumped by every shape. A deferred shrink that wakes to find this changed
    /// has been superseded and does nothing.
    shape_gen: Arc<AtomicU64>,
    /// The layout viewport the frontend last reported, in CSS pixels.
    viewport: Mutex<(f64, f64)>,
    /// Physical pixels per CSS pixel, as last measured. See [`css_to_physical`].
    webview_scale: Mutex<f64>,
    /// Whether the two scales currently disagree, so the transition can be logged
    /// once rather than the state logged on every shape.
    scale_wrong: AtomicBool,
}

/// A rectangle in logical window coordinates: left, top, right, bottom.
type Rect = (f64, f64, f64, f64);

/// What the frontend is asking the window to be clipped to.
///
/// One struct rather than six positional arguments, because four of them are
/// `f64` and two are `bool`, and a call site that transposes any pair still
/// compiles. The fields are the whole of the shape protocol; see [`Overlay::set_shape`].
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    /// The layout viewport the pill was centred in, in CSS pixels.
    pub view: (f64, f64),
    /// The pill's painted width.
    pub pill_w: f64,
    /// The painted height: the pill, plus the measured menu when one is open.
    pub pill_h: f64,
    /// Space reserved around the pill for its glow.
    pub margin: f64,
    /// Whether the menu opens upward, which decides which side of the pill the
    /// extra height is taken from.
    pub above: bool,
    /// Whether the Flow Menu is part of this shape.
    ///
    /// Not derivable from `above`: a menu opening downward sets `above: false`
    /// and is otherwise indistinguishable from a bare pill. See `set_shape` for
    /// what turns on it.
    pub menu: bool,
}

/// How long to leave the window clipped to the union of the old and new shapes
/// before tightening to the new one.
///
/// This serves two jobs, and the second one is why it is no longer 120ms.
///
/// 1. The webview has to have repainted. The layout viewport was measured
///    settling in 6-20ms, so 120 was ample for that alone.
/// 2. The bar *animates* between shapes. The region is the window's clip: while
///    a morph is in flight the painted pill is briefly larger than the shape it
///    is heading for, and a region that tightened first would slice the
///    animation off mid-flight. It has to outlast the longest transition in
///    `overlay.css`.
///
/// The cost is that the dead zone punched into whatever is underneath lingers
/// for this long after the bar shrinks. It is transparent and it is the size of
/// a bar the user chose to place there, so it is a fair price for motion that
/// is actually visible.
const SHAPE_SETTLE_MS: u64 = 460;

/// How many times the webview's scale has parted company with the monitor's.
///
/// See [`css_to_physical`]. Counted rather than merely logged because this is the
/// mechanism behind "the Flow Bar vanished and only a restart brought it back",
/// and a symptom that cannot be counted cannot be told apart from three others
/// that look identical from the outside.
static SCALE_DESYNCS: AtomicU64 = AtomicU64::new(0);

/// Total desyncs since launch.
pub fn scale_desyncs() -> u64 {
    SCALE_DESYNCS.load(Ordering::Relaxed)
}

/// The pill's rectangle **in the webview's own CSS pixels**, including the margin
/// its glow paints into.
///
/// `view` is the layout viewport the frontend measured, not `OVERLAY_W`/`OVERLAY_H`.
/// Those two are normally the same number and were assumed to be for months. They
/// are not the same number, and the day they differ this window disappears — see
/// [`css_to_physical`] for the failure and the evidence.
///
/// The rule this reproduces is the one `overlay.css` actually applies: the pill is
/// centred in the layout viewport, and its top edge is pinned at `PILL_TOP` CSS
/// pixels from the top of it. Both terms have to come from the same viewport the
/// stylesheet used, or the region and the paint describe different rectangles.
fn shape_rect(view: (f64, f64), pill_w: f64, pill_h: f64, margin: f64, above: bool) -> Rect {
    let (view_w, view_h) = view;
    let left = (view_w - pill_w) / 2.0 - margin;
    let right = left + pill_w + margin * 2.0;
    let (top, bottom) = if above {
        let top = PILL_TOP - (pill_h - PILL_H).max(0.0) - margin;
        let bottom = PILL_TOP + PILL_H + margin;
        (top, bottom)
    } else {
        let top = PILL_TOP - margin;
        let bottom = top + pill_h + margin * 2.0;
        (top, bottom)
    };
    // Clamped to the window, so a pill that somehow outgrew it produces a region
    // that is merely the whole window rather than one hanging off the edge. The
    // tests assert no real state gets near this.
    (
        left.max(0.0),
        top.max(0.0),
        right.min(view_w),
        bottom.min(view_h),
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
            pill_shape: Mutex::new(None),
            shape_gen: Arc::new(AtomicU64::new(0)),
            // Nominal until the frontend measures itself. These are what the
            // window is supposed to be, so a state read before the first shape
            // reports agreement rather than a phantom desync.
            viewport: Mutex::new((OVERLAY_W, OVERLAY_H)),
            webview_scale: Mutex::new(1.0),
            scale_wrong: AtomicBool::new(false),
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
    ) -> (f64, f64, Edge) {
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
            return (x, y, self.placement().edge);
        }

        let (cx, top) = anchor_for(x, y);
        let pill = (pill_w, pill_h);
        let (sx, sy) = snap_box(win, cx - pill_w / 2.0, top, pill);
        let (ncx, ntop) = (sx + pill_w / 2.0, sy);
        let (wx, wy) = origin_for(ncx, ntop);
        // Which edge the pill came to rest against, decided from the snapped
        // position rather than the raw one. See `edge_at`.
        let edge = monitor_logical(win)
            .map(|(origin, area)| edge_at(origin.0, area.0, pill_w, sx))
            .unwrap_or(Edge::Bottom);

        tracing::debug!(
            from = ?(x, y),
            to = ?(wx, wy),
            anchor = ?(ncx, ntop),
            pill = ?pill,
            ?edge,
            "overlay move_to committed"
        );
        {
            let mut p = self.placement.lock().expect("placement");
            p.cx = ncx;
            p.top = ntop;
            p.edge = edge;
            p.save();
        }
        // Recorded as commanded even though nothing was commanded: the frontend
        // is about to move the window here, and those moves coming back must not
        // be mistaken for a fresh drag.
        self.remember(wx, wy);
        (wx, wy, edge)
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
    ///
    /// **Except across the menu closing.** That rule assumes the old shape is
    /// still being painted while the new one arrives, which is true of the pill —
    /// it morphs between sizes over a CSS transition — and false of the menu,
    /// which is unmounted in the same frame it is dismissed. Unioning against it
    /// held the region open over a menu that no longer existed for the whole
    /// `SHAPE_SETTLE_MS`, and an unpainted region is not transparent: it is the
    /// webview's background, so closing the menu flashed a white panel where the
    /// menu had been. `menu` says which kind of shape this is, and a shape with no
    /// menu unions against the last shape that also had no menu — so the pill's
    /// own morph is still protected and the dead menu box is not.
    pub fn set_shape(&self, win: &WebviewWindow, s: Shape) {
        let Shape {
            view,
            pill_w,
            pill_h,
            margin,
            above,
            menu,
        } = s;
        // Bumped for *every* shape, including the ones that need no settle.
        //
        // It used to be bumped only on the path that spawns one, which meant a
        // grow could not cancel a shrink already in flight: the shrink woke 460ms
        // later, found the generation it was given still current, and clipped the
        // window down to a rectangle two states old. With `SHAPE_SETTLE_MS` raised
        // to outlast the collapse animation that window is nearly half a second
        // wide, and "collapse, then press the hotkey" lands squarely inside it.
        let generation = self.shape_gen.fetch_add(1, Ordering::SeqCst) + 1;

        let css_to_phys = css_to_physical(win, view.0);
        self.note_scale(win, view, css_to_phys);

        let next = shape_rect(view, pill_w, pill_h, margin, above);
        let now = {
            let mut cur = self.shape.lock().expect("shape");
            let mut pill_only = self.pill_shape.lock().expect("pill shape");
            // A menu-less shape unions against the last menu-less shape, so a menu
            // that has just been unmounted cannot hold the region open over an
            // area nothing paints. See the note above.
            let base = if menu { *cur } else { *pill_only };
            let union = base.map_or(next, |c| union_rect(c, next));
            *cur = Some(next);
            if !menu {
                *pill_only = Some(next);
            }
            union
        };

        apply_region(win, now, css_to_phys);

        if now == next {
            return;
        }

        // Cancelled by identity rather than by a handle: any newer shape bumps the
        // generation, and the task that wakes to find it changed does nothing. The
        // window's state is never read back to decide, so two settles racing cannot
        // apply each other's rectangles.
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
                // Re-measured rather than captured. This runs up to half a
                // second later, and the whole reason it exists is that the
                // webview's scale is a thing that changes without warning.
                let phys = css_to_physical(&target, view.0);
                apply_region(&target, next, phys);
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

    /// Cancel a snooze and bring the bar back now.
    ///
    /// The counterpart to `snooze`, and it was missing. Every control for this
    /// window lived in a menu on the window, so hiding it also hid the only way
    /// to unhide it: a snoozed bar could not be recovered for an hour, and a bar
    /// the user believed was broken could not be told apart from one obeying an
    /// instruction it had no way to report. Reached from the tray, which cannot
    /// itself be hidden.
    pub fn unsnooze(&self, win: &WebviewWindow) {
        {
            let mut p = self.placement.lock().expect("placement");
            p.hidden_until = 0;
            p.always_visible = true;
            p.save();
        }
        self.apply(win);
    }

    /// Forget the remembered anchor and re-place the bar at bottom-centre.
    ///
    /// `on_screen` already guards against an anchor on a monitor that has since
    /// been unplugged, but it cannot help with a bar the user dragged somewhere
    /// they regret — behind a taskbar, onto a sliver of a second display, hard
    /// into a corner. Clearing to NaN puts it back through the same
    /// auto-placement path a first launch takes.
    pub fn reset_position(&self, win: &WebviewWindow) {
        {
            let mut p = self.placement.lock().expect("placement");
            p.cx = f64::NAN;
            p.top = f64::NAN;
            p.edge = Edge::Bottom;
            p.save();
        }
        self.park(win);
        self.apply(win);
    }

    /// Switch between the compact indicator and the full pill.
    pub fn set_mini(&self, on: bool) {
        let mut p = self.placement.lock().expect("placement");
        p.mini = on;
        p.save();
    }

    /// Turn the idle collapse on or off.
    ///
    /// Separate from [`set_mini`](Self::set_mini) because they answer different
    /// questions. `mini` is "keep it small"; this is "let it get small on its
    /// own". Someone can want the second without the first, and pinning it
    /// small should not be the only way to stop it holding full width.
    pub fn set_auto_collapse(&self, on: bool) {
        let mut p = self.placement.lock().expect("placement");
        p.auto_collapse = on;
        p.save();
    }

    /// Record what the webview says its scale is, and say so when it is wrong.
    ///
    /// Silent in the ordinary case, which is every case until WebView2 changes its
    /// mind. The log line is written on the *transition* rather than on every
    /// shape, because a shape is sent several times a dictation and an error
    /// repeated that often is one nobody reads.
    fn note_scale(&self, win: &WebviewWindow, view: (f64, f64), css_to_phys: f64) {
        let monitor = win.scale_factor().unwrap_or(1.0);
        // One percent. Scale factors move in quarters, so this cannot miss a real
        // desync; and the viewport arrives as an integer, so a 404px viewport
        // carries up to 1/404 — a quarter of a percent — of rounding that is not
        // a disagreement about anything.
        let wrong = (css_to_phys - monitor).abs() > 0.01;
        let was = self.scale_wrong.swap(wrong, Ordering::Relaxed);
        *self.viewport.lock().expect("viewport") = view;
        *self.webview_scale.lock().expect("scale") = css_to_phys;
        if wrong == was {
            return;
        }
        if wrong {
            SCALE_DESYNCS.fetch_add(1, Ordering::Relaxed);
            tracing::error!(
                view_w = view.0,
                view_h = view.1,
                webview_scale = css_to_phys,
                monitor_scale = monitor,
                total = scale_desyncs(),
                "flow bar webview lost the window's scale; the region is now                  measured from the viewport rather than the monitor, so the bar                  stays visible at the wrong size instead of vanishing"
            );
        } else {
            tracing::info!(
                view_w = view.0,
                view_h = view.1,
                scale = css_to_phys,
                "flow bar webview scale agrees with the window again"
            );
        }
    }

    /// Everything a settings screen needs to say what this window is doing.
    ///
    /// Reported rather than inferred. The bug that prompted this was invisible
    /// from inside the app — a bar that had lost its z-order looked exactly like
    /// a bar obeying a snooze, which looked exactly like a bar that had crashed,
    /// and nothing could distinguish them. Each field makes one of those
    /// nameable.
    pub fn state(&self, win: &WebviewWindow) -> OverlayState {
        let p = self.placement();
        OverlayState {
            visible: win.is_visible().unwrap_or(false),
            always_visible: p.always_visible,
            snoozed_until: p.snoozed().then_some(p.hidden_until),
            mini: p.mini,
            edge: p.edge,
            topmost: crate::topmost::is_topmost(win),
            topmost_recoveries: crate::topmost::recoveries(),
            webview_scale: *self.webview_scale.lock().expect("scale"),
            viewport: *self.viewport.lock().expect("viewport"),
            monitor_scale: win.scale_factor().unwrap_or(1.0),
            scale_desyncs: scale_desyncs(),
        }
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
            // Before the hide, and it has to be before: the menu is frontend
            // state, and a bar snoozed with it open came back an hour later still
            // wearing a 280px panel it had no way to explain. Nothing else in the
            // app closes it — the webview cannot see the snooze, and the user who
            // triggered it from the Hub was not looking at the bar.
            //
            // Safe to send to a window on its way out: the webview keeps running
            // while hidden, so the event is processed and the shape is already
            // right by the time the bar is shown again.
            let _ = win.emit("overlay-menu-dismiss", ());
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
/// Which edge a pill whose left edge landed at `x` is docked to.
///
/// Pure, and takes the monitor rectangle rather than a window, so the rule can be
/// tested without a window manager. `snap_box` has always known where the screen
/// edges are and has always thrown that knowledge away after moving the bar; this
/// keeps it, because the answer decides how the pill is laid out and not just
/// where it sits.
///
/// Compared against the already-snapped x. A bar is docked when it is *flush*,
/// not when it is nearby: releasing 27px from the left snaps flush and docks,
/// releasing 29px away leaves it floating and horizontal. That the same threshold
/// decides both is deliberate — two thresholds would let the bar snap to an edge
/// without adopting the layout for it.
fn edge_at(origin_x: f64, width: f64, pill_w: f64, x: f64) -> Edge {
    let left = origin_x;
    let right = origin_x + width - pill_w;
    if (x - left).abs() < 0.5 {
        Edge::Left
    } else if (x - right).abs() < 0.5 {
        Edge::Right
    } else {
        Edge::Bottom
    }
}

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

/// A rectangle of the webview's CSS pixels in physical pixels, rounded *outward*.
///
/// `css_to_phys` used to be the monitor's scale factor, on the assumption that the
/// webview lays out one CSS pixel per logical pixel. It is now measured — see
/// [`css_to_physical`] — because that assumption is the bug this pair of functions
/// most recently caused.
///
/// Outward, not nearest. The region clips both painting and input, so a half
/// pixel lost off an edge is a half pixel off the pill's 1px border — visible,
/// and permanent at 125%. Rounding out can only ever leave a sliver of the
/// window unclipped, which paints nothing and swallows nothing anyone notices.
fn region_box((l, t, r, b): Rect, css_to_phys: f64) -> (i32, i32, i32, i32) {
    (
        (l * css_to_phys).floor() as i32,
        (t * css_to_phys).floor() as i32,
        (r * css_to_phys).ceil() as i32,
        (b * css_to_phys).ceil() as i32,
    )
}

/// How many physical pixels one of the webview's CSS pixels covers.
///
/// # The bug
///
/// This used to be `win.scale_factor()`, and the difference between that and this
/// is a bar that vanishes off the user's screen until the app is restarted.
///
/// `SetWindowRgn` takes physical pixels. The pill's position is decided by CSS,
/// in CSS pixels. Converting between them with the *monitor's* scale asserts that
/// the webview lays out at one CSS pixel per logical pixel — that its
/// `devicePixelRatio` equals the window's scale factor. Nothing enforces that,
/// and WebView2 does not always honour it: it keeps its own rasterization scale,
/// and it can drop that to 1 on a display change, a lock/unlock, or a monitor
/// wake, without any DPI message reaching this process.
///
/// Caught in the wild on 2026-09-05, on a 125% panel, from the frontend's own
/// instrumentation, with no shape sent for the previous 217 seconds:
///
/// ```text
/// flowbar: viewport settled  via="window" w=505 h=800   <- 404x640 x 1.25
/// flowbar: pill displaced from anchor  dx=51  view={505,800}  at={205,300}
/// overlay set_shape scale=1.25 logical=(154,300,250,324) physical=(192,375,313,405)
/// ```
///
/// The bar painted at physical y 300..324 and the window was clipped to y
/// 375..405. **Disjoint** — and disjoint in every state, because every state's
/// region was scaled by 1.25 while every state painted at 1.0. So the bar was
/// invisible collapsed, invisible expanded, invisible while dictating, and no
/// keypress could bring it back: there was nothing wrong with it except that the
/// window was clipped to a rectangle it did not occupy.
///
/// # Why measuring is the fix rather than correcting the webview
///
/// Whatever WebView2 decides its scale is, the client area is a known number of
/// physical pixels and the frontend can say how many CSS pixels it was given to
/// lay out in. Their ratio is the conversion, by construction, for any scale the
/// webview picks — including one this process was never told about. Correcting
/// the webview instead would leave the region trusting a number it cannot see.
///
/// Falls back to the scale factor when the viewport is not yet known or the
/// window will not report its size, which reproduces the old behaviour exactly.
fn css_to_physical(win: &WebviewWindow, view_w: f64) -> f64 {
    let scale = win.scale_factor().unwrap_or(1.0);
    if view_w <= 0.0 {
        return scale;
    }
    let Ok(size) = win.inner_size() else {
        return scale;
    };
    if size.width == 0 {
        return scale;
    }
    f64::from(size.width) / view_w
}

/// Clip the window to a rectangle, so the rest of it neither paints nor takes
/// clicks.
///
/// This is what makes a window permanently larger than its content acceptable. A
/// transparent window still swallows OS-level clicks across its whole rectangle —
/// `pointer-events: none` governs the webview, not the window — so without this
/// the fixed 404x640 box would punch a dead zone into whatever is underneath.
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
fn apply_region(win: &WebviewWindow, rect: Rect, css_to_phys: f64) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, SetWindowRgn};

    let Ok(handle) = win.hwnd() else {
        return;
    };
    let (l, t, r, b) = region_box(rect, css_to_phys);
    let hwnd = HWND(handle.0 as _);

    // SAFETY: `hwnd` is a live window handle owned by this process. The region is
    // handed to the system on success and deleted by us only when it was refused.
    unsafe {
        let rgn = CreateRectRgn(l, t, r, b);
        if rgn.is_invalid() {
            tracing::warn!(l, t, r, b, "overlay could not create a window region");
            return;
        }
        if SetWindowRgn(hwnd, rgn, false) == 0 {
            let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(rgn.0));
            tracing::warn!(l, t, r, b, "overlay SetWindowRgn was refused");
            return;
        }
    }
    // The click-away hook needs the same box, and this is the one place that
    // computes it. Publishing here rather than letting the hook ask GDI keeps a
    // syscall off the critical path of every mouse button in the system -- see
    // `clickaway`'s rule 1.
    //
    // After the `SetWindowRgn`, not before: both paths above return without
    // having changed the window's shape, and a published region the system
    // refused would put the hit test somewhere the bar is not.
    crate::clickaway::set_region(l, t, r, b);
    tracing::debug!(
        css_to_phys,
        css = ?rect,
        physical = ?(l, t, r, b),
        "overlay set_shape"
    );
}

#[cfg(not(windows))]
fn apply_region(_win: &WebviewWindow, _rect: Rect, _css_to_phys: f64) {}

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

    // -- Docking ---------------------------------------------------------

    // A 1920-wide monitor at the origin, holding a 150px pill.
    const ORIGIN: f64 = 0.0;
    const WIDTH: f64 = 1920.0;
    const PILL: f64 = 150.0;

    #[test]
    fn flush_left_docks_left() {
        assert_eq!(edge_at(ORIGIN, WIDTH, PILL, 0.0), Edge::Left);
    }

    #[test]
    fn flush_right_docks_right() {
        assert_eq!(edge_at(ORIGIN, WIDTH, PILL, WIDTH - PILL), Edge::Right);
    }

    #[test]
    fn the_middle_of_the_screen_is_not_docked() {
        assert_eq!(edge_at(ORIGIN, WIDTH, PILL, 800.0), Edge::Bottom);
    }

    // Near an edge is not on it. `snap_box` decides whether a release becomes
    // flush; this only reads the result. Were this fuzzy too, a bar could sit
    // 20px from the left and still be laid out as though docked to it.
    #[test]
    fn near_an_edge_but_not_flush_stays_horizontal() {
        assert_eq!(edge_at(ORIGIN, WIDTH, PILL, 20.0), Edge::Bottom);
        assert_eq!(
            edge_at(ORIGIN, WIDTH, PILL, WIDTH - PILL - 20.0),
            Edge::Bottom
        );
    }

    // A second monitor to the right of the first: edges are relative to the
    // monitor the bar is on, not to the desktop origin. Without the offset every
    // position on a secondary display reads as "not docked".
    #[test]
    fn edges_are_relative_to_the_monitor() {
        assert_eq!(edge_at(1920.0, WIDTH, PILL, 1920.0), Edge::Left);
        assert_eq!(
            edge_at(1920.0, WIDTH, PILL, 1920.0 + WIDTH - PILL),
            Edge::Right
        );
        assert_eq!(edge_at(1920.0, WIDTH, PILL, 0.0), Edge::Bottom);
    }

    #[test]
    fn edge_round_trips_through_json() {
        let json = serde_json::to_string(&Edge::Left).expect("serialize");
        assert_eq!(json, "\"left\"");
        assert_eq!(
            serde_json::from_str::<Edge>(&json).expect("deserialize"),
            Edge::Left
        );
    }

    // The fields added after the fixed-window migration are all serde(default),
    // so a file written by any older build still loads. This branch already
    // migrates a legacy x/y; dropping mini or edge on top of that would be a
    // second silent reset of something the user chose.
    #[test]
    fn a_placement_without_mini_or_edge_still_loads() {
        let json = r#"{"cx":900.0,"top":940.0,"always_visible":true,"hidden_until":0}"#;
        let p: Placement = serde_json::from_str(json).expect("placement");
        assert_eq!(p.cx, 900.0);
        assert!(!p.mini);
        assert_eq!(p.edge, Edge::Bottom);
    }
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

    /// The layout viewport a healthy webview reports: exactly the window.
    ///
    /// Named rather than inlined so the tests that assume it are visibly the ones
    /// assuming it, and the two below that deliberately do not are visibly
    /// different.
    const NOMINAL: (f64, f64) = (OVERLAY_W, OVERLAY_H);

    /// Where `overlay.css` actually paints the pill, in CSS pixels.
    ///
    /// Centred in the layout viewport, top edge pinned at `PILL_TOP`. This is a
    /// second statement of the stylesheet's rule, and that is the point: the
    /// region is only correct if it agrees with a rule written down independently
    /// of it.
    fn painted_rect(view_w: f64, pill_w: f64, pill_h: f64) -> Rect {
        let left = (view_w - pill_w) / 2.0;
        (left, PILL_TOP, left + pill_w, PILL_TOP + pill_h)
    }

    /// Every state's region contains the pill, at any viewport the webview picks.
    ///
    /// The property the window rests on, and the one that was silently false. The
    /// region used to be centred in `OVERLAY_W` regardless of what the webview
    /// laid out in, so a viewport of any other width put the clip and the paint in
    /// different places — and at 505 CSS pixels they did not even overlap.
    #[test]
    fn the_region_contains_the_painted_pill_at_any_viewport() {
        for view in [
            NOMINAL,
            // WebView2 dropping to devicePixelRatio 1 on a 125% panel: the exact
            // numbers out of the 2026-09-05 log.
            (505.0, 800.0),
            // And the other direction, for a webview that scales up instead.
            (323.2, 512.0),
        ] {
            for (w, h, m) in [
                (96.0, 24.0, 0.0),
                (150.0, PILL_H, 0.0),
                (240.0, PILL_H, 22.0),
                (170.0, PILL_H, 0.0),
            ] {
                let r = shape_rect(view, w, h, m, false);
                let p = painted_rect(view.0, w, h);
                assert!(
                    r.0 <= p.0 && r.1 <= p.1 && r.2 >= p.2 && r.3 >= p.3,
                    "region {r:?} does not contain the pill {p:?}                      for {w}x{h}+{m} in a {view:?} viewport"
                );
            }
        }
    }

    /// The pill is centred horizontally and its top edge never moves, whatever
    /// the state. This is the property the whole fix rests on: no term in the
    /// pill's position depends on its width.
    #[test]
    fn pill_top_is_constant_across_states() {
        let idle = shape_rect(NOMINAL, 150.0, PILL_H, 0.0, false);
        let listening = shape_rect(NOMINAL, 240.0, PILL_H, 22.0, false);
        let working = shape_rect(NOMINAL, 170.0, PILL_H, 0.0, false);
        let alert = shape_rect(NOMINAL, 360.0, PILL_H, 0.0, false);

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

    /// Every state's rectangle fits inside the window, for both menu directions.
    /// If one did not, the region would clip the pill permanently rather than transiently.
    #[test]
    fn every_state_fits_the_window() {
        for (w, h, m, above) in [
            (150.0, PILL_H, 0.0, false),
            (240.0, PILL_H, 22.0, false),
            (170.0, PILL_H, 0.0, false),
            (360.0, PILL_H, 0.0, false),
            (280.0, 302.0, 0.0, false),
            (280.0, 302.0, 0.0, true),
        ] {
            let (l, t, r, b) = shape_rect(NOMINAL, w, h, m, above);
            assert!(
                l >= 0.0,
                "{w}x{h}+{m} (above={above}) overflows the left edge: {l}"
            );
            assert!(
                t >= 0.0,
                "{w}x{h}+{m} (above={above}) overflows the top edge: {t}"
            );
            assert!(
                r <= OVERLAY_W,
                "{w}x{h}+{m} (above={above}) overflows the right edge: {r}"
            );
            assert!(
                b <= OVERLAY_H,
                "{w}x{h}+{m} (above={above}) overflows the bottom edge: {b}"
            );
        }
    }

    /// The union covers both rectangles, in both directions. This is what makes
    /// grow-now/shrink-late safe: whichever way the bar changes size, the region
    /// applied immediately contains the pill that is still painted.
    #[test]
    fn union_is_a_superset_both_ways() {
        let states = [
            shape_rect(NOMINAL, 150.0, PILL_H, 0.0, false),
            shape_rect(NOMINAL, 240.0, PILL_H, 22.0, false),
            shape_rect(NOMINAL, 170.0, PILL_H, 0.0, false),
            shape_rect(NOMINAL, 360.0, PILL_H, 0.0, false),
            shape_rect(NOMINAL, 280.0, 302.0, 0.0, false),
            shape_rect(NOMINAL, 280.0, 302.0, 0.0, true),
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
        let (l, t, r, b) = shape_rect(NOMINAL, 240.0, PILL_H, 22.0, false);
        assert_eq!(r - l, 240.0 + 44.0);
        assert_eq!(b - t, PILL_H + 44.0);
    }

    /// A menu shape's top edge is exactly the top of the menu.
    ///
    /// The frontend sends `pill_h` as the pill plus the *measured* menu, and the
    /// menu is laid out flush against the pill, so this arithmetic has to put the
    /// region's top edge exactly where the menu starts painting. It used to be
    /// handed a modelled height that overshot by 6px, and every one of those
    /// pixels was region with nothing painted in it.
    #[test]
    fn a_menu_shape_starts_where_the_menu_starts() {
        // 280 tall menu sitting flush on a 40px pill: 320 total.
        let (_, t, _, b) = shape_rect(NOMINAL, 280.0, PILL_H + 280.0, 0.0, true);
        assert_eq!(
            t,
            PILL_TOP - 280.0,
            "the region must start at the menu's top"
        );
        assert_eq!(b, PILL_TOP + PILL_H, "and end at the pill's bottom");
    }

    /// Closing the menu must not leave the region spanning where it used to be.
    ///
    /// `union_rect` is the grow-now half of "grow now, shrink late", and it is
    /// right for the pill, which morphs between sizes while still painted. The
    /// menu does not morph: it is unmounted in the frame it is dismissed. Unioning
    /// against it held a region open over nothing for `SHAPE_SETTLE_MS`, which is
    /// half a second of unpainted window. `set_shape` therefore unions a
    /// menu-less shape against the last menu-less shape, and this is that rule.
    #[test]
    fn a_closing_menu_does_not_drag_its_region_along() {
        let wide_pill = shape_rect(NOMINAL, 280.0, PILL_H, 0.0, false);
        let with_menu = shape_rect(NOMINAL, 280.0, PILL_H + 280.0, 0.0, true);
        let narrow_pill = shape_rect(NOMINAL, 173.0, PILL_H, 0.0, false);

        // What the old rule did: union the dead menu box into the new pill shape.
        let old = union_rect(with_menu, narrow_pill);
        assert!(
            old.1 < PILL_TOP,
            "the bug: unioning against the menu keeps the region above the pill"
        );

        // What the new rule does: union against the last shape that had no menu.
        let new = union_rect(wide_pill, narrow_pill);
        assert_eq!(
            new.1, PILL_TOP,
            "the region starts at the pill, not the menu"
        );
        assert_eq!(new.3, PILL_TOP + PILL_H);
        // The pill's own width morph is still protected.
        assert_eq!(new.0, wide_pill.0.min(narrow_pill.0));
        assert_eq!(new.2, wide_pill.2.max(narrow_pill.2));
    }
}

#[cfg(all(test, windows))]
mod region_tests {
    use super::*;

    /// The layout viewport a healthy webview reports. See `tests::NOMINAL`.
    const NOMINAL: (f64, f64) = (OVERLAY_W, OVERLAY_H);

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

    /// The bug: a webview that loses the window's scale must not lose the bar.
    ///
    /// Reconstructed from the 2026-09-05 log. The window is a 404x640 logical
    /// rectangle on a 125% panel, so 505x800 physical. WebView2 dropped its
    /// rasterization scale to 1, so the layout viewport became 505x800 CSS pixels
    /// and the collapsed bar painted at physical y 300..324.
    ///
    /// The old conversion centred the region in 404 and multiplied by 1.25,
    /// producing physical (192, 375, 313, 405) — which does not touch the bar at
    /// any point. Every other state missed by a similar margin, in the same
    /// direction, which is why the bar was invisible whatever the user pressed and
    /// why only a restart brought it back.
    #[test]
    fn a_webview_that_loses_its_scale_does_not_lose_the_bar() {
        // 505 physical pixels of client area, laid out as 505 CSS pixels.
        let view = (505.0, 800.0);
        let css_to_phys = 505.0 / view.0;
        assert_eq!(css_to_phys, 1.0, "one CSS pixel per physical pixel");

        for (w, h, m) in [
            (96.0, 24.0, 0.0),
            (150.0, PILL_H, 0.0),
            (240.0, PILL_H, 22.0),
        ] {
            let (l, t, r, b) = region_box(shape_rect(view, w, h, m, false), css_to_phys);
            // Where the bar is painted, in the same physical pixels.
            let left = ((view.0 - w) / 2.0) as i32;
            let (pl, pt, pr, pb) = (
                left,
                PILL_TOP as i32,
                left + w as i32,
                (PILL_TOP + h) as i32,
            );
            assert!(
                l <= pl && t <= pt && r >= pr && b >= pb,
                "{w}x{h}+{m}: region ({l},{t},{r},{b}) misses the bar at                  ({pl},{pt},{pr},{pb})"
            );
        }

        // And the shape the old code produced, kept as the thing being ruled out.
        let old_way = region_box(shape_rect(NOMINAL, 96.0, 24.0, 0.0, false), 1.25);
        assert_eq!(old_way, (192, 375, 313, 405));
        assert!(
            old_way.1 > (PILL_TOP + 24.0) as i32,
            "the region the bug produced starts below the bar it was clipping to"
        );
    }

    /// A rectangle is never inverted by rounding, at any scale.
    #[test]
    fn stays_well_formed_at_every_scale() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            for (w, m) in [(150.0, 0.0), (240.0, 22.0), (170.0, 0.0), (360.0, 0.0)] {
                let (l, t, r, b) = region_box(shape_rect(NOMINAL, w, PILL_H, m, false), scale);
                assert!(r > l && b > t, "inverted at {scale} for {w}+{m}");
            }
        }
    }
}

/// The persisted collapse preferences.
///
/// Split into its own module rather than added to `tests` above because it tests
/// serialisation rather than geometry, and the two share no fixtures.
#[cfg(test)]
mod collapse_tests {
    use super::*;

    /// A settings file written before the collapse existed must load, and must
    /// load with the collapse *on*.
    ///
    /// This is the migration that matters. Every existing user has a file with
    /// no `auto_collapse` key in it, and `#[serde(default)]` on a `bool` would
    /// give them `false` — the feature silently off for exactly the people who
    /// already use the app. `default = "yes"` is not decoration.
    #[test]
    fn a_file_from_before_the_feature_gets_the_collapse_on() {
        let legacy = r#"{"cx": 960.0, "top": 900.0, "mini": false}"#;
        let p: Placement = serde_json::from_str(legacy).expect("legacy placement parses");
        assert!(p.auto_collapse, "collapse must default on, not off");
        assert_eq!(p.collapse_delay_ms, 5_000);
    }

    /// Both fields survive a save/load cycle, including the non-default values.
    #[test]
    fn the_preferences_round_trip() {
        let mut p = Placement {
            auto_collapse: false,
            collapse_delay_ms: 12_000,
            ..Placement::default()
        };
        p.cx = 100.0;
        p.top = 200.0;

        let json = serde_json::to_string(&p).expect("serialises");
        let back: Placement = serde_json::from_str(&json).expect("deserialises");

        assert!(!back.auto_collapse);
        assert_eq!(back.collapse_delay_ms, 12_000);
    }

    /// `mini` and `auto_collapse` are independent.
    ///
    /// They read similarly and it would be easy to collapse them into one flag.
    /// They are not the same question: `mini` pins the bar small and ignores the
    /// clock, `auto_collapse` lets the clock make it small. Someone can want
    /// either without the other, and all four combinations are legal.
    #[test]
    fn pinning_small_and_collapsing_on_a_timer_are_different_settings() {
        for (mini, auto) in [(false, false), (false, true), (true, false), (true, true)] {
            let p = Placement {
                mini,
                auto_collapse: auto,
                ..Placement::default()
            };
            let json = serde_json::to_string(&p).expect("serialises");
            let back: Placement = serde_json::from_str(&json).expect("deserialises");
            assert_eq!(back.mini, mini);
            assert_eq!(back.auto_collapse, auto);
        }
    }

    /// A preference set before the bar has ever been placed survives a restart.
    ///
    /// Regression test for a silent reset. `cx`/`top` are `NAN` until the bar is
    /// first positioned; `serde_json` writes NaN as `null`, `null` will not read
    /// back as `f64`, and `load` turns any parse failure into `default()`. So
    /// snoozing a never-yet-dragged bar, or turning off the collapse, used to be
    /// forgotten on the next launch along with every other preference in the
    /// file. Found by the four-combination test below failing on the default.
    #[test]
    fn preferences_survive_a_restart_before_the_bar_is_ever_placed() {
        let fresh = Placement {
            auto_collapse: false,
            hidden_until: 42,
            mini: true,
            ..Placement::default()
        };
        assert!(fresh.cx.is_nan(), "precondition: never placed");

        let json = serde_json::to_string(&fresh).expect("serialises");
        assert!(
            !json.contains("null"),
            "an unplaced anchor must be absent, not null: {json}"
        );

        let back: Placement = serde_json::from_str(&json).expect("survives the round trip");
        assert!(!back.auto_collapse, "the collapse preference was lost");
        assert_eq!(back.hidden_until, 42, "the snooze was lost");
        assert!(back.mini, "the pinned-small preference was lost");
        assert!(back.cx.is_nan(), "still unplaced");
    }
}
