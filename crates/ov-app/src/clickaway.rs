//! Closing the Flow Menu when the user clicks somewhere else.
//!
//! # The bug this module exists for
//!
//! Right-clicking the bar opened the menu and nothing could close it. Clicking
//! anywhere else on the screen did nothing at all; the panel stayed up until the
//! user came back and right-clicked the bar a second time.
//!
//! That is not a missing handler. The Flow Bar is `WS_EX_NOACTIVATE`, so it never
//! takes focus and therefore never loses it — `blur` cannot fire — and it is
//! clipped by `SetWindowRgn` to the pill, so every pixel outside that region
//! belongs to some other window and no click there is ever delivered here. The
//! window is structurally incapable of noticing. It is the same failure as
//! `TrackPopupMenu` without `SetForegroundWindow`, for the same reason.
//!
//! # Why a hook, and why only sometimes
//!
//! The signal has to come from outside the window, which on Windows means a
//! low-level input hook. `WH_MOUSE_LL` sees every button-down system-wide
//! regardless of focus, including clicks inside the application that already has
//! it — which is the common case and the one a foreground-change hook misses.
//!
//! It is installed when the menu opens and removed when it closes. A dictation
//! tool that watched every click for the life of the process would be claiming a
//! capability it does not need, and this one needs it for the few seconds a menu
//! is on screen.
//!
//! # The rules the callback lives by
//!
//! These are the rules from `ov-input`'s keyboard hook, and they are not advice.
//!
//! 1. **Return in well under 10 ms.** Windows silently evicts a slow low-level
//!    hook. There is no error, no log, no event — the menu simply stops closing
//!    again. So the callback does four integer comparisons and one non-blocking
//!    `GetWindowRect`. No locks, no allocation, no logging, no I/O.
//! 2. **Never swallow the click.** `CallNextHookEx` on every path. A native menu
//!    eats the click that dismisses it; this bar is a passive overlay, and eating
//!    someone's click on their own editor would be both surprising and, for a
//!    tool that types into that editor, dangerous.
//! 3. **Store nothing.** The callback reads a coordinate, compares it, and drops
//!    it. No history, no buffer, no path off this thread but a single unit send.
//!
//! # Why the region and not the window
//!
//! ADR 0007 fixed the window at 404x640 and made the *region* the thing that
//! changes per state. A click inside that rectangle but outside the region lands
//! on whatever is painted underneath, so treating the window as the boundary
//! would leave a 404x640 patch of screen where clicking did nothing. The region
//! is the bar.
//!
//! `WindowFromPoint` would give this for free and is region-aware, but it sends
//! `WM_NCHITTEST` to the windows it probes — so a hung third-party window under
//! the cursor could block the callback past the eviction timeout. Cached
//! arithmetic cannot block.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// The current region box in physical pixels, relative to the window's top-left.
///
/// Atomics rather than a `Mutex` because the hook callback reads them on the
/// critical path of the user's mouse, where taking a lock is forbidden (rule 1).
/// The four are written together and read together; a tear between them costs at
/// worst one misjudged click on the frame a shape changes, which is not worth a
/// lock on every button press in the system.
static REGION_L: AtomicI32 = AtomicI32::new(0);
static REGION_T: AtomicI32 = AtomicI32::new(0);
static REGION_R: AtomicI32 = AtomicI32::new(0);
static REGION_B: AtomicI32 = AtomicI32::new(0);
/// Whether a region has ever been published. See [`outside`].
static REGION_SET: AtomicBool = AtomicBool::new(false);

/// Publish the region the window is currently clipped to.
///
/// Called from `overlay::apply_region`, which already computes exactly these four
/// numbers on every shape change. Kept as a push rather than a read-back because
/// the alternative — asking GDI for the window's region inside the hook callback
/// — is a syscall on the critical path for a value this side already knows.
pub fn set_region(l: i32, t: i32, r: i32, b: i32) {
    REGION_L.store(l, Ordering::Relaxed);
    REGION_T.store(t, Ordering::Relaxed);
    REGION_R.store(r, Ordering::Relaxed);
    REGION_B.store(b, Ordering::Relaxed);
    REGION_SET.store(true, Ordering::Relaxed);
}

