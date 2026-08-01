//! # ov-store — dictation history
//!
//! Implements [`ov_core::ports::HistoryStore`] over SQLite, with FTS5 for search.
//!
//! ## Why this exists rather than the JSONL file it replaces
//!
//! A newline-delimited JSON file was the right first answer: no schema, no
//! migration, greppable. It stops being the right answer once history is worth
//! searching, because every query means reading and parsing the entire file.
//!
//! ## What is stored, and why both texts
//!
//! Every session records `raw_text` (straight from the model) **and** `final_text`
//! (after formatting). Keeping both is what makes the formatter improvable: the
//! whole history can be replayed through a new rule set and diffed before the
//! change ships. Discarding the raw text would make every past session useless as
//! evidence.
//!
//! ## Durability
//!
//! WAL journaling, `synchronous = NORMAL`. A dictation tool writes one small row
//! every few seconds; WAL keeps that cheap while surviving a crash. `FULL` would
//! mean an fsync per utterance for a durability guarantee nobody needs on a
//! transcript of "hello".

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ov_core::error::{Error, Result};
use ov_core::ports::{HistoryStore, Utterance};
use rusqlite::{params, Connection, OptionalExtension};

mod migrate;
mod schema;

pub use migrate::import_jsonl;

/// SQLite-backed history.
pub struct SqliteStore {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl SqliteStore {
    /// Open (creating if absent) the history database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| Error::Storage(format!("creating {}: {e}", dir.display())))?;
        }

        let conn = Connection::open(&path)
            .map_err(|e| Error::Storage(format!("opening {}: {e}", path.display())))?;

        schema::configure(&conn)?;
        schema::migrate(&conn)?;

        tracing::info!(path = %path.display(), "history store ready");
        Ok(Self { conn: Mutex::new(conn), path })
    }

    /// Where the database lives, for the "open data folder" action.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Total sessions and words, computed in SQL rather than by loading every row.
    ///
    /// The UI previously fetched 200 rows and summed them in JavaScript, which is
    /// both wrong (it silently ignored everything older) and wasteful.
    pub fn totals(&self) -> Result<Totals> {
        let conn = self.conn.lock().expect("store mutex");
        conn.query_row(
            "SELECT
                 COUNT(*),
                 COALESCE(SUM(word_count), 0),
                 COALESCE(SUM(duration_ms), 0)
             FROM utterance
             WHERE status IN ('delivered', 'clipboard_fallback')",
            [],
            |r| {
                Ok(Totals {
                    sessions: r.get(0)?,
                    words: r.get(1)?,
                    speaking_ms: r.get(2)?,
                })
            },
        )
        .map_err(|e| Error::Storage(format!("totals: {e}")))
    }

    /// Distinct local dates that have at least one delivered session, newest first.
    ///
    /// Returned as dates rather than a streak number because the streak depends on
    /// the user's timezone, and the database has no business guessing it.
    pub fn active_days(&self, limit: usize) -> Result<Vec<i64>> {
        let conn = self.conn.lock().expect("store mutex");
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT created_at FROM utterance
                 WHERE status IN ('delivered', 'clipboard_fallback')
                 ORDER BY created_at DESC LIMIT ?1",
            )
            .map_err(|e| Error::Storage(format!("active days: {e}")))?;

        let rows = stmt
            .query_map(params![limit as i64], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("active days: {e}")))?;

        rows.collect::<std::result::Result<Vec<i64>, _>>()
            .map_err(|e| Error::Storage(format!("active days: {e}")))
    }

    /// The application dictated into most often.
    pub fn top_app(&self) -> Result<Option<(String, i64)>> {
        let conn = self.conn.lock().expect("store mutex");
        conn.query_row(
            "SELECT target_app, COUNT(*) AS n FROM utterance
             WHERE status IN ('delivered', 'clipboard_fallback') AND target_app <> ''
             GROUP BY target_app ORDER BY n DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| Error::Storage(format!("top app: {e}")))
    }

    /// Most recent sessions that produced text, newest first.
    ///
    /// Empty results are excluded here rather than in the UI: a fat-finger tap is
    /// not history, and filtering after fetching wastes a page of rows on things
    /// that will never be shown.
    pub fn recent(&self, limit: usize) -> Result<Vec<Utterance>> {
        let conn = self.conn.lock().expect("store mutex");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM utterance
                 WHERE final_text <> ''
                 ORDER BY created_at DESC LIMIT ?1",
                schema::COLUMNS
            ))
            .map_err(|e| Error::Storage(format!("recent: {e}")))?;

        let rows = stmt
            .query_map(params![limit as i64], schema::row_to_utterance)
            .map_err(|e| Error::Storage(format!("recent: {e}")))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Storage(format!("recent: {e}")))
    }

    /// Delete everything. Backs the "panic purge" action.
    pub fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex");
        conn.execute_batch("DELETE FROM utterance;")
            .map_err(|e| Error::Storage(format!("clear: {e}")))?;
        tracing::info!("history cleared");
        Ok(())
    }
}

