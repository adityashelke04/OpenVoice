//! `ov bench`: how long a release waits, and what it costs in accuracy.
//!
//! "Release latency" here is decoding work left to do once the key comes up.
//! It excludes stopping the microphone, formatting and typing, which
//! `ov latency` measures in the real app. It is the part a decoding strategy
//! changes, isolated from everything a decoding strategy cannot.

use std::time::Instant;

use ov_core::ports::{DecodeHint, Pcm16k, Transcriber};

use crate::corpus::Clip;
use crate::stats::percentile;

/// One clip's outcome.
#[derive(Debug, Clone)]
pub struct Measured {
    /// Clip id.
    pub id: String,
    /// Length of the speech, excluding the pause before release.
    pub audio_ms: u64,
    /// Wall time from release to transcript.
    pub release_ms: u64,
    /// What was said.
    pub reference: String,
    /// What the model heard.
    pub hypothesis: String,
    /// Nothing was left to decode at release.
    pub reused: bool,
}

/// A run's totals.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Clips decoded.
    pub clips: usize,
    /// Seconds of speech.
    pub audio_s: f64,
    /// Pooled word error rate, percent.
    pub wer: f64,
    /// Median release latency.
    pub p50: Option<u64>,
    /// 90th-percentile release latency.
    pub p90: Option<u64>,
    /// Worst release latency.
    pub max: Option<u64>,
    /// Clips with nothing left to decode at release.
    pub reused: usize,
}

/// Decode `clip`, followed by `pause_ms` of silence, in one call: what the app
/// does today.
///
/// The pause is included because the app includes it. People stop talking a
/// moment before they let go, and that silence is in the recording.
pub fn run_full<T: Transcriber>(t: &T, clip: &Clip, pause_ms: u64) -> Result<Measured, String> {
    let mut samples = clip.samples.clone();
    samples.resize(samples.len() + (pause_ms * 16) as usize, 0.0);
    let audio = Pcm16k { samples };
    let released = Instant::now();
    let out = t
        .transcribe(&audio, &DecodeHint::default())
        .map_err(|e| e.to_string())?;
    Ok(Measured {
        id: clip.id.clone(),
        audio_ms: clip.samples.len() as u64 * 1_000 / 16_000,
        release_ms: released.elapsed().as_millis() as u64,
        reference: clip.reference.clone(),
        hypothesis: out.text,
        reused: false,
    })
}

/// Totals for a run.
#[must_use]
pub fn summarize(runs: &[Measured]) -> Summary {
    let latencies: Vec<u64> = runs.iter().map(|m| m.release_ms).collect();
    let pairs: Vec<(String, String)> = runs
        .iter()
        .map(|m| (m.reference.clone(), m.hypothesis.clone()))
        .collect();
    Summary {
        clips: runs.len(),
        audio_s: runs.iter().map(|m| m.audio_ms as f64 / 1_000.0).sum(),
        wer: crate::wer::wer(&pairs),
        p50: percentile(&latencies, 50.0),
        p90: percentile(&latencies, 90.0),
        max: percentile(&latencies, 100.0),
        reused: runs.iter().filter(|m| m.reused).count(),
    }
}

/// One line, stable enough to paste into `docs/benchmarks`.
#[must_use]
pub fn render(label: &str, threads: i32, s: &Summary) -> String {
    let f = |v: Option<u64>| v.map_or_else(|| "-".to_owned(), |v| v.to_string());
    format!(
        "mode={label} threads={threads} clips={} audio={:.0}s wer={:.2}% release_ms p50={} p90={} max={} reused={}/{}",
        s.clips, s.audio_s, s.wer, f(s.p50), f(s.p90), f(s.max), s.reused, s.clips
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ov_core::error::Result as CoreResult;
    use ov_core::types::Transcript;
    use std::sync::Mutex;

    /// Says the same words whatever it hears, and remembers how much it heard.
    struct Parrot(Mutex<Vec<usize>>);

    impl Transcriber for Parrot {
        fn warm(&self) -> CoreResult<()> {
            Ok(())
        }
        fn transcribe(&self, audio: &Pcm16k, _: &DecodeHint) -> CoreResult<Transcript> {
            self.0.lock().unwrap().push(audio.samples.len());
            Ok(Transcript {
                text: "hello world".into(),
                language: None,
                confidence: None,
            })
        }
        fn model_id(&self) -> String {
            "parrot".into()
        }
    }

    fn clip() -> Clip {
        Clip {
            id: "c1".into(),
            reference: "HELLO THERE".into(),
            samples: vec![0.1; 32_000],
        }
    }

    #[test]
    fn a_full_run_decodes_the_clip_plus_the_pause_before_release() {
        let parrot = Parrot(Mutex::new(Vec::new()));
        let m = run_full(&parrot, &clip(), 400).expect("run");
        assert_eq!(*parrot.0.lock().unwrap(), vec![32_000 + 6_400]);
        assert_eq!(
            m.audio_ms, 2_000,
            "audio length is the speech, not the pause"
        );
        assert_eq!(m.hypothesis, "hello world");
        assert!(!m.reused);
    }

    #[test]
    fn the_summary_pools_accuracy_and_ranks_latency() {
        let m = |ms, hyp: &str| Measured {
            id: "x".into(),
            audio_ms: 2_000,
            release_ms: ms,
            reference: "HELLO THERE".into(),
            hypothesis: hyp.into(),
            reused: ms < 50,
        };
        let s = summarize(&[
            m(10, "hello there"),
            m(100, "hello where"),
            m(300, "hello there"),
        ]);
        assert_eq!(s.clips, 3);
        assert!((s.wer - 100.0 / 6.0).abs() < 1e-9);
        assert_eq!((s.p50, s.p90, s.max), (Some(100), Some(300), Some(300)));
        assert_eq!(s.reused, 1);
        assert!((s.audio_s - 6.0).abs() < 1e-9);
    }
}
