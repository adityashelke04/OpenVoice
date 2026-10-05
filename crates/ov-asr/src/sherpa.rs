//! # In-process speech recognition
//!
//! Implements [`ov_core::ports::Transcriber`] for any model in
//! [`crate::catalog`], running inside this process through k2-fsa's
//! `sherpa-onnx` bindings.
//!
//! ## One loader, two model shapes
//!
//! sherpa-onnx configures a transducer and a Whisper model through different
//! structs, so the loader behind [`SherpaTranscriber::with_threads`] matches on
//! [`crate::catalog::ModelKind`] to fill in the right one. Everything after
//! `OfflineRecognizer::create` is identical: the same stream, the same decode,
//! the same result. That is why adding a model is a catalogue entry rather than
//! a code change.
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
//! retention has to become deliberate rather than incidental. The behaviour the
//! user sees is unchanged; what changed is that the app now writes a recording
//! because it was asked to, rather than because of how the decoder happened to
//! be fed -- which is what `ov_asr::recordings::keep` is, called by the engine
//! once per session. The transcriber itself never touches the disk.
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
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig,
    OfflineWhisperModelConfig,
};

use crate::catalog::{ModelKind, ModelSpec};

/// Decode threads, unless a caller measured a better number for its machine.
///
/// Four, not "all of them". Measured on a 12-thread machine: 535 ms median at
/// four threads against 645 ms at twelve. The extra eight threads buy 110 ms and
/// cost the responsiveness of whatever the user is dictating into. This is a
/// background tool that runs while someone is playing a game or on a call, so it
/// takes the smaller share deliberately. `ov bench --threads` is how to revisit it.
pub const DEFAULT_DECODE_THREADS: i32 = 4;

/// How hard the decoder leans toward a hotword, in sherpa-onnx's score units.
///
/// Measured on Parakeet v2 against 150 LibriSpeech clips (2,563 words) and 16
/// synthesized dictations of coding-tool names:
///
/// | score | tool-name errors | everyday English errors |
/// |------:|-----------------:|------------------------:|
/// | none  |              14% |                    1.4% |
/// | 1.5   |             2.5% |                    1.4% |
/// | 2.0   |             0.8% |                    1.7% |
///
/// 2.0 is a little better on the names and starts costing ordinary speech, which
/// is the wrong trade for a tool used mostly for ordinary speech.
pub const HOTWORD_SCORE: f32 = 1.5;

/// Most terms offered to one decode. The score applies per term, so a long list
/// stops being a hint and starts being a pull on every word.
pub const MAX_HOTWORDS: usize = 64;

/// Audio quieter than this is never decoded with hotwords.
///
/// Measured: with hotwords on, two seconds of digital silence came back as
/// "Claude" twelve times. A model that is told a word is likely will say it
/// into nothing. Plain decoding returns empty text for silence, and that is the
/// property the app leans on instead of a voice-activity filter.
const MIN_HOTWORD_RMS: f32 = 0.004;

/// The vocabulary as sherpa-onnx wants it: terms separated by `/`.
///
/// A term holding `/` would split in two and one holding `:` would be read as a
/// score, so those are dropped rather than quietly changing meaning.
fn hotwords_arg(terms: &[String]) -> Option<String> {
    let mut kept: Vec<&str> = Vec::new();
    for t in terms {
        let t = t.trim();
        if t.is_empty() || t.contains('/') || t.contains(':') || kept.contains(&t) {
            continue;
        }
        kept.push(t);
        if kept.len() == MAX_HOTWORDS {
            break;
        }
    }
    (!kept.is_empty()).then(|| kept.join("/"))
}

/// Whether `text` says one word three times running.
///
/// Nobody dictates that on purpose, and it is what a decoder biased toward a
/// word produces from noise. Used to throw a boosted decode away.
fn repeats_a_word(text: &str) -> bool {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect();
    words
        .windows(3)
        .any(|w| !w[0].is_empty() && w[0] == w[1] && w[1] == w[2])
}