/// Aggregate figures computed in SQL.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct Totals {
    /// Sessions that delivered text.
    pub sessions: i64,
    /// Words delivered.
    pub words: i64,
    /// Total time spent speaking, in milliseconds. The denominator for words per
    /// minute — wall-clock time would punish the user for pausing to think.
    pub speaking_ms: i64,
}

impl HistoryStore for SqliteStore {
    fn append(&self, entry: &Utterance) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex");
        // Word count is stored rather than computed on read: it never changes, and
        // computing it in SQL across the whole table for every stats refresh is
        // work done thousands of times to save it once.
        let words = entry.final_text.split_whitespace().count() as i64;

        conn.execute(
            "INSERT INTO utterance
               (created_at, duration_ms, raw_text, final_text, profile,
                target_app, window_title, model, status, latency_ms, word_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                entry.created_at as i64,
                entry.duration_ms as i64,
                entry.raw_text,
                entry.final_text,
                entry.profile,
                entry.target_app.clone().unwrap_or_default(),
                "",
                entry.model,
                entry.status,
                entry.latency_ms as i64,
                words,
            ],
        )
        .map_err(|e| Error::Storage(format!("append: {e}")))?;

        Ok(conn.last_insert_rowid())
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<Utterance>> {
        if query.trim().is_empty() {
            return self.recent(limit);
        }

        let conn = self.conn.lock().expect("store mutex");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM utterance
                 JOIN utterance_fts ON utterance_fts.rowid = utterance.id
                 WHERE utterance_fts MATCH ?1
                 ORDER BY rank, created_at DESC
                 LIMIT ?2",
                schema::COLUMNS
                    .split(", ")
                    .map(|c| format!("utterance.{c}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .map_err(|e| Error::Storage(format!("search: {e}")))?;

        let rows = stmt
            .query_map(params![fts_query(query), limit as i64], schema::row_to_utterance)
            .map_err(|e| Error::Storage(format!("search: {e}")))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Storage(format!("search: {e}")))
    }

    fn purge_older_than(&self, days: u32) -> Result<u64> {
        // Zero means keep forever. Deleting everything would be a catastrophic
        // reading of "retention: 0".
        if days == 0 {
            return Ok(0);
        }
        let cutoff = now_ms().saturating_sub(u64::from(days) * 86_400_000);

        let conn = self.conn.lock().expect("store mutex");
        let n = conn
            .execute("DELETE FROM utterance WHERE created_at < ?1", params![cutoff as i64])
            .map_err(|e| Error::Storage(format!("purge: {e}")))?;

        if n > 0 {
            tracing::info!(removed = n, days, "purged old history");
        }
        Ok(n as u64)
    }
}

/// Turn user input into an FTS5 query that cannot be a syntax error.
///
/// FTS5 has its own grammar: bare `AND`, a stray quote, or a `*` in the wrong place
/// is a hard error, not a miss. Someone typing a search box is writing words, not a
/// query language — so every token is quoted as a literal and the last one gets a
/// prefix wildcard, which is what makes search feel responsive as you type.
fn fts_query(input: &str) -> String {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| !t.is_empty())
        .collect();

    if terms.is_empty() {
        return String::from("\"\"");
    }

    let last = terms.len() - 1;
    terms
        .iter()
        .enumerate()
        .map(|(i, t)| if i == last { format!("\"{t}\"*") } else { format!("\"{t}\"") })
        .collect::<Vec<_>>()
        .join(" ")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ov_core::ports::HistoryStore;

    fn store() -> SqliteStore {
        // In-memory databases are per-connection, and the store holds exactly one,
        // so this is a real database with no file to clean up.
        let s = SqliteStore::open(":memory:").expect("open");
        s
    }

    fn utt(text: &str, app: &str, status: &str) -> Utterance {
        Utterance {
            created_at: now_ms(),
            duration_ms: 2000,
            raw_text: text.into(),
            final_text: text.into(),
            profile: "prose".into(),
            target_app: Some(app.into()),
            model: "test".into(),
            status: status.into(),
            latency_ms: 500,
        }
    }

