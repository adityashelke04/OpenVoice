//! The session state machine.
//!
//! This is the correctness core of OpenVoice, and it is deliberately a *pure*
//! function of its inputs: `handle(input) -> Vec<Effect>`. It performs no IO, reads
//! no clock, spawns no task, and allocates nothing it does not return. Every timing
//! decision comes from a [`Millis`] stamped by the caller.
//!
//! That purity is what lets the entire lifecycle — including cancellation races,
//! queued sessions, and stuck-key cutoffs — be tested exhaustively in microseconds.
//!
//! # Concurrency model
//!
//! Capture is concurrent; everything after capture is serialized. The user may hold
//! the hotkey again while a previous utterance is still being transcribed, and that
//! second utterance must not be dropped, interleaved, or reordered. So the machine
//! holds at most one live capture plus an ordered queue of post-capture sessions,
//! rather than a single flat state.
//!
//! # Invariants
//!
//! Enforced by the tests at the bottom of this file:
//!
//! 1. Every session that starts produces **exactly one** [`Effect::Persist`].
//! 2. [`Input::Cancelled`] from any state drains the machine back to idle.
//! 3. A session never reaches injection without having been formatted first.
//! 4. Key auto-repeat cannot start a second capture.
//! 5. A stuck key cannot record past `max_duration_ms`.

use crate::config::SessionLimits;
use crate::event::{Event, NoticeLevel};
use crate::types::{ForegroundApp, Millis, Outcome, SessionId, Transcript};
use std::collections::VecDeque;

/// Something that happened, fed into the machine by the composition root.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// The dictation chord went down.
    HotkeyPressed {
        /// When it happened.
        at: Millis,
        /// Foreground application, captured on press rather than release: by the
        /// time text is injected the user may have switched windows, and the
        /// profile must reflect where they were *speaking*.
        app: ForegroundApp,
        /// Formatting profile resolved from `app`.
        profile: String,
    },
    /// The dictation chord came up.
    HotkeyReleased {
        /// When it happened.
        at: Millis,
    },
    /// The user pressed Escape.
    Cancelled {
        /// When it happened.
        at: Millis,
    },
    /// Capture finished and audio is available.
    AudioCaptured {
        /// Session the audio belongs to.
        session: SessionId,
        /// Length of the captured audio.
        duration_ms: u64,
        /// RMS amplitude, used to detect a muted microphone.
        rms: f32,
        /// When capture completed.
        at: Millis,
    },
    /// Capture failed.
    AudioFailed {
        /// Session that failed.
        session: SessionId,
        /// Diagnostic detail.
        error: String,
        /// When it failed.
        at: Millis,
    },
    /// The transcriber returned text.
    Transcribed {
        /// Session transcribed.
        session: SessionId,
        /// Model output.
        transcript: Transcript,
        /// When it completed.
        at: Millis,
    },
    /// The transcriber failed.
    TranscriptionFailed {
        /// Session that failed.
        session: SessionId,
        /// Diagnostic detail.
        error: String,
        /// When it failed.
        at: Millis,
    },
    /// The formatting pipeline produced final text.
    Formatted {
        /// Session formatted.
        session: SessionId,
        /// Text to deliver.
        text: String,
        /// When it completed.
        at: Millis,
    },
    /// Text reached the target application.
    Injected {
        /// Session delivered.
        session: SessionId,
        /// When it completed.
        at: Millis,
    },
    /// Text could not be delivered.
    InjectionFailed {
        /// Session that failed.
        session: SessionId,
        /// Diagnostic detail.
        error: String,
        /// When it failed.
        at: Millis,
    },
    /// Periodic timer, which drives the maximum-duration cutoff.
    Tick {
        /// Current time.
        at: Millis,
    },
}

impl Input {
    /// The timestamp carried by this input.
    #[must_use]
    pub fn at(&self) -> Millis {
        match self {
            Self::HotkeyPressed { at, .. }
            | Self::HotkeyReleased { at }
            | Self::Cancelled { at }
            | Self::AudioCaptured { at, .. }
            | Self::AudioFailed { at, .. }
            | Self::Transcribed { at, .. }
            | Self::TranscriptionFailed { at, .. }
            | Self::Formatted { at, .. }
            | Self::Injected { at, .. }
            | Self::InjectionFailed { at, .. }
            | Self::Tick { at } => *at,
        }
    }
}

