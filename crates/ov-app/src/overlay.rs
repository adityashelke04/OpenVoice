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
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, LogicalPosition, Manager, WebviewWindow};

/// Distance from a screen edge within which the bar snaps flush to it.
const SNAP_PX: f64 = 28.0;
/// Clearance left below the bar when it is auto-placed, enough for a taskbar.
const BOTTOM_GAP: f64 = 96.0;

/// Persisted overlay state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    /// Whether the bar is shown when nothing is being dictated.
    #[serde(default = "yes")]
    pub always_visible: bool,
    /// Unix millis until which the user has asked for quiet.
    #[serde(default)]
    pub hidden_until: u64,
}

fn yes() -> bool {
    true
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            x: f64::NAN,
            y: f64::NAN,
            always_visible: true,
            hidden_until: 0,
        }
    }
}

impl Placement {
    fn path() -> PathBuf {
        crate::history::data_dir().join("overlay.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
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
    pub fn move_to(&self, win: &WebviewWindow, x: f64, y: f64) -> (f64, f64) {
        let echo = self.is_echo(x, y);
        // Logged unconditionally, including the echoes.
        //
        // This function writes the user's saved position to disk, and it had no
        // instrumentation at all. That is how a spurious commit — an automatic
        // resize mistaken for a drag — could rewrite the placement with nothing
        // to show for it, and how days were spent reading a log that recorded
        // every resize perfectly while saying nothing about the one call that
        // was moving the bar.
        let size = win_size(win);
        if echo {
            tracing::debug!(x, y, "overlay move_to ignored as echo");
            return (x, y);
        }
        let (sx, sy) = snap_box(win, x, y, size);
        tracing::debug!(
            from = ?(x, y),
            to = ?(sx, sy),
            box_size = ?size,
            "overlay move_to committed"
        );
        {
            let mut p = self.placement.lock().expect("placement");
            p.x = sx;
            p.y = sy;
            p.save();
        }
        // Recorded as commanded even though nothing was commanded: the frontend
        // is about to move the window here, and those moves coming back must not
        // be mistaken for a fresh drag.
        self.remember(sx, sy);
        (sx, sy)
    }

    /// Move and resize in one step.
    ///
    /// Two calls from the frontend are two round trips, and between them the
    /// window is briefly the new size at the old position. With the glow margin
    /// that gap was plainly visible: the pill appeared to slide down and to the
    /// right every time the hotkey went down, then snap back. Doing both here
    /// closes the gap to the width of one function body.
    ///
    /// Position first when growing, size first when shrinking, so the window is
    /// never momentarily covering ground it has no business covering.
    pub fn set_box(&self, win: &WebviewWindow, x: f64, y: f64, w: f64, h: f64) {
        self.remember(x, y);

        #[cfg(windows)]
        if set_box_atomic(win, x, y, w, h) {
            return;
        }

        // Fallback, and visibly worse — see `set_box_atomic`. Size first, so the
        // window is never briefly the old size at the new position, which is the
        // ordering that throws the pill toward the top-left.
        //
        // Logged, and at `warn`. This path reintroduces the intermediate frame the
        // atomic path exists to delete, so a bar that flickers because the atomic
        // call is failing on this machine is otherwise indistinguishable from a bar
        // that flickers for a new reason. The return value used to be discarded and
        // neither branch said anything, which meant a whole class of report could
        // not be triaged from a log.
        tracing::warn!(x, y, w, h, "overlay set_box fell back to two calls");
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
        let _ = win.set_position(LogicalPosition::new(x, y));
    }

    /// Where the bar *would* land if it were released at this position.
    ///
    /// Pure: reads nothing, writes nothing, moves nothing. It exists so the bar
    /// can show the user where it is about to snap while they are still
    /// dragging, without the frontend having to reimplement `snap` — which would
    /// split one rule across two languages and guarantee they drift.
    pub fn snap_preview(&self, win: &WebviewWindow, x: f64, y: f64) -> (f64, f64) {
        snap(win, x, y)
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
        if p.x.is_finite() && p.y.is_finite() && on_screen(win, p.x, p.y) {
            self.command(win, p.x, p.y);
            return;
        }

        let Some((origin, area)) = monitor_logical(win) else {
            return;
        };
        let size = win_size(win);
        let x = origin.0 + (area.0 - size.0) / 2.0;
        let y = origin.1 + area.1 - size.1 - BOTTOM_GAP;
        self.command(win, x, y);
        tracing::debug!(x, y, "overlay auto-placed");
    }

    /// Move the window, remembering that this side asked for it.
    ///
    /// Also tells the frontend, which keeps its own idea of where the bar
    /// belongs so it can resize the window around a stationary pill. This side
    /// places the bar at startup and whenever it comes back from hidden, and
    /// without being told, the frontend would go on sizing around wherever the
    /// bar used to be.
    fn command(&self, win: &WebviewWindow, x: f64, y: f64) {
        self.remember(x, y);
        let _ = win.set_position(LogicalPosition::new(x, y));
        let _ = win.emit("overlay-parked", (x, y));
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

/// The bar's current size, which auto-placement centres on.
///
/// The window is declared in `tauri.conf.json` at the same size the frontend gives
/// the idle pill, so this answers the same thing whether it is asked before or
/// after the webview's first resize. When the two disagreed, a bar parked during
/// startup and a bar parked later landed tens of pixels apart.
fn win_size(win: &WebviewWindow) -> (f64, f64) {
    let scale = win.scale_factor().unwrap_or(1.0);
    win.outer_size()
        .map(|s| {
            let l = s.to_logical::<f64>(scale);
            (l.width, l.height)
        })
        .unwrap_or((150.0, 40.0))
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

/// Pull the bar flush to a screen edge when released near one.
fn snap(win: &WebviewWindow, x: f64, y: f64) -> (f64, f64) {
    snap_box(win, x, y, win_size(win))
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

/// Move and resize in a single, atomic Win32 call.
///
/// `set_position` and `set_size` are two separate window messages, and the
/// compositor is free to paint between them. Whichever order they go in, one
/// intermediate frame shows the wrong geometry: position first puts the old,
/// smaller window at the new origin — throwing the pill up and to the left by
/// the glow margin — and size first grows it at the old origin, throwing it the
/// other way. Spamming the hotkey makes that single frame land often enough to
/// look like the bar is crawling into the corner, and a half-painted window in
/// that frame is the stray triangle.
///
/// `SetWindowPos` does both at once, so no such frame exists. Win32 works in
/// physical pixels, hence the scaling; `SWP_NOZORDER` preserves always-on-top
/// and `SWP_NOACTIVATE` preserves the thing this whole window depends on — that
/// it never takes focus, and so never steals the caret from the user's editor.
///
/// `SWP_NOCOPYBITS` because every call here changes position *and* size. Without
/// it Windows preserves the client bits it considers still valid and invalidates
/// only the newly exposed region, so part of the window keeps showing the
/// previous frame — a rounded corner and a length of border from the old, smaller
/// pill, sitting in the new, larger box. Nothing painted here is worth salvaging;
/// the whole client area is about to be redrawn anyway.
#[cfg(windows)]
fn set_box_atomic(win: &WebviewWindow, x: f64, y: f64, w: f64, h: f64) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOCOPYBITS, SWP_NOZORDER,
    };

    let (Ok(scale), Ok(handle)) = (win.scale_factor(), win.hwnd()) else {
        return false;
    };
    let (px, py, pw, ph) = physical_box(x, y, w, h, scale);

    // Rebuilt from the raw pointer rather than passed through, so this does not
    // depend on Tauri and this crate resolving the same `windows` version.
    let hwnd = HWND(handle.0 as _);
    let ok = unsafe {
        SetWindowPos(
            hwnd,
            None,
            px,
            py,
            pw,
            ph,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS,
        )
        .is_ok()
    };

    if ok {
        sync_webview_child(hwnd);
    }
    trace_box(hwnd, scale, (x, y, w, h), (px, py, pw, ph), ok);
    ok
}

/// Logical box to physical, as Win32 wants it.
///
/// Separated out to be testable: this rounding is the only place a scale factor
/// touches the bar's geometry, and getting it wrong is invisible at 100% and
/// permanent at 125%.
#[cfg(windows)]
fn physical_box(x: f64, y: f64, w: f64, h: f64, scale: f64) -> (i32, i32, i32, i32) {
    (
        (x * scale).round() as i32,
        (y * scale).round() as i32,
        (w * scale).round() as i32,
        (h * scale).round() as i32,
    )
}

/// Resize the WebView2 child window to match its parent *now*, not next tick.
///
/// This is the fix for the bar appearing as a cropped rectangle with one rounded
/// corner and a green border running into a hard edge.
///
/// `wry` subclasses this window and handles `WM_SIZE` in two steps with two
/// different timings. It calls `ICoreWebView2Controller::SetBounds` synchronously,
/// which resizes the *layout viewport* immediately — the page relayouts to the new
/// size before `SetWindowPos` above has even returned. It then moves the WebView2
/// child HWND with `SWP_ASYNCWINDOWPOS`, which by definition only *posts* the
/// request. So for at least one turn of the message pump the page is laid out at
/// the new size while the surface actually on screen is still the old, smaller
/// rectangle at the client origin — and what the user sees is the top-left crop of
/// a correctly drawn pill. At the idle-to-listening transition that crop is 150x40
/// of a 284x84 layout, which is exactly the top-left corner of the pill, its top
/// border cut off mid-run, and two hard square edges where the surface ends.
///
/// Doing it again synchronously here closes that window. `wry`'s posted call
/// lands afterwards with the same values and is a no-op.
///
/// Every child is walked rather than just the first: the child chain is `wry`'s
/// business, not ours, and one that stayed the old size would leave the same crop.
#[cfg(windows)]
fn sync_webview_child(parent: windows::Win32::Foundation::HWND) {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindow, SetWindowPos, GW_CHILD, GW_HWNDNEXT, SWP_NOACTIVATE,
        SWP_NOCOPYBITS, SWP_NOZORDER,
    };

    let mut client = RECT::default();
    // SAFETY: `parent` is a live window handle owned by this process.
    if unsafe { GetClientRect(parent, &mut client) }.is_err() {
        return;
    }
    let (cw, ch) = (client.right - client.left, client.bottom - client.top);

    // SAFETY: as above; `GetWindow` returns either a live handle or an error.
    let mut child: Option<HWND> = unsafe { GetWindow(parent, GW_CHILD) }.ok();
    // Bounded: the chain is two or three windows deep in practice, and a cycle
    // here would hang the UI thread.
    for _ in 0..8 {
        let Some(hwnd) = child.filter(|h| !h.is_invalid()) else {
            return;
        };
        // SAFETY: `hwnd` is a live child of a window owned by this process.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                cw,
                ch,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS,
            );
            child = GetWindow(hwnd, GW_HWNDNEXT).ok();
        }
    }
}

/// Everything needed to tell which side of the resize handshake went wrong.
///
/// The parent's client rect and the child's are the direct test: if they disagree
/// after the call, the surface is lagging the window (the cropped-rectangle
/// symptom) and [`sync_webview_child`] did not do its job. If they agree and the
/// bar still looks wrong, the fault is on the web side, where `overlay-trace.ts`
/// is watching the other half of the same handshake.
#[cfg(windows)]
fn trace_box(
    parent: windows::Win32::Foundation::HWND,
    scale: f64,
    logical: (f64, f64, f64, f64),
    physical: (i32, i32, i32, i32),
    ok: bool,
) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindow, GW_CHILD};

    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }

    let client = |h| {
        let mut r = RECT::default();
        // SAFETY: `h` is a live window handle owned by this process.
        unsafe { GetClientRect(h, &mut r) }
            .ok()
            .map(|()| (r.right - r.left, r.bottom - r.top))
    };
    let parent_client = client(parent);
    // SAFETY: as above.
    let child_client = unsafe { GetWindow(parent, GW_CHILD) }
        .ok()
        .filter(|h| !h.is_invalid())
        .and_then(client);

    tracing::debug!(
        ok,
        scale,
        logical = ?logical,
        physical = ?physical,
        parent_client = ?parent_client,
        child_client = ?child_client,
        synced = parent_client == child_client,
        "overlay set_box"
    );
}

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

