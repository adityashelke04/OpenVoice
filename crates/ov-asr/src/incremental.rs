//! Decoding a dictation while it is still being spoken.
//!
//! [`crate::segment::SegmentPlanner`] says which parts of the audio can be decoded
//! early. This runs those decodes on one long-lived thread, in the order that gets
//! text to the caret soonest, and on release joins them into one transcript.
//!
//! # Why one thread
//!
//! Decoding is already multi-threaded inside sherpa-onnx, at the thread count
//! measured for this machine. Two decodes at once would each be slower and would
//! take cores from whatever the user is dictating into. So work is serialised,
//! and ordering carries the priority: the release's tail first, then committed
//! segments, then speculative checkpoints, then priming.
//!
//! # Never the reason text is lost
//!
//! Every path that is not the fast path ends in one whole-utterance decode, the
//! way the app decoded before this existed: an adapter that did not stream, a
//! stream that disagrees with the final recording, a segment that failed.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ov_core::error::{Error, Result};
use ov_core::latency::DecodeFacts;
use ov_core::ports::{DecodeHint, Pcm16k, Transcriber};
use ov_core::types::{SessionId, Transcript};

use crate::segment::{Decision, SegmentPlanner, SegmentPolicy};

/// A dictation that begins after this long without any decode primes the model.
///
/// Windows trims an idle process's working set. After a few minutes away, the
/// first decode paid to page 650 MB of weights back in, and measured decode time
/// per second of audio doubled at p90.
pub const PRIME_AFTER: Duration = Duration::from_secs(30);

/// Priming decodes half a second of silence: enough to touch every layer, cheap
/// enough to finish before the user has said a word.
pub const PRIME_SAMPLES: usize = 8_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Prime,
    Checkpoint,
    Commit,
    Tail,
}

struct Job {
    session: SessionId,
    /// Which slot the result belongs in. Slot ids start at 1, so a priming job's
    /// 0 never matches one.
    slot: u64,
    kind: Kind,
    samples: Vec<f32>,
    hint: DecodeHint,
}

enum Decoded {
    Pending,
    Done(std::result::Result<Transcript, String>),
}

/// One stretch of a session's audio and what decoding it produced.
struct Slot {
    id: u64,
    start: usize,
    end: usize,
    decoded: Decoded,
}

struct Session {
    planner: SegmentPlanner,
    samples: Vec<f32>,
    hint: DecodeHint,
    /// In order. Every one of these is part of the transcript.
    committed: Vec<Slot>,
    /// The current speculative decode, if any.
    checkpoint: Option<Slot>,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    sessions: HashMap<SessionId, Session>,
    next_slot: u64,
    last_decode: Option<Instant>,
    shutdown: bool,
    /// The decode thread is gone and nothing will ever signal `settled` again.
    ///
    /// Only an unwinding decode gets here: `transcribe` is sherpa-onnx across an
    /// FFI boundary, and a model that unwinds took the one thread that answers
    /// every dictation with it. `finish` waits on `settled`, so without this the
    /// wait had no way to learn that its notify was never coming -- that dictation
    /// and every dictation after it blocked the engine's worker forever, with
    /// Escape the only way out. Now the wait breaks and the release falls back to
    /// one whole-utterance decode on the caller's own thread, which is exactly
    /// what the module promises for every path that is not the fast path.
    worker_gone: bool,
}

struct Shared<T> {
    transcriber: Arc<T>,
    policy: SegmentPolicy,
    state: Mutex<State>,
    /// Signalled when a job is queued.
    work: Condvar,
    /// Signalled when a job finishes or a session goes away.
    settled: Condvar,
}

/// Decodes dictations while they are spoken. One per engine.
pub struct IncrementalDecoder<T: Transcriber + 'static> {
    shared: Arc<Shared<T>>,
    worker: Option<JoinHandle<()>>,
}