/// A completed session, handed to the history store.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRecord {
    /// Session identifier.
    pub id: SessionId,
    /// How the session ended.
    pub outcome: Outcome,
    /// Unformatted model output.
    pub raw_text: String,
    /// Text as delivered.
    pub final_text: String,
    /// Profile applied.
    pub profile: String,
    /// Application that had focus when the user began speaking.
    pub app: ForegroundApp,
    /// Length of captured audio.
    pub audio_ms: u64,
    /// Hotkey release to delivery.
    pub latency_ms: u64,
}

/// A command from the domain to the composition root.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Begin microphone capture.
    StartCapture {
        /// Session starting.
        session: SessionId,
    },
    /// End capture and return the audio.
    StopCapture {
        /// Session ending.
        session: SessionId,
    },
    /// Throw away the current capture without returning it.
    AbortCapture {
        /// Session aborted.
        session: SessionId,
    },
    /// Run the transcriber.
    Transcribe {
        /// Session to transcribe.
        session: SessionId,
    },
    /// Run the formatting pipeline.
    Format {
        /// Session to format.
        session: SessionId,
        /// Raw model output.
        raw: String,
        /// Profile to apply.
        profile: String,
    },
    /// Deliver text to the target application.
    Inject {
        /// Session to deliver.
        session: SessionId,
        /// Text to deliver.
        text: String,
    },
    /// Write the session to history. Emitted exactly once per session.
    Persist {
        /// The completed record.
        record: Box<SessionRecord>,
    },
    /// Publish an event for the UI.
    Emit(Event),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Capturing,
    Transcribing,
    Formatting,
    Injecting,
}

#[derive(Debug, Clone)]
struct Active {
    id: SessionId,
    started_at: Millis,
    released_at: Option<Millis>,
    profile: String,
    app: ForegroundApp,
    audio_ms: u64,
    raw_text: String,
    /// Text after formatting, carried so the delivered outcome can record what
    /// actually reached the user.
    final_text: String,
    phase: Phase,
}

impl Active {
    /// Latency is measured from hotkey *release*, not press: the user does not
    /// perceive the time they spent speaking as waiting.
    fn latency_from_release(&self, now: Millis) -> u64 {
        self.released_at.map_or(0, |r| now.since(r))
    }
}

/// The session state machine.
#[derive(Debug)]
pub struct SessionMachine {
    limits: SessionLimits,
    next_id: u64,
    /// At most one live capture.
    capturing: Option<Active>,
    /// Post-capture sessions, processed strictly in order. The front element is the
    /// one currently in flight.
    pipeline: VecDeque<Active>,
}

impl SessionMachine {
    /// Create a machine in the idle state.
    #[must_use]
    pub fn new(limits: SessionLimits) -> Self {
        Self {
            limits,
            next_id: 1,
            capturing: None,
            pipeline: VecDeque::new(),
        }
    }