/// The token list rewritten as the vocabulary file hotwords are encoded with.
///
/// The release ships `tokens.txt` and no `bpe.vocab`. sherpa-onnx's own Parakeet
/// hotword example derives one this way: every token at an equal score, which
/// makes the encoder prefer the longest match.
fn bpe_vocab_from_tokens(tokens: &str) -> String {
    tokens
        .lines()
        .filter_map(|l| l.split(' ').next().filter(|t| !t.is_empty()))
        .map(|t| format!("{t}\t-1.0\n"))
        .collect()
}

/// Whether `samples` is loud enough to be speech rather than silence or hiss.
fn has_speech_energy(samples: &[f32]) -> bool {
    if samples.is_empty() {
        return false;
    }
    let mean_square = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    mean_square.sqrt() >= MIN_HOTWORD_RMS
}

/// A loaded model, ready to decode.
pub struct SherpaTranscriber {
    /// Which catalogue entry this is. Carried so `model_id` reports the id that
    /// history rows are attributed with, rather than something derived from a
    /// directory name a user could rename.
    spec: &'static ModelSpec,
    recognizer: OfflineRecognizer,
    /// Whether the recognizer was built to accept hotwords. False for a model
    /// that was not measured with them, and when the vocabulary file they need
    /// could not be written -- in which case dictation still works, unboosted.
    hotwords: bool,
}

// `OfflineRecognizer` wraps an opaque C pointer and has nothing printable in it,
// so this reports what is actually useful about the value: which model it holds.
impl std::fmt::Debug for SherpaTranscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SherpaTranscriber")
            .field("model", &self.spec.id)
            .finish_non_exhaustive()
    }
}

impl SherpaTranscriber {
    /// Load `spec` from `dir`.
    ///
    /// Expensive — two to three seconds, and up to 750 MB resident — and done
    /// once at startup, which is why [`Transcriber::warm`] is a no-op.
    pub fn new(spec: &'static ModelSpec, dir: PathBuf) -> Result<Self> {
        Self::with_threads(spec, dir, DEFAULT_DECODE_THREADS)
    }

    /// Load the model decoding on `threads` threads.
    ///
    /// For measuring. The app uses [`SherpaTranscriber::new`]; `ov bench`
    /// uses this to find out whether a different count is worth shipping.
    pub fn with_threads(spec: &'static ModelSpec, dir: PathBuf, threads: i32) -> Result<Self> {
        if threads < 1 {
            return Err(Error::Transcription(format!(
                "at least one decode thread is needed, not {threads}"
            )));
        }
        Self::load(spec, dir, threads)
    }

    fn load(spec: &'static ModelSpec, dir: PathBuf, threads: i32) -> Result<Self> {
        // Check the files before handing paths to the C library. It reports a
        // missing or unreadable model as a null pointer with no detail, and
        // "could not create recognizer" is not something a user can act on.
        // Naming the missing file is the difference between a support thread and
        // a fixed install.
        for f in spec.files {
            if !dir.join(f).exists() {
                return Err(Error::Transcription(format!(
                    "the {} model is incomplete: {f} is missing from {}",
                    spec.id,
                    dir.display()
                )));
            }
        }

        let mut cfg = OfflineRecognizerConfig::default();
        match spec.kind {
            ModelKind::Transducer => {
                cfg.model_config.transducer = OfflineTransducerModelConfig {
                    encoder: Some(path_str(&dir, "encoder.int8.onnx")?),
                    decoder: Some(path_str(&dir, "decoder.int8.onnx")?),
                    joiner: Some(path_str(&dir, "joiner.int8.onnx")?),
                };
                cfg.model_config.tokens = Some(path_str(&dir, "tokens.txt")?);
                cfg.model_config.model_type = Some("nemo_transducer".into());
            }
            ModelKind::Whisper => {
                cfg.model_config.whisper = OfflineWhisperModelConfig {
                    encoder: Some(path_str(&dir, "tiny.en-encoder.int8.onnx")?),
                    decoder: Some(path_str(&dir, "tiny.en-decoder.int8.onnx")?),
                    language: Some("en".into()),
                    task: Some("transcribe".into()),
                    // -1 lets sherpa-onnx decide. Whisper is trained on
                    // 30-second windows and pads short audio itself; overriding
                    // this was not measured, so it is not overridden.
                    tail_paddings: -1,
                    enable_token_timestamps: false,
                    enable_segment_timestamps: false,
                };
                cfg.model_config.tokens = Some(path_str(&dir, "tiny.en-tokens.txt")?);
                cfg.model_config.model_type = Some("whisper".into());
            }
        }
        cfg.model_config.num_threads = threads;
        let hotwords = spec.hotwords && enable_hotwords(&mut cfg, &dir, spec);

        let started = std::time::Instant::now();
        let recognizer = OfflineRecognizer::create(&cfg).ok_or_else(|| {
            Error::Transcription(format!(
                "the {} model at {} could not be loaded",
                spec.id,
                dir.display()
            ))
        })?;
        tracing::info!(
            model = spec.id,
            threads,
            took_ms = started.elapsed().as_millis() as u64,
            "speech model loaded"
        );

        Ok(Self {
            spec,
            recognizer,
            hotwords,
        })
    }
}

