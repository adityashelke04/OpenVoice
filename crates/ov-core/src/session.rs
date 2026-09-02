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

use crate::config::{ActivationMode, SessionLimits};
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
    /// The user changed how dictation is activated, from the Settings screen.
    ///
    /// Delivered as an input rather than written through a shared cell because the
    /// machine is owned outright by the loop thread. Routing the change down the
    /// same channel as every other transition keeps that ownership intact and
    /// means the new mode is picked up between inputs, never in the middle of one.
    ActivationChanged {
        /// The style to use from the next press onwards.
        mode: ActivationMode,
        /// When it happened.
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
            | Self::ActivationChanged { at, .. }
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
        /// Executable that had focus when the user started speaking. Transcription
        /// and formatting can take several seconds, and injection has no way of
        /// knowing on its own whether focus moved in that window — carrying this
        /// through lets the caller notice a mismatch and log it, rather than
        /// silently sending a paste to whatever now happens to be foreground.
        target_exe: String,
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
    /// Whether the key is held to speak, or pressed once to start and again to
    /// stop.
    activation: ActivationMode,
    /// Whether the trigger key is physically down right now.
    ///
    /// Tracked even in push-to-talk, where the capture state would almost serve
    /// instead, because toggle mode genuinely cannot work without it: Windows
    /// delivers auto-repeat key-down events every ~30 ms while a key is held, and
    /// they are indistinguishable from a real second press by any other means. A
    /// second press is only genuine if a release came between, so this is what
    /// separates "the user pressed again to stop" from "the user is still holding
    /// the key down".
    key_down: bool,
    next_id: u64,
    /// At most one live capture.
    capturing: Option<Active>,
    /// Post-capture sessions, processed strictly in order. The front element is the
    /// one currently in flight.
    pipeline: VecDeque<Active>,
    /// A press that landed while the previous capture was still handing its audio
    /// back. See [`SessionMachine::on_pressed`].
    pending: Option<PendingPress>,
}

/// A genuine press that could not be honoured the instant it arrived.
///
/// Releasing the key clears `released_at` but leaves the capture occupying the
/// slot until the audio adapter returns the buffer, which takes a few tens of
/// milliseconds. A press inside that gap used to be dropped on the floor: no
/// session, no effects, no events — the user spoke a whole utterance into
/// nothing and the bar never moved. Held here instead, and replayed the moment
/// the slot frees.
#[derive(Debug)]
struct PendingPress {
    app: ForegroundApp,
    profile: String,
}

impl SessionMachine {
    /// Create a machine in the idle state, holding the key to speak.
    #[must_use]
    pub fn new(limits: SessionLimits) -> Self {
        Self::with_activation(limits, ActivationMode::PushToTalk)
    }