/// Forget the published region. Test support, and the state before the first shape.
#[cfg_attr(not(test), allow(dead_code))]
pub fn clear_region() {
    REGION_SET.store(false, Ordering::Relaxed);
}

/// Whether a click at this screen point is outside the bar.
///
/// `px, py` are physical screen coordinates, as `MSLLHOOKSTRUCT.pt` reports them.
/// `origin_x, origin_y` is the window's physical screen origin, read per event
/// because the bar can be moved while a shape is live.
///
/// The boundary counts as inside. `region_box` in `overlay.rs` rounds outward for
/// the same reason: a click on the pill's own border is a click on the pill, and
/// dismissing the menu out from under an aimed pointer is the worse mistake.
///
/// Answers `false` for everything when no region has been published. "Unknown"
/// must not mean "outside", or the very first click after launch — the one that
/// opened the menu — would close it again.
// The hook callback is the only caller, and it arrives in the next commit.
#[cfg_attr(not(test), allow(dead_code))]
#[must_use]
pub fn outside(px: i32, py: i32, origin_x: i32, origin_y: i32) -> bool {
    if !REGION_SET.load(Ordering::Relaxed) {
        return false;
    }
    let x = px - origin_x;
    let y = py - origin_y;
    x < REGION_L.load(Ordering::Relaxed)
        || x > REGION_R.load(Ordering::Relaxed)
        || y < REGION_T.load(Ordering::Relaxed)
        || y > REGION_B.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The region is process-wide state and cargo runs tests in parallel, so
    /// without this one test's `clear_region` lands in the middle of another's
    /// assertions. The same hazard `ov-input`'s hook tests have with `BOUND_VK`.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Take the lock and publish a known region. Returns the guard, which the
    /// caller must hold for the length of the test.
    fn guard() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The window is a fixed 404x640 rectangle clipped by `SetWindowRgn` to the
    /// pill (or the pill plus its menu). A click inside that 404x640 rect but
    /// outside the region lands on whatever application is underneath, so from
    /// the user's side it *is* a click somewhere else. The hit test is therefore
    /// against the region, never against the window.
    #[test]
    fn a_point_inside_the_window_but_outside_the_region_is_outside() {
        let _g = guard();
        // Window origin at (1000, 500); region occupies (82,300)-(322,340)
        // within it -- the idle pill, in window-relative physical pixels.
        set_region(82, 300, 322, 340);

        // Dead centre of the pill: inside.
        assert!(!outside(1000 + 200, 500 + 320, 1000, 500));

        // Same window, 40px above the pill's top edge. Inside the 404x640
        // window, outside the region, and painted by somebody else's app.
        assert!(outside(1000 + 200, 500 + 260, 1000, 500));

        // Well clear of the window entirely.
        assert!(outside(50, 50, 1000, 500));
    }

    /// The bar can be dragged while a shape is live, so the region is cached
    /// window-relative and the origin is read per event. A moved window must not
    /// make old coordinates read as inside.
    #[test]
    fn the_region_travels_with_the_window() {
        let _g = guard();
        set_region(82, 300, 322, 340);
        assert!(!outside(1200, 820, 1000, 500));
        // Same screen point, window has since moved 300px right.
        assert!(outside(1200, 820, 1300, 500));
    }

    /// Edges belong to the bar. `region_box` in overlay.rs rounds outward for
    /// exactly this reason: a click on the pill's own border must not dismiss
    /// the menu the user is aiming at.
    #[test]
    fn the_boundary_counts_as_inside() {
        let _g = guard();
        set_region(82, 300, 322, 340);
        assert!(!outside(1082, 800, 1000, 500));
        assert!(!outside(1322, 840, 1000, 500));
        assert!(outside(1081, 800, 1000, 500));
        assert!(outside(1323, 840, 1000, 500));
    }

    /// Before the first shape arrives there is no region. Treating "unknown" as
    /// "everything is outside" would close the menu on the click that opened it.
    #[test]
    fn an_unpublished_region_swallows_nothing() {
        let _g = guard();
        clear_region();
        assert!(!outside(0, 0, 0, 0));
        assert!(!outside(9999, 9999, 0, 0));
    }
}