    /// True when nothing is capturing and nothing is queued.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.capturing.is_none() && self.pipeline.is_empty()
    }

    /// Number of sessions past capture and not yet finished.
    #[must_use]
    pub fn queue_depth(&self) -> usize {
        self.pipeline.len()
    }

    /// Feed one input and receive the effects to perform.
    ///
    /// Unknown or out-of-order inputs — a `Transcribed` for a session that was
    /// already cancelled, for instance — are ignored rather than panicking. Those
    /// races are normal: cancellation and a completing background task can always
    /// cross, and the machine must survive it.
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        let mut fx = Vec::new();
        match input {
            Input::HotkeyPressed { at, app, profile } => self.on_pressed(at, app, profile, &mut fx),
            Input::HotkeyReleased { at } => self.on_released(at, &mut fx),
            Input::Cancelled { at } => self.on_cancelled(at, &mut fx),
            Input::AudioCaptured {
                session,
                duration_ms,
                rms,
                at,
            } => {
                self.on_audio(session, duration_ms, rms, at, &mut fx);
            }
            Input::AudioFailed { session, error, at } => {
                self.fail_capture(session, at, Outcome::AsrFailed(error), &mut fx);
            }
            Input::Transcribed {
                session,
                transcript,
                at,
            } => {
                self.on_transcribed(session, transcript, at, &mut fx);
            }
            Input::TranscriptionFailed { session, error, at } => {
                self.finish_pipeline(
                    session,
                    at,
                    Outcome::AsrFailed(error),
                    String::new(),
                    &mut fx,
                );
            }
            Input::Formatted { session, text, at } => self.on_formatted(session, text, at, &mut fx),
            Input::Injected { session, at } => {
                let text = self.front_final_text(session);
                self.finish_pipeline(session, at, Outcome::Delivered, text, &mut fx);
            }
            Input::InjectionFailed {
                session,
                error: _,
                at,
            } => {
                // Not a failure from the user's point of view: the text is on the
                // clipboard, so nothing was lost. Say what to do, not what broke.
                let text = self.front_text(session);
                self.finish_pipeline(
                    session,
                    at,
                    Outcome::ClipboardFallback(text.clone()),
                    text,
                    &mut fx,
                );
            }
            Input::Tick { at } => self.on_tick(at, &mut fx),
        }
        fx
    }

    fn on_pressed(
        &mut self,
        at: Millis,
        app: ForegroundApp,
        profile: String,
        fx: &mut Vec<Effect>,
    ) {
        // The Windows low-level keyboard hook delivers auto-repeat key-down events
        // while a key is held. Without this guard a held hotkey would start a new
        // capture roughly every 30 ms.
        if self.capturing.is_some() {
            return;
        }
        let id = SessionId(self.next_id);
        self.next_id += 1;
        self.capturing = Some(Active {
            id,
            started_at: at,
            released_at: None,
            profile: profile.clone(),
            app,
            audio_ms: 0,
            raw_text: String::new(),
            final_text: String::new(),
            phase: Phase::Capturing,
        });
        fx.push(Effect::StartCapture { session: id });
        fx.push(Effect::Emit(Event::Listening {
            session: id,
            profile,
        }));
    }

    fn on_released(&mut self, at: Millis, fx: &mut Vec<Effect>) {
        let Some(active) = self.capturing.as_mut() else {
            return;
        };
        if active.released_at.is_some() {
            return; // already releasing; ignore duplicate key-up
        }
        active.released_at = Some(at);
        fx.push(Effect::StopCapture { session: active.id });
    }

    fn on_cancelled(&mut self, at: Millis, fx: &mut Vec<Effect>) {
        if let Some(active) = self.capturing.take() {
            fx.push(Effect::AbortCapture { session: active.id });
            self.persist(&active, Outcome::Cancelled, String::new(), at, fx);
        }
        // Cancel everything already past capture too. A user hitting Escape means
        // "stop, all of it" — leaving a queued utterance to appear seconds later
        // would be the opposite of what they asked for.
        while let Some(queued) = self.pipeline.pop_front() {
            self.persist(&queued, Outcome::Cancelled, String::new(), at, fx);
        }
        fx.push(Effect::Emit(Event::Idle));
    }

    fn on_audio(
        &mut self,
        session: SessionId,
        duration_ms: u64,
        rms: f32,
        at: Millis,
        fx: &mut Vec<Effect>,
    ) {
        let Some(active) = self.capturing.take_if_id(session) else {
            return; // cancelled while capture was finishing
        };
        let mut active = active;
        active.audio_ms = duration_ms;

        if duration_ms < self.limits.min_duration_ms {
            // A fat-finger tap. Deliberately silent: a toast for every accidental
            // brush of the key would be far more annoying than the tap itself.
            self.persist(&active, Outcome::TooShort, String::new(), at, fx);
            fx.push(Effect::Emit(Event::Idle));
            return;
        }

        if rms < self.limits.silence_rms {
            self.persist(&active, Outcome::Silent, String::new(), at, fx);
            fx.push(Effect::Emit(Event::Notice {
                level: NoticeLevel::Warn,
                message: "No speech detected — is your microphone muted?".into(),
            }));
            fx.push(Effect::Emit(Event::Idle));
            return;
        }

        active.phase = Phase::Transcribing;
        let id = active.id;
        let was_empty = self.pipeline.is_empty();
        self.pipeline.push_back(active);
        if was_empty {
            fx.push(Effect::Transcribe { session: id });
            fx.push(Effect::Emit(Event::Transcribing {
                session: id,
                audio_ms: duration_ms,
            }));
        }
    }

    fn fail_capture(
        &mut self,
        session: SessionId,
        at: Millis,
        outcome: Outcome,
        fx: &mut Vec<Effect>,
    ) {
        if let Some(active) = self.capturing.take_if_id(session) {
            self.persist(&active, outcome, String::new(), at, fx);
            fx.push(Effect::Emit(Event::Idle));
        }
    }

    fn on_transcribed(
        &mut self,
        session: SessionId,
        transcript: Transcript,
        _at: Millis,
        fx: &mut Vec<Effect>,
    ) {
        let Some(front) = self.pipeline.front_mut() else {
            return;
        };
        if front.id != session || front.phase != Phase::Transcribing {
            return;
        }
        front.raw_text = transcript.text.clone();
        front.phase = Phase::Formatting;
        fx.push(Effect::Format {
            session,
            raw: transcript.text,
            profile: front.profile.clone(),
        });
    }

    fn on_formatted(&mut self, session: SessionId, text: String, at: Millis, fx: &mut Vec<Effect>) {
        let Some(front) = self.pipeline.front_mut() else {
            return;
        };
        if front.id != session || front.phase != Phase::Formatting {
            return;
        }
        // Formatting can legitimately empty the text — an utterance that was nothing
        // but filler words, for example. Injecting an empty string would be a no-op
        // that still costs a clipboard round trip, so finish here instead.
        if text.trim().is_empty() {
            self.finish_pipeline(session, at, Outcome::Silent, String::new(), fx);
            return;
        }
        front.phase = Phase::Injecting;
        // Remember what we are about to deliver. Previously the delivered outcome
        // persisted an empty string, so every successful session recorded
        // `final_text: ""` -- which made history useless and actively concealed a
        // real injection bug, because the log could not show that the correct text
        // had been handed to the injector.
        front.final_text = text.clone();
        let chars = text.chars().count();
        fx.push(Effect::Emit(Event::Injecting { session, chars }));
        fx.push(Effect::Inject { session, text });
    }

    fn front_text(&self, session: SessionId) -> String {
        self.pipeline
            .front()
            .filter(|f| f.id == session)
            .map(|f| f.raw_text.clone())
            .unwrap_or_default()
    }

    /// The formatted text of the in-flight session, for recording what was
    /// delivered.
    fn front_final_text(&self, session: SessionId) -> String {
        self.pipeline
            .front()
            .filter(|f| f.id == session)
            .map(|f| f.final_text.clone())
            .unwrap_or_default()
    }

    fn finish_pipeline(
        &mut self,
        session: SessionId,
        at: Millis,
        outcome: Outcome,
        final_text: String,
        fx: &mut Vec<Effect>,
    ) {
        let Some(front) = self.pipeline.front() else {
            return;
        };
        if front.id != session {
            return;
        }
        let done = self.pipeline.pop_front().expect("checked above");
        self.persist(&done, outcome, final_text, at, fx);

        // Serialized hand-off: start the next queued session, if any.
        if let Some(next) = self.pipeline.front() {
            let (id, audio_ms) = (next.id, next.audio_ms);
            fx.push(Effect::Transcribe { session: id });
            fx.push(Effect::Emit(Event::Transcribing {
                session: id,
                audio_ms,
            }));
        } else if self.capturing.is_none() {
            fx.push(Effect::Emit(Event::Idle));
        }
    }

    fn on_tick(&mut self, at: Millis, fx: &mut Vec<Effect>) {
        let Some(active) = self.capturing.as_mut() else {
            return;
        };
        if active.released_at.is_some() {
            return; // already stopping
        }
        if at.since(active.started_at) >= self.limits.max_duration_ms {
            // A stuck or forgotten key must not record indefinitely. Treat it
            // exactly as a release so the audio is kept, not thrown away.
            active.released_at = Some(at);
            fx.push(Effect::StopCapture { session: active.id });
            fx.push(Effect::Emit(Event::Notice {
                level: NoticeLevel::Info,
                message: "Maximum recording length reached".into(),
            }));
        }
    }

    /// Emit the single `Persist` effect and the matching `Finished` event.
    ///
    /// Every terminal path routes through here, which is what makes "exactly one
    /// persist per session" a structural property rather than a rule to remember.
    fn persist(
        &self,
        active: &Active,
        outcome: Outcome,
        final_text: String,
        at: Millis,
        fx: &mut Vec<Effect>,
    ) {
        let latency_ms = active.latency_from_release(at);
        fx.push(Effect::Persist {
            record: Box::new(SessionRecord {
                id: active.id,
                outcome: outcome.clone(),
                raw_text: active.raw_text.clone(),
                final_text: final_text.clone(),
                profile: active.profile.clone(),
                app: active.app.clone(),
                audio_ms: active.audio_ms,
                latency_ms,
            }),
        });
        fx.push(Effect::Emit(Event::Finished {
            session: active.id,
            outcome,
            text: final_text,
            latency_ms,
        }));
    }
}

