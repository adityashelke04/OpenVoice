//! Where a dictation can be cut, so it can be decoded while it is still spoken.
//!
//! Parakeet decodes a whole utterance at about 75 ms per second of audio. Waiting
//! for the key to come up before starting made a 30-second dictation wait two
//! seconds after the user had finished. This module decides, from the audio alone,
//! which parts are safe to decode early.
//!
//! Two kinds of decision, because they trade different costs:
//!
//! * A **checkpoint** at a short pause says "everything so far might be the whole
//!   thing". Decoding it is speculative. If the user lets go before speaking again,
//!   the release has nothing left to do. If they carry on, the work is wasted.
//! * A **commit** at a long pause after a sentence's worth of speech says "this part
//!   is finished". It is decoded once and joined into the transcript whatever comes
//!   after. Commits are what keep a long dictation's tail short.
//!
//! Pure, and therefore exhaustively testable: no clock, no model, no threads. It
//! sees samples and returns positions.

use std::collections::VecDeque;

/// Analysis frame: 20 ms at 16 kHz. Every position this module reports is a
/// multiple of it.
pub const FRAME: usize = 320;

const RATE: u64 = 16_000;

/// How far back the noise floor looks. Two seconds of speech always contains a
/// gap between words, so the quietest frame in it is the room, not the speaker.
const NOISE_WINDOW_FRAMES: usize = 100;

/// How far back a forced cut searches for a quiet frame: two seconds.
const FORCED_CUT_SEARCH_FRAMES: usize = 100;

/// The thresholds a planner decides with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentPolicy {
    /// A frame quieter than this is never speech, however quiet the room is.
    pub floor_rms: f32,
    /// How many times louder than the room a frame must be to count as speech.
    pub noise_ratio: f32,
    /// Pause after speech that triggers a speculative decode.
    pub checkpoint_pause_ms: u64,
    /// Pause after speech that ends a segment for good.
    pub commit_pause_ms: u64,
    /// A segment shorter than this is never committed at a pause. Short phrases
    /// decode fast anyway, and cutting them off costs the model context.
    pub min_segment_ms: u64,
    /// A segment this long is committed even without a pause, at its quietest
    /// recent frame, so a monologue cannot grow the tail without bound.
    pub max_segment_ms: u64,
}

impl Default for SegmentPolicy {
    /// Chosen at Gate B; see `docs/benchmarks/2026-09-13-release-latency.md`.
    fn default() -> Self {
        Self {
            floor_rms: 0.004,
            noise_ratio: 2.5,
            checkpoint_pause_ms: 240,
            commit_pause_ms: 480,
            min_segment_ms: 3_000,
            max_segment_ms: 20_000,
        }
    }
}

/// Something worth decoding now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// `start..end` may be the rest of the dictation. Decode it speculatively.
    Checkpoint {
        /// First sample.
        start: usize,
        /// One past the last sample.
        end: usize,
    },
    /// `start..end` is final and belongs in the transcript.
    Commit {
        /// First sample.
        start: usize,
        /// One past the last sample.
        end: usize,
    },
}

/// What is left to decode when the key comes up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tail {
    /// First sample not yet committed.
    pub start: usize,
    /// One past the last sample worth decoding.
    pub end: usize,
    /// The last checkpoint is exactly `start..end`, and nothing but pause followed
    /// it. Its decode is the answer.
    pub reuse: bool,
    /// Nothing but pause since `start`. There is nothing to decode.
    pub silent: bool,
}

/// Watches one dictation's audio and says when parts of it can be decoded.
#[derive(Debug)]
pub struct SegmentPlanner {
    policy: SegmentPolicy,
    /// Samples not yet a whole frame.
    carry: Vec<f32>,
    /// Samples consumed as whole frames.
    pos: usize,
    /// Where the current, uncommitted segment starts.
    seg_start: usize,
    /// Speech seen since `seg_start`.
    heard: bool,
    /// Consecutive pause frames.
    pause_frames: u64,
    /// End of the checkpoint in the current segment, if one was made.
    checkpoint: Option<usize>,
    /// Speech seen since that checkpoint.
    speech_after_checkpoint: bool,
    /// Recent frame levels, for the noise floor.
    recent: VecDeque<f32>,
    /// Level and verdict of every frame in the current segment, for forced cuts.
    frames: Vec<(f32, bool)>,
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn ms(samples: usize) -> u64 {
    samples as u64 * 1_000 / RATE
}

impl SegmentPlanner {
    /// A planner for one dictation.
    #[must_use]
    pub fn new(policy: SegmentPolicy) -> Self {
        Self {
            policy,
            carry: Vec::with_capacity(FRAME),
            pos: 0,
            seg_start: 0,
            heard: false,
            pause_frames: 0,
            checkpoint: None,
            speech_after_checkpoint: false,
            recent: VecDeque::with_capacity(NOISE_WINDOW_FRAMES + 1),
            frames: Vec::new(),
        }
    }

