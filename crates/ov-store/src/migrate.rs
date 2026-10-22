//! One-time import of the JSONL history this store replaces.
//!
//! Existing users have a `history.jsonl` full of real transcripts. Shipping SQLite
//! without importing it would silently reset their word count, streak and search to
//! zero — the app would look like it had lost their data, which from their side is
//! exactly what happened.

use std::path::Path;

use ov_core::error::{Error, Result};
use ov_core::ports::Utterance;
use serde::Deserialize;

use crate::SqliteStore;

/// A row as the JSONL writer produced it.
#[derive(Deserialize)]
struct Row {
    #[serde(default)]
    created_at: u64,
    #[serde(default)]
    outcome: String,
    #[serde(default)]
    raw_text: String,
    #[serde(default)]
    final_text: String,
    #[serde(default)]
    profile: String,
    #[serde(default)]
    target_app: String,
    #[serde(default)]
    audio_ms: u64,
    #[serde(default)]
    latency_ms: u64,
}

/// Import `path` into `store`, then rename it aside.
///
/// Returns the number of rows imported. Does nothing if the file is absent, so it
/// is safe to call on every start.
///
/// The original file is renamed rather than deleted. It is the only copy of the
/// user's history until the import is proven good, and a rename costs nothing.
pub fn import_jsonl(store: &SqliteStore, path: impl AsRef<Path>) -> Result<usize> {
    let path = path.as_ref();
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(Error::Storage(format!("reading legacy history: {e}"))),
    };
    let source = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let mut conn = store.conn.lock().expect("store mutex");
    let tx = conn
        .transaction()
        .map_err(|e| Error::Storage(format!("starting history import: {e}")))?;
    let done: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM history_import WHERE source = ?1)",
            [&source],
            |r| r.get(0),
        )
        .map_err(|e| Error::Storage(format!("checking history import: {e}")))?;

    let mut imported = 0usize;
    let mut skipped = 0usize;

    for line in text.lines().filter(|_| !done) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // One malformed line must not abort the import and strand everything after
        // it. Count and continue.
        let Ok(r) = serde_json::from_str::<Row>(line) else {
            skipped += 1;
            continue;
        };

        let entry = Utterance {
            created_at: r.created_at,
            duration_ms: r.audio_ms,
            raw_text: r.raw_text,
            final_text: r.final_text,
            profile: r.profile,
            target_app: (!r.target_app.is_empty()).then_some(r.target_app),
            // The JSONL format never recorded which model produced a transcript.
            // Left empty rather than guessed: a wrong attribution is worse than a
            // missing one when the point of keeping history is to compare models.
            model: String::new(),
            status: r.outcome,
            latency_ms: r.latency_ms,
        };

        crate::append_on(&tx, &entry)?;
        imported += 1;
    }

    tx.execute(
        "INSERT OR IGNORE INTO history_import(source) VALUES (?1)",
        [&source],
    )
    .map_err(|e| Error::Storage(format!("marking history import: {e}")))?;
    tx.commit()
        .map_err(|e| Error::Storage(format!("committing history import: {e}")))?;
    drop(conn);
    if !store.is_persistent() {
        return Ok(imported);
    }

    // Keep the original, out of the way. If the import turns out to be wrong it is
    // still recoverable, and it costs one filename.
    let archived = path.with_extension("jsonl.imported");
    if let Err(e) = std::fs::rename(path, &archived) {
        return Err(Error::Storage(format!(
            "history imported safely, but could not archive legacy file: {e}"
        )));
    }

    tracing::info!(
        imported,
        skipped,
        archived = %archived.display(),
        "imported history from JSONL"
    );
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ov_core::ports::HistoryStore;

    fn temp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("ov-import-{}-{name}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(p.with_extension("jsonl.imported"));
        p
    }

    #[test]
    fn volatile_import_keeps_source_and_does_not_duplicate() {
        let path = temp("volatile");
        std::fs::write(
            &path,
            r#"{"created_at":1,"outcome":"delivered","final_text":"keep me"}"#,
        )
        .unwrap();
        let store = SqliteStore::open(":memory:").unwrap();
        assert_eq!(import_jsonl(&store, &path).unwrap(), 1);
        assert!(
            path.exists(),
            "volatile database must never archive the only durable copy"
        );
        assert_eq!(import_jsonl(&store, &path).unwrap(), 0);
        assert_eq!(store.recent(10).unwrap().len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn insert_failure_rolls_back_and_preserves_source() {
        let path = temp("rollback");
        std::fs::write(
            &path,
            "{\"final_text\":\"first\"}\n{\"final_text\":\"fail\"}",
        )
        .unwrap();
        let store = SqliteStore::open(":memory:").unwrap();
        store.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_import BEFORE INSERT ON utterance WHEN new.final_text = 'fail' BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(import_jsonl(&store, &path).is_err());
        assert!(path.exists());
        assert!(store.recent(10).unwrap().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn archive_failure_does_not_duplicate_after_reopen() {
        let path = temp("archive-failure");
        let db = path.with_extension("db");
        let _ = std::fs::remove_file(&db);
        let archived = path.with_extension("jsonl.imported");
        std::fs::create_dir(&archived).unwrap();
        std::fs::write(&path, r#"{"final_text":"once"}"#).unwrap();
        {
            let store = SqliteStore::open(&db).unwrap();
            let _ = import_jsonl(&store, &path);
            assert_eq!(store.recent(10).unwrap().len(), 1);
        }
        let store = SqliteStore::open(&db).unwrap();
        let _ = import_jsonl(&store, &path);
        assert_eq!(store.recent(10).unwrap().len(), 1);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(archived).unwrap();
        drop(store);
        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn imports_rows_and_archives_the_file() {
        let path = temp("basic");
        std::fs::write(
            &path,
            r#"{"created_at":1000,"outcome":"delivered","raw_text":"hello there","final_text":"Hello there","profile":"prose","target_app":"Code.exe","audio_ms":2000,"latency_ms":600}
{"created_at":2000,"outcome":"delivered","raw_text":"second one","final_text":"Second one","profile":"prose","target_app":"chrome.exe","audio_ms":1500,"latency_ms":500}
"#,
        )
        .unwrap();

        let store = SqliteStore::open(path.with_extension("db")).unwrap();
        assert_eq!(import_jsonl(&store, &path).unwrap(), 2);

        let rows = store.recent(10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].final_text, "Second one");
        assert_eq!(
            rows[0].raw_text, "second one",
            "raw text must survive the import"
        );

        assert!(!path.exists(), "original is moved aside");
        assert!(path.with_extension("jsonl.imported").exists());
        let _ = std::fs::remove_file(path.with_extension("jsonl.imported"));
    }

    #[test]
    fn a_corrupt_line_does_not_abort_the_import() {
        // A truncated write leaves a partial last line. Everything before it is
        // still the user's history and must survive.
        let path = temp("corrupt");
        std::fs::write(
            &path,
            "{\"created_at\":1,\"outcome\":\"delivered\",\"final_text\":\"first\",\"audio_ms\":100}\n\
             not json at all\n\
             {\"created_at\":2,\"outcome\":\"delivered\",\"final_text\":\"third\",\"audio_ms\":100}\n\
             {\"created_at\":3,\"outcome\":\"deliv",
        )
        .unwrap();

        let store = SqliteStore::open(":memory:").unwrap();
        assert_eq!(import_jsonl(&store, &path).unwrap(), 2);
        assert_eq!(store.recent(10).unwrap().len(), 2);
        let _ = std::fs::remove_file(path.with_extension("jsonl.imported"));
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        // Called on every start; a fresh install has no JSONL to import.
        let store = SqliteStore::open(":memory:").unwrap();
        assert_eq!(import_jsonl(&store, "nowhere/at/all.jsonl").unwrap(), 0);
    }

    #[test]
    fn imported_history_is_searchable() {
        let path = temp("search");
        std::fs::write(
            &path,
            r#"{"created_at":1,"outcome":"delivered","final_text":"the deploy is done","audio_ms":100}"#,
        )
        .unwrap();

        let store = SqliteStore::open(":memory:").unwrap();
        import_jsonl(&store, &path).unwrap();
        assert_eq!(store.search("deploy", 10).unwrap().len(), 1);
        let _ = std::fs::remove_file(path.with_extension("jsonl.imported"));
    }
}
