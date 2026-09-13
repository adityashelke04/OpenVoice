//! Sample-rate conversion to the 16 kHz the speech model expects.
//!
//! Microphones typically run at 44.1 or 48 kHz, so this runs on essentially every
//! utterance. The filter is built once per capture, and the conversion runs
//! incrementally on the capture thread as audio arrives -- never in the realtime
//! callback, which has a hard deadline this does not. Converting as it goes is
//! what lets the decoder start on the audio before the key comes up.
//!
//! Quality matters more than it looks. Naive decimation aliases high frequencies
//! down into the speech band, and the damage shows up as a transcript that is
//! subtly, unaccountably worse — which is a miserable thing to debug later.

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

const TARGET_RATE: u32 = 16_000;
/// Frames handed to the resampler at a time. Only affects internal buffering.
const CHUNK: usize = 1024;

fn params() -> SincInterpolationParameters {
    SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    }
}

/// Sample-rate conversion to 16 kHz that runs for the life of one capture.
///
/// Two things the one-shot version did not do. It builds the 16,384-tap filter
/// table once per capture, not once per conversion. And on
/// [`StreamingResampler::finish`] it feeds silence until the filter has given
/// back everything it was holding, instead of dropping the last partial chunk.
///
/// Output sample *n* describes the same moment as input sample *n / ratio*,
/// which matters because the segment planner cuts on those positions. That
/// alignment comes from rubato itself: `SincFixedIn` in 0.15 already starts its
/// output on time, and trimming `output_delay()` frames on top of that was
/// measured to make the output 22 samples (1.4 ms) *early*. Nothing is trimmed
/// here, and `the_output_is_not_late` pins the alignment either way.
pub(crate) struct StreamingResampler {
    /// `None` when the device already runs at 16 kHz.
    inner: Option<SincFixedIn<f32>>,
    /// Input not yet a whole chunk.
    pending: Vec<f32>,
    /// Reused output buffer, one channel.
    block: Vec<Vec<f32>>,
    /// Input frames accepted so far.
    consumed: usize,
    /// Output frames delivered so far.
    emitted: usize,
    ratio: f64,
}

impl StreamingResampler {
    /// A converter from `src_rate` to 16 kHz.
    pub(crate) fn new(src_rate: u32) -> Result<Self, String> {
        if src_rate == 0 {
            return Err("source sample rate is zero".into());
        }
        let ratio = f64::from(TARGET_RATE) / f64::from(src_rate);
        let inner = if src_rate == TARGET_RATE {
            None
        } else {
            Some(
                SincFixedIn::<f32>::new(ratio, 1.0, params(), CHUNK, 1)
                    .map_err(|e| format!("constructing resampler: {e}"))?,
            )
        };
        let block = vec![vec![
            0.0;
            inner.as_ref().map_or(0, |r| r.output_frames_max())
        ]];
        Ok(Self {
            inner,
            pending: Vec::with_capacity(CHUNK * 2),
            block,
            consumed: 0,
            emitted: 0,
            ratio,
        })
    }

    /// Convert `mono`, appending whatever output is ready to `out`.
    pub(crate) fn push(&mut self, mono: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        let Some(r) = self.inner.as_mut() else {
            out.extend_from_slice(mono);
            return Ok(());
        };
        self.pending.extend_from_slice(mono);
        self.consumed += mono.len();
        let mut at = 0;
        while self.pending.len() - at >= CHUNK {
            let (_, produced) = r
                .process_into_buffer(
                    &[&self.pending[at..at + CHUNK]][..],
                    self.block.as_mut_slice(),
                    None,
                )
                .map_err(|e| format!("resampling: {e}"))?;
            out.extend_from_slice(&self.block[0][..produced]);
            self.emitted += produced;
            at += CHUNK;
        }
        self.pending.drain(..at);
        Ok(())
    }

