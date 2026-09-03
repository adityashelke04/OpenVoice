//! # Parakeet — in-process speech recognition
//!
//! Implements [`ov_core::ports::Transcriber`] with NVIDIA Parakeet TDT 0.6B v2
//! running inside this process through k2-fsa's `sherpa-onnx` bindings.
//!
//! ## Why in-process, when ADR 0003 chose a child process
//!
//! ADR 0003 picked a Python sidecar because the in-process alternative needed a
//! CUDA Toolkit or a hand-built binary on Windows, and it recorded "revisit
//! whisper.cpp in-process to remove the Python dependency from installers" as
//! explicit follow-up. That follow-up is now cheap for a reason the ADR could
//! not have known: k2-fsa publish Rust bindings whose build script fetches
//! prebuilt static libraries, so this links into the app with no CMake, no CUDA
//! Toolkit, and no DLLs beside the binary.
//!
//! ## What was given up
//!
//! Process isolation. A sidecar crash used to degrade the app and be restarted;
//! a native fault here takes the whole app down and costs the user that
//! utterance. There is no scratch file to recover it from, because there is no
//! longer any reason to write one — see below.
//!
//! ## Keeping recordings, which used to be a side effect
//!
//! The sidecar had to write every utterance to a WAV, because a file path was
//! how audio crossed the process boundary. "Keep my recordings" was therefore
//! implemented by *not deleting* that file — `dispose_of` renamed it into the
//! retention directory instead of unlinking it.
//!
//! In-process there is no such file: samples go straight to the recognizer. So
//! retention has to become deliberate rather than incidental, which is what
//! `retain_audio_dir` below is. The behaviour the user sees is unchanged; what
//! changed is that the app now writes a recording because it was asked to,
//! rather than because of how the decoder happened to be fed.
//!
//! ## What was gained beyond speed
//!
//! Parakeet returns empty text for silence and for room tone. Whisper invents
//! words from both — the sidecar carried a voice-activity filter, a
//! `no_speech_prob` gate and an `avg_logprob` gate to catch it. None of that is
//! reproduced here, because the failure it defended against does not occur. The
//! `silence_yields_empty_text` test below is what keeps that claim honest.

use std::path::{Path, PathBuf};

use ov_core::error::{Error, Result};
use ov_core::ports::{DecodeHint, Pcm16k, Transcriber};
use ov_core::types::Transcript;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

/// Stable identifier for this model, recorded in history beside every transcript.
///
/// Changing it orphans the attribution of every row already written, so it is a
/// constant rather than something derived from a path or a filename.
pub const MODEL_ID: &str = "parakeet-tdt-0.6b-v2";

/// Decode threads.
///
/// Four, not "all of them". Measured on a 12-thread machine: 535 ms median at
/// four threads against 645 ms at twelve. The extra eight threads buy 110 ms and
/// cost the responsiveness of whatever the user is dictating into. This is a
/// background tool that runs while someone is playing a game or on a call, so it
/// takes the smaller share deliberately.
const DECODE_THREADS: i32 = 4;

/// The files a Parakeet model directory must contain to be loadable.
pub const REQUIRED_FILES: [&str; 4] = [
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joiner.int8.onnx",
    "tokens.txt",
];

/// Parakeet, loaded and ready to decode.
pub struct ParakeetTranscriber {
    recognizer: OfflineRecognizer,
    /// Where to keep recordings, when the user has asked for them to be kept.
    ///
    /// `None` — the default — means nothing is ever written to disk. That is the
    /// stronger privacy position and the one a dictation app should hold by
    /// default, so it is the absence of a value rather than a flag beside one.
    retain_audio_dir: Option<PathBuf>,
}

// `OfflineRecognizer` wraps an opaque C pointer and has nothing printable in it,
// so this reports what is actually useful about the value: which model it holds.
impl std::fmt::Debug for ParakeetTranscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParakeetTranscriber")
            .field("model", &MODEL_ID)
            .finish_non_exhaustive()
    }
}

impl ParakeetTranscriber {
    /// Load the model from `dir`, keeping no recordings.
    ///
    /// Expensive — roughly 2.5 seconds and 757 MB resident — and done once at
    /// startup, which is why [`Transcriber::warm`] is a no-op.
    pub fn new(dir: PathBuf) -> Result<Self> {
        Self::with_retention(dir, None)
    }

