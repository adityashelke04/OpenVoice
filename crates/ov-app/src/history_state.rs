//! Startup history status retained for windows that attach after migration.
use std::{path::Path, sync::Arc};

pub fn open(dir: &Path) -> (Arc<ov_store::SqliteStore>, Option<String>) {
    let mut warning = None;
    let store = ov_store::SqliteStore::open(dir.join("history.db")).unwrap_or_else(|error| {
        tracing::error!(%error, "history unavailable; using memory only");
        warning = Some(format!("History is available only in memory ({error}). Check disk space and folder permissions, then restart. Existing history is preserved."));
        ov_store::SqliteStore::open(":memory:").expect("in-memory database")
    });
    if let Err(error) = ov_store::import_jsonl(&store, dir.join("history.jsonl")) {
        tracing::warn!(%error, "legacy history migration failed");
        let message = format!("Previous history could not be imported ({error}). The original history file is preserved. Check disk space and folder permissions, then restart to retry.");
        warning = Some(warning.map_or(message.clone(), |previous| format!("{previous} {message}")));
    }
    (Arc::new(store), warning)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_database_does_not_hide_failed_legacy_import() {
        let dir = std::env::temp_dir().join(format!(
            "ov-history-status-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("history.jsonl"), [0xff]).unwrap();
        let (store, warning) = open(&dir);
        assert!(store.is_persistent());
        assert!(dir.join("history.jsonl").exists());
        drop(store);
        let base = std::env::temp_dir().canonicalize().unwrap();
        assert!(dir.canonicalize().unwrap().starts_with(base));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            warning.is_some(),
            "failed import must remain visible after the window attaches"
        );
    }
}
