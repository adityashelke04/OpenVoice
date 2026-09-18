//! Re-delivering a history row from the Hub.
//!
//! The Hub's own window has focus when its "paste again" button is clicked, so
//! injecting immediately would type into the Hub. The command minimises the
//! Hub first and then waits here for Windows to hand focus to whatever was
//! behind it, before falling through to the engine's normal injection path.

use std::time::{Duration, Instant};

/// Poll `cond` every `every` until it returns `true`, or give up after `timeout`.
///
/// Checked before the first sleep, so a condition that already holds returns
/// immediately rather than paying for one wasted interval.
pub fn poll_until(timeout: Duration, every: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if cond() {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(every);
    }
}

/// The window Windows currently considers foreground, as a raw handle value.
///
/// Returned as `isize` rather than a `HWND` newtype because the caller compares
/// it against a handle obtained through Tauri's `windows` dependency, which can
/// be pinned to a different version of the `windows` crate than this one. The
/// two `HWND` types are then structurally identical but nominally distinct, so
/// a direct `==` would not compile; comparing the raw pointer value sidesteps
/// that entirely.
#[cfg(windows)]
pub fn foreground_hwnd() -> isize {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    // SAFETY: GetForegroundWindow takes no arguments and has no preconditions.
    unsafe { GetForegroundWindow() }.0 as isize
}

/// Off Windows there is no foreground window to ask about; `0` is never a real
/// handle, and [`focus_moved`] treats it as "not moved" rather than "moved".
#[cfg(not(windows))]
pub fn foreground_hwnd() -> isize {
    0
}

/// Whether focus has genuinely left the Hub.
///
/// `GetForegroundWindow` briefly returns `NULL` (`0`) *during* the minimise
/// transition -- a real gap where nothing holds focus yet, not another window
/// taking it. Treating that as "moved" (as a plain `fg != hub` would) made
/// `paste_again` inject into whatever had no focus at all and still report
/// success. `0` is excluded explicitly rather than relied on to merely differ
/// from `hub`.
pub fn focus_moved(fg: isize, hub: isize) -> bool {
    fg != 0 && fg != hub
}

#[cfg(test)]
mod tests {
    use super::{focus_moved, poll_until};
    use std::time::Duration;

    #[test]
    fn stops_as_soon_as_the_condition_holds() {
        let mut n = 0;
        assert!(poll_until(
            Duration::from_millis(600),
            Duration::from_millis(1),
            || {
                n += 1;
                n == 3
            }
        ));
        assert_eq!(n, 3);
    }

    #[test]
    fn gives_up_after_the_timeout() {
        let start = std::time::Instant::now();
        assert!(!poll_until(
            Duration::from_millis(60),
            Duration::from_millis(20),
            || false
        ));
        assert!(start.elapsed() >= Duration::from_millis(60));
    }

    #[test]
    fn null_and_the_hub_itself_do_not_count_as_moved() {
        // NULL during the minimise transition, and the Hub still holding focus,
        // are the two cases a plain `fg != hub` would get wrong.
        assert!(!focus_moved(0, 42));
        assert!(!focus_moved(42, 42));
    }

    #[test]
    fn a_real_other_window_counts_as_moved() {
        assert!(focus_moved(7, 42));
    }
}