    #[test]
    fn round_trips_an_utterance() {
        let s = store();
        s.append(&utt("hello world", "Code.exe", "delivered")).unwrap();

        let rows = s.recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].final_text, "hello world");
        assert_eq!(rows[0].target_app.as_deref(), Some("Code.exe"));
    }

    #[test]
    fn keeps_raw_text_alongside_final() {
        // History is only useful for improving the formatter if the model's
        // original output survives; without it a past session cannot be replayed.
        let s = store();
        let mut u = utt("Call useEffect here", "Code.exe", "delivered");
        u.raw_text = "call use effect here".into();
        s.append(&u).unwrap();

        let rows = s.recent(1).unwrap();
        assert_eq!(rows[0].raw_text, "call use effect here");
        assert_eq!(rows[0].final_text, "Call useEffect here");
    }

    #[test]
    fn finds_text_by_word() {
        let s = store();
        s.append(&utt("the deploy is finished", "slack.exe", "delivered")).unwrap();
        s.append(&utt("kubectl get pods", "wt.exe", "delivered")).unwrap();

        assert_eq!(s.search("deploy", 10).unwrap().len(), 1);
        assert_eq!(s.search("kubectl", 10).unwrap().len(), 1);
        assert_eq!(s.search("nothing", 10).unwrap().len(), 0);
    }

    #[test]
    fn matches_partial_words_as_you_type() {
        let s = store();
        s.append(&utt("the deployment finished", "slack.exe", "delivered")).unwrap();
        // A search box should respond before the word is finished.
        assert_eq!(s.search("depl", 10).unwrap().len(), 1);
    }

    #[test]
    fn survives_fts_syntax_in_the_search_box() {
        // Bare AND, quotes and asterisks are FTS5 operators. A person typing into a
        // search box is writing words; none of these may become an error.
        let s = store();
        s.append(&utt("meeting notes", "Notion.exe", "delivered")).unwrap();

        for q in ["AND", "\"", "*", "NOT OR", "a\"b", "()", "^", "meeting AND"] {
            assert!(s.search(q, 10).is_ok(), "query {q:?} must not error");
        }
    }

    #[test]
    fn empty_sessions_are_stored_but_never_listed() {
        // A fat-finger tap is worth recording for diagnostics and worth hiding from
        // the user.
        let s = store();
        let mut u = utt("", "Discord.exe", "too_short");
        u.raw_text = String::new();
        s.append(&u).unwrap();
        s.append(&utt("real text", "Code.exe", "delivered")).unwrap();

        assert_eq!(s.recent(10).unwrap().len(), 1);
        assert_eq!(s.totals().unwrap().sessions, 1);
    }

    #[test]
    fn totals_count_only_delivered_sessions() {
        let s = store();
        s.append(&utt("one two three", "Code.exe", "delivered")).unwrap();
        s.append(&utt("four five", "Code.exe", "clipboard_fallback")).unwrap();
        s.append(&utt("ignored entirely", "Code.exe", "asr_failed")).unwrap();

        let t = s.totals().unwrap();
        assert_eq!(t.sessions, 2);
        assert_eq!(t.words, 5, "failed sessions must not inflate the word count");
        assert_eq!(t.speaking_ms, 4000);
    }

    #[test]
    fn retention_of_zero_keeps_everything() {
        // "0 days" in the settings means "forever". Reading it as "delete all" would
        // destroy the user's entire history on a mis-set dropdown.
        let s = store();
        s.append(&utt("keep me", "Code.exe", "delivered")).unwrap();
        assert_eq!(s.purge_older_than(0).unwrap(), 0);
        assert_eq!(s.recent(10).unwrap().len(), 1);
    }

    #[test]
    fn purge_removes_only_what_is_older() {
        let s = store();
        let mut old = utt("ancient", "Code.exe", "delivered");
        old.created_at = now_ms() - 40 * 86_400_000;
        s.append(&old).unwrap();
        s.append(&utt("recent", "Code.exe", "delivered")).unwrap();

        assert_eq!(s.purge_older_than(30).unwrap(), 1);
        let rows = s.recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].final_text, "recent");
    }

    #[test]
    fn search_index_follows_deletions() {
        // FTS5 external-content tables do not update themselves; without triggers a
        // purged row keeps matching and search returns rows that no longer exist.
        let s = store();
        let mut old = utt("forgotten words", "Code.exe", "delivered");
        old.created_at = now_ms() - 40 * 86_400_000;
        s.append(&old).unwrap();

        assert_eq!(s.search("forgotten", 10).unwrap().len(), 1);
        s.purge_older_than(30).unwrap();
        assert_eq!(s.search("forgotten", 10).unwrap().len(), 0);
    }

    #[test]
    fn reports_the_most_used_application() {
        let s = store();
        for _ in 0..3 {
            s.append(&utt("a", "chrome.exe", "delivered")).unwrap();
        }
        s.append(&utt("b", "Code.exe", "delivered")).unwrap();

        let (app, n) = s.top_app().unwrap().expect("some app");
        assert_eq!(app, "chrome.exe");
        assert_eq!(n, 3);
    }
}