/// Small helper so `take` and "is it the session I expect" read as one operation.
trait TakeIfId {
    fn take_if_id(&mut self, id: SessionId) -> Option<Active>;
}

impl TakeIfId for Option<Active> {
    fn take_if_id(&mut self, id: SessionId) -> Option<Active> {
        if self.as_ref().is_some_and(|a| a.id == id) {
            self.take()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn machine() -> SessionMachine {
        SessionMachine::new(SessionLimits::default())
    }

    fn app() -> ForegroundApp {
        ForegroundApp {
            exe: "Code.exe".into(),
            title: "main.rs".into(),
        }
    }

    fn press(m: &mut SessionMachine, t: u64) -> Vec<Effect> {
        m.handle(Input::HotkeyPressed {
            at: Millis(t),
            app: app(),
            profile: "editor".into(),
        })
    }

    fn transcript(s: &str) -> Transcript {
        Transcript {
            text: s.into(),
            language: Some("en".into()),
            confidence: Some(-0.2),
        }
    }

    /// Drive one session all the way through to delivery.
    fn full_run(m: &mut SessionMachine) -> Vec<Effect> {
        let mut all = press(m, 0);
        all.extend(m.handle(Input::HotkeyReleased { at: Millis(2_000) }));
        all.extend(m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 2_000,
            rms: 0.1,
            at: Millis(2_010),
        }));
        all.extend(m.handle(Input::Transcribed {
            session: SessionId(1),
            transcript: transcript("hello world"),
            at: Millis(2_500),
        }));
        all.extend(m.handle(Input::Formatted {
            session: SessionId(1),
            text: "Hello world".into(),
            at: Millis(2_505),
        }));
        all.extend(m.handle(Input::Injected {
            session: SessionId(1),
            at: Millis(2_600),
        }));
        all
    }

