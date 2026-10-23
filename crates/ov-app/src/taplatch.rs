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
        self.pressed_at = None;
        self.swallow_next_release = false;
    }
}

use ov_core::{
    config::ActivationMode,
    event::Event,
    ports::HotkeyEvent,
    session::{Effect, Input},
    types::{ForegroundApp, Millis, Outcome, SessionId},
};

/// All activation decisions run on the session loop, in the same order as faults
/// and cutoffs. An old completion cannot clear a newer gesture.
pub(crate) enum Command {
    Input(Input),
    Hotkey {
        event: HotkeyEvent,
        at: Millis,
        app: ForegroundApp,
        profile: String,
    },
    Toggle {
        at: Millis,
        app: ForegroundApp,
        profile: String,
    },
}

pub(crate) struct Controls {
    mode: ActivationMode,
    latch: TapLatch,
    synthetic_down: bool,
    tap_session: Option<SessionId>,
}

impl Controls {
    pub(crate) fn new(mode: ActivationMode) -> Self {
        Self {
            mode,
            latch: TapLatch::default(),
            synthetic_down: false,
            tap_session: None,
        }
    }

    pub(crate) fn is_latched(&self) -> bool {
        self.latch.is_latched()
    }

    pub(crate) fn inputs(&mut self, command: Command) -> Vec<Input> {
        match command {
            Command::Input(input) => {
                if let Input::ActivationChanged { mode, at } = input {
                    self.reset();
                    self.mode = mode;
                    // A rebound key cannot deliver the old key's release. Release
                    // on both sides so either activation mode can finish a hold.
                    vec![
                        Input::HotkeyReleased { at },
                        Input::ActivationChanged { mode, at },
                        Input::HotkeyReleased { at },
                    ]
                } else {
                    if matches!(input, Input::Cancelled { .. }) {
                        self.reset();
                    }
                    vec![input]
                }
            }
            Command::Hotkey {
                event,
                at,
                app,
                profile,
            } => match event {
                HotkeyEvent::Cancelled => {
                    self.reset();
                    vec![Input::Cancelled { at }]
                }
                HotkeyEvent::Pressed if self.mode == ActivationMode::PushToTalk => {
                    match self.latch.press(at.0) {
                        OnPress::Start => {
                            self.tap_session = None;
                            vec![Input::HotkeyPressed { at, app, profile }]
                        }
                        OnPress::LatchOpen => vec![Input::HotkeyLatched {
                            at,
                            discard: self.tap_session.take(),
                            app,
                            profile,
                        }],
                        OnPress::StopLatched => vec![Input::HotkeyReleased { at }],
                    }
                }
                HotkeyEvent::Released if self.mode == ActivationMode::PushToTalk => {
                    if self.latch.release(at.0) == OnRelease::Swallow {
                        vec![]
                    } else {
                        vec![Input::HotkeyReleased { at }]
                    }
                }
                HotkeyEvent::Pressed => vec![Input::HotkeyPressed { at, app, profile }],
                HotkeyEvent::Released => vec![Input::HotkeyReleased { at }],
            },
            Command::Toggle { at, app, profile } => {
                if self.latch.is_latched() {
                    self.reset();
                    return vec![Input::HotkeyReleased { at }];
                }
                if self.mode == ActivationMode::Toggle {
                    return vec![
                        Input::HotkeyPressed { at, app, profile },
                        Input::HotkeyReleased { at },
                    ];
                }
                self.synthetic_down = !self.synthetic_down;
                if self.synthetic_down {
                    vec![Input::HotkeyPressed { at, app, profile }]
                } else {
                    vec![Input::HotkeyReleased { at }]
                }
            }
        }
    }

    /// Return whether a virtual hold must be released in the domain machine too.
    pub(crate) fn observe(&mut self, input: &Input, effects: &[Effect]) -> bool {
        if !self.latch.is_latched() && !self.synthetic_down {
            for effect in effects {
                if let Effect::StartCapture { session } = effect {
                    self.tap_session = Some(*session);
                }
            }
        }
        let ended = effects.iter().any(|effect| {
            matches!(
                effect,
                Effect::StopCapture { .. }
                    | Effect::AbortCapture { .. }
                    | Effect::Emit(Event::Finished {
                        outcome: Outcome::CaptureFailed(_),
                        ..
                    })
            )
        });
        let replacement_started = effects
            .iter()
            .any(|effect| matches!(effect, Effect::StartCapture { .. }));
        if ended
            && !replacement_started
            && !matches!(input, Input::HotkeyLatched { .. })
            && (self.latch.is_latched() || self.synthetic_down)
        {
            self.reset();
            true
        } else {
            false
        }
    }

