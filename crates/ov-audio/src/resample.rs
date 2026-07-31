//! Sample-rate conversion to the 16 kHz Whisper expects.
//!
//! Microphones typically run at 44.1 or 48 kHz, so this runs on essentially every
//! utterance. It is done once, offline, over the whole buffer at stop time rather
//! than streaming in the realtime callback: the callback has a hard deadline and
//! this does not, and resampling ten seconds of audio costs single-digit
//! milliseconds against a ~500 ms decode.
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

/// Convert mono `f32` samples from `src_rate` to 16 kHz.
///
/// Returns the input unchanged when it is already at the target rate, which is the
/// common case for headset microphones that natively support 16 kHz.
pub fn to_16k(mono: &[f32], src_rate: u32) -> Result<Vec<f32>, String> {
    if mono.is_empty() {
        return Ok(Vec::new());
    }
    if src_rate == TARGET_RATE {
        return Ok(mono.to_vec());
    }
    if src_rate == 0 {
        return Err("source sample rate is zero".into());
    }

    let ratio = f64::from(TARGET_RATE) / f64::from(src_rate);
    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    };

    let mut resampler = SincFixedIn::<f32>::new(ratio, 1.0, params, CHUNK, 1)
        .map_err(|e| format!("constructing resampler: {e}"))?;

    let mut out = Vec::with_capacity((mono.len() as f64 * ratio) as usize + CHUNK);
    let mut pos = 0;

    while pos + CHUNK <= mono.len() {
        let block = vec![mono[pos..pos + CHUNK].to_vec()];
        let done = resampler
            .process(&block, None)
            .map_err(|e| format!("resampling: {e}"))?;
        out.extend_from_slice(&done[0]);
        pos += CHUNK;
    }

    // Zero-pad the final partial chunk. `SincFixedIn` requires exactly `CHUNK`
    // frames per call, and dropping the remainder would clip the last fraction of a
    // second -- which in dictation is usually the end of the final word.
    if pos < mono.len() {
        let mut tail = mono[pos..].to_vec();
        let real = tail.len();
        tail.resize(CHUNK, 0.0);
        let done = resampler
            .process(&[tail], None)
            .map_err(|e| format!("resampling tail: {e}"))?;
        let keep = (real as f64 * ratio).round() as usize;
        out.extend(done[0].iter().take(keep.min(done[0].len())));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn downsampling_produces_roughly_the_expected_length() {
        // One second at 48 kHz should come out as about one second at 16 kHz.
        let input = vec![0.0f32; 48_000];
        let out = to_16k(&input, 48_000).unwrap();
        let expected = 16_000f64;
        let drift = (out.len() as f64 - expected).abs() / expected;
        assert!(drift < 0.02, "expected ~16000 samples, got {}", out.len());
    }

    #[test]
    fn awkward_ratio_still_lands_close() {
        // 44.1 kHz is not an integer multiple of 16 kHz, which is the case most
        // likely to accumulate error.
        let input = vec![0.0f32; 44_100];
        let out = to_16k(&input, 44_100).unwrap();
        let drift = (out.len() as f64 - 16_000.0).abs() / 16_000.0;
        assert!(drift < 0.02, "expected ~16000 samples, got {}", out.len());
    }

    #[test]
    fn preserves_a_tone_rather_than_aliasing_it() {
        // A 440 Hz tone is well below the 8 kHz Nyquist limit of the target rate,
        // so it must survive. If this fails, the filter is wrong and transcripts
        // will be quietly worse.
        let rate = 48_000;
        let input: Vec<f32> = (0..rate)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin())
            .collect();
        let out = to_16k(&input, rate as u32).unwrap();

        let energy: f32 = out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32;
        assert!(
            energy > 0.4 && energy < 0.6,
            "sine RMS^2 should stay near 0.5, got {energy}"
        );
    }

    #[test]
    fn rejects_a_zero_source_rate() {
        assert!(to_16k(&[0.0, 1.0], 0).is_err());
    }
}