    fn persists(fx: &[Effect]) -> Vec<&SessionRecord> {
        fx.iter()
            .filter_map(|e| match e {
                Effect::Persist { record } => Some(&**record),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn happy_path_delivers_and_returns_to_idle() {
        let mut m = machine();
        let fx = full_run(&mut m);

        let records = persists(&fx);
        assert_eq!(records.len(), 1, "exactly one persist per session");
        assert_eq!(records[0].outcome, Outcome::Delivered);
        // Regression: a delivered session used to persist an empty `final_text`,
        // which made history worthless and hid an injection bug for hours -- the
        // log could not show that the correct string had reached the injector.
        assert_eq!(
            records[0].final_text, "Hello world",
            "a delivered session must record what was delivered"
        );
        assert_eq!(records[0].profile, "editor");
        assert_eq!(records[0].raw_text, "hello world");
        assert_eq!(records[0].audio_ms, 2_000);
        // Latency is measured from release (2000), not press (0).
        assert_eq!(records[0].latency_ms, 600);
        assert!(m.is_idle());
        assert!(matches!(fx.last(), Some(Effect::Emit(Event::Idle))));
    }

    #[test]
    fn key_autorepeat_does_not_start_a_second_capture() {
        let mut m = machine();
        let first = press(&mut m, 0);
        assert_eq!(first.len(), 2, "StartCapture + Listening");

        // The low-level hook fires key-down repeatedly while the key is held.
        for t in [30, 60, 90, 120] {
            assert!(
                press(&mut m, t).is_empty(),
                "repeat at {t}ms must be ignored"
            );
        }
        assert_eq!(m.queue_depth(), 0);
    }

    #[test]
    fn tap_shorter_than_minimum_is_discarded_silently() {
        let mut m = machine();
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(100) });
        let fx = m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 100,
            rms: 0.2,
            at: Millis(105),
        });

