//! Where the time goes between letting go of the key and text appearing.
//!
//! History records one number per session, `latency_ms`: release to delivery.
//! It showed that about one dictation in ten lost a second or more *outside*
//! the decoder, and it could never say where. This module is the answer to
//! "where": a timestamp per stage, the durations between them, and a single
//! line format.
//!
//! One format, defined here, because two programs need it. The app writes a
//! line per session into `openvoice.log`, and `ov latency` in the CLI reads
//! those lines back. Defining the format twice is how the two would drift apart
//! and produce a report that silently lies.
//!
//! Pure data, like the rest of this crate. The composition root stamps
//! [`Millis`] values from its own clock; nothing here reads one.

use crate::types::{Millis, SessionId};

/// Every latency line contains this, and nothing else in the log does.
pub const LINE_MARKER: &str = "latency session=";

/// What the decoder did for a session, beyond how long it took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeFacts {
    /// The release found the tail already decoded, so no decode ran after it.
    pub reused: bool,
    /// Decoded segments joined into the transcript.
    pub segments: u32,
    /// The incremental path was abandoned and the whole utterance decoded at once.
    pub fallback: bool,
}

impl Default for DecodeFacts {
    /// A whole-utterance decode: one segment, nothing reused, nothing abandoned.
    fn default() -> Self {
        Self {
            reused: false,
            segments: 1,
            fallback: false,
        }
    }
}

/// When each stage of one session finished, in the order they happen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StageClock {
    /// The engine executed `StopCapture`, which is the release as the engine saw it.
    pub stop_requested: Option<Millis>,
    /// The audio adapter handed the recording back.
    pub captured: Option<Millis>,
    /// Transcription began.
    pub decode_started: Option<Millis>,
    /// Transcription returned, successfully or not.
    pub decoded: Option<Millis>,
    /// Formatting returned.
    pub formatted: Option<Millis>,
    /// Injection returned, successfully or not.
    pub injected: Option<Millis>,
    /// What the decoder did.
    pub facts: DecodeFacts,
}

/// Stage durations for one session: exactly what a latency line carries.
#[derive(Debug, Clone, PartialEq)]
pub struct StageTimes {
    /// The session.
    pub session: SessionId,
    /// Length of the captured audio.
    pub audio_ms: u64,
    /// Hook release to delivery, as persisted to history.
    pub latency_ms: u64,
    /// Stop requested → audio returned.
    pub stop_ms: Option<u64>,
    /// Audio returned → decode started. Non-zero means waiting behind another session.
    pub queue_ms: Option<u64>,
    /// Decode started → decode returned.
    pub decode_ms: Option<u64>,
    /// Decode returned → formatting returned.
    pub format_ms: Option<u64>,
    /// Formatting returned → injection returned.
    pub inject_ms: Option<u64>,
    /// Stop requested → injection returned.
    pub total_ms: Option<u64>,
    /// What the decoder did.
    pub facts: DecodeFacts,
}

fn between(from: Option<Millis>, to: Option<Millis>) -> Option<u64> {
    Some(to?.since(from?))
}

impl StageClock {
    /// Whether this session belongs in the latency log at all.
    ///
    /// Only a session the decoder was asked to transcribe has a release-to-text
    /// story. Taps, silent captures and cancelled sessions all reach
    /// `StopCapture` too, and logging them put zero-length recordings into the
    /// shortest bucket, where their odd timings read as slow dictations.
    #[must_use]
    pub fn worth_logging(&self) -> bool {
        self.stop_requested.is_some() && self.decode_started.is_some()
    }

    /// Durations for this clock. `latency_ms` is the session record's own figure,
    /// carried alongside so `latency_ms - total_ms` shows how long the release sat
    /// in the session loop before the engine acted on it.
    #[must_use]
    pub fn times(&self, session: SessionId, audio_ms: u64, latency_ms: u64) -> StageTimes {
        StageTimes {
            session,
            audio_ms,
            latency_ms,
            stop_ms: between(self.stop_requested, self.captured),
            queue_ms: between(self.captured, self.decode_started),
            decode_ms: between(self.decode_started, self.decoded),
            format_ms: between(self.decoded, self.formatted),
            inject_ms: between(self.formatted, self.injected),
            total_ms: between(self.stop_requested, self.injected),
            facts: self.facts,
        }
    }
}

impl StageTimes {
    /// The log line. Absent stages are written as `-`, never as `0`.
    #[must_use]
    pub fn to_line(&self) -> String {
        fn ms(v: Option<u64>) -> String {
            v.map_or_else(|| "-".to_owned(), |v| v.to_string())
        }
        format!(
            "{LINE_MARKER}{} audio_ms={} latency_ms={} stop_ms={} queue_ms={} decode_ms={} \
             format_ms={} inject_ms={} total_ms={} reused={} segments={} fallback={}",
            self.session.0,
            self.audio_ms,
            self.latency_ms,
            ms(self.stop_ms),
            ms(self.queue_ms),
            ms(self.decode_ms),
            ms(self.format_ms),
            ms(self.inject_ms),
            ms(self.total_ms),
            self.facts.reused,
            self.facts.segments,
            self.facts.fallback,
        )
    }