    fn reset(&mut self) {
        self.latch.forget();
        self.synthetic_down = false;
        self.tap_session = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ov_core::{config::SessionLimits, session::SessionMachine};

    fn physical(event: HotkeyEvent, at: u64) -> Command {
        Command::Hotkey {
            event,
            at: Millis(at),
            app: ForegroundApp::default(),
            profile: "prose".into(),
        }
    }

    fn click(at: u64) -> Command {
        Command::Toggle {
            at: Millis(at),
            app: ForegroundApp::default(),
            profile: "prose".into(),
        }
    }

    fn step(
        controls: &mut Controls,
        machine: &mut SessionMachine,
        command: Command,
    ) -> Vec<Effect> {
        let mut all = Vec::new();
        for input in controls.inputs(command) {
            let mut effects = machine.handle(input.clone());
            if controls.observe(&input, &effects) {
                effects.extend(machine.handle(Input::HotkeyReleased { at: input.at() }));
            }
            all.extend(effects);
        }
        all
    }

    fn latched() -> (Controls, SessionMachine) {
        let mut controls = Controls::new(ActivationMode::PushToTalk);
        let mut machine = SessionMachine::new(SessionLimits::default());
        for command in [
            physical(HotkeyEvent::Pressed, 0),
            physical(HotkeyEvent::Released, 40),
            physical(HotkeyEvent::Pressed, 100),
            physical(HotkeyEvent::Released, 140),
        ] {
            step(&mut controls, &mut machine, command);
        }
        assert!(controls.is_latched());
        (controls, machine)
    }

    #[test]
    fn escape_from_latched_capture_rearms_next_physical_hold() {
        let (mut controls, mut machine) = latched();
        step(
            &mut controls,
            &mut machine,
            physical(HotkeyEvent::Cancelled, 1_000),
        );
        let effects = step(
            &mut controls,
            &mut machine,
            physical(HotkeyEvent::Pressed, 2_000),
        );
        assert!(effects
            .iter()
            .any(|e| matches!(e, Effect::StartCapture { .. })));
        assert!(!controls.is_latched());
        let effects = step(
            &mut controls,
            &mut machine,
            physical(HotkeyEvent::Released, 3_000),
        );
        assert!(effects
            .iter()
            .any(|e| matches!(e, Effect::StopCapture { .. })));
    }

    #[test]
    fn double_tap_keeps_the_previous_transcript_in_the_queue() {
        let mut controls = Controls::new(ActivationMode::PushToTalk);
        let mut machine = SessionMachine::new(SessionLimits::default());
        step(
            &mut controls,
            &mut machine,
            physical(HotkeyEvent::Pressed, 0),
        );
        step(
            &mut controls,
            &mut machine,
            physical(HotkeyEvent::Released, 1_000),
        );
        step(
            &mut controls,
            &mut machine,
            Command::Input(Input::AudioCaptured {
                session: SessionId(1),
                duration_ms: 1_000,
                rms: 0.1,
                at: Millis(1_001),
            }),
        );
        step(
            &mut controls,
            &mut machine,
            physical(HotkeyEvent::Pressed, 1_100),
        );
        step(
            &mut controls,
            &mut machine,
            physical(HotkeyEvent::Released, 1_140),
        );
        let effects = step(
            &mut controls,
            &mut machine,
            physical(HotkeyEvent::Pressed, 1_200),
        );
        assert_eq!(machine.queue_depth(), 1);
        assert!(controls.is_latched());
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::StartCapture {
                session: SessionId(3)
            }
        )));
        assert!(!effects.iter().any(|e| matches!(
            e,
            Effect::Discard {
                session: SessionId(1)
            }
        )));
    }

    #[test]
    fn old_stop_failure_does_not_release_a_new_pending_click_hold() {
        let mut controls = Controls::new(ActivationMode::PushToTalk);
        let mut machine = SessionMachine::new(SessionLimits::default());
        step(
            &mut controls,
            &mut machine,
            physical(HotkeyEvent::Pressed, 0),
        );
        step(
            &mut controls,
            &mut machine,
            physical(HotkeyEvent::Released, 1_000),
        );
        step(&mut controls, &mut machine, click(1_001));
        let effects = step(
            &mut controls,
            &mut machine,
            Command::Input(Input::AudioFailed {
                session: SessionId(1),
                error: "old stop failed".into(),
                at: Millis(1_002),
            }),
        );
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::StartCapture {
                session: SessionId(2)
            }
        )));
        assert!(!effects.iter().any(|e| matches!(
            e,
            Effect::StopCapture {
                session: SessionId(2)
            }
        )));
        assert!(step(&mut controls, &mut machine, click(2_000))
            .iter()
            .any(|e| matches!(
                e,
                Effect::StopCapture {
                    session: SessionId(2)
                }
            )));
    }

    #[test]
    fn cutoff_and_capture_failure_reset_hands_free_and_click_holds() {
        for clicked in [false, true] {
            for failure in [false, true] {
                let (mut controls, mut machine) = if clicked {
                    let mut c = Controls::new(ActivationMode::PushToTalk);
                    let mut m = SessionMachine::new(SessionLimits::default());
                    step(&mut c, &mut m, click(0));
                    (c, m)
                } else {
                    latched()
                };
                let session = machine.capturing_session().unwrap();
                let input = if failure {
                    Input::AudioFailed {
                        session,
                        error: "unplugged".into(),
                        at: Millis(1_000),
                    }
                } else {
                    Input::Tick {
                        at: Millis(130_000),
                    }
                };
                step(&mut controls, &mut machine, Command::Input(input));
                if !failure {
                    step(
                        &mut controls,
                        &mut machine,
                        Command::Input(Input::AudioCaptured {
                            session,
                            duration_ms: 120_000,
                            rms: 0.1,
                            at: Millis(130_001),
                        }),
                    );
                }
                assert!(!controls.is_latched());
                let next = if clicked {
                    click(140_000)
                } else {
                    physical(HotkeyEvent::Pressed, 140_000)
                };
                let effects = step(&mut controls, &mut machine, next);
                assert!(
                    effects
                        .iter()
                        .any(|e| matches!(e, Effect::StartCapture { .. })),
                    "clicked={clicked}, failure={failure}"
                );
            }
        }
    }

    #[test]
    fn ordinary_hold_toggle_and_click_keep_their_stop_behavior() {
        for mode in [ActivationMode::PushToTalk, ActivationMode::Toggle] {
            for clicked in [false, true] {
                let mut controls = Controls::new(mode);
                let mut machine = SessionMachine::with_activation(SessionLimits::default(), mode);
                let command = if clicked {
                    click(0)
                } else {
                    physical(HotkeyEvent::Pressed, 0)
                };
                let effects = step(&mut controls, &mut machine, command);
                assert!(effects
                    .iter()
                    .any(|e| matches!(e, Effect::StartCapture { .. })));
                if mode == ActivationMode::Toggle && !clicked {
                    assert!(!step(
                        &mut controls,
                        &mut machine,
                        physical(HotkeyEvent::Released, 50)
                    )
                    .iter()
                    .any(|e| matches!(e, Effect::StopCapture { .. })));
                }
                let stop = if clicked {
                    click(1_000)
                } else if mode == ActivationMode::Toggle {
                    physical(HotkeyEvent::Pressed, 1_000)
                } else {
                    physical(HotkeyEvent::Released, 1_000)
                };
                assert!(step(&mut controls, &mut machine, stop)
                    .iter()
                    .any(|e| matches!(e, Effect::StopCapture { .. })));
            }
        }
    }

    #[test]
    fn changing_binding_ends_virtual_hold_and_rearms_new_trigger() {
        let (mut controls, mut machine) = latched();
        let effects = step(
            &mut controls,
            &mut machine,
            Command::Input(Input::ActivationChanged {
                mode: ActivationMode::PushToTalk,
                at: Millis(1_000),
            }),
        );
        assert!(effects
            .iter()
            .any(|e| matches!(e, Effect::StopCapture { .. })));
        assert!(!controls.is_latched());
        let session = machine.capturing_session().unwrap();
        step(
            &mut controls,
            &mut machine,
            Command::Input(Input::AudioCaptured {
                session,
                duration_ms: 1_000,
                rms: 0.1,
                at: Millis(1_001),
            }),
        );
        assert!(step(
            &mut controls,
            &mut machine,
            physical(HotkeyEvent::Pressed, 2_000)
        )
        .iter()
        .any(|e| matches!(e, Effect::StartCapture { .. })));
    }

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
