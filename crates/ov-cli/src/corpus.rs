//! LibriSpeech `test-clean`, the corpus ADR 0008 measured Parakeet on.
//!
//! Real read speech with human reference transcripts, so a speed change can be
//! checked for what it costs in accuracy rather than assumed free. Downloaded by
//! `scripts/fetch-bench-set.ps1` into `fixtures/librispeech`, which git ignores:
//! it is 346 MB and CC BY 4.0, and neither belongs in this repository.

use std::path::{Path, PathBuf};

/// One utterance and what was actually said.
#[derive(Debug, Clone)]
pub struct Clip {
    /// LibriSpeech utterance id, or ids joined with `+` for a long-form clip.
    pub id: String,
    /// Reference transcript.
    pub reference: String,
    /// 16 kHz mono samples.
    pub samples: Vec<f32>,
}

/// Approved local fixture with explicit provenance and sensitive terms.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fixture {
    pub id: String,
    pub path: PathBuf,
    pub sha256: String,
    pub reference: String,
    pub categories: Vec<String>,
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub numbers: Vec<String>,
}

/// Manifest is deliberately local: private clips are never fetched or uploaded.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub approved: bool,
    pub personal: bool,
    pub license: String,
    pub source: String,
    pub clips: Vec<Fixture>,
}

/// Categories required before a run can claim dictation quality coverage.
pub const CATEGORIES: &[&str] = &[
    "onset",
    "low-volume",
    "noise",
    "names",
    "numbers",
    "code",
    "punctuation",
    "no-pause-release",
    "forced-cut",
    "continuous",
];

/// Load only explicitly approved fixtures, verify bytes before decoding them.
pub fn load_manifest(path: &Path, allow_personal: bool) -> Result<(Manifest, Vec<Clip>), String> {
    use sha2::{Digest, Sha256};
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|e| format!("invalid quality manifest: {e}"))?;
    if manifest.version != 1 || !manifest.approved || manifest.clips.is_empty() {
        return Err("quality requires an approved version-1 manifest with real clips; no fixtures is not a pass".into());
    }
    if manifest.personal && !allow_personal {
        return Err(
            "personal corpus requires explicit --allow-personal opt-in; nothing was read".into(),
        );
    }
    if manifest.license.trim().is_empty() || manifest.source.trim().is_empty() {
        return Err("fixture license and source must be recorded".into());
    }
    for category in CATEGORIES {
        if !manifest
            .clips
            .iter()
            .any(|c| c.categories.iter().any(|s| s == category))
        {
            return Err(format!("missing dictation quality category: {category}"));
        }
    }
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let mut ids = std::collections::HashSet::new();
    let mut clips = Vec::new();
    for fixture in &manifest.clips {
        if !ids.insert(&fixture.id)
            || fixture.id.trim().is_empty()
            || fixture.reference.trim().is_empty()
        {
            return Err("fixture IDs must be unique and references nonempty".into());
        }
        for (category, terms) in [("names", &fixture.names), ("numbers", &fixture.numbers)] {
            if fixture.categories.iter().any(|s| s == category) && terms.is_empty() {
                return Err(format!("{} needs annotated {category}", fixture.id));
            }
            if crate::wer::protected_errors(terms, &fixture.reference) != 0 {
                return Err(format!(
                    "{} has sensitive terms absent from its reference",
                    fixture.id
                ));
            }
        }
        let audio_path = root.join(&fixture.path);
        let bytes = std::fs::read(&audio_path)
            .map_err(|e| format!("missing quality fixture {}: {e}", audio_path.display()))?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != fixture.sha256 {
            return Err(format!("fixture {} SHA-256 mismatch", fixture.id));
        }
        let samples = if audio_path.extension().and_then(|e| e.to_str()) == Some("flac") {
            read_flac_16k(&audio_path)?
        } else {
            let mut reader =
                hound::WavReader::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
            let spec = reader.spec();
            if spec.channels != 1
                || spec.sample_rate != 16_000
                || spec.bits_per_sample != 16
                || spec.sample_format != hound::SampleFormat::Int
            {
                return Err("quality WAV must be signed PCM16, 16kHz mono".into());
            }
            reader
                .samples::<i16>()
                .map(|sample| {
                    sample
                        .map(|v| f32::from(v) / 32768.0)
                        .map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        if samples.is_empty() {
            return Err(format!("fixture {} has no audio", fixture.id));
        }
        if fixture.categories.iter().any(|c| c == "forced-cut") && samples.len() <= 20 * 16_000 {
            return Err("forced-cut fixture must exceed the decoder's 20-second boundary".into());
        }
        clips.push(Clip {
            id: fixture.id.clone(),
            reference: fixture.reference.clone(),
            samples,
        });
    }
    Ok((manifest, clips))
}

/// Explicit weights requirement for quality jobs; never resolves an unrelated user model.
pub fn require_weights(dir: &Path, files: &[&str]) -> Result<(), String> {
    for file in files {
        if !dir.join(file).is_file() {
            return Err(format!(
                "missing required quality model weight: {}",
                dir.join(file).display()
            ));
        }
    }
    Ok(())
}

/// Where the fetch script puts the corpus.
#[must_use]
pub fn default_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/librispeech/LibriSpeech/test-clean")
}

/// `<id> <WORDS...>` lines from a `.trans.txt` file.
#[must_use]
pub fn parse_transcripts(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (id, words) = line.trim().split_once(' ')?;
            Some((id.to_owned(), words.trim().to_owned()))
        })
        .collect()
}