    /// Feed the next samples of the dictation. Returns what became decodable.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Decision> {
        let mut decisions = Vec::new();
        self.carry.extend_from_slice(samples);
        let whole = self.carry.len() / FRAME * FRAME;
        let mut at = 0;
        while at < whole {
            let level = rms(&self.carry[at..at + FRAME]);
            self.frame(level, &mut decisions);
            at += FRAME;
        }
        self.carry.drain(..whole);
        decisions
    }

    /// The key came up after `total` samples. What is left to decode?
    pub fn finish(&mut self, total: usize) -> Tail {
        // A partial frame at the very end still counts if someone spoke into it.
        if !self.carry.is_empty() && rms(&self.carry) >= self.threshold() {
            self.heard = true;
            if self.checkpoint.is_some() {
                self.speech_after_checkpoint = true;
            }
        }
        match self.checkpoint {
            Some(end) if !self.speech_after_checkpoint => Tail {
                start: self.seg_start,
                end,
                reuse: true,
                silent: false,
            },
            _ => Tail {
                start: self.seg_start,
                end: total,
                reuse: false,
                silent: !self.heard,
            },
        }
    }

    /// The level a frame must reach to be speech: well above the quietest recent
    /// frame, and never below the floor.
    fn threshold(&self) -> f32 {
        let room = self.recent.iter().copied().fold(f32::INFINITY, f32::min);
        if room.is_finite() {
            (room * self.policy.noise_ratio).max(self.policy.floor_rms)
        } else {
            self.policy.floor_rms
        }
    }

    fn frame(&mut self, level: f32, decisions: &mut Vec<Decision>) {
        self.recent.push_back(level);
        if self.recent.len() > NOISE_WINDOW_FRAMES {
            self.recent.pop_front();
        }
        let speech = level >= self.threshold();
        self.frames.push((level, speech));
        self.pos += FRAME;

        if speech {
            self.heard = true;
            self.pause_frames = 0;
            if self.checkpoint.is_some() {
                self.speech_after_checkpoint = true;
            }
        } else {
            self.pause_frames += 1;
            if self.heard && self.pause_frames == self.policy.checkpoint_pause_ms / 20 {
                self.checkpoint = Some(self.pos);
                self.speech_after_checkpoint = false;
                decisions.push(Decision::Checkpoint {
                    start: self.seg_start,
                    end: self.pos,
                });
            }
            if self.heard && self.pause_frames == self.policy.commit_pause_ms / 20 {
                let long_enough = self
                    .checkpoint
                    .filter(|&end| ms(end - self.seg_start) >= self.policy.min_segment_ms);
                if let Some(end) = long_enough {
                    decisions.push(Decision::Commit {
                        start: self.seg_start,
                        end,
                    });
                    self.cut_at(end);
                }
            }
        }

        if ms(self.pos - self.seg_start) >= self.policy.max_segment_ms {
            let end = self.quietest_recent_cut();
            decisions.push(Decision::Commit {
                start: self.seg_start,
                end,
            });
            self.cut_at(end);
        }
    }

    /// End of the quietest frame in the last two seconds of the segment.
    fn quietest_recent_cut(&self) -> usize {
        let from = self.frames.len().saturating_sub(FORCED_CUT_SEARCH_FRAMES);
        let k = (from..self.frames.len())
            .min_by(|&a, &b| self.frames[a].0.total_cmp(&self.frames[b].0))
            .unwrap_or(self.frames.len() - 1);
        self.seg_start + (k + 1) * FRAME
    }