fn forget_job(queue: &mut VecDeque<Job>, session: SessionId, slot: u64) {
    queue.retain(|j| !(j.session == session && j.slot == slot));
}

impl<T: Transcriber + 'static> IncrementalDecoder<T> {
    /// Start the decode thread.
    pub fn new(transcriber: Arc<T>, policy: SegmentPolicy) -> Result<Self> {
        let shared = Arc::new(Shared {
            transcriber,
            policy,
            state: Mutex::new(State::default()),
            work: Condvar::new(),
            settled: Condvar::new(),
        });
        let worker = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("ov-decode".into())
                .spawn(move || run(&shared))
                .map_err(|e| Error::Transcription(format!("decode thread: {e}")))?
        };
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    /// The model underneath, for its id and for warming at startup.
    pub fn transcriber(&self) -> &Arc<T> {
        &self.shared.transcriber
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared.state.lock().expect("decoder state")
    }

    /// A dictation started. Returns whether the model is being primed.
    pub fn begin(&self, session: SessionId, hint: DecodeHint) -> bool {
        self.begin_at(session, hint, Instant::now())
    }

    /// [`IncrementalDecoder::begin`] with the time supplied, so priming is testable
    /// without waiting thirty seconds.
    pub fn begin_at(&self, session: SessionId, hint: DecodeHint, now: Instant) -> bool {
        let mut st = self.lock();
        let idle = st
            .last_decode
            .is_none_or(|t| now.saturating_duration_since(t) >= PRIME_AFTER);
        let primed = idle && !st.queue.iter().any(|j| j.kind == Kind::Prime);
        if primed {
            st.queue.push_back(Job {
                session,
                slot: 0,
                kind: Kind::Prime,
                samples: vec![0.0; PRIME_SAMPLES],
                hint: DecodeHint::default(),
            });
            // Counted now, so a burst of dictations primes once.
            st.last_decode = Some(now);
        }
        st.sessions.insert(
            session,
            Session {
                planner: SegmentPlanner::new(self.shared.policy),
                samples: Vec::with_capacity(16_000 * 60),
                hint,
                committed: Vec::new(),
                checkpoint: None,
            },
        );
        drop(st);
        if primed {
            self.shared.work.notify_one();
        }
        primed
    }

    /// The next 16 kHz samples of `session`, as captured.
    pub fn push(&self, session: SessionId, chunk: &[f32]) {
        let mut st = self.lock();
        let State {
            sessions,
            queue,
            next_slot,
            ..
        } = &mut *st;
        let Some(s) = sessions.get_mut(&session) else {
            return;
        };
        s.samples.extend_from_slice(chunk);
        let mut queued = false;
        for decision in s.planner.push(chunk) {
            queued = true;
            match decision {
                Decision::Checkpoint { start, end } => {
                    // A checkpoint nobody has started is worthless once a newer one exists.
                    if let Some(old) = s.checkpoint.take() {
                        forget_job(queue, session, old.id);
                    }
                    *next_slot += 1;
                    let id = *next_slot;
                    queue.push_back(Job {
                        session,
                        slot: id,
                        kind: Kind::Checkpoint,
                        samples: s.samples[start..end].to_vec(),
                        hint: s.hint.clone(),
                    });
                    s.checkpoint = Some(Slot {
                        id,
                        start,
                        end,
                        decoded: Decoded::Pending,
                    });
                }
                Decision::Commit { start, end } => match s.checkpoint.take() {
                    // The planner commits at the checkpoint it just made, so its decode
                    // (queued, running, or finished) is this segment's decode.
                    Some(cp) if cp.start == start && cp.end == end => s.committed.push(cp),
                    other => {
                        if let Some(old) = other {
                            forget_job(queue, session, old.id);
                        }
                        *next_slot += 1;
                        let id = *next_slot;
                        // Ahead of every speculative decode: a commit is certain to be
                        // needed, a checkpoint only might be.
                        let at = queue
                            .iter()
                            .position(|j| j.kind == Kind::Checkpoint || j.kind == Kind::Prime)
                            .unwrap_or(queue.len());
                        queue.insert(
                            at,
                            Job {
                                session,
                                slot: id,
                                kind: Kind::Commit,
                                samples: s.samples[start..end].to_vec(),
                                hint: s.hint.clone(),
                            },
                        );
                        s.committed.push(Slot {
                            id,
                            start,
                            end,
                            decoded: Decoded::Pending,
                        });
                    }
                },
            }
        }
        drop(st);
        if queued {
            self.shared.work.notify_one();
        }
    }

    /// The key came up and `audio` is the whole recording. Wait for the transcript.
    pub fn finish(&self, session: SessionId, audio: &Pcm16k) -> Result<(Transcript, DecodeFacts)> {
        let mut st = self.lock();
        let streamed = st
            .sessions
            .get(&session)
            .is_some_and(|s| !s.samples.is_empty() && s.samples.len() == audio.samples.len());
        if !streamed {
            // Whatever it had queued is for audio the fallback is about to decode
            // whole; running it as well would only delay that decode.
            let hint = st
                .sessions
                .remove(&session)
                .map(|s| s.hint)
                .unwrap_or_default();
            st.queue
                .retain(|j| j.session != session || j.kind == Kind::Prime);
            drop(st);
            return self.whole(audio, &hint);
        }

        let reused = self.schedule_tail(&mut st, session);
        let mut st = self
            .shared
            .settled
            .wait_while(st, |st| {
                !st.worker_gone
                    && st.sessions.get(&session).is_some_and(|s| {
                        s.committed
                            .iter()
                            .any(|slot| matches!(slot.decoded, Decoded::Pending))
                    })
            })
            .expect("decoder state");
        let Some(s) = st.sessions.remove(&session) else {
            return Err(Error::Transcription(
                "the dictation was cancelled while it was being decoded".into(),
            ));
        };
        drop(st);

        let mut texts = Vec::with_capacity(s.committed.len());
        let mut language = None;
        for slot in &s.committed {
            match &slot.decoded {
                Decoded::Done(Ok(t)) => {
                    if language.is_none() {
                        language.clone_from(&t.language);
                    }
                    let text = t.text.trim();
                    if !text.is_empty() {
                        texts.push(text.to_owned());
                    }
                }
                Decoded::Done(Err(e)) => {
                    tracing::warn!(%session, error = %e, "a segment failed; decoding the whole dictation instead");
                    return self.whole(audio, &s.hint);
                }
                Decoded::Pending => return self.whole(audio, &s.hint),
            }
        }
        Ok((
            Transcript {
                text: texts.join(" "),
                language,
                confidence: None,
            },
            DecodeFacts {
                reused,
                segments: s.committed.len() as u32,
                fallback: false,
            },
        ))
    }

    /// Decide what the release still needs. Returns whether a checkpoint was reused.
    fn schedule_tail(&self, st: &mut State, session: SessionId) -> bool {
        let State {
            sessions,
            queue,
            next_slot,
            ..
        } = st;
        let Some(s) = sessions.get_mut(&session) else {
            return false;
        };
        let tail = s.planner.finish(s.samples.len());
        // Checked before it is taken, not after: `take().filter(..)` dropped the
        // checkpoint whenever the filter rejected it, so the `take` below saw None
        // and its queued decode was never forgotten -- a speculative decode still
        // burning a core after the transcript had already been delivered.
        if tail.reuse && s.checkpoint.as_ref().is_some_and(|cp| cp.end == tail.end) {
            let cp = s.checkpoint.take().expect("just checked");
            s.committed.push(cp);
            return true;
        }
        if let Some(old) = s.checkpoint.take() {
            forget_job(queue, session, old.id);
        }
        if !tail.silent {
            *next_slot += 1;
            let id = *next_slot;
            // The release outranks everything, and priming a model that is about
            // to decode anyway is pointless.
            queue.retain(|j| j.kind != Kind::Prime);
            queue.push_front(Job {
                session,
                slot: id,
                kind: Kind::Tail,
                samples: s.samples[tail.start..tail.end].to_vec(),
                hint: s.hint.clone(),
            });
            s.committed.push(Slot {
                id,
                start: tail.start,
                end: tail.end,
                decoded: Decoded::Pending,
            });
            self.shared.work.notify_one();
        }
        false
    }

    /// Forget a session: its audio, its queued decodes, and anyone waiting on it.
    pub fn discard(&self, session: SessionId) {
        let mut st = self.lock();
        st.sessions.remove(&session);
        st.queue
            .retain(|j| j.session != session || j.kind == Kind::Prime);
        drop(st);
        self.shared.settled.notify_all();
    }

    /// Decode the whole recording in one call, with the dictation's own hints:
    /// exactly what the app did before incremental decoding existed.
    fn whole(&self, audio: &Pcm16k, hint: &DecodeHint) -> Result<(Transcript, DecodeFacts)> {
        let transcript = self.shared.transcriber.transcribe(audio, hint)?;
        self.lock().last_decode = Some(Instant::now());
        Ok((
            transcript,
            DecodeFacts {
                reused: false,
                segments: 1,
                fallback: true,
            },
        ))
    }
}

