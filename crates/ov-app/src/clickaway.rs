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

// ---------------------------------------------------------------------------
// The hook itself. Windows-only; see the no-ops at the foot of this file.
// ---------------------------------------------------------------------------

/// Which mouse messages mean "the user has committed to something elsewhere".
///
/// Button-down, not button-up: a menu should close the instant the pointer is
/// pressed somewhere else, the way every other menu on the system does. Movement
/// and the wheel are excluded because a menu that closed when the pointer crossed
/// it would be unusable by anyone who does not travel in a straight line.
///
/// Pure and public so the rule can be asserted without installing a hook.
#[cfg(windows)]
#[must_use]
pub fn is_button_down(msg: u32) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
    };
    // The non-client variants are deliberately absent: `WH_MOUSE_LL` reports
    // screen-space button messages only and never delivers a WM_NC* form, so
    // listing them would be a rule that reads as load-bearing and never fires.
    matches!(
        msg,
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
    )
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
    use std::sync::{Mutex, OnceLock};

    use tauri::{AppHandle, Emitter, Manager};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, GetWindowRect, PostThreadMessageW,
        SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, EVENT_SYSTEM_FOREGROUND, MSG,
        MSLLHOOKSTRUCT, WH_MOUSE_LL, WINEVENT_OUTOFCONTEXT, WM_QUIT,
    };

    /// The overlay window, for the hit test's origin and for the emit.
    ///
    /// An `isize` in an atomic rather than an `HWND` behind a lock, because the
    /// callback reads it on the critical path of the user's mouse.
    static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);

    /// The pumped thread's id, so `disarm` can post it a `WM_QUIT`.
    ///
    /// Zero when nothing is armed. Guarded by [`ARM_LOCK`] on the write side; the
    /// read side is the callback's and is allowed to be a plain load.
    static THREAD_ID: AtomicU32 = AtomicU32::new(0);

    /// Serialises `arm` against `disarm`.
    ///
    /// The frontend drives both from a React effect, and effects re-run for
    /// reasons unrelated to the menu. Without this, a fast open/close/open could
    /// leave two hook threads alive with one of them holding the only handle that
    /// could remove the other's hook.
    static ARM_LOCK: Mutex<()> = Mutex::new(());

    /// Where the callback posts. `OnceLock` because a hook procedure is an
    /// `extern "system" fn` and cannot capture.
    ///
    /// The channel exists so the callback never touches Tauri: emitting an event
    /// allocates, serialises, and crosses a process boundary, all of which are
    /// forbidden on the hook's critical path (rule 1). It sends a unit and returns.
    static TX: OnceLock<std::sync::mpsc::Sender<()>> = OnceLock::new();

    /// Whether a hook is currently installed.
    ///
    /// Test-only in production terms, and deliberately so: nothing in the app
    /// gates behaviour on this. Escape and the hide path emit their dismissal
    /// unconditionally, because a gate on "did the hook install" would make the
    /// backstops depend on the very thing they are backing up.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_armed() -> bool {
        THREAD_ID.load(Ordering::Relaxed) != 0
    }

    /// Start the dispatcher, once, for the life of the process.
    ///
    /// Its job is to be the slow half. Emitting a Tauri event allocates and
    /// crosses a process boundary, and doing that on the hook thread would risk
    /// the eviction that rule 1 exists to prevent.
    fn dispatcher(app: &AppHandle) -> &'static std::sync::mpsc::Sender<()> {
        TX.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            let app = app.clone();
            std::thread::Builder::new()
                .name("ov-clickaway-dispatch".into())
                .spawn(move || {
                    while rx.recv().is_ok() {
                        if let Some(win) = app.get_webview_window("overlay") {
                            let _ = win.emit("overlay-menu-dismiss", ());
                        }
                    }
                })
                .expect("clickaway dispatch thread");
            tx
        })
    }

    /// Install the hooks. Idempotent.
    pub fn arm(app: AppHandle) {
        let _guard = ARM_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if THREAD_ID.load(Ordering::Relaxed) != 0 {
            return;
        }
        let Some(win) = app.get_webview_window("overlay") else {
            tracing::warn!("no overlay window to watch for click-away");
            return;
        };
        let Ok(raw) = win.hwnd() else {
            tracing::warn!("overlay window has no HWND; click-away disabled");
            return;
        };
        OVERLAY_HWND.store(raw.0 as isize, Ordering::Relaxed);
        dispatcher(&app);

        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<u32>();
        std::thread::Builder::new()
            .name("ov-clickaway".into())
            .spawn(move || {
                // SAFETY: `mouse_proc` is a valid `extern "system"` callback with
                // the signature Windows expects. A null module handle with a zero
                // thread id installs a global low-level hook owned by this
                // thread, which is the documented contract for `WH_MOUSE_LL`.
                let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) };
                let Ok(hook) = hook else {
                    tracing::warn!("could not install the click-away mouse hook");
                    let _ = ready_tx.send(0);
                    return;
                };
                // SAFETY: out-of-context WinEvent hooks take a callback with this
                // signature and are delivered to this thread's message queue.
                let winevent = unsafe {
                    SetWinEventHook(
                        EVENT_SYSTEM_FOREGROUND,
                        EVENT_SYSTEM_FOREGROUND,
                        None,
                        Some(winevent_proc),
                        0,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    )
                };

                // SAFETY: no preconditions; returns this thread's own id.
                let tid = unsafe { GetCurrentThreadId() };
                let _ = ready_tx.send(tid);

                pump();

                // SAFETY: both handles came from successful installs on this
                // thread and are unhooked exactly once, here, as the thread ends.
                unsafe {
                    let _ = UnhookWindowsHookEx(hook);
                    if !winevent.is_invalid() {
                        let _ = UnhookWinEvent(winevent);
                    }
                }
            })
            .expect("clickaway hook thread");

        match ready_rx.recv() {
            Ok(0) | Err(_) => tracing::warn!("click-away hook did not start"),
            Ok(tid) => THREAD_ID.store(tid, Ordering::Relaxed),
        }
    }

    /// Remove the hooks. Idempotent.
    ///
    /// `WM_QUIT` rather than a flag, because the thread is blocked in
    /// `GetMessageW` and a flag it never wakes to read would leave the hook
    /// installed for the life of the process — which is exactly the standing
    /// capability this module is arranged to avoid.
    pub fn disarm() {
        let _guard = ARM_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tid = THREAD_ID.swap(0, Ordering::Relaxed);
        if tid == 0 {
            return;
        }
        // SAFETY: posting to a thread id this module created. A failed post means
        // the thread has already gone, which is the state we wanted anyway.
        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }

    /// Message pump for the hook thread.
    ///
    /// Low-level hooks are dispatched through the installing thread's message
    /// queue. Without this loop the callback is never invoked at all — which
    /// presents as "the menu still does not close", with nothing to go on.
    fn pump() {
        let mut msg = MSG::default();
        // SAFETY: `msg` is valid for the duration of each call; a null HWND asks
        // for messages belonging to this thread, which is where `WM_QUIT` lands.
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// The mouse callback. Runs on every mouse event system-wide while a menu is
    /// open. See the module's rules: four comparisons, one non-blocking
    /// `GetWindowRect`, one unit send, and never a swallowed event.
    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code < 0 {
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }
        if !super::is_button_down(wparam.0 as u32) {
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }

        let raw = OVERLAY_HWND.load(Ordering::Relaxed);
        if raw == 0 {
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }

        // SAFETY: for code >= 0 in a WH_MOUSE_LL hook, Windows guarantees lparam
        // points to a valid MSLLHOOKSTRUCT for the duration of this call.
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let POINT { x, y } = info.pt;

        // `GetWindowRect` sends no messages and cannot block on another process,
        // which is why it is here and `WindowFromPoint` is not.
        let mut r = RECT::default();
        // SAFETY: `raw` is a live window handle owned by this process.
        if unsafe { GetWindowRect(HWND(raw as *mut _), &mut r) }.is_err() {
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }

        if super::outside(x, y, r.left, r.top) {
            if let Some(tx) = TX.get() {
                let _ = tx.send(());
            }
        }

        // Always. The click belongs to whatever the user aimed it at.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// Another application came forward: alt-tab, the Win key, a notification.
    ///
    /// A click is not the only way to walk away from a menu, and the mouse hook
    /// cannot see any of these.
    unsafe extern "system" fn winevent_proc(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        _obj: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        // Our own window coming forward is not somebody else's. It should not be
        // able to happen -- the bar is WS_EX_NOACTIVATE -- but a foreground event
        // naming it would close the menu the user just opened.
        if hwnd.0 as isize == OVERLAY_HWND.load(Ordering::Relaxed) {
            return;
        }
        if let Some(tx) = TX.get() {
            let _ = tx.send(());
        }
    }
}

