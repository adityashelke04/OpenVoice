//! Database schema and migrations.
//!
//! Versioned through SQLite's own `user_version` pragma rather than a metadata
//! table: it is atomic with the transaction that performs the migration, so a
//! process killed mid-upgrade cannot leave the version and the schema disagreeing.

use ov_core::error::{Error, Result};
use ov_core::ports::Utterance;
use rusqlite::{Connection, Row};

/// Schema version this build expects.
const VERSION: i64 = 1;

/// Column list, in the order [`row_to_utterance`] reads them.
///
/// Shared between queries so a column added to one is never missed by the other —
/// a mismatch here is a runtime type error, not a compile error.
pub const COLUMNS: &str = "id, created_at, duration_ms, raw_text, final_text, \
                           profile, target_app, model, status, latency_ms";

/// Connection settings applied on open.
pub fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         -- NORMAL rather than FULL: one fsync per checkpoint instead of one per
         -- utterance. The worst case is losing the last few seconds of history
         -- after a power cut, which is not worth a disk flush every time someone
         -- says a sentence.
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         -- Without this a locked database fails instantly; the app briefly holds
         -- write locks from two threads at session end.
         PRAGMA busy_timeout = 5000;",
    )
    .map_err(|e| Error::Storage(format!("configuring database: {e}")))
}

/// Bring the database up to [`VERSION`].
pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| Error::Storage(format!("reading schema version: {e}")))?;

    if current > VERSION {
        return Err(Error::Storage(format!(
            "history database is version {current}, but this build understands {VERSION}. \
             It was written by a newer version of OpenVoice."
        )));
    }

    if current < 1 {
        conn.execute_batch(V1)
            .map_err(|e| Error::Storage(format!("applying schema v1: {e}")))?;
        tracing::info!("history schema created");
    }

    // Future versions add `if current < N { … }` arms here. Each must be
    // idempotent and must bump user_version inside its own batch.
    Ok(())
}

/// Initial schema.
///
/// FTS5 uses an *external content* table: the index stores no copy of the text,
/// only pointers into `utterance`. That halves the storage and — more importantly —
/// makes it impossible for the two to hold different text. The cost is that the
/// index does not maintain itself, hence the triggers; without them a deleted row
/// keeps matching searches forever.
const V1: &str = r#"
BEGIN;

CREATE TABLE IF NOT EXISTS utterance (
    id           INTEGER PRIMARY KEY,
    created_at   INTEGER NOT NULL,   -- unix milliseconds
    duration_ms  INTEGER NOT NULL,   -- length of the audio
    raw_text     TEXT    NOT NULL,   -- straight from the model
    final_text   TEXT    NOT NULL,   -- after formatting; what was delivered
    profile      TEXT    NOT NULL,
    target_app   TEXT    NOT NULL DEFAULT '',
    window_title TEXT    NOT NULL DEFAULT '',
    model        TEXT    NOT NULL DEFAULT '',
    status       TEXT    NOT NULL,
    latency_ms   INTEGER NOT NULL DEFAULT 0,
    word_count   INTEGER NOT NULL DEFAULT 0
);

-- Every list and every statistic is ordered or filtered by these two.
CREATE INDEX IF NOT EXISTS idx_utterance_created ON utterance (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_utterance_status  ON utterance (status);

CREATE VIRTUAL TABLE IF NOT EXISTS utterance_fts USING fts5(
    final_text,
    content = 'utterance',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);

-- Keep the index in step with the table. `delete` rows carry the old text because
-- FTS5 needs it to unpick the posting lists.
CREATE TRIGGER IF NOT EXISTS utterance_ai AFTER INSERT ON utterance BEGIN
    INSERT INTO utterance_fts (rowid, final_text) VALUES (new.id, new.final_text);
END;

CREATE TRIGGER IF NOT EXISTS utterance_ad AFTER DELETE ON utterance BEGIN
    INSERT INTO utterance_fts (utterance_fts, rowid, final_text)
    VALUES ('delete', old.id, old.final_text);
END;

CREATE TRIGGER IF NOT EXISTS utterance_au AFTER UPDATE ON utterance BEGIN
    INSERT INTO utterance_fts (utterance_fts, rowid, final_text)
    VALUES ('delete', old.id, old.final_text);
    INSERT INTO utterance_fts (rowid, final_text) VALUES (new.id, new.final_text);
END;

PRAGMA user_version = 1;

COMMIT;
"#;

/// Read a row selected with [`COLUMNS`].
pub fn row_to_utterance(row: &Row<'_>) -> rusqlite::Result<Utterance> {
    let target_app: String = row.get(6)?;
    Ok(Utterance {
        created_at: row.get::<_, i64>(1)? as u64,
        duration_ms: row.get::<_, i64>(2)? as u64,
        raw_text: row.get(3)?,
        final_text: row.get(4)?,
        profile: row.get(5)?,
        // Empty is stored rather than NULL so the column is never null-checked in
        // SQL; the distinction only matters at this boundary.
        target_app: (!target_app.is_empty()).then_some(target_app),
        model: row.get(7)?,
        status: row.get(8)?,
        latency_ms: row.get::<_, i64>(9)? as u64,
    })
}