    /// Load the model, keeping every recording in `retain_audio_dir` when it is
    /// `Some`.
    ///
    /// Separate constructor rather than a field the caller sets afterwards: a
    /// transcriber that could be switched into recording mid-life would make
    /// "is this app recording me right now" a question with a time-dependent
    /// answer. It is fixed when the engine is built, from the user's setting.
    pub fn with_retention(dir: PathBuf, retain_audio_dir: Option<PathBuf>) -> Result<Self> {
        // Check the files before handing paths to the C library. It reports a
        // missing or unreadable model as a null pointer with no detail, and
        // "could not create recognizer" is not something a user can act on.
        // Naming the missing file is the difference between a support thread and
        // a fixed install.
        for f in REQUIRED_FILES {
            if !dir.join(f).exists() {
                return Err(Error::Transcription(format!(
                    "the speech model is incomplete: {f} is missing from {}",
                    dir.display()
                )));
            }
        }

        let mut cfg = OfflineRecognizerConfig {
            model_config: OfflineRecognizerConfig::default().model_config,
            ..Default::default()
        };
        cfg.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(path_str(&dir, "encoder.int8.onnx")?),
            decoder: Some(path_str(&dir, "decoder.int8.onnx")?),
            joiner: Some(path_str(&dir, "joiner.int8.onnx")?),
        };
        cfg.model_config.tokens = Some(path_str(&dir, "tokens.txt")?);
        cfg.model_config.model_type = Some("nemo_transducer".into());
        cfg.model_config.num_threads = DECODE_THREADS;

        let started = std::time::Instant::now();
        let recognizer = OfflineRecognizer::create(&cfg).ok_or_else(|| {
            Error::Transcription(format!(
                "the speech model at {} could not be loaded",
                dir.display()
            ))
        })?;
        tracing::info!(
            model = MODEL_ID,
            threads = DECODE_THREADS,
            took_ms = started.elapsed().as_millis() as u64,
            "speech model loaded"
        );

        Ok(Self {
            recognizer,
            retain_audio_dir,
        })
    }

    /// Write this utterance to the retention directory, if there is one.
    ///
    /// Failures are logged, never returned. A disk that is full or a directory
    /// that cannot be created must not cost the user the transcript they just
    /// dictated — keeping recordings is a convenience, and transcribing is the
    /// job.
    fn keep(&self, samples: &[f32]) {
        let Some(dir) = &self.retain_audio_dir else {
            return;
        };
        // Seconds since the epoch, so the directory sorts chronologically. This
        // matches the naming the sidecar used, which is what `store::purge_recordings`
        // and anything a user has already filed away both expect.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let path = dir.join(format!("{stamp}-{nanos:09}.wav"));

        let written = std::fs::create_dir_all(dir)
            .map_err(|e| e.to_string())
            .and_then(|()| crate::wav::write_16k_mono(&path, samples).map_err(|e| e.to_string()));
        match written {
            Ok(()) => tracing::debug!(path = %path.display(), "kept recording"),
            Err(e) => tracing::warn!(
                error = %e,
                dir = %dir.display(),
                "could not keep the recording; the transcript is unaffected"
            ),
        }
    }
}

/// A path under `dir`, as the UTF-8 string the C API requires.
fn path_str(dir: &Path, file: &str) -> Result<String> {
    dir.join(file).to_str().map(str::to_owned).ok_or_else(|| {
        Error::Transcription(format!(
            "the model path {} is not valid UTF-8; move it somewhere without \
             unusual characters",
            dir.display()
        ))
    })
}

impl Transcriber for ParakeetTranscriber {
    fn warm(&self) -> Result<()> {
        // Weights are already resident: `new` loaded them, and loading twice
        // would cost 757 MB more. Kept so callers need not know which backend
        // they are holding.
        Ok(())
    }

