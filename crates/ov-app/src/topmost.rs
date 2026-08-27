//! Keeping the Flow Bar on top, and noticing when Windows disagrees.
//!
//! # The bug this module exists for
//!
//! `alwaysOnTop: true` in `tauri.conf.json` sets `WS_EX_TOPMOST` once, at window
//! creation. It is not a subscription. The Windows shell **silently strips that
//! bit from other windows when any application enters exclusive fullscreen** — a
//! browser playing video, a game, a screen share, a slideshow — and never puts it
//! back. No window message is sent, no Tauri event fires, and `is_visible()` still
//! answers `true`, because the window genuinely is visible: it is simply at the
//! bottom of the z-order with everything else painted over it.
//!
//! From the user's side that is indistinguishable from the bar hiding itself,
//! which is exactly how it was reported — "it automatically gets hidden after
//! maybe 5 to 10 minutes". There is no timer. Five to ten minutes is how long it
//! takes to watch one fullscreen video.
//!
//! Nothing in the process ever asked for `HWND_TOPMOST` again, so once the bar
//! sank it stayed sunk until the app was restarted.
//!
//! # Why a poll, and not a hook into something the bar already does
//!
//! On an earlier arrangement the window was resized on every state change, and
//! the cheapest fix was to name `HWND_TOPMOST` in the `SetWindowPos` that was
//! happening anyway — an ordinary dictation would then repair the z-order as a
//! side effect. ADR 0007 removed that opportunity on purpose: the window is a
//! fixed rectangle that never moves or resizes, and what changes per state is its
//! *region*, which `SetWindowRgn` applies without touching z-order at all.
//!
//! So there is no longer any moment in normal use where the bar's z-order is
//! incidentally re-stated, and the watchdog is not a backstop for the idle case —
//! it is the whole mechanism. That is the argument for polling rather than
//! waiting for an event: the OS sends none when it strips the bit, and this
//! process no longer performs any operation that would restore it by accident.
//!
//! # Why this reads the flag before writing it
//!
//! The obvious fix is to re-assert on a timer and not think about it. Two reasons
//! not to:
//!
//! 1. A blind `SetWindowPos` every couple of seconds, forever, is a window message
//!    every couple of seconds, forever — for a process whose entire job is to sit
//!    still and wait for a keypress. Reading `GWL_EXSTYLE` is a cheap userspace
//!    read of window state, and in the overwhelmingly common case where nothing is
//!    wrong it is the only thing that happens.
//! 2. It makes recovery *countable*. A re-assert that fires unconditionally can
//!    never tell you whether the problem is happening. Checking first means every
//!    write is a real recovery, so [`recoveries`] is a true count of how many times
//!    Windows took the bar away — which is what turns "it feels like it hides
//!    itself" into a number in Settings.
//!
//! Going straight to Win32 also sidesteps a trap that has bitten other overlay
//! apps: a framework that caches its own idea of "always on top" will short-circuit
//! `set_always_on_top(true)` into a no-op when its cached value already says
//! `true`, which is precisely the state the window is in after the OS strips the
//! real bit. The cached flag says yes, the OS says no, and the re-assert never
//! reaches the OS. Reading and writing the actual extended style cannot desync from
//! itself.

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::AppHandle;
#[cfg(windows)]
use tauri::Manager;

/// How often the watchdog looks. Two seconds is chosen against the cost of being
/// wrong in each direction: the check is a userspace read, so a fast cadence is
/// nearly free, while a slow one leaves the bar buried for however long it takes
/// to come round again. Two seconds is below the threshold where someone alt-tabs
/// out of a video and starts wondering where the bar went.
#[cfg(windows)]
const POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// How many times Windows has taken the bar off the top and this module has put it
/// back, for the life of the process.
///
/// Surfaced in Settings rather than kept for debugging. The user reported this bug
/// as a feeling — the bar "automatically gets hidden" — because nothing in the app
/// could confirm or deny it. A number that goes up when it happens is the smallest
/// honest answer to that.
static RECOVERIES: AtomicU64 = AtomicU64::new(0);

/// Total recoveries since launch.
pub fn recoveries() -> u64 {
    RECOVERIES.load(Ordering::Relaxed)
}

/// Whether the OS currently considers this window topmost.
///
/// Reads the live extended style rather than any cached copy. See the module note
/// on why that distinction is the whole point.
#[cfg(windows)]
pub fn is_topmost(win: &tauri::WebviewWindow) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOPMOST};

    let Ok(raw) = win.hwnd() else {
        return false;
    };
    // SAFETY: `raw` is a live window handle owned by this process.
    let style = unsafe { GetWindowLongPtrW(HWND(raw.0 as _), GWL_EXSTYLE) };
    style & (WS_EX_TOPMOST.0 as isize) != 0
}

/// Put the window back on top. Position and size are untouched.
///
/// `SWP_NOACTIVATE` is not optional. Without it this call would focus the overlay,
/// which costs the user the caret in whatever they were typing into — the one
/// failure the entire window design exists to prevent.
#[cfg(windows)]
pub fn assert_topmost(win: &tauri::WebviewWindow) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    let Ok(raw) = win.hwnd() else {
        return false;
    };
    // SAFETY: `raw` is a live window handle owned by this process. The call moves
    // and resizes nothing (`SWP_NOMOVE | SWP_NOSIZE`) and activates nothing.
    unsafe {
        SetWindowPos(
            HWND(raw.0 as _),
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .is_ok()
    }
}

/// Restore topmost **only if it has actually been lost**, and say whether it had.
///
/// The return value is the interesting part: `true` means this call repaired real
/// damage, which is what [`RECOVERIES`] counts and what the log line records.
#[cfg(windows)]
pub fn reassert_if_lost(win: &tauri::WebviewWindow) -> bool {
    if is_topmost(win) {
        return false;
    }
    let ok = assert_topmost(win);
    RECOVERIES.fetch_add(1, Ordering::Relaxed);
    tracing::warn!(
        restored = ok,
        total = recoveries(),
        "flow bar had lost topmost; the shell strips it when an app goes fullscreen"
    );
    ok
}

/// Watch the Flow Bar's z-order for the life of the app.
///
/// The only thing that restores it. See the module note on why nothing else in
/// the app re-states z-order any more.
///
/// Skips the check entirely while the bar is hidden — a snoozed or
/// dictation-only bar has no z-order worth defending, and re-asserting topmost on
/// a hidden window would be the one thing capable of flashing it back on screen.
#[cfg(windows)]
pub fn spawn_watchdog(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL);
        let Some(win) = app.get_webview_window("overlay") else {
            // The window is gone: the app is shutting down and so is this thread.
            return;
        };
        if !win.is_visible().unwrap_or(false) {
            continue;
        }
        reassert_if_lost(&win);
    });
}

// -- Everything above is Win32. The bar is Windows-only today; these keep the
//    crate compiling elsewhere without scattering `cfg` through the call sites.

#[cfg(not(windows))]
pub fn is_topmost(_win: &tauri::WebviewWindow) -> bool {
    true
}

#[cfg(not(windows))]
pub fn assert_topmost(_win: &tauri::WebviewWindow) -> bool {
    true
}

#[cfg(not(windows))]
pub fn reassert_if_lost(_win: &tauri::WebviewWindow) -> bool {
    false
}

#[cfg(not(windows))]
pub fn spawn_watchdog(_app: AppHandle) {}