    /// Create a machine in the idle state with an explicit activation style.
    #[must_use]
    pub fn with_activation(limits: SessionLimits, activation: ActivationMode) -> Self {
        Self {
            limits,
            activation,
            key_down: false,
            next_id: 1,
            capturing: None,
            pipeline: VecDeque::new(),
            pending: None,
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
                // A microphone/device failure, not a transcriber failure -- it never
                // reached the ASR step. Keeping the outcome distinct from
                // `AsrFailed` is what lets history and the UI tell "your mic died"
                // apart from "the model failed".
                self.fail_capture(session, at, Outcome::CaptureFailed(error), &mut fx);
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
                //
                // This notice is the only *proactive* signal the user gets. Without
                // it, the only way to discover an injection failed is to notice a
                // non-"delivered" badge in History later — which defeats the point
                // of a clipboard fallback that exists to be used in the moment.
                fx.push(Effect::Emit(Event::Notice {
                    level: NoticeLevel::Warn,
                    message: "Couldn't type that in — copied to your clipboard. Press Ctrl+V."
                        .into(),
                }));
                // Must be the *formatted* text: that's what was actually handed to
                // the injector and what really ended up on the clipboard (see
                // `on_formatted`, which sets `final_text` before emitting
                // `Effect::Inject`). Recording the raw ASR text here would make
                // history and the clipboard disagree about what the user has.
                let text = self.front_final_text(session);
                self.finish_pipeline(
                    session,
                    at,
                    Outcome::ClipboardFallback(text.clone()),
                    text,
                    &mut fx,
                );
            }
            Input::Tick { at } => self.on_tick(at, &mut fx),
            Input::ActivationChanged { mode, .. } => self.set_activation(mode),
        }
        fx
    }

    /// Adopt a new activation style without disturbing anything in flight.
    ///
    /// `key_down` is deliberately left alone. Both modes maintain it identically
    /// -- `on_pressed` sets it, `on_released` clears it -- so a switch made while
    /// the key happens to be held still sees a matching release. Clearing it here
    /// would be the bug: the release would then look like a stray one, and in
    /// toggle mode the next genuine press would be read as auto-repeat and
    /// silently dropped.
    fn set_activation(&mut self, mode: ActivationMode) {
        self.activation = mode;
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
        // capture roughly every 30 ms — and in toggle mode it would start and stop
        // one that often, which is worse.
        if self.key_down {
            return;
        }
        self.key_down = true;

        // In toggle mode a press while capturing means "stop", which is the whole
        // difference between the two modes.
        if let Some(active) = self.capturing.as_ref() {
            if self.activation == ActivationMode::Toggle {
                self.stop_capture(at, fx);
                return;
            }
            // Push-to-talk. Whether this press is genuine depends on which half
            // of the capture's life it landed in.
            //
            // Still recording — the release has not arrived — and it can only be
            // a stray event, so it is dropped as before. But once `released_at`
            // is set the capture is merely waiting for the audio adapter to hand
            // its buffer back, and a press then is a real second utterance. That
            // case used to take the same early return, which silently discarded
            // it; dictating twice in quick succession lost the second one.
            if active.released_at.is_some() {
                self.pending = Some(PendingPress { app, profile });
            }
            return;
        }

        self.begin_capture(at, app, profile, fx);
    }

    /// Open a new capture. The one place a session is born.
    fn begin_capture(
        &mut self,
        at: Millis,
        app: ForegroundApp,
        profile: String,
        fx: &mut Vec<Effect>,
    ) {
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

    /// Start a press that was held back while the previous capture finished.
    ///
    /// Started at `at`, not at the moment the key went down: the microphone is
    /// only open from here, and dating the session earlier would credit it with
    /// audio that was never recorded.
    ///
    /// Only if the key is still held. A press *and* release that both happened
    /// inside the gap is a tap shorter than the minimum, and this machine
    /// deliberately treats those as nothing at all rather than as a session.
    fn replay_pending(&mut self, at: Millis, fx: &mut Vec<Effect>) {
        let Some(press) = self.pending.take() else {
            return;
        };
        if !self.key_down {
            return;
        }
        self.begin_capture(at, press.app, press.profile, fx);
    }

    fn on_released(&mut self, at: Millis, fx: &mut Vec<Effect>) {
        // Recorded in both modes. In toggle it is what arms the *next* press to be
        // treated as genuine rather than as auto-repeat.
        self.key_down = false;

        if self.activation == ActivationMode::Toggle {
            // Letting go does nothing: the session ends on the next press, or at
            // the maximum-recording cutoff.
            return;
        }
        self.stop_capture(at, fx);
    }

    /// End the live capture, keeping whatever audio it gathered.
    ///
    /// Shared by the key release in push-to-talk, the second press in toggle, and
    /// the maximum-duration cutoff, so all three produce exactly the same effects.
    /// They used to be written out separately, which is how they would drift.
    fn stop_capture(&mut self, at: Millis, fx: &mut Vec<Effect>) {
        let Some(active) = self.capturing.as_mut() else {
            return;
        };
        if active.released_at.is_some() {
            return; // already stopping; ignore a duplicate
        }
        active.released_at = Some(at);
        fx.push(Effect::StopCapture { session: active.id });
    }

    fn on_cancelled(&mut self, at: Millis, fx: &mut Vec<Effect>) {
        // "Stop, all of it" includes the press that has not started yet.
        self.pending = None;
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
            self.replay_pending(at, fx);
            self.emit_idle_if_settled(fx);
            return;
        }

        if rms < self.limits.silence_rms {
            self.persist(&active, Outcome::Silent, String::new(), at, fx);
            fx.push(Effect::Emit(Event::Notice {
                level: NoticeLevel::Warn,
                message: "No speech detected — is your microphone muted?".into(),
            }));
            self.replay_pending(at, fx);
            self.emit_idle_if_settled(fx);
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
        // After the old session's effects, so a listener applying them in order
        // ends on the new session rather than on the old one's progress.
        self.replay_pending(at, fx);
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
            self.replay_pending(at, fx);
            self.emit_idle_if_settled(fx);
        }
    }

    /// Emit `Event::Idle` only if the engine is actually settling: no capture in
    /// progress *and* nothing left in the transcribe/format/inject pipeline.
    ///
    /// The three callers of this (a too-short tap, a silent capture, and a capture
    /// failure) each know their own capture slot is now empty, but that is not the
    /// same as the whole engine being idle: a second session queued behind one
    /// still being transcribed is completely unaffected by the first session's
    /// disposition. Emitting Idle unconditionally there told the UI the engine had
    /// gone idle while a session was still actively in flight.
    /// Idle means nothing is happening — including nothing recording.
    ///
    /// The `capturing` half of that used to be missing, which was harmless only
    /// because no path could reach here with a live capture. Replaying a pending
    /// press is exactly such a path: it starts a new capture while the previous
    /// session is finishing, and without this the machine would announce it was
    /// idle with the microphone open.
    fn emit_idle_if_settled(&self, fx: &mut Vec<Effect>) {
        if self.pipeline.is_empty() && self.capturing.is_none() {
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
        fx.push(Effect::Inject {
            session,
            text,
            target_exe: front.app.exe.clone(),
        });
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
        let Some(active) = self.capturing.as_ref() else {
            return;
        };
        if active.released_at.is_some() {
            return; // already stopping
        }
        if at.since(active.started_at) >= self.limits.max_duration_ms {
            // A stuck or forgotten key must not record indefinitely. Treat it
            // exactly as a release so the audio is kept, not thrown away.
            //
            // This is also the backstop for toggle mode, where there is no release
            // to rely on at all: a session started and never stopped ends here.
            self.stop_capture(at, fx);
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

    fn toggle_machine() -> SessionMachine {
        SessionMachine::with_activation(SessionLimits::default(), ActivationMode::Toggle)
    }

    fn release(m: &mut SessionMachine, t: u64) -> Vec<Effect> {
        m.handle(Input::HotkeyReleased { at: Millis(t) })
    }

    fn stops(fx: &[Effect]) -> usize {
        fx.iter()
            .filter(|e| matches!(e, Effect::StopCapture { .. }))
            .count()
    }

    fn starts(fx: &[Effect]) -> usize {
        fx.iter()
            .filter(|e| matches!(e, Effect::StartCapture { .. }))
            .count()
    }

    #[test]
    fn toggle_starts_on_the_first_press_and_stops_on_the_second() {
        let mut m = toggle_machine();

        let first = press(&mut m, 0);
        assert_eq!(starts(&first), 1, "first press must start capturing");
        assert_eq!(stops(&first), 0);

        // Letting go does nothing at all -- that is the whole point of the mode.
        let up = release(&mut m, 100);
        assert_eq!(stops(&up), 0, "releasing must not stop a toggled session");

        let second = press(&mut m, 3_000);
        assert_eq!(stops(&second), 1, "second press must stop capturing");
        assert_eq!(starts(&second), 0, "and must not start a new one");
    }

    #[test]
    fn toggle_ignores_autorepeat_while_the_key_is_held() {
        // The trap this mode exists to fall into: Windows delivers a key-down
        // every ~30 ms while a key is held. Treating those as real presses would
        // start and stop a session dozens of times a second, and the user would
        // see a hotkey that does nothing at all.
        let mut m = toggle_machine();
        press(&mut m, 0);

        let mut repeats = Vec::new();
        for t in 1..=20 {
            repeats.extend(press(&mut m, t * 30));
        }

        assert_eq!(stops(&repeats), 0, "auto-repeat must not stop the session");
        assert_eq!(starts(&repeats), 0, "nor start another");
    }

    #[test]
    fn toggle_requires_a_release_before_a_press_counts_again() {
        // The rule that makes auto-repeat detectable: a second press is only
        // genuine if a release came between.
        let mut m = toggle_machine();
        press(&mut m, 0);
        release(&mut m, 50);
        let stop = press(&mut m, 100);
        assert_eq!(stops(&stop), 1);

        // And after that stop, a further held-down repeat cannot resurrect it.
        let repeat = press(&mut m, 130);
        assert_eq!(starts(&repeat), 0);
    }

    #[test]
    fn toggle_can_run_a_second_session_after_the_first_completes() {
        let mut m = toggle_machine();
        press(&mut m, 0);
        release(&mut m, 50);
        press(&mut m, 2_000);
        release(&mut m, 2_050);

        m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 2_000,
            rms: 0.1,
            at: Millis(2_060),
        });

        let again = press(&mut m, 5_000);
        assert_eq!(starts(&again), 1, "a new toggled session must be startable");
    }

    #[test]
    fn the_maximum_length_cutoff_still_ends_a_toggled_session() {
        // Toggle mode has no release to fall back on, so this is the only thing
        // standing between a forgotten session and an unbounded recording.
        let limits = SessionLimits::default();
        let mut m = toggle_machine();
        press(&mut m, 0);
        release(&mut m, 50);

        let before = m.handle(Input::Tick {
            at: Millis(limits.max_duration_ms - 1),
        });
        assert_eq!(stops(&before), 0);

        let at_limit = m.handle(Input::Tick {
            at: Millis(limits.max_duration_ms),
        });
        assert_eq!(stops(&at_limit), 1, "the cutoff must end a toggled session");
    }

    #[test]
    fn push_to_talk_is_unchanged_by_the_key_down_tracking() {
        // The regression risk of adding `key_down`: push-to-talk must still stop
        // on release, not wait for a second press.
        let mut m = machine();
        let down = press(&mut m, 0);
        assert_eq!(starts(&down), 1);
        let up = release(&mut m, 1_000);
        assert_eq!(
            stops(&up),
            1,
            "push-to-talk must stop when the key is let go"
        );
    }

    #[test]
    fn a_duplicate_release_is_still_ignored_in_push_to_talk() {
        let mut m = machine();
        press(&mut m, 0);
        release(&mut m, 1_000);
        let second = release(&mut m, 1_010);
        assert_eq!(stops(&second), 0, "a duplicate key-up must not stop twice");
    }

    #[test]
    fn escape_during_a_toggled_session_cancels_it() {
        let mut m = toggle_machine();
        press(&mut m, 0);
        release(&mut m, 50);

        let cancelled = m.handle(Input::Cancelled { at: Millis(1_000) });
        assert!(
            cancelled
                .iter()
                .any(|e| matches!(e, Effect::AbortCapture { .. })),
            "Escape must abort a toggled capture"
        );
        assert!(m.is_idle());

        // And the machine is usable afterwards: the key was never released during
        // the cancel, so the next press must still be able to start a session
        // once the user does let go.
        release(&mut m, 1_100);
        assert_eq!(starts(&press(&mut m, 1_200)), 1);
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

    /// Dictating twice in quick succession used to lose the second utterance.
    ///
    /// Releasing the key marks the capture as stopping, but the slot stays
    /// occupied until the audio adapter hands its buffer back tens of
    /// milliseconds later. A press inside that gap took the same early return as
    /// a stray auto-repeat event and was discarded with no effects at all — the
    /// user spoke into nothing and the Flow Bar never moved.
    #[test]
    fn press_while_previous_capture_is_finishing_is_not_lost() {
        let mut m = machine();
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(2_000) });

        // Pressed again before AudioCaptured for session 1 has come back.
        let held = press(&mut m, 2_010);
        assert!(
            held.is_empty(),
            "nothing can start while the slot is still occupied"
        );

        let fx = m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 2_000,
            rms: 0.2,
            at: Millis(2_030),
        });

        // Session 1 proceeds, and session 2 opens in the same batch.
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Transcribe { session } if *session == SessionId(1))),
            "the first utterance must still be transcribed"
        );
        assert!(
            fx.iter().any(
                |e| matches!(e, Effect::Emit(Event::Listening { session, .. }) if *session == SessionId(2))
            ),
            "the held press must open a second capture"
        );
        // Never idle: the microphone is open.
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::Emit(Event::Idle))),
            "must not announce idle while capturing"
        );
    }

    /// The other half of the same gap: pressed *and* released inside it. That is
    /// a tap far shorter than the minimum, which this machine treats as nothing.
    #[test]
    fn press_and_release_inside_the_gap_starts_nothing() {
        let mut m = machine();
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(2_000) });
        press(&mut m, 2_005);
        m.handle(Input::HotkeyReleased { at: Millis(2_010) });

        let fx = m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 2_000,
            rms: 0.2,
            at: Millis(2_030),
        });

        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::Emit(Event::Listening { .. }))),
            "a tap inside the gap must not open a capture"
        );
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
            transcript: transcript("use effect"),
            at: Millis(1_500),
        });
        // Deliberately different from the raw text, so a test that accidentally
        // reads `raw_text` instead of `final_text` cannot pass by coincidence.
        m.handle(Input::Formatted {
            session: SessionId(1),
            text: "useEffect".into(),
            at: Millis(1_505),
        });
        let fx = m.handle(Input::InjectionFailed {
            session: SessionId(1),
            error: "target refused paste".into(),
            at: Millis(1_600),
        });

        let record = persists(&fx)[0];
        assert!(record.outcome.is_success(), "user still has their words");
        assert!(
            !record.final_text.is_empty(),
            "text must survive to history"
        );
        // The clipboard fallback must carry the *formatted* text: that is what was
        // actually handed to the injector and what really ended up on the
        // clipboard. Regression: this used to read the raw ASR transcript
        // ("use effect"), which would silently disagree with the clipboard.
        assert_eq!(record.final_text, "useEffect");
        match &record.outcome {
            Outcome::ClipboardFallback(text) => assert_eq!(text, "useEffect"),
            other => panic!("expected ClipboardFallback, got {other:?}"),
        }
        // The only proactive signal the user gets that something needs a manual
        // Ctrl+V. Without it, discovering a failed injection means noticing a
        // non-"delivered" badge in History later -- too late to be useful in the
        // moment the fallback exists for.
        assert!(
            fx.iter().any(|e| matches!(
                e,
                Effect::Emit(Event::Notice { message, .. }) if message.contains("Ctrl+V")
            )),
            "a failed injection must tell the user to paste manually, not just log it"
        );
    }

    #[test]
    fn microphone_failure_is_not_reported_as_a_transcriber_failure() {
        let mut m = machine();
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(1_000) });
        let fx = m.handle(Input::AudioFailed {
            session: SessionId(1),
            error: "device disconnected".into(),
            at: Millis(1_010),
        });

        let record = persists(&fx)[0];
        assert!(!record.outcome.is_success());
        assert_eq!(record.outcome.code(), "capture_failed");
        assert!(matches!(&record.outcome, Outcome::CaptureFailed(e) if e == "device disconnected"));
    }

    #[test]
    fn a_disposed_second_session_does_not_announce_idle_while_the_first_is_still_in_flight() {
        // Session 1 is queued behind capture and is being transcribed. Session 2
        // starts and ends as a fat-finger tap (TooShort) while session 1 is still
        // in flight -- the engine as a whole must not report Idle.
        let mut m = machine();
        press(&mut m, 0);
        m.handle(Input::HotkeyReleased { at: Millis(1_000) });
        m.handle(Input::AudioCaptured {
            session: SessionId(1),
            duration_ms: 1_000,
            rms: 0.1,
            at: Millis(1_005),
        });
        assert_eq!(m.queue_depth(), 1, "session 1 is mid-transcription");

        press(&mut m, 1_100);
        m.handle(Input::HotkeyReleased { at: Millis(1_150) });
        let fx = m.handle(Input::AudioCaptured {
            session: SessionId(2),
            duration_ms: 50, // below min_duration_ms: a fat-finger tap
            rms: 0.1,
            at: Millis(1_155),
        });

        assert_eq!(
            persists(&fx)[0].outcome,
            Outcome::TooShort,
            "session 2 is disposed of as a fat-finger tap"
        );
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::Emit(Event::Idle))),
            "session 1 is still being transcribed; the engine is not idle"
        );

        // Same check for a silent second session.
        press(&mut m, 1_200);
        m.handle(Input::HotkeyReleased { at: Millis(2_200) });
        let fx = m.handle(Input::AudioCaptured {
            session: SessionId(3),
            duration_ms: 1_000,
            rms: 0.0001, // below silence_rms
            at: Millis(2_205),
        });
        assert_eq!(persists(&fx)[0].outcome, Outcome::Silent);
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::Emit(Event::Idle))),
            "session 1 is still being transcribed; the engine is not idle"
        );

        // And for an outright capture failure.
        press(&mut m, 2_300);
        m.handle(Input::HotkeyReleased { at: Millis(2_310) });
        let fx = m.handle(Input::AudioFailed {
            session: SessionId(4),
            error: "device disconnected".into(),
            at: Millis(2_315),
        });
        assert_eq!(persists(&fx)[0].outcome.code(), "capture_failed");
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::Emit(Event::Idle))),
            "session 1 is still being transcribed; the engine is not idle"
        );

        // Finishing session 1 is what actually settles the engine.
        m.handle(Input::Transcribed {
            session: SessionId(1),
            transcript: transcript("first"),
            at: Millis(2_400),
        });
        m.handle(Input::Formatted {
            session: SessionId(1),
            text: "First".into(),
            at: Millis(2_405),
        });
        let fx = m.handle(Input::Injected {
            session: SessionId(1),
            at: Millis(2_410),
        });
        assert!(
            fx.iter().any(|e| matches!(e, Effect::Emit(Event::Idle))),
            "now the pipeline really is empty"
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

    /// The Settings screen used to write a new activation style to disk and
    /// nothing more: the machine kept the mode it was constructed with, so
    /// switching to "press to start and stop" still behaved as hold-to-talk until
    /// the app was relaunched.
    #[test]
    fn activation_change_applies_without_rebuilding_the_machine() {
        // Starts in push-to-talk, where a complete tap records nothing at all.
        let mut m = machine();

        let fx = m.handle(Input::ActivationChanged {
            mode: ActivationMode::Toggle,
            at: Millis(0),
        });
        assert!(fx.is_empty(), "a mode change is not itself an event");

        let first = press(&mut m, 10);
        assert_eq!(starts(&first), 1, "first press must start capturing");
        release(&mut m, 20);
        assert_eq!(
            stops(&first),
            0,
            "a release must not stop it in toggle mode"
        );

        let second = press(&mut m, 30);
        assert_eq!(stops(&second), 1, "the second press must stop it");
    }

    /// Switching back must be just as live, and must not leave the machine
    /// half-way between the two readings of a press.
    #[test]
    fn activation_change_back_to_push_to_talk_applies_immediately() {
        let mut m = toggle_machine();

        m.handle(Input::ActivationChanged {
            mode: ActivationMode::PushToTalk,
            at: Millis(0),
        });

        let down = press(&mut m, 10);
        assert_eq!(starts(&down), 1);
        let up = release(&mut m, 500);
        assert_eq!(stops(&up), 1, "push-to-talk must stop on the release");
    }

    /// A mode change landing while the key is physically held must not strand the
    /// machine. `key_down` is shared bookkeeping, so clearing it on a switch would
    /// make the coming release look stray and swallow the next genuine press.
    #[test]
    fn activation_change_mid_hold_still_sees_the_release() {
        let mut m = machine();

        let down = press(&mut m, 0);
        assert_eq!(starts(&down), 1);

        m.handle(Input::ActivationChanged {
            mode: ActivationMode::Toggle,
            at: Millis(100),
        });

        // Toggle mode reads this release as "still holding the first press", so it
        // does not stop -- but it must clear `key_down`, or the next press is
        // mistaken for auto-repeat.
        release(&mut m, 200);
        let next = press(&mut m, 300);
        assert_eq!(stops(&next), 1, "the next press must still be honoured");
    }
}