    /// Flush the filter. Afterwards exactly `round(consumed * ratio)` samples
    /// have been appended in total.
    pub(crate) fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        let Some(r) = self.inner.as_mut() else {
            return Ok(());
        };
        let expected = (self.consumed as f64 * self.ratio).round() as usize;
        // Bounded: the pending input and the filter's delay together are less than
        // two chunks, so this runs two or three times. The bound only turns a
        // misbehaving resampler into a short recording instead of a hung thread.
        for _ in 0..16 {
            if self.emitted >= expected {
                break;
            }
            let mut padded = vec![0.0f32; CHUNK];
            let n = self.pending.len().min(CHUNK);
            padded[..n].copy_from_slice(&self.pending[..n]);
            self.pending.drain(..n);
            let (_, produced) = r
                .process_into_buffer(&[&padded[..]][..], self.block.as_mut_slice(), None)
                .map_err(|e| format!("resampling tail: {e}"))?;
            out.extend_from_slice(&self.block[0][..produced]);
            self.emitted += produced;
        }
        if self.emitted > expected {
            out.truncate(out.len() - (self.emitted - expected));
            self.emitted = expected;
        }
        Ok(())
    }
}

/// Convert a whole mono recording to 16 kHz in one go.
pub fn to_16k(mono: &[f32], src_rate: u32) -> Result<Vec<f32>, String> {
    if mono.is_empty() {
        return Ok(Vec::new());
    }
    let mut r = StreamingResampler::new(src_rate)?;
    let mut out = Vec::with_capacity((mono.len() as f64 * r.ratio) as usize + CHUNK);
    r.push(mono, &mut out)?;
    r.finish(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| (i as f32 * hz * std::f32::consts::TAU / rate as f32).sin())
            .collect()
    }

    #[test]
    fn passthrough_when_already_at_target_rate() {
        let input: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        assert_eq!(to_16k(&input, TARGET_RATE).unwrap(), input);
    }

    #[test]
    fn empty_input_is_empty_output() {
        assert!(to_16k(&[], 48_000).unwrap().is_empty());
    }

    #[test]
    fn rejects_a_zero_source_rate() {
        assert!(StreamingResampler::new(0).is_err());
    }

    #[test]
    fn output_length_follows_the_rate_ratio_exactly() {
        // Not "roughly". Every input sample is accounted for, including the ones
        // still inside the filter when the stream ends.
        for rate in [44_100, 48_000] {
            let out = to_16k(&vec![0.0; rate as usize], rate).unwrap();
            assert!(
                (out.len() as i64 - 16_000).abs() <= 1,
                "{rate} Hz gave {}",
                out.len()
            );
        }
    }

    #[test]
    fn preserves_a_tone_rather_than_aliasing_it() {
        let out = to_16k(&sine(48_000, 440.0, 1.0), 48_000).unwrap();
        let energy: f32 = out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32;
        assert!(
            energy > 0.4 && energy < 0.6,
            "sine RMS^2 should stay near 0.5, got {energy}"
        );
    }

    #[test]
    fn the_output_is_not_late() {
        // A tone that starts at exactly 0.5 s must start at sample 8000 of the
        // output, give or take the filter's smearing of a hard onset. Every cut the
        // segment planner makes is on these positions, so a resampler that ran
        // late -- or one "corrected" into running early -- would move every cut.
        let mut input = vec![0.0; 24_000];
        input.extend(sine(48_000, 1_000.0, 0.5));
        let out = to_16k(&input, 48_000).unwrap();
        let onset = out
            .iter()
            .position(|s| s.abs() > 0.1)
            .expect("the tone is there");
        assert!((onset as i64 - 8_000).abs() <= 4, "onset at {onset}");
    }

    #[test]
    fn how_the_input_is_split_does_not_change_the_output() {
        // The capture thread hands over whatever arrived in the last 40 ms. If the
        // result depended on that, streaming and a one-shot conversion of the same
        // recording would disagree, and the decoder's fallback check would fire.
        let input = sine(48_000, 440.0, 1.3);
        let whole = to_16k(&input, 48_000).unwrap();

        let mut r = StreamingResampler::new(48_000).unwrap();
        let mut out = Vec::new();
        let (mut rest, mut n) = (&input[..], 1usize);
        while !rest.is_empty() {
            let k = n.min(rest.len());
            r.push(&rest[..k], &mut out).unwrap();
            rest = &rest[k..];
            n = n * 7 % 997 + 1;
        }
        r.finish(&mut out).unwrap();
        assert_eq!(out, whole);
    }
}