        assert_eq!(persists(&fx)[0].outcome, Outcome::TooShort);
        // No toast: accidental taps must not nag.
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::Emit(Event::Notice { .. }))),
            "a fat-finger tap must not produce a notice"
        );
        assert!(m.is_idle());
    }

    #[test]
    fn muted_microphone_is_reported_not_transcribed() {
        let mut m = machine();
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(3_000) });
        let fx = m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 3_000,
            rms: 0.0001,
            at: Millis(3_005),
        });

        assert_eq!(persists(&fx)[0].outcome, Outcome::Silent);
        assert!(!fx.iter().any(|e| matches!(e, Effect::Transcribe { .. })));
        assert!(fx
            .iter()
            .any(|e| matches!(e, Effect::Emit(Event::Notice { .. }))));
    }

    #[test]
    fn stuck_key_stops_capture_at_the_limit() {
        let mut m = machine();
        let limits = SessionLimits::default();
        press(&mut m, 0);

        assert!(m
            .handle(Input::Tick {
                at: Millis(limits.max_duration_ms - 1)
            })
            .is_empty());

        let fx = m.handle(Input::Tick {
            at: Millis(limits.max_duration_ms),
        });
        assert!(fx.iter().any(|e| matches!(e, Effect::StopCapture { .. })));

        // Cutoff must keep the audio, not discard it.
        assert!(!fx.iter().any(|e| matches!(e, Effect::AbortCapture { .. })));
    }

    #[test]
    fn second_press_while_transcribing_queues_rather_than_dropping() {
        let mut m = machine();
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(1_000) });
        m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 1_000,
            rms: 0.1,
            at: Millis(1_005),
        });

        // Session 1 is mid-transcription; the user starts speaking again.
        let fx = press(&mut m, 1_100);
        assert!(fx.iter().any(|e| matches!(e, Effect::StartCapture { .. })));
        m.handle(Input::HotkeyReleased { at: Millis(2_100) });
        let fx = m.handle(Input::AudioCaptured {
            session: SessionId(2),
            duration_ms: 1_000,
            rms: 0.1,
            at: Millis(2_105),
        });

        // Must NOT start a second transcription: post-capture work is serialized.
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::Transcribe { .. })),
            "transcription must be serialized"
        );
        assert_eq!(m.queue_depth(), 2);

        // Finishing session 1 releases session 2, in order.
        m.handle(Input::Transcribed {
            session: SessionId(1),
            transcript: transcript("first"),
            at: Millis(2_200),
        });
        m.handle(Input::Formatted {
            session: SessionId(1),
            text: "First".into(),
            at: Millis(2_205),
        });
        let fx = m.handle(Input::Injected {
            session: SessionId(1),
            at: Millis(2_300),
        });
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Transcribe { session } if *session == SessionId(2))),
            "session 2 must start once session 1 finishes"
        );
    }

    #[test]
    fn injection_failure_is_a_degraded_success_not_a_loss() {
        let mut m = machine();
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(1_000) });
        m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 1_000,
            rms: 0.1,
            at: Millis(1_005),
        });
        m.handle(Input::Transcribed {
            session: SessionId(1),
            transcript: transcript("some text"),
            at: Millis(1_500),
        });
        m.handle(Input::Formatted {
            session: SessionId(1),
            text: "Some text".into(),
            at: Millis(1_505),
        });
        let fx = m.handle(Input::InjectionFailed {
            session: SessionId(1),
            error: "target refused paste".into(),
            at: Millis(1_600),
        });

        let record = persists(&fx)[0];
        assert!(record.outcome.is_success(), "user still has their words");
        assert!(matches!(record.outcome, Outcome::ClipboardFallback(_)));
        assert!(
            !record.final_text.is_empty(),
            "text must survive to history"
        );
    }

    #[test]
    fn empty_formatter_output_skips_injection() {
        let mut m = machine();
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(1_000) });
        m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 1_000,
            rms: 0.1,
            at: Millis(1_005),
        });
        m.handle(Input::Transcribed {
            session: SessionId(1),
            transcript: transcript("um, uh"),
            at: Millis(1_400),
        });
        // Every word was filler; the formatter emptied it.
        let fx = m.handle(Input::Formatted {
            session: SessionId(1),
            text: "   ".into(),
            at: Millis(1_405),
        });

        assert!(!fx.iter().any(|e| matches!(e, Effect::Inject { .. })));
        assert_eq!(persists(&fx)[0].outcome, Outcome::Silent);
        assert!(m.is_idle());
    }

    #[test]
    fn late_events_for_cancelled_sessions_are_ignored() {
        let mut m = machine();
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(1_000) });
        m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 1_000,
            rms: 0.1,
            at: Millis(1_005),
        });
        m.handle(Input::Cancelled { at: Millis(1_100) });
        assert!(m.is_idle());

        // The transcriber was already running and finishes after the cancel. This
        // race is normal and must not panic or resurrect the session.
        let fx = m.handle(Input::Transcribed {
            session: SessionId(1),
            transcript: transcript("ignored"),
            at: Millis(1_500),
        });
        assert!(fx.is_empty());
        assert!(m.is_idle());
    }

    #[test]
    fn cancel_drains_capture_and_queue_together() {
        let mut m = machine();
        // One session queued behind, one capturing.
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(1_000) });
        m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 1_000,
            rms: 0.1,
            at: Millis(1_005),
        });
        press(&mut m, 1_100);

        let fx = m.handle(Input::Cancelled { at: Millis(1_200) });
        assert!(m.is_idle(), "cancel must drain everything");
        assert_eq!(persists(&fx).len(), 2, "both sessions accounted for");
        assert!(persists(&fx)
            .iter()
            .all(|r| r.outcome == Outcome::Cancelled));
    }

    /// Invariant 2, exhaustively: cancelling after *any* prefix of a normal run
    /// must always land back at idle with every started session accounted for.
    #[test]
    fn cancel_from_every_state_returns_to_idle() {
        /// One step of a normal run, so the cancel point can be swept across all
        /// of them.
        type Step = Box<dyn Fn(&mut SessionMachine)>;

        let steps: Vec<Step> = vec![
            Box::new(|m| {
                press(m, 0);
            }),
            Box::new(|m| {
                m.handle(Input::HotkeyReleased { at: Millis(1_000) });
            }),
            Box::new(|m| {
                m.handle(Input::AudioCaptured {
                    session: SessionId(1),
                    duration_ms: 1_000,
                    rms: 0.1,
                    at: Millis(1_005),
                });
            }),
            Box::new(|m| {
                m.handle(Input::Transcribed {
                    session: SessionId(1),
                    transcript: transcript("text"),
                    at: Millis(1_500),
                });
            }),
            Box::new(|m| {
                m.handle(Input::Formatted {
                    session: SessionId(1),
                    text: "Text".into(),
                    at: Millis(1_505),
                });
            }),
        ];

        for cut in 0..=steps.len() {
            let mut m = machine();
            let mut fx = Vec::new();
            for step in steps.iter().take(cut) {
                step(&mut m);
            }
            fx.extend(m.handle(Input::Cancelled { at: Millis(9_999) }));

            assert!(m.is_idle(), "not idle after cancelling at step {cut}");
            // Step 0 started nothing, so there is nothing to persist.
            let expected = usize::from(cut > 0);
            assert_eq!(
                persists(&fx).len(),
                expected,
                "wrong persist count cancelling at step {cut}"
            );
        }
    }

    #[test]
    fn injection_never_precedes_formatting() {
        let mut m = machine();
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(1_000) });
        m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 1_000,
            rms: 0.1,
            at: Millis(1_005),
        });

        // A stray `Formatted` while still transcribing must not skip the model.
        let fx = m.handle(Input::Formatted {
            session: SessionId(1),
            text: "sneaky".into(),
            at: Millis(1_100),
        });
        assert!(fx.is_empty(), "out-of-phase input must be ignored");
    }
}