    fn cut_at(&mut self, end: usize) {
        let dropped = ((end - self.seg_start) / FRAME).min(self.frames.len());
        self.frames.drain(..dropped);
        self.seg_start = end;
        self.checkpoint = None;
        self.speech_after_checkpoint = false;
        self.heard = self.frames.iter().any(|&(_, speech)| speech);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The thresholds every expected position below was computed with.
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

    fn level(ms: u64, a: f32) -> Vec<f32> {
        (0..(ms / 20) as usize * FRAME)
            .map(|i| if i % 2 == 0 { a } else { -a })
            .collect()
    }

    /// Speech-like: 0.1 RMS, with a 20 ms dip to `dip` in the middle of every half
    /// second, the way real speech drops between words.
    fn speech_with_dips(ms: u64, dip: f32) -> Vec<f32> {
        (0..(ms / 20) as usize)
            .flat_map(|f| {
                let a = if f % 25 == 12 { dip } else { 0.1 };
                (0..FRAME).map(move |i| if i % 2 == 0 { a } else { -a })
            })
            .collect()
    }

    fn speech(ms: u64) -> Vec<f32> {
        speech_with_dips(ms, 0.002)
    }

    fn quiet(ms: u64) -> Vec<f32> {
        level(ms, 0.001)
    }

    fn plan(audio: &[f32], chunk: usize) -> (SegmentPlanner, Vec<Decision>) {
        let mut p = SegmentPlanner::new(policy());
        let decisions: Vec<Decision> = audio.chunks(chunk).flat_map(|c| p.push(c)).collect();
        (p, decisions)
    }

    #[test]
    fn the_defaults_are_the_ones_the_benchmark_chose() {
        // Change this together with the Gate B record in
        // docs/benchmarks/2026-09-13-release-latency.md, never on its own.
        assert_eq!(SegmentPolicy::default(), policy());
    }

    #[test]
    fn continuous_speech_makes_no_decisions() {
        assert_eq!(plan(&speech(5_000), 640).1, vec![]);
    }

    #[test]
    fn a_short_pause_is_worth_a_speculative_decode_but_not_a_cut() {
        // 1 s of speech (16,000 samples), then the 12th quiet frame is 240 ms.
        let audio = [speech(1_000), quiet(300)].concat();
        assert_eq!(
            plan(&audio, 640).1,
            vec![Decision::Checkpoint {
                start: 0,
                end: 19_840
            }]
        );
    }

    #[test]
    fn a_long_pause_after_enough_speech_commits_at_the_checkpoint() {
        let audio = [speech(4_000), quiet(600)].concat();
        assert_eq!(
            plan(&audio, 640).1,
            vec![
                Decision::Checkpoint {
                    start: 0,
                    end: 67_840
                },
                Decision::Commit {
                    start: 0,
                    end: 67_840
                },
            ]
        );
    }

    #[test]
    fn a_long_pause_after_too_little_speech_does_not_commit() {
        // Two seconds is a phrase, not a sentence. Cutting it off from what follows
        // costs the model context for no gain: it decodes in 150 ms anyway.
        let audio = [speech(2_000), quiet(600)].concat();
        let decisions = plan(&audio, 640).1;
        assert!(
            decisions
                .iter()
                .all(|d| matches!(d, Decision::Checkpoint { .. })),
            "{decisions:?}"
        );
    }

    #[test]
    fn a_monologue_with_no_pause_is_cut_at_its_quietest_moment() {
        // 19 s of speech, one very quiet frame at 19.00 s (frame 950), then more
        // speech. At 20 s the planner must cut, and the least damaging place in the
        // last two seconds is the end of that frame: 951 * 320.
        let audio = [speech(19_000), level(20, 0.0005), speech(6_000)].concat();
        assert_eq!(
            plan(&audio, 640).1,
            vec![Decision::Commit {
                start: 0,
                end: 304_320
            }]
        );
    }

    #[test]
    fn releasing_after_a_pause_reuses_the_speculative_decode() {
        let audio = [speech(1_000), quiet(300)].concat();
        let (mut p, _) = plan(&audio, 640);
        assert_eq!(
            p.finish(audio.len()),
            Tail {
                start: 0,
                end: 19_840,
                reuse: true,
                silent: false
            }
        );
    }

    #[test]
    fn speech_after_the_checkpoint_needs_a_fresh_decode() {
        let audio = [speech(1_000), quiet(300), speech(500)].concat();
        let (mut p, _) = plan(&audio, 640);
        assert_eq!(
            p.finish(audio.len()),
            Tail {
                start: 0,
                end: audio.len(),
                reuse: false,
                silent: false
            }
        );
    }

    #[test]
    fn a_tail_of_nothing_but_silence_needs_no_decode() {
        let audio = [speech(4_000), quiet(1_000)].concat();
        let (mut p, _) = plan(&audio, 640);
        assert_eq!(
            p.finish(audio.len()),
            Tail {
                start: 67_840,
                end: audio.len(),
                reuse: false,
                silent: true
            }
        );
    }

    #[test]
    fn how_the_audio_is_chunked_does_not_change_the_plan() {
        let audio = [speech(4_000), quiet(600), speech(3_000), quiet(300)].concat();
        assert_eq!(plan(&audio, 64_000).1, plan(&audio, 7).1);
    }

    #[test]
    fn room_noise_is_not_mistaken_for_speech() {
        // A fan at 0.015 RMS, louder than the fixed floor. Measured against a fixed
        // threshold it would read as someone talking, and no pause would ever be found.
        let audio = [
            level(500, 0.015),
            speech_with_dips(1_000, 0.015),
            level(300, 0.015),
        ]
        .concat();
        let checkpoints = plan(&audio, 640)
            .1
            .iter()
            .filter(|d| matches!(d, Decision::Checkpoint { .. }))
            .count();
        assert_eq!(checkpoints, 1, "exactly the pause after the speech");
    }
}