#[cfg(all(test, windows))]
mod tests {
    use super::physical_box;

    /// At 100% the logical box survives untouched. This is the case every
    /// developer sees and the reason the scaled cases below went unnoticed.
    #[test]
    fn unscaled_box_is_the_identity() {
        assert_eq!(
            physical_box(100.0, 200.0, 284.0, 84.0, 1.0),
            (100, 200, 284, 84)
        );
    }

    /// The listening box at 125%, the scale a 1080p laptop panel ships at.
    #[test]
    fn scales_position_and_size_together() {
        assert_eq!(
            physical_box(100.0, 200.0, 284.0, 84.0, 1.25),
            (125, 250, 355, 105)
        );
    }

    /// Rounding, not truncation.
    ///
    /// The pill's width is measured off rendered text and is routinely odd, so
    /// half-pixels are the normal case rather than the corner one. Truncating
    /// loses up to a physical pixel off the right edge on every resize, and the
    /// pill fills its window exactly — a pixel off the window is a pixel off the
    /// pill's border, which is 1px to begin with.
    #[test]
    fn rounds_rather_than_truncates() {
        assert_eq!(physical_box(0.0, 0.0, 241.0, 40.0, 1.5), (0, 0, 362, 60));
        assert_eq!(
            physical_box(10.5, 10.5, 151.0, 40.0, 1.0),
            (11, 11, 151, 40)
        );
    }

    /// Negative coordinates round the same way. A second monitor placed left of
    /// the primary has a negative origin, and the bar can legitimately live there.
    #[test]
    fn handles_negative_origins() {
        assert_eq!(
            physical_box(-1920.0, -100.0, 150.0, 40.0, 1.0),
            (-1920, -100, 150, 40)
        );
    }
}
