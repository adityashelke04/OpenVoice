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
    fn the_corpus_loads_as_16k_clips_with_references() {
        let root = default_root();
        if !root.is_dir() {
            eprintln!(
                "skipping: no corpus at {}; run scripts/fetch-bench-set.ps1",
                root.display()
            );
            return;
        }
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