/// Join clips into one long-form clip, with `gap_ms` of silence between them.
///
/// Dictation of a paragraph is several sentences with pauses between them. This
/// builds that shape from real speech, which is what incremental decoding has
/// to cut correctly.
#[must_use]
pub fn concat(clips: &[Clip], gap_ms: u64) -> Clip {
    let gap = vec![0.0; (gap_ms * 16) as usize];
    let mut samples = Vec::new();
    for (i, clip) in clips.iter().enumerate() {
        if i > 0 {
            samples.extend_from_slice(&gap);
        }
        samples.extend_from_slice(&clip.samples);
    }
    Clip {
        id: clips
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>()
            .join("+"),
        reference: clips
            .iter()
            .map(|c| c.reference.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        samples,
    }
}

/// Decode a 16 kHz mono FLAC file to `f32` samples.
pub fn read_flac_16k(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader =
        claxon::FlacReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let info = reader.streaminfo();
    if info.sample_rate != 16_000 || info.channels != 1 {
        return Err(format!(
            "{}: expected 16 kHz mono, found {} Hz x{}",
            path.display(),
            info.sample_rate,
            info.channels
        ));
    }
    let scale = (1u32 << (info.bits_per_sample - 1)) as f32;
    reader
        .samples()
        .map(|s| {
            s.map(|v| v as f32 / scale)
                .map_err(|e| format!("{}: {e}", path.display()))
        })
        .collect()
}

/// The first `limit` utterances between `min_ms` and `max_ms`, in id order.
///
/// Sorted, so every run of the benchmark decodes the same clips and two runs
/// can be compared.
pub fn load_librispeech(
    root: &Path,
    min_ms: u64,
    max_ms: u64,
    limit: usize,
) -> Result<Vec<Clip>, String> {
    if !root.is_dir() {
        return Err(format!(
            "no corpus at {}; run scripts/fetch-bench-set.ps1",
            root.display()
        ));
    }
    let mut listed = Vec::new();
    for speaker in sorted_dirs(root)? {
        for chapter in sorted_dirs(&speaker)? {
            let name = format!("{}-{}.trans.txt", leaf(&speaker), leaf(&chapter));
            let text =
                std::fs::read_to_string(chapter.join(&name)).map_err(|e| format!("{name}: {e}"))?;
            for (id, reference) in parse_transcripts(&text) {
                listed.push((chapter.join(format!("{id}.flac")), id, reference));
            }
        }
    }
    let mut clips = Vec::new();
    for (path, id, reference) in listed {
        if clips.len() == limit {
            break;
        }
        let samples = read_flac_16k(&path)?;
        let ms = samples.len() as u64 * 1_000 / 16_000;
        if (min_ms..=max_ms).contains(&ms) {
            clips.push(Clip {
                id,
                reference,
                samples,
            });
        }
    }
    if clips.len() < limit || clips.is_empty() {
        return Err(format!(
            "quality corpus needs {limit} matching clips, found {}",
            clips.len()
        ));
    }
    Ok(clips)
}

fn sorted_dirs(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    Ok(dirs)
}

fn leaf(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: &str, reference: &str, n: usize) -> Clip {
        Clip {
            id: id.into(),
            reference: reference.into(),
            samples: vec![0.5; n],
        }
    }

    #[test]
    fn empty_corpus_is_not_quality_success() {
        let root = std::env::temp_dir().join(format!("ov-empty-corpus-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(load_librispeech(&root, 2_000, 12_000, 3).is_err());
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn missing_weights_and_missing_manifest_fail_quality() {
        let missing = std::env::temp_dir().join("ov-no-quality-assets-ever");
        assert!(require_weights(&missing, &["encoder.onnx"]).is_err());
        assert!(load_manifest(&missing.join("manifest.json"), false).is_err());
    }

    #[test]
    fn unavailable_or_personal_fixtures_do_not_silently_pass() {
        let root = std::env::temp_dir().join(format!("ov-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("manifest.json");
        let mut manifest = serde_json::json!({"version":1,"approved":true,"personal":false,"license":"test-only synthetic fixture","source":"unit test","clips":[]});
        std::fs::write(&path, manifest.to_string()).unwrap();
        assert!(load_manifest(&path, false)
            .unwrap_err()
            .contains("no fixtures"));
        manifest["clips"] = serde_json::json!([{"id":"unit-only","path":"absent.wav","sha256":"0".repeat(64),"reference":"Aditya -12", "categories":CATEGORIES, "names":["Aditya"],"numbers":["-12"]}]);
        std::fs::write(&path, manifest.to_string()).unwrap();
        assert!(load_manifest(&path, false)
            .unwrap_err()
            .contains("missing quality fixture"));
        manifest["personal"] = true.into();
        std::fs::write(&path, manifest.to_string()).unwrap();
        assert!(load_manifest(&path, false).unwrap_err().contains("opt-in"));
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn manifest_checks_actual_wav_bytes_before_loading_samples() {
        use sha2::{Digest, Sha256};
        let root = std::env::temp_dir().join(format!("ov-manifest-wav-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let audio = root.join("synthetic.wav");
        let mut writer = hound::WavWriter::create(
            &audio,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..320_001 {
            writer.write_sample::<i16>(0).unwrap();
        }
        writer.finalize().unwrap();
        let digest = format!("{:x}", Sha256::digest(std::fs::read(&audio).unwrap()));
        let mut manifest = serde_json::json!({"version":1,"approved":true,"personal":false,"license":"synthetic unit test only","source":"generated silence; not speech accuracy evidence","clips":[{"id":"test","path":"synthetic.wav","sha256":digest,"reference":"Ann paid -12.5", "categories":CATEGORIES,"names":["Ann"],"numbers":["-12.5"]}]});
        let path = root.join("manifest.json");
        std::fs::write(&path, manifest.to_string()).unwrap();
        let (_, clips) = load_manifest(&path, false).unwrap();
        assert_eq!(clips[0].samples.len(), 320_001);
        manifest["clips"][0]["sha256"] = "0".repeat(64).into();
        std::fs::write(&path, manifest.to_string()).unwrap();
        assert!(load_manifest(&path, false)
            .unwrap_err()
            .contains("SHA-256 mismatch"));
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(audio).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn transcript_lines_split_into_id_and_words() {
        let parsed = parse_transcripts(
            "1089-134686-0000 HE HOPED THERE WOULD BE STEW\n\n1089-134686-0001 STUFF IT\n",
        );
        assert_eq!(
            parsed,
            vec![
                (
                    "1089-134686-0000".to_owned(),
                    "HE HOPED THERE WOULD BE STEW".to_owned()
                ),
                ("1089-134686-0001".to_owned(), "STUFF IT".to_owned()),
            ]
        );
    }

    #[test]
    fn concatenation_puts_silence_between_clips_but_not_after_the_last() {
        let long = concat(&[clip("a", "ONE", 1_600), clip("b", "TWO", 800)], 100);
        assert_eq!(long.samples.len(), 1_600 + 1_600 + 800);
        assert!(long.samples[1_600..3_200].iter().all(|&s| s == 0.0));
        assert_eq!(long.reference, "ONE TWO");
        assert_eq!(long.id, "a+b");
    }

    #[test]
    #[ignore = "requires deliberately fetched real LibriSpeech assets; quality command never skips missing assets"]
    fn the_corpus_loads_as_16k_clips_with_references() {
        let root = default_root();
        let clips = load_librispeech(&root, 2_000, 12_000, 3).expect("load the corpus");
        assert_eq!(clips.len(), 3);
        for c in &clips {
            let ms = c.samples.len() as u64 * 1_000 / 16_000;
            assert!((2_000..=12_000).contains(&ms), "{} is {ms} ms", c.id);
            assert!(!c.reference.is_empty());
            assert!(c.samples.iter().all(|s| (-1.0..=1.0).contains(s)));
        }
    }
}