#[cfg(all(windows, test))]
pub use imp::is_armed;
#[cfg(windows)]
pub use imp::{arm, disarm};

// -- The bar is Windows-only today. These keep the crate compiling elsewhere
//    without scattering `cfg` through the call sites, exactly as `topmost` does.

#[cfg(not(windows))]
pub fn arm(_app: tauri::AppHandle) {}

#[cfg(not(windows))]
pub fn disarm() {}

#[cfg(all(not(windows), test))]
#[must_use]
pub fn is_armed() -> bool {
    false
}

#[cfg(not(windows))]
#[must_use]
pub fn is_button_down(_msg: u32) -> bool {
    false
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

    /// Arming twice must not install two hooks or leak a thread. The frontend
    /// drives this from a React effect, and effects re-run for reasons that have
    /// nothing to do with the menu.
    #[test]
    fn arming_is_idempotent_and_disarming_is_safe_when_idle() {
        let _g = guard();
        // Nothing is armed at rest.
        assert!(!is_armed());
        // Disarming an idle module is a no-op, not a panic. The frontend's
        // cleanup runs on unmount whether or not the menu was ever opened.
        disarm();
        assert!(!is_armed());
    }

    /// The buttons that dismiss a menu are the ones that go *down*. A mouse move
    /// across the screen must not close it, or the menu would be unusable for
    /// anyone who does not travel in a straight line.
    #[cfg(windows)]
    #[test]
    fn only_button_down_messages_dismiss() {
        use windows::Win32::UI::WindowsAndMessaging::{
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL,
            WM_RBUTTONDOWN, WM_XBUTTONDOWN,
        };
        for m in [
            WM_LBUTTONDOWN,
            WM_RBUTTONDOWN,
            WM_MBUTTONDOWN,
            WM_XBUTTONDOWN,
        ] {
            assert!(is_button_down(m), "{m:#x} should dismiss");
        }
        for m in [WM_MOUSEMOVE, WM_LBUTTONUP, WM_MOUSEWHEEL] {
            assert!(!is_button_down(m), "{m:#x} must not dismiss");
        }
    }
}
