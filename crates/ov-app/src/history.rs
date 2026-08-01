//! Local history.
//!
//! Newline-delimited JSON in `%APPDATA%\OpenVoice\history.jsonl`. SQLite with
//! full-text search is planned (`ov-store`); until then this keeps the property
//! that actually matters: `raw_text` is recorded alongside `final_text` from the
//! very first session, so the whole history can be replayed through an improved
//! formatter and diffed before the change ships.

use std::io::Write;
use std::path::PathBuf;

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

/// Append one completed session. Failures are reported, never fatal.
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

    let line = serde_json::to_string(&row).map_err(|e| format!("encoding row: {e}"))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("history.jsonl"))
        .map_err(|e| format!("opening history: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("writing history: {e}"))
}

/// Most recent sessions, newest first. Reads the whole file, which is fine at the
/// scale a single person generates and avoids a database before one is needed.
#[must_use]
pub fn recent(limit: usize) -> Vec<serde_json::Value> {
    let path = data_dir().join("history.jsonl");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut rows: Vec<serde_json::Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    rows.reverse();
    rows.truncate(limit);
    rows
}
