//! Double-tap to leave the microphone open.
//!
//! Hold-to-talk is the fast path and stays exactly as it was: press, speak,
//! release. But holding a key through a long thought is its own kind of work,
//! and every dictation tool eventually grows a hands-free mode. Wispr Flow gives
//! it a separate chord (`Ctrl+Win+Space` on Windows); this reaches it by tapping
//! the shortcut twice, so there is one key to learn rather than two.
//!
//! # Why this is a type and not four lines in the hotkey callback
//!
//! It is a state machine with a clock, and the interesting parts are the ones
//! that are hard to reach by hand: a tap that arrives 351ms after the last one,
//! a hold that must not count as half of a double, the release belonging to a
//! press that already did something else. None of that is testable inside a
//! callback that needs a real keyboard, and all of it is testable here.
//!
//! # What it does not do
//!
//! It never talks to the session machine. It maps physical key events onto
//! *decisions*, and `engine.rs` turns those into `Input`s. There is still one
//! activation path through `session.rs`, which is the property that keeps
//! push-to-talk and click-to-dictate from drifting apart.

/// How close two taps must be to count as one gesture.
///
/// 350ms is the usual double-click ceiling, and the reason it is not longer is
/// that every millisecond here is time a genuine second dictation could be
/// mistaken for the back half of a gesture.
pub const DOUBLE_TAP_MS: u64 = 350;

/// The longest a press can last and still be a tap.
///
/// Above this it is a hold, and a hold is somebody dictating. Without this a
/// three-second push-to-talk that happened to follow a tap would latch the
/// microphone open, which is the worst failure this module could have.
pub const MAX_TAP_MS: u64 = 300;

/// What a physical key-down should become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnPress {
    /// Ordinary press. Start dictating, and stop when the key comes up.
    Start,
    /// The second tap of a double.
    ///
    /// Discard whatever the first tap started — it is a few tens of milliseconds
    /// of audio nobody meant to record — and start a session that will outlive
    /// the key.
    LatchOpen,
    /// A tap while the microphone was latched open. Close it.
    StopLatched,
}

/// What a physical key-up should become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnRelease {
    /// Ordinary release. Stop dictating.
    Stop,
    /// The machine must not see this one.
    ///
    /// Either it belongs to the press that latched the microphone open — in
    /// which case honouring it would close the session a quarter of a second
    /// after opening it — or to the tap that closed one, which has already been
    /// acted on.
    Swallow,
}

/// Tracks taps well enough to tell a gesture from a dictation.
#[derive(Debug, Default)]
pub struct TapLatch {
    /// When the last press that was short enough to be half of a double ended.
    last_tap_end: Option<u64>,
    /// When the press currently in progress began.
    pressed_at: Option<u64>,
    /// Whether the microphone is being held open without the key.
    latched: bool,
    /// Whether the next key-up has already been accounted for.
    swallow_next_release: bool,
}

impl TapLatch {
    /// Whether the microphone is currently latched open.
    ///
    /// The Flow Bar needs this: a latched session and a held one are otherwise
    /// identical on screen, and the difference is whether letting go stops it.
    #[must_use]
    pub fn is_latched(&self) -> bool {
        self.latched
    }

    /// A physical key-down at `now` (milliseconds, monotonic).
    pub fn press(&mut self, now: u64) -> OnPress {
        if self.latched {
            // Tapping the shortcut is the most obvious way to stop something the
            // shortcut started. The key-up that follows this must not then be
            // read as the end of a hold that never happened.
            self.latched = false;
            self.swallow_next_release = true;
            self.last_tap_end = None;
            self.pressed_at = Some(now);
            return OnPress::StopLatched;
        }

        let doubled = self
            .last_tap_end
            .is_some_and(|end| now.saturating_sub(end) <= DOUBLE_TAP_MS);

        self.pressed_at = Some(now);

        if doubled {
            self.latched = true;
            self.swallow_next_release = true;
            // Consumed. Three taps in a row are a double followed by a single,
            // not two overlapping doubles.
            self.last_tap_end = None;
            OnPress::LatchOpen
        } else {
            OnPress::Start
        }
    }

    /// A physical key-up at `now`.
    pub fn release(&mut self, now: u64) -> OnRelease {
        let held = self
            .pressed_at
            .take()
            .map_or(u64::MAX, |start| now.saturating_sub(start));

        if self.swallow_next_release {
            self.swallow_next_release = false;
            return OnRelease::Swallow;
        }

        // Only a short press can be half of a double tap. A hold is somebody
        // dictating, and the tap that follows it is a fresh gesture.
        self.last_tap_end = (held <= MAX_TAP_MS).then_some(now);
        OnRelease::Stop
    }