/// Switch `cfg` to beam search with hotwords. Returns false, leaving `cfg` as it
/// was, if the vocabulary file they need cannot be made.
///
/// The file goes in the temp directory rather than beside the model: the model
/// lives under the install directory, which a normal user cannot write to.
fn enable_hotwords(cfg: &mut OfflineRecognizerConfig, dir: &Path, spec: &ModelSpec) -> bool {
    let vocab = std::fs::read_to_string(dir.join("tokens.txt"))
        .map(|t| bpe_vocab_from_tokens(&t))
        .and_then(|v| {
            let path = std::env::temp_dir().join(format!("openvoice-{}.bpe.vocab", spec.id));
            std::fs::write(&path, v).map(|()| path)
        });
    let Some(path) = vocab.ok().and_then(|p| p.to_str().map(str::to_owned)) else {
        tracing::warn!(
            model = spec.id,
            "hotword vocabulary could not be written; decoding without hotwords"
        );
        return false;
    };
    cfg.model_config.modeling_unit = Some("bpe".into());
    cfg.model_config.bpe_vocab = Some(path);
    cfg.decoding_method = Some("modified_beam_search".into());
    cfg.hotwords_score = HOTWORD_SCORE;
    true
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

impl SherpaTranscriber {
    /// One decode, optionally leaning toward `hotwords`.
    fn decode(&self, samples: &[f32], hotwords: Option<&str>) -> String {
        let stream = match hotwords {
            Some(h) => self.recognizer.create_stream_with_hotwords(h),
            None => self.recognizer.create_stream(),
        };
        stream.accept_waveform(Pcm16k::RATE as i32, samples);
        self.recognizer.decode(&stream);
        stream.get_result().map(|r| r.text).unwrap_or_default()
    }
}

impl Transcriber for SherpaTranscriber {
    fn warm(&self) -> Result<()> {
        // Weights are already resident: `new` loaded them, and loading twice
        // would cost 757 MB more. Kept so callers need not know which backend
        // they are holding.
        Ok(())
    }

    fn transcribe(&self, audio: &Pcm16k, hint: &DecodeHint) -> Result<Transcript> {
        if audio.samples.is_empty() {
            return Err(Error::Transcription("no audio to transcribe".into()));
        }

        let started = std::time::Instant::now();
        let boost = (self.hotwords && has_speech_energy(&audio.samples))
            .then(|| hotwords_arg(&hint.vocabulary))
            .flatten();
        let mut text = self.decode(&audio.samples, boost.as_deref());
        if boost.is_some() && repeats_a_word(&text) {
            tracing::warn!(%text, "boosted decode repeated a word; decoding without hotwords");
            text = self.decode(&audio.samples, None);
        }

        tracing::debug!(
            decode_ms = started.elapsed().as_millis() as u64,
            audio_ms = audio.duration_ms(),
            "decoded"
        );

        Ok(Transcript {
            text: text.trim().to_owned(),
            // Reported, not forced. An English-only model can honestly say
            // "en"; a multilingual one detects per utterance and sherpa-onnx
            // does not surface which it chose, so naming a language there would
            // put a guess into history. None is the truthful answer.
            language: self.spec.english_only.then(|| "en".to_owned()),
            // A transducer emits no per-segment log-probability, so there is no
            // honest number to put here. The field stays in the persisted
            // Transcript type; it is simply never filled.
            confidence: None,
        })
    }

    fn model_id(&self) -> String {
        self.spec.id.to_owned()
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
            .join(crate::catalog::DEFAULT_MODEL);
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

    /// The bundled model's spec, which every model-dependent test loads.
    fn bundled() -> &'static ModelSpec {
        crate::catalog::default_spec()
    }

    fn load() -> SherpaTranscriber {
        SherpaTranscriber::new(bundled(), model_dir().expect("guarded by skip()"))
            .expect("load the model")
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
    fn hotwords_are_joined_with_slashes_and_cleaned() {
        let v: Vec<String> = [" Claude Code ", "", "a/b", "c:d", "Codex", "Codex"]
            .map(String::from)
            .into();
        // sherpa-onnx splits on `/` and reads `:` as a score, so a term holding
        // either would silently become two terms or a boost it never asked for.
        assert_eq!(hotwords_arg(&v).as_deref(), Some("Claude Code/Codex"));
        assert_eq!(hotwords_arg(&[]), None);
        assert_eq!(hotwords_arg(&[String::new()]), None);
    }

    #[test]
    fn hotwords_are_capped() {
        let v: Vec<String> = (0..MAX_HOTWORDS + 20).map(|i| format!("term{i}")).collect();
        let arg = hotwords_arg(&v).expect("some terms");
        assert_eq!(arg.split('/').count(), MAX_HOTWORDS);
    }

    #[test]
    fn a_word_said_three_times_running_is_a_hallucination() {
        assert!(repeats_a_word("Claude Claude Claude"));
        assert!(repeats_a_word("so Claude, Claude, claude."));
        assert!(!repeats_a_word("Claude Claude"));
        assert!(!repeats_a_word("that that is odd"));
        assert!(!repeats_a_word(""));
    }

    #[test]
    fn the_vocabulary_for_hotwords_is_derived_from_tokens() {
        let tokens = "<unk> 0\n\u{2581}t 1\n\n\u{2581}th 2\n";
        assert_eq!(
            bpe_vocab_from_tokens(tokens),
            "<unk>\t-1.0\n\u{2581}t\t-1.0\n\u{2581}th\t-1.0\n"
        );
    }

    #[test]
    fn quiet_audio_is_not_trusted_with_hotwords() {
        assert!(!has_speech_energy(&vec![0.0; 16_000]));
        assert!(!has_speech_energy(&vec![0.001; 16_000]));
        assert!(has_speech_energy(&vec![0.1; 16_000]));
        assert!(!has_speech_energy(&[]));
    }

    /// Deterministic noise, so a failure is reproducible.
    fn noise(len: usize, amplitude: f32) -> Pcm16k {
        let mut x: u32 = 0x1234_5678;
        Pcm16k {
            samples: (0..len)
                .map(|_| {
                    x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    ((x >> 8) as f32 / 8_388_608.0 - 1.0) * amplitude
                })
                .collect(),
        }
    }

    fn names() -> DecodeHint {
        DecodeHint {
            vocabulary: ["Claude Code", "Codex", "Docling", "PaddleOCR", "Claude"]
                .map(String::from)
                .into(),
            language: None,
        }
    }

    #[test]
    fn silence_with_a_vocabulary_still_yields_empty_text() {
        if skip() {
            return;
        }
        // Measured: with hotwords on and no guard, two seconds of silence came
        // back as "Claude Claude Claude Claude ...". Silence must stay empty.
        let out = load().transcribe(&silence(), &names()).expect("decode");
        assert_eq!(out.text, "", "silence must not produce words");
    }

    #[test]
    fn noise_with_a_vocabulary_does_not_invent_the_vocabulary() {
        if skip() {
            return;
        }
        let t = load();
        for amplitude in [0.002, 0.02, 0.2] {
            let out = t
                .transcribe(&noise(48_000, amplitude), &names())
                .expect("decode");
            let text = out.text.to_lowercase();
            assert!(
                !text.contains("claude") && !text.contains("codex") && !text.contains("docling"),
                "noise at {amplitude} produced {:?}",
                out.text
            );
        }
    }

    #[test]
    fn a_vocabulary_does_not_change_what_ordinary_speech_says() {
        if skip() {
            return;
        }
        let t = load();
        let plain = t
            .transcribe(&speech(), &DecodeHint::default())
            .expect("decode");
        let boosted = t.transcribe(&speech(), &names()).expect("decode");
        assert_eq!(plain.text, boosted.text);
    }

    #[test]
    fn only_the_verified_model_decodes_with_hotwords() {
        // Upstream reports empty or invented text about one time in five when
        // beam search runs on Parakeet v3 (k2-fsa/sherpa-onnx#3267). v2 was
        // measured here; v3 has not been, so it stays on greedy search.
        assert!(bundled().hotwords);
        for spec in crate::catalog::CATALOG
            .iter()
            .filter(|m| m.id != bundled().id)
        {
            assert!(
                !spec.hotwords,
                "{} was never measured with hotwords",
                spec.id
            );
        }
    }

    #[test]
    fn a_missing_model_names_the_path_it_wanted() {
        // The likeliest packaging bug is an installer built without weights. It
        // has to fail loudly, naming what it looked for, rather than leaving the
        // user on a spinner they cannot diagnose.
        let err = SherpaTranscriber::new(bundled(), "Z:/definitely/not/here".into())
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
    fn zero_decode_threads_is_refused_before_anything_loads() {
        // Checked before the files, so this needs no model on disk.
        let err = SherpaTranscriber::with_threads(bundled(), "Z:/nope".into(), 0)
            .expect_err("zero threads must be refused")
            .to_string();
        assert!(err.contains("at least one decode thread"), "{err}");
    }

    #[test]
    fn a_non_default_thread_count_still_decodes() {
        if skip() {
            return;
        }
        let t = SherpaTranscriber::with_threads(bundled(), model_dir().expect("model"), 6)
            .expect("load at six threads");
        let out = t
            .transcribe(&speech(), &DecodeHint::default())
            .expect("decode");
        assert!(
            out.text.to_lowercase().contains("portrait"),
            "{:?}",
            out.text
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
        assert_eq!(bundled().id, "parakeet-tdt-0.6b-v2");
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
        assert!(
            out.confidence.is_none(),
            "a transducer has no logprob to report"
        );
        assert_eq!(out.language.as_deref(), Some("en"));
    }

    #[test]
    fn a_whisper_spec_names_its_own_missing_file() {
        // The two model kinds have different file names, and the error must come
        // from the spec rather than a hardcoded Parakeet list -- otherwise a
        // missing Whisper model would complain about encoder.int8.onnx, a file
        // it never had.
        let spec = crate::catalog::find("whisper-tiny.en").expect("in catalogue");
        let err = SherpaTranscriber::new(spec, "Z:/nope".into())
            .expect_err("must not load")
            .to_string();
        assert!(err.contains("Z:/nope"), "must name the path: {err}");
        assert!(
            err.contains("tiny.en-encoder.int8.onnx"),
            "must name the file this model actually needs: {err}"
        );
    }

    #[test]
    fn a_multilingual_model_reports_no_language() {
        // v3 detects per utterance and sherpa-onnx does not report which it
        // chose. Writing "en" into history for it would be a guess recorded as
        // fact, so the field stays empty.
        let spec = crate::catalog::find("parakeet-tdt-0.6b-v3").expect("in catalogue");
        assert!(!spec.english_only);
        assert!(
            bundled().english_only,
            "the bundled model can honestly say en"
        );
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
