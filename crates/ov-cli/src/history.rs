//! Local history, and reading wav fixtures.
//!
//! History is newline-delimited JSON in `%APPDATA%\OpenVoice\history.jsonl`.
//!
//! The architecture calls for SQLite with full-text search, and the GUI will use it.
//! A JSONL file is the right thing for the command-line build: it needs no schema
//! migration, it is trivially greppable, and — the point — it stores `raw_text`
//! alongside `final_text` from the very first session. That pairing is what lets the
//! whole history be replayed through a new formatter version and diffed before the
//! change ships, which is the engine of improving this tool day by day.

use std::io::Write;
use std::path::{Path, PathBuf};

use ov_core::session::SessionRecord;
use serde::Serialize;

#[derive(Serialize)]
struct Row<'a> {
    created_at: u64,
    outcome: &'a str,
    raw_text: &'a str,
    final_text: &'a str,
    profile: &'a str,
    target_app: &'a str,
    window_title: &'a str,
    audio_ms: u64,
    latency_ms: u64,
}

/// Directory holding OpenVoice's user data.
#[must_use]
pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenVoice")
}

/// Append one completed session.
pub fn append(record: &SessionRecord) -> Result<(), String> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;

    let row = Row {
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        outcome: record.outcome.code(),
        raw_text: &record.raw_text,
        final_text: &record.final_text,
        profile: &record.profile,
        target_app: &record.app.exe,
        window_title: &record.app.title,
        audio_ms: record.audio_ms,
        latency_ms: record.latency_ms,
    };

    let line = serde_json::to_string(&row).map_err(|e| format!("encoding history row: {e}"))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("history.jsonl"))
        .map_err(|e| format!("opening history: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("writing history: {e}"))
}

/// Read a wav file as mono `f32` at 16 kHz, for `ov transcribe`.
///
/// Rejects rather than resamples a file at the wrong rate: this path exists to test
/// the pipeline against known fixtures, and silently accepting a mismatched rate
/// would make a bad transcript look like a model problem.
pub fn read_wav_16k(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| format!("opening {}: {e}", path.display()))?;
    let spec = reader.spec();

    if spec.sample_rate != 16_000 {
        return Err(format!(
            "{} is {} Hz; this command expects 16 kHz mono (the daemon resamples, \
             this path does not)",
            path.display(),
            spec.sample_rate
        ));
    }

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .map(|s| s.map(|v| v as f32 / f32::from(i16::MAX)))
            .collect::<Result<_, _>>()
            .map_err(|e| format!("reading samples: {e}"))?,
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| format!("reading samples: {e}"))?,
    };

    if spec.channels <= 1 {
        return Ok(samples);
    }
    let ch = spec.channels as usize;
    Ok(samples
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect())
}