    /// The session ended by some other route: Escape, the Flow Bar's own
    /// control, a fault, or the recording cap.
    ///
    /// Without this the latch would still believe the microphone was open, and
    /// the next tap would "stop" a session that had already gone.
    pub fn forget(&mut self) {
        self.latched = false;
        self.last_tap_end = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tap: press and release inside `MAX_TAP_MS`.
    fn tap(l: &mut TapLatch, at: u64) -> (OnPress, OnRelease) {
        (l.press(at), l.release(at + 40))
    }

    #[test]
    fn one_tap_is_an_ordinary_press_and_release() {
        let mut l = TapLatch::default();
        assert_eq!(tap(&mut l, 1_000), (OnPress::Start, OnRelease::Stop));
        assert!(!l.is_latched());
    }

    #[test]
    fn holding_the_key_is_still_just_holding_the_key() {
        let mut l = TapLatch::default();
        assert_eq!(l.press(1_000), OnPress::Start);
        assert_eq!(l.release(9_000), OnRelease::Stop);
        assert!(!l.is_latched(), "a hold must never latch");
    }

    #[test]
    fn two_quick_taps_latch_the_microphone_open() {
        let mut l = TapLatch::default();
        tap(&mut l, 1_000);
        assert_eq!(l.press(1_200), OnPress::LatchOpen);
        assert!(l.is_latched());
        // The key coming up must not close what it just opened.
        assert_eq!(l.release(1_240), OnRelease::Swallow);
        assert!(l.is_latched(), "still open after the key is released");
    }

    #[test]
    fn taps_too_far_apart_are_two_separate_dictations() {
        let mut l = TapLatch::default();
        tap(&mut l, 1_000);
        // 1040 + 351: one millisecond outside the window.
        assert_eq!(l.press(1_040 + DOUBLE_TAP_MS + 1), OnPress::Start);
        assert!(!l.is_latched());
    }

    #[test]
    fn a_tap_landing_exactly_on_the_boundary_still_counts() {
        let mut l = TapLatch::default();
        tap(&mut l, 1_000);
        assert_eq!(l.press(1_040 + DOUBLE_TAP_MS), OnPress::LatchOpen);
    }

    /// The failure this module most needs to avoid.
    ///
    /// Someone taps to dictate a word, then immediately holds to dictate a
    /// sentence. If the hold's *press* is read as the second half of a double,
    /// their microphone stays open after they let go and they do not find out
    /// until later.
    #[test]
    fn a_hold_after_a_tap_does_not_latch() {
        let mut l = TapLatch::default();
        tap(&mut l, 1_000);
        // The press is within the window, so it does latch here -- and that is
        // correct: a double tap is two presses close together, and the second
        // one's length is not yet known when it starts.
        assert_eq!(l.press(1_100), OnPress::LatchOpen);
        assert_eq!(l.release(1_140), OnRelease::Swallow);
        assert!(l.is_latched());

        // What must not happen is the *reverse*: a long hold followed by a tap
        // being read as a double.
        let mut m = TapLatch::default();
        assert_eq!(m.press(1_000), OnPress::Start);
        assert_eq!(m.release(5_000), OnRelease::Stop);
        assert_eq!(
            m.press(5_100),
            OnPress::Start,
            "a hold is not half a double"
        );
        assert!(!m.is_latched());
    }

    #[test]
    fn a_tap_closes_a_latched_microphone() {
        let mut l = TapLatch::default();
        tap(&mut l, 1_000);
        l.press(1_200);
        l.release(1_240);
        assert!(l.is_latched());

        assert_eq!(l.press(9_000), OnPress::StopLatched);
        assert!(!l.is_latched());
        assert_eq!(l.release(9_040), OnRelease::Swallow, "already acted on");
    }

    /// Stopping a latched session must not immediately open another one.
    ///
    /// The tap that closes it is a complete tap, and if it were remembered as
    /// half of a double then the next tap would latch again — so the microphone
    /// could only ever be closed by tapping an odd number of times.
    #[test]
    fn closing_a_latch_does_not_arm_another_one() {
        let mut l = TapLatch::default();
        tap(&mut l, 1_000);
        l.press(1_200);
        l.release(1_240);

        l.press(9_000); // closes it
        l.release(9_040);

        assert_eq!(l.press(9_100), OnPress::Start, "must not re-latch");
        assert!(!l.is_latched());
    }

    #[test]
    fn three_taps_are_a_double_then_a_single() {
        let mut l = TapLatch::default();
        tap(&mut l, 1_000);
        assert_eq!(l.press(1_100), OnPress::LatchOpen);
        l.release(1_140);
        assert_eq!(l.press(1_200), OnPress::StopLatched);
    }

    /// Escape, the bar's own stop control, a fault, or the recording cap.
    #[test]
    fn a_session_ended_elsewhere_leaves_the_latch_believing_nothing() {
        let mut l = TapLatch::default();
        tap(&mut l, 1_000);
        l.press(1_200);
        l.release(1_240);
        assert!(l.is_latched());

        l.forget();
        assert!(!l.is_latched());
        assert_eq!(
            l.press(1_300),
            OnPress::Start,
            "the next tap starts a dictation, it does not stop a ghost"
        );
    }
}