impl<T: Transcriber + 'static> Drop for IncrementalDecoder<T> {
    fn drop(&mut self) {
        self.lock().shutdown = true;
        self.shared.work.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Marks the decode thread gone and wakes everyone waiting on it, however `run`
/// ends -- returning on shutdown, or unwinding out of a decode. The lock is taken
/// through `into_inner` because the panic that brought us here may have poisoned
/// it on the way past.
struct WorkerExit<'a, T>(&'a Shared<T>);

impl<T> Drop for WorkerExit<'_, T> {
    fn drop(&mut self) {
        let mut st = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        st.worker_gone = true;
        drop(st);
        self.0.settled.notify_all();
    }
}

fn run<T: Transcriber>(shared: &Shared<T>) {
    let _exit = WorkerExit(shared);
    loop {
        let job = {
            let guard = shared.state.lock().expect("decoder state");
            let mut st = shared
                .work
                .wait_while(guard, |st| st.queue.is_empty() && !st.shutdown)
                .expect("decoder state");
            if st.shutdown {
                return;
            }
            let Some(job) = st.queue.pop_front() else {
                continue;
            };
            // Stamped at the start as well as the end, so a long decode in progress
            // reads as "recently used" and does not trigger priming.
            st.last_decode = Some(Instant::now());
            job
        };
        let Job {
            session,
            slot,
            samples,
            hint,
            ..
        } = job;
        let decoded = shared
            .transcriber
            .transcribe(&Pcm16k { samples }, &hint)
            .map_err(|e| e.to_string());

        let mut st = shared.state.lock().expect("decoder state");
        st.last_decode = Some(Instant::now());
        if let Some(s) = st.sessions.get_mut(&session) {
            if let Some(target) = s
                .committed
                .iter_mut()
                .chain(s.checkpoint.iter_mut())
                .find(|target| target.id == slot)
            {
                target.decoded = Decoded::Done(decoded);
            }
        }
        drop(st);
        shared.settled.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::FRAME;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A transcriber whose "text" is the number of samples it was given, so a test
    /// can see exactly which slices were decoded and how they were joined.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<usize>>,
        /// The vocabulary each decode was given, in call order.
        hints: Mutex<Vec<Vec<String>>>,
        fail_next: AtomicBool,
        /// Panic inside the next decode, as a model that segfaults its way into an
        /// unwind would, to prove the release still answers.
        panic_next: AtomicBool,
        blocked: Mutex<bool>,
        unblocked: Condvar,
    }

    impl Fake {
        fn block(&self) {
            *self.blocked.lock().unwrap() = true;
        }
        fn panic_next(&self) {
            self.panic_next.store(true, Ordering::SeqCst);
        }
        fn unblock(&self) {
            *self.blocked.lock().unwrap() = false;
            self.unblocked.notify_all();
        }
        fn all_calls(&self) -> Vec<usize> {
            self.calls.lock().unwrap().clone()
        }
        /// Decodes of real audio, leaving out priming.
        fn decodes(&self) -> Vec<usize> {
            self.all_calls()
                .into_iter()
                .filter(|&n| n != PRIME_SAMPLES)
                .collect()
        }
    }

    impl Transcriber for Fake {
        fn warm(&self) -> Result<()> {
            Ok(())
        }
        fn transcribe(&self, audio: &Pcm16k, hint: &DecodeHint) -> Result<Transcript> {
            drop(
                self.unblocked
                    .wait_while(self.blocked.lock().unwrap(), |b| *b)
                    .unwrap(),
            );
            if self.panic_next.swap(false, Ordering::SeqCst) {
                panic!("decode panicked");
            }
            self.calls.lock().unwrap().push(audio.samples.len());
            self.hints.lock().unwrap().push(hint.vocabulary.clone());
            if self.fail_next.swap(false, Ordering::SeqCst) {
                return Err(Error::Transcription("boom".into()));
            }
            Ok(Transcript {
                text: format!("<{}>", audio.samples.len()),
                language: Some("en".into()),
                confidence: None,
            })
        }
        fn model_id(&self) -> String {
            "fake".into()
        }
    }

    const S: SessionId = SessionId(1);

    /// The thresholds the expected positions were computed with (see segment.rs).
    fn policy() -> SegmentPolicy {
        SegmentPolicy {
            floor_rms: 0.004,
            noise_ratio: 2.5,
            checkpoint_pause_ms: 240,
            commit_pause_ms: 480,
            min_segment_ms: 3_000,
            max_segment_ms: 20_000,
        }
    }

    fn decoder() -> (Arc<Fake>, IncrementalDecoder<Fake>) {
        let fake = Arc::new(Fake::default());
        let d = IncrementalDecoder::new(Arc::clone(&fake), policy()).expect("decoder");
        (fake, d)
    }

    /// Speech-like: 0.1 RMS with a 20 ms dip in the middle of every half second.
    fn speech(ms: u64) -> Vec<f32> {
        (0..(ms / 20) as usize)
            .flat_map(|f| {
                let a = if f % 25 == 12 { 0.002 } else { 0.1 };
                (0..FRAME).map(move |i| if i % 2 == 0 { a } else { -a })
            })
            .collect()
    }

    fn quiet(ms: u64) -> Vec<f32> {
        (0..(ms / 20) as usize * FRAME)
            .map(|i| if i % 2 == 0 { 0.001 } else { -0.001 })
            .collect()
    }

    /// Begin a session and feed it in 40 ms chunks, as `ov-audio` does.
    fn stream(d: &IncrementalDecoder<Fake>, session: SessionId, audio: &[f32]) {
        d.begin(session, DecodeHint::default());
        for chunk in audio.chunks(640) {
            d.push(session, chunk);
        }
    }

    fn pcm(audio: &[f32]) -> Pcm16k {
        Pcm16k {
            samples: audio.to_vec(),
        }
    }

    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_pause_before_release_leaves_nothing_to_decode_after_it() {
        let (fake, d) = decoder();
        let audio = [speech(1_000), quiet(300)].concat();
        stream(&d, S, &audio);
        let (t, facts) = d.finish(S, &pcm(&audio)).expect("finish");
        assert_eq!(t.text, "<19840>");
        assert_eq!(
            facts,
            DecodeFacts {
                reused: true,
                segments: 1,
                fallback: false
            }
        );
        assert_eq!(
            fake.decodes(),
            vec![19_840],
            "the release must not start a decode of its own"
        );
    }

    #[test]
    fn speech_after_the_pause_is_decoded_at_release() {
        let (fake, d) = decoder();
        let audio = [speech(1_000), quiet(300), speech(500)].concat();
        stream(&d, S, &audio);
        let (t, facts) = d.finish(S, &pcm(&audio)).expect("finish");
        assert_eq!(t.text, "<28800>");
        assert!(!facts.reused);
        assert_eq!(fake.decodes().last(), Some(&28_800));
    }

    #[test]
    fn a_long_dictation_is_decoded_in_pieces_and_joined_in_order() {
        let (fake, d) = decoder();
        let audio = [
            speech(4_000),
            quiet(600),
            speech(4_000),
            quiet(600),
            speech(2_000),
        ]
        .concat();
        stream(&d, S, &audio);
        let (t, facts) = d.finish(S, &pcm(&audio)).expect("finish");
        assert_eq!(t.text, "<67840> <73600> <37760>");
        assert_eq!(facts.segments, 3);
        assert!(!facts.reused && !facts.fallback);
        assert!(
            fake.decodes().contains(&37_760),
            "only the tail was left at release"
        );
    }

    #[test]
    fn a_newer_speculative_decode_replaces_one_that_never_started() {
        let (fake, d) = decoder();
        // The priming decode holds the thread while both pauses arrive.
        fake.block();
        let audio = [speech(1_000), quiet(300), speech(1_000), quiet(300)].concat();
        stream(&d, S, &audio);
        fake.unblock();
        let (t, facts) = d.finish(S, &pcm(&audio)).expect("finish");
        assert_eq!(t.text, "<40640>");
        assert!(facts.reused);
        assert_eq!(
            fake.decodes(),
            vec![40_640],
            "the stale checkpoint must never have run"
        );
    }

    #[test]
    fn an_adapter_that_never_streamed_gets_a_whole_decode() {
        let (fake, d) = decoder();
        let audio = speech(1_000);
        let (t, facts) = d.finish(S, &pcm(&audio)).expect("finish");
        assert_eq!(t.text, "<16000>");
        assert!(facts.fallback);
        assert_eq!(fake.decodes(), vec![16_000]);
    }

    #[test]
    fn a_whole_decode_fallback_keeps_the_dictations_vocabulary() {
        // Falling back is supposed to be "decode it the way the app always did".
        // The app always passed the user's proper nouns, so the fallback must too.
        let (fake, d) = decoder();
        let hint = DecodeHint {
            vocabulary: vec!["Claude".into()],
            language: None,
        };
        d.begin(S, hint);
        for chunk in speech(1_000).chunks(640) {
            d.push(S, chunk);
        }
        let (_, facts) = d.finish(S, &pcm(&speech(2_000))).expect("finish");
        assert!(facts.fallback);
        assert_eq!(
            fake.hints.lock().unwrap().last(),
            Some(&vec!["Claude".to_owned()])
        );
    }

    #[test]
    fn a_recording_that_disagrees_with_the_stream_gets_a_whole_decode() {
        let (_fake, d) = decoder();
        stream(&d, S, &speech(1_000));
        let (t, facts) = d.finish(S, &pcm(&speech(2_000))).expect("finish");
        assert_eq!(t.text, "<32000>");
        assert!(facts.fallback);
    }

    #[test]
    fn a_segment_that_fails_costs_a_whole_decode_not_the_text() {
        let (fake, d) = decoder();
        d.begin(S, DecodeHint::default());
        wait_for("priming", || fake.all_calls() == vec![PRIME_SAMPLES]);
        fake.fail_next.store(true, Ordering::SeqCst);
        let audio = [speech(1_000), quiet(300)].concat();
        for chunk in audio.chunks(640) {
            d.push(S, chunk);
        }
        let (t, facts) = d
            .finish(S, &pcm(&audio))
            .expect("the user keeps their words");
        assert_eq!(t.text, "<20800>");
        assert!(facts.fallback);
    }

    #[test]
    fn a_discarded_session_is_never_decoded() {
        let (fake, d) = decoder();
        fake.block();
        let audio = [speech(1_000), quiet(300)].concat();
        stream(&d, S, &audio);
        d.discard(S);
        fake.unblock();
        let (_, facts) = d.finish(S, &pcm(&audio)).expect("finish");
        assert!(facts.fallback, "nothing is left of a discarded session");
        assert_eq!(
            fake.decodes(),
            vec![20_800],
            "only the fallback ran, never the checkpoint"
        );
    }

    #[test]
    fn an_idle_model_is_primed_when_a_dictation_begins() {
        let (fake, d) = decoder();
        let t0 = Instant::now();
        assert!(
            d.begin_at(SessionId(1), DecodeHint::default(), t0),
            "never decoded: prime"
        );
        wait_for("priming", || fake.all_calls() == vec![PRIME_SAMPLES]);
        assert!(
            !d.begin_at(
                SessionId(2),
                DecodeHint::default(),
                t0 + Duration::from_secs(10)
            ),
            "decoded ten seconds ago: still warm"
        );
        assert!(
            d.begin_at(
                SessionId(3),
                DecodeHint::default(),
                t0 + Duration::from_secs(40)
            ),
            "idle for forty seconds: prime again"
        );
    }

    #[test]
    fn streaming_a_real_recording_says_what_decoding_it_whole_says() {
        // The accuracy claim, on real weights. If a cut in this clip changes the
        // words, that is exactly what Gate B exists to catch: tune the policy there,
        // do not loosen this.
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../models")
            .join(crate::catalog::DEFAULT_MODEL);
        if !dir.join("tokens.txt").exists() {
            eprintln!("skipping: no model on disk; run scripts/fetch-model.ps1");
            return;
        }
        let mut reader = hound::WavReader::open(dir.join("test.wav")).expect("fixture");
        let mut samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| f32::from(s.expect("sample")) / 32_768.0)
            .collect();
        samples.extend(vec![0.0; 8_000]);

        let t = Arc::new(
            crate::sherpa::SherpaTranscriber::new(crate::catalog::default_spec(), dir)
                .expect("load the model"),
        );
        let whole = t
            .transcribe(&pcm(&samples), &DecodeHint::default())
            .expect("whole")
            .text;

        let d = IncrementalDecoder::new(t, SegmentPolicy::default()).expect("decoder");
        d.begin(S, DecodeHint::default());
        for chunk in samples.chunks(640) {
            d.push(S, chunk);
        }
        let (streamed, facts) = d.finish(S, &pcm(&samples)).expect("streamed");
        assert!(!facts.fallback);

        let norm = |s: &str| {
            s.to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric() || c.is_whitespace())
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(norm(&streamed.text), norm(&whole));
    }
    /// A decode that unwinds kills the one decode thread. Before this, `finish`
    /// waited on `settled` with no way to learn that nothing would ever signal it
    /// again, so the dictation -- and every dictation after it -- hung forever.
    #[test]
    fn a_decode_that_panics_falls_back_instead_of_hanging() {
        let (fake, d) = decoder();
        // Let the prime run first, so the panic below lands on the release's own
        // decode rather than on priming.
        d.begin(S, DecodeHint::default());
        wait_for("priming", || fake.all_calls() == vec![PRIME_SAMPLES]);

        let audio = speech(1_000);
        for chunk in audio.chunks(640) {
            d.push(S, chunk);
        }
        fake.panic_next();
        let (t, facts) = d.finish(S, &pcm(&audio)).expect("finish answers");
        assert!(facts.fallback, "the whole dictation is decoded instead");
        assert_eq!(t.text, format!("<{}>", audio.len()));
    }
}