    /// Read a line written by [`StageTimes::to_line`], wherever it sits in a log line.
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        let rest = &line[line.find(LINE_MARKER)?..];
        let mut t = Self {
            session: SessionId(0),
            audio_ms: 0,
            latency_ms: 0,
            stop_ms: None,
            queue_ms: None,
            decode_ms: None,
            format_ms: None,
            inject_ms: None,
            total_ms: None,
            facts: DecodeFacts::default(),
        };
        for pair in rest.split_whitespace() {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let optional = value.parse::<u64>().ok();
            match key {
                "session" => t.session = SessionId(value.parse().ok()?),
                "audio_ms" => t.audio_ms = value.parse().ok()?,
                "latency_ms" => t.latency_ms = value.parse().ok()?,
                "stop_ms" => t.stop_ms = optional,
                "queue_ms" => t.queue_ms = optional,
                "decode_ms" => t.decode_ms = optional,
                "format_ms" => t.format_ms = optional,
                "inject_ms" => t.inject_ms = optional,
                "total_ms" => t.total_ms = optional,
                "reused" => t.facts.reused = value == "true",
                "segments" => t.facts.segments = value.parse().ok()?,
                "fallback" => t.facts.fallback = value == "true",
                _ => {}
            }
        }
        Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// A session that went through every stage: 12 ms to stop, 180 ms to decode,
    /// 33 ms to inject, and 5 ms the loop took to see the release at all.
    fn clock() -> StageClock {
        StageClock {
            stop_requested: Some(Millis(1_000)),
            captured: Some(Millis(1_012)),
            decode_started: Some(Millis(1_013)),
            decoded: Some(Millis(1_193)),
            formatted: Some(Millis(1_194)),
            injected: Some(Millis(1_227)),
            facts: DecodeFacts {
                reused: true,
                segments: 2,
                fallback: false,
            },
        }
    }

    #[test]
    fn every_stage_is_the_gap_between_its_neighbours() {
        let t = clock().times(SessionId(3), 2_140, 232);
        assert_eq!(t.stop_ms, Some(12));
        assert_eq!(t.queue_ms, Some(1));
        assert_eq!(t.decode_ms, Some(180));
        assert_eq!(t.format_ms, Some(1));
        assert_eq!(t.inject_ms, Some(33));
        assert_eq!(t.total_ms, Some(227));
        assert_eq!(t.latency_ms, 232);
    }

    #[test]
    fn a_stage_that_never_happened_is_absent_not_zero() {
        // A decode that never returned is not a decode that took no time. Writing
        // 0 would drag every percentile toward "fast" exactly when something broke.
        let mut c = clock();
        c.decoded = None;
        let t = c.times(SessionId(3), 2_140, 232);
        assert_eq!(t.decode_ms, None);
        assert_eq!(t.format_ms, None);
        assert_eq!(t.total_ms, Some(227), "the ends are still known");
        let line = t.to_line();
        assert!(line.contains("decode_ms=-"), "{line}");
        assert_eq!(StageTimes::parse(&line).map(|p| p.decode_ms), Some(None));
    }

    #[test]
    fn a_line_survives_the_trip_through_the_log() {
        let t = clock().times(SessionId(3), 2_140, 232);
        // What tracing writes with `with_target(false)`: a timestamp, a level, then
        // the message. The parser has to find the marker, not assume column zero.
        let logged = format!("2026-09-13T11:24:41.482082Z  INFO {}", t.to_line());
        assert_eq!(StageTimes::parse(&logged), Some(t));
    }

    #[test]
    fn only_a_session_that_reached_the_decoder_is_worth_a_line() {
        // Found in the first real session: taps, silent captures and Escape all
        // reach StopCapture, and their lines (audio_ms=0, latency_ms=371) became
        // the slow tail of the "<3s" bucket. None of them has a release-to-text
        // story to tell.
        let mut tap = StageClock {
            stop_requested: Some(Millis(1_000)),
            captured: Some(Millis(1_001)),
            ..StageClock::default()
        };
        assert!(!tap.worth_logging(), "a tap never reaches the decoder");
        tap.decode_started = Some(Millis(1_002));
        assert!(tap.worth_logging());
        assert!(
            !StageClock::default().worth_logging(),
            "nor does a session stamped only by a late decode"
        );
    }

    #[test]
    fn other_log_lines_are_not_latency_lines() {
        assert_eq!(
            StageTimes::parse("2026-09-13T11:24:41Z DEBUG decoded decode_ms=1199 audio_ms=17258"),
            None
        );
    }
}
