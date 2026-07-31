//! Writing captured audio out for the sidecar to read.
//!
//! A temporary WAV file is the transport. Shared memory would be faster, but for a
//! ten-second utterance this costs roughly two milliseconds against a decode of
//! several hundred, and a file that can be opened in any audio editor is worth far
//! more than that when a transcript comes back wrong and you need to hear what the
//! model actually heard.
//!
//! Files are deleted immediately after each decode, success or failure.

use std::path::Path;

/// Write mono `f32` samples as 16-bit PCM at 16 kHz.
pub fn write_16k_mono(path: &Path, samples: &[f32]) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|e| format!("creating {}: {e}", path.display()))?;

    for &s in samples {
        // Clamp before scaling. A sample slightly outside [-1, 1] -- which a hot
        // microphone will produce -- would otherwise wrap around and turn a loud
        // moment into a burst of noise the model cannot read.
        let clamped = s.clamp(-1.0, 1.0);
        let v = (clamped * f32::from(i16::MAX)) as i16;
        writer.write_sample(v).map_err(|e| format!("writing sample: {e}"))?;
    }

    writer.finalize().map_err(|e| format!("finalising wav: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("ov-wav-test-{name}.wav"));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn writes_a_readable_16k_mono_file() {
        let path = scratch("basic");
        write_16k_mono(&path, &[0.0, 0.5, -0.5, 1.0]).unwrap();

        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().bits_per_sample, 16);
        assert_eq!(reader.len(), 4);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn clamps_hot_samples_instead_of_wrapping() {
        // Without the clamp this wraps to a large negative value, turning a loud
        // syllable into noise that transcribes as nothing at all.
        let path = scratch("clamp");
        write_16k_mono(&path, &[2.0, -2.0]).unwrap();

        let mut reader = hound::WavReader::open(&path).unwrap();
        let samples: Vec<i16> = reader.samples::<i16>().map(std::result::Result::unwrap).collect();
        assert!(samples[0] > 32_000, "positive overload must clamp high");
        assert!(samples[1] < -32_000, "negative overload must clamp low");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_input_produces_a_valid_empty_file() {
        let path = scratch("empty");
        write_16k_mono(&path, &[]).unwrap();
        assert_eq!(hound::WavReader::open(&path).unwrap().len(), 0);
        let _ = std::fs::remove_file(&path);
    }
}