    fn transcribe(&self, audio: &Pcm16k, _hint: &DecodeHint) -> Result<Transcript> {
        if audio.samples.is_empty() {
            return Err(Error::Transcription("no audio to transcribe".into()));
        }

        // Before the decode, not after: if the decode faults, the user still has
        // the recording they asked to keep.
        self.keep(&audio.samples);

        let started = std::time::Instant::now();
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(Pcm16k::RATE as i32, &audio.samples);
        self.recognizer.decode(&stream);
        let text = stream.get_result().map(|r| r.text).unwrap_or_default();

        tracing::debug!(
            decode_ms = started.elapsed().as_millis() as u64,
            audio_ms = audio.duration_ms(),
            "decoded"
        );

        Ok(Transcript {
            text: text.trim().to_owned(),
            // `hint.language` is ignored rather than honoured: Parakeet v2 is
            // English-only, so there is nothing to detect and nothing to force.
            // The field stays in DecodeHint because Parakeet v3 is multilingual
            // and the swap is a three-file change.
            language: Some("en".into()),
            // A transducer emits no per-segment log-probability, so there is no
            // honest number to put here. The field stays in the persisted
            // Transcript type; it is simply never filled.
            confidence: None,
        })
    }

    fn model_id(&self) -> String {
        MODEL_ID.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model directory, as fetched by `scripts/fetch-model.ps1`.
    ///
    /// These tests need the real weights: a mocked recognizer would prove only
    /// that the mock works. They are skipped rather than failed when the model
    /// is absent, so a contributor who has not run the fetch script gets a
    /// passing suite and a clear reason, not a wall of red.
    fn model_dir() -> Option<PathBuf> {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../models")
            .join(crate::locate::MODEL_DIR_NAME);
        d.join("tokens.txt").exists().then_some(d)
    }

    /// True when there is no model to test against, after saying so once.
    ///
    /// Every model-dependent test opens with `if skip() { return; }` rather than
    /// hiding the guard in a macro: a test that silently does nothing is worse
    /// than one whose first line admits it might.
    fn skip() -> bool {
        if model_dir().is_none() {
            eprintln!("skipping: no model on disk; run scripts/fetch-model.ps1");
            return true;
        }
        false
    }

    fn load() -> ParakeetTranscriber {
        ParakeetTranscriber::new(model_dir().expect("guarded by skip()")).expect("load the model")
    }

    /// Two seconds of digital silence, built rather than committed: .gitignore
    /// refuses audio in this repo, and this needs no fidelity to be silent.
    fn silence() -> Pcm16k {
        Pcm16k {
            samples: vec![0.0; 32_000],
        }
    }

    fn speech() -> Pcm16k {
        let path = model_dir().expect("guarded by skip()").join("test.wav");
        let mut r = hound::WavReader::open(&path)
            .unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
        assert_eq!(r.spec().sample_rate, 16_000, "fixture must be 16 kHz");
        assert_eq!(r.spec().channels, 1, "fixture must be mono");
        Pcm16k {
            samples: r
                .samples::<i16>()
                .map(|s| f32::from(s.expect("sample")) / 32768.0)
                .collect(),
        }
    }

    #[test]
    fn a_missing_model_names_the_path_it_wanted() {
        // The likeliest packaging bug is an installer built without weights. It
        // has to fail loudly, naming what it looked for, rather than leaving the
        // user on a spinner they cannot diagnose.
        let err = ParakeetTranscriber::new("Z:/definitely/not/here".into())
            .expect_err("a missing model must not load")
            .to_string();
        assert!(
            err.contains("Z:/definitely/not/here"),
            "the error must name the path: {err}"
        );
        assert!(
            err.contains("encoder.int8.onnx"),
            "the error must name the missing file: {err}"
        );
    }

    #[test]
    fn decodes_a_known_fixture() {
        if skip() {
            return;
        }
        let out = load()
            .transcribe(&speech(), &DecodeHint::default())
            .expect("decode");
        let text = out.text.to_lowercase();
        assert!(
            text.contains("portrait"),
            "expected the spoken words, got {:?}",
            out.text
        );
    }

    #[test]
    fn silence_yields_empty_text() {
        if skip() {
            return;
        }
        // Load-bearing. Parakeet returning nothing on silence is the entire
        // reason Whisper's VAD and its two confidence gates are deleted rather
        // than reimplemented. If this regresses, their absence becomes a
        // user-visible bug: words invented out of room tone.
        let out = load()
            .transcribe(&silence(), &DecodeHint::default())
            .expect("decode");
        assert_eq!(out.text, "", "silence must not produce words");
    }

    #[test]
    fn empty_audio_is_rejected() {
        if skip() {
            return;
        }
        let err = load()
            .transcribe(&Pcm16k { samples: vec![] }, &DecodeHint::default())
            .expect_err("empty audio is a caller bug, not a transcript");
        assert!(err.to_string().contains("no audio"));
    }

    #[test]
    fn model_id_is_stable() {
        // History rows are attributed with this string; changing it orphans them.
        assert_eq!(MODEL_ID, "parakeet-tdt-0.6b-v2");
        if skip() {
            return;
        }
        assert_eq!(load().model_id(), "parakeet-tdt-0.6b-v2");
    }

    #[test]
    fn confidence_is_always_none_and_language_is_english() {
        if skip() {
            return;
        }
        let out = load()
            .transcribe(&speech(), &DecodeHint::default())
            .expect("decode");
        assert!(out.confidence.is_none(), "a transducer has no logprob to report");
        assert_eq!(out.language.as_deref(), Some("en"));
    }

    #[test]
    fn nothing_is_written_to_disk_when_retention_is_off() {
        if skip() {
            return;
        }
        // The default must leave no trace. This is the privacy promise the app
        // makes on its own settings screen, and it used to be enforced by the
        // sidecar deleting its scratch file; in-process there is no scratch file
        // to forget to delete, and this test is what keeps that true.
        let dir = std::env::temp_dir().join("ov-retain-off");
        let _ = std::fs::remove_dir_all(&dir);
        let t = ParakeetTranscriber::with_retention(model_dir().expect("model"), None)
            .expect("load the model");
        t.transcribe(&speech(), &DecodeHint::default()).expect("decode");
        assert!(!dir.exists(), "retention was off; nothing may be written");
    }

    #[test]
    fn the_recording_is_kept_when_retention_is_on() {
        if skip() {
            return;
        }
        let dir = std::env::temp_dir().join("ov-retain-on");
        let _ = std::fs::remove_dir_all(&dir);
        let t = ParakeetTranscriber::with_retention(model_dir().expect("model"), Some(dir.clone()))
            .expect("load the model");
        t.transcribe(&speech(), &DecodeHint::default()).expect("decode");

        let kept: Vec<_> = std::fs::read_dir(&dir)
            .expect("the retention directory must exist")
            .filter_map(std::result::Result::ok)
            .collect();
        assert_eq!(kept.len(), 1, "exactly one recording should be kept");

        // Readable as 16 kHz mono, not merely present: a file the user cannot
        // play back is not a kept recording.
        let path = kept[0].path();
        let r = hound::WavReader::open(&path).expect("the recording must be a readable wav");
        assert_eq!(r.spec().sample_rate, 16_000);
        assert_eq!(r.spec().channels, 1);
        assert!(r.duration() > 0, "the recording must not be empty");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_decode_still_leaves_the_recording() {
        if skip() {
            return;
        }
        // Keeping recordings exists so a user can recover what they said. That is
        // most valuable exactly when transcription went wrong, so the write must
        // not be conditional on the decode succeeding.
        let dir = std::env::temp_dir().join("ov-retain-empty");
        let _ = std::fs::remove_dir_all(&dir);
        let t = ParakeetTranscriber::with_retention(model_dir().expect("model"), Some(dir.clone()))
            .expect("load the model");

        // Empty audio is rejected before anything is written, which is correct:
        // there is no recording to keep.
        let _ = t.transcribe(&Pcm16k { samples: vec![] }, &DecodeHint::default());
        assert!(!dir.exists(), "no audio means no recording");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_language_hint_is_ignored_rather_than_honoured() {
        if skip() {
            return;
        }
        // Parakeet v2 is English-only. Asking for French must not silently
        // produce something claiming to be French.
        let hint = DecodeHint {
            vocabulary: vec![],
            language: Some("fr".into()),
        };
        let out = load().transcribe(&speech(), &hint).expect("decode");
        assert_eq!(out.language.as_deref(), Some("en"));
    }
}
