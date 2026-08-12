//! Flow Bar window behaviour: placement, persistence, and visibility policy.
//!
//! # Why clicks work on a non-activating window
//!
//! `WS_EX_NOACTIVATE` stops a window *taking focus*; it does not stop it receiving
//! input. So the bar can be dragged and right-clicked while the user's editor keeps
//! the caret — which is the only reason an interactive always-on-top overlay is
//! possible for a dictation tool at all.

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
    /// Where this side last told the window to go.
    ///
    /// The frontend reports every move the OS makes, including the ones caused by
    /// [`Self::park`], and filters them with a "the user is dragging" flag. That
    /// flag is not trustworthy: `startDragging` hands the pointer to the Windows
    /// move loop, which swallows the mouse-up, so a press on the bar that moved
    /// nothing leaves it set — and the next auto-placement then comes back as a
    /// drag, gets snapped, and is written to disk as a position the user chose.
    commanded: Mutex<Option<(f64, f64)>>,
}

impl Overlay {
    pub fn new() -> Self {
        Self {
            placement: Mutex::new(Placement::load()),
            active: Mutex::new(false),
            commanded: Mutex::new(None),
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
        if self.is_echo(x, y) {
            return (x, y);
        }
        let (x, y) = snap(win, x, y);
        {
            let mut p = self.placement.lock().expect("placement");
            p.x = x;
            p.y = y;
            p.save();
        }
        // Recorded as commanded even though nothing was commanded: the frontend
        // is about to move the window here, and those moves coming back must not
        // be mistaken for a fresh drag.
        *self.commanded.lock().expect("commanded") = Some((x, y));
        (x, y)
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
        *self.commanded.lock().expect("commanded") = Some((x, y));

        #[cfg(windows)]
        if set_box_atomic(win, x, y, w, h) {
            return;
        }

        // Fallback, and visibly worse — see `set_box_atomic`. Size first, so the
        // window is never briefly the old size at the new position, which is the
        // ordering that throws the pill toward the top-left.
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
            let _ = win.show();
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
        *self.commanded.lock().expect("commanded") = Some((x, y));
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
        matches!(
            *self.commanded.lock().expect("commanded"),
            Some((cx, cy)) if (cx - x).abs() < 2.0 && (cy - y).abs() < 2.0
        )
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
    let Some((origin, area)) = monitor_logical(win) else {
        return (x, y);
    };
    let (w, h) = win_size(win);
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
#[cfg(windows)]
fn set_box_atomic(win: &WebviewWindow, x: f64, y: f64, w: f64, h: f64) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};

    let (Ok(scale), Ok(handle)) = (win.scale_factor(), win.hwnd()) else {
        return false;
    };
    let px = (x * scale).round() as i32;
    let py = (y * scale).round() as i32;
    let pw = (w * scale).round() as i32;
    let ph = (h * scale).round() as i32;

    // Rebuilt from the raw pointer rather than passed through, so this does not
    // depend on Tauri and this crate resolving the same `windows` version.
    let hwnd = HWND(handle.0 as _);
    unsafe { SetWindowPos(hwnd, None, px, py, pw, ph, SWP_NOZORDER | SWP_NOACTIVATE).is_ok() }
}

/// Convenience for command handlers.
pub fn window(app: &tauri::AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("overlay")
}
