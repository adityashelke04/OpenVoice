//! From what the device delivers to what the decoder wants, while it is delivered.
//!
//! The realtime callback only appends interleaved device-rate samples to a
//! buffer, as it always has. Every 40 ms the capture thread takes that buffer and
//! runs it through here: downmix, resample, hand the new 16 kHz samples to the
//! sink. When capture stops, whatever is still inside the resampler is flushed
//! and the whole recording is returned. It is the same recording the sink already
//! saw, in the same order.

use ov_core::ports::PcmSink;

use crate::resample::StreamingResampler;

/// Averages interleaved channels to mono, carrying a frame split across calls.
pub(crate) struct Downmixer {
    channels: usize,
    carry: Vec<f32>,
}

impl Downmixer {
    pub(crate) fn new(channels: u16) -> Self {
        Self {
            channels: usize::from(channels.max(1)),
            carry: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, interleaved: &[f32], out: &mut Vec<f32>) {
        if self.channels == 1 {
            out.extend_from_slice(interleaved);
            return;
        }
        let ch = self.channels;
        self.carry.extend_from_slice(interleaved);
        let whole = self.carry.len() / ch * ch;
        out.extend(
            self.carry[..whole]
                .chunks_exact(ch)
                .map(|frame| frame.iter().sum::<f32>() / ch as f32),
        );
        self.carry.drain(..whole);
    }
}

/// One capture's conversion, from first callback to stop.
pub(crate) struct Converter {
    downmix: Downmixer,
    resample: StreamingResampler,
    /// Scratch space for the downmixed block, reused between feeds.
    mono: Vec<f32>,
    /// Every 16 kHz sample so far. Returned, not copied, by `finish`.
    all: Vec<f32>,
    sink: PcmSink,
}

impl Converter {
    pub(crate) fn new(src_rate: u32, channels: u16, sink: PcmSink) -> Result<Self, String> {
        Ok(Self {
            downmix: Downmixer::new(channels),
            resample: StreamingResampler::new(src_rate)?,
            mono: Vec::new(),
            // A minute of 16 kHz, so a normal dictation never reallocates.
            all: Vec::with_capacity(16_000 * 60),
            sink,
        })
    }

    /// Convert newly captured interleaved samples and pass on what is ready.
    pub(crate) fn feed(&mut self, raw: &[f32]) -> Result<(), String> {
        self.mono.clear();
        self.downmix.push(raw, &mut self.mono);
        let before = self.all.len();
        self.resample.push(&self.mono, &mut self.all)?;
        if self.all.len() > before {
            (self.sink)(&self.all[before..]);
        }
        Ok(())
    }

    /// Flush the resampler, pass on the tail, and return the whole recording.
    pub(crate) fn finish(mut self) -> Result<Vec<f32>, String> {
        let before = self.all.len();
        self.resample.finish(&mut self.all)?;
        if self.all.len() > before {
            (self.sink)(&self.all[before..]);
        }
        Ok(self.all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn recording_sink() -> (PcmSink, Arc<Mutex<Vec<f32>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let into = seen.clone();
        (
            Arc::new(move |c: &[f32]| into.lock().unwrap().extend_from_slice(c)),
            seen,
        )
    }

    #[test]
    fn downmixing_does_not_care_where_a_frame_was_split() {
        let mut d = Downmixer::new(2);
        let mut out = Vec::new();
        d.push(&[1.0, 3.0, 5.0], &mut out);
        d.push(&[7.0, 2.0, 4.0], &mut out);
        assert_eq!(out, vec![2.0, 6.0, 3.0]);
    }

    #[test]
    fn mono_passes_straight_through() {
        let mut d = Downmixer::new(1);
        let mut out = Vec::new();
        d.push(&[0.1, 0.2], &mut out);
        assert_eq!(out, vec![0.1, 0.2]);
    }

    #[test]
    fn what_the_sink_saw_is_exactly_what_finish_returns() {
        // The contract on AudioSource::start_streaming. The decoder relies on it
        // to know that what it decoded ahead is the recording.
        let (sink, seen) = recording_sink();
        let mut c = Converter::new(48_000, 2, sink).unwrap();
        let raw: Vec<f32> = (0..124_800)
            .map(|i| ((i / 2) as f32 * 0.01).sin())
            .collect();
        for chunk in raw.chunks(3_840) {
            c.feed(chunk).unwrap();
        }
        let all = c.finish().unwrap();
        assert_eq!(*seen.lock().unwrap(), all);
    }

    #[test]
    fn streaming_conversion_matches_converting_the_recording_at_the_end() {
        let (sink, _) = recording_sink();
        let mut c = Converter::new(48_000, 2, sink).unwrap();
        let raw: Vec<f32> = (0..96_000).map(|i| ((i / 2) as f32 * 0.02).sin()).collect();
        for chunk in raw.chunks(1_000) {
            c.feed(chunk).unwrap();
        }
        let streamed = c.finish().unwrap();

        let mut mono = Vec::new();
        Downmixer::new(2).push(&raw, &mut mono);
        assert_eq!(streamed, crate::resample::to_16k(&mono, 48_000).unwrap());
    }
}
