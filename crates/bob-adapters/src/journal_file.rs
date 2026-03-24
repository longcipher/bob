//! # File-Backed Activity Journal
//!
//! Append-only NDJSON activity journal persisted to a single file.
//!
//! Each entry is serialized as one JSON line (newline-delimited JSON).
//! Writes are atomic at the line level: open in append mode, write the
//! serialized line, flush. Readers scan the full file and filter by
//! the requested time window.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use bob_adapters::journal_file::FileActivityJournal;
//! let journal = FileActivityJournal::new("/var/bob/activity.journal").await?;
//! ```

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use bob_core::{
    error::StoreError,
    ports::ActivityJournalPort,
    types::{ActivityEntry, ActivityQuery},
};

/// File-backed activity journal using NDJSON format.
///
/// All entries are appended to a single file. A write mutex ensures
/// line-level atomicity. The entry count is tracked in-memory and
/// incremented on every successful append.
#[derive(Debug)]
pub struct FileActivityJournal {
    path: PathBuf,
    write_guard: tokio::sync::Mutex<()>,
    count: AtomicU64,
}

impl FileActivityJournal {
    /// Open (or create) an activity journal at the given file path.
    ///
    /// Parent directories are created automatically. The initial entry
    /// count is determined by scanning the existing file.
    ///
    /// # Errors
    /// Returns a backend error if the directory cannot be created or the
    /// file cannot be opened for the first scan.
    pub async fn new(path: PathBuf) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|err| {
                StoreError::Backend(format!("failed to create journal dir: {err}"))
            })?;
        }

        let count = Self::count_lines(&path).await?;
        Ok(Self { path, write_guard: tokio::sync::Mutex::new(()), count: AtomicU64::new(count) })
    }

    /// Return the file path backing this journal.
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    async fn count_lines(path: &PathBuf) -> Result<u64, StoreError> {
        let raw = match tokio::fs::read_to_string(path).await {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(err) => {
                return Err(StoreError::Backend(format!(
                    "failed to read journal '{}': {err}",
                    path.display()
                )));
            }
        };
        let count = raw.lines().filter(|l| !l.trim().is_empty()).count() as u64;
        Ok(count)
    }
}

#[async_trait::async_trait]
impl ActivityJournalPort for FileActivityJournal {
    async fn append(&self, entry: ActivityEntry) -> Result<(), StoreError> {
        let line = serde_json::to_string(&entry)
            .map_err(|err| StoreError::Serialization(err.to_string()))?;
        let _lock = self.write_guard.lock().await;

        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .map_err(|err| {
                StoreError::Backend(format!(
                    "failed to open journal '{}': {err}",
                    self.path.display()
                ))
            })?
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|err| StoreError::Backend(format!("failed to write journal entry: {err}")))?;

        self.count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn query(&self, query: &ActivityQuery) -> Result<Vec<ActivityEntry>, StoreError> {
        let raw = match tokio::fs::read_to_string(&self.path).await {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                return Err(StoreError::Backend(format!(
                    "failed to read journal '{}': {err}",
                    self.path.display()
                )));
            }
        };

        let lower = query.lower_bound_ms();
        let upper = query.upper_bound_ms();

        let entries: Vec<ActivityEntry> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|line| serde_json::from_str::<ActivityEntry>(line).ok())
            .filter(|entry| entry.timestamp_ms >= lower && entry.timestamp_ms <= upper)
            .filter(|entry| query.role_filter.as_ref().is_none_or(|role| entry.role == *role))
            .collect();

        Ok(entries)
    }

    async fn count(&self) -> Result<u64, StoreError> {
        Ok(self.count.load(Ordering::Relaxed))
    }
}

// Need the write trait for `write_all`.
use tokio::io::AsyncWriteExt;

#[cfg(test)]
mod tests {
    use bob_core::ports::ActivityJournalPort;

    use super::*;

    #[tokio::test]
    async fn append_and_query_roundtrip() {
        let temp_dir = tempfile::tempdir();
        assert!(temp_dir.is_ok(), "tempdir should be created");
        let temp_dir = match temp_dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let journal_path = temp_dir.path().join("activity.journal");
        let journal = FileActivityJournal::new(journal_path).await;
        assert!(journal.is_ok(), "journal should initialize");
        let journal = match journal {
            Ok(value) => value,
            Err(_) => return,
        };

        let entry = ActivityEntry {
            timestamp_ms: 1_000_000,
            session_key: "sess-1".into(),
            role: "user".into(),
            content: "hello world".into(),
            event_type: None,
            metadata: None,
        };

        let appended = journal.append(entry).await;
        assert!(appended.is_ok(), "append should succeed");

        let query = ActivityQuery { anchor_ms: 1_000_000, window_minutes: 10, role_filter: None };
        let results = journal.query(&query).await;
        assert!(results.is_ok(), "query should succeed");
        let results = results.unwrap_or_default();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "hello world");
    }

    #[tokio::test]
    async fn time_window_filtering() {
        let temp_dir = tempfile::tempdir();
        assert!(temp_dir.is_ok(), "tempdir should be created");
        let temp_dir = match temp_dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let journal_path = temp_dir.path().join("activity.journal");
        let journal = FileActivityJournal::new(journal_path).await;
        assert!(journal.is_ok(), "journal should initialize");
        let journal = match journal {
            Ok(value) => value,
            Err(_) => return,
        };

        // Entry at t=0
        let _ = journal
            .append(ActivityEntry {
                timestamp_ms: 0,
                session_key: "s".into(),
                role: "user".into(),
                content: "early".into(),
                event_type: None,
                metadata: None,
            })
            .await;
        // Entry at t=1_000_000 (1000 seconds later)
        let _ = journal
            .append(ActivityEntry {
                timestamp_ms: 1_000_000,
                session_key: "s".into(),
                role: "agent".into(),
                content: "middle".into(),
                event_type: None,
                metadata: None,
            })
            .await;
        // Entry at t=2_000_000
        let _ = journal
            .append(ActivityEntry {
                timestamp_ms: 2_000_000,
                session_key: "s".into(),
                role: "system".into(),
                content: "late".into(),
                event_type: None,
                metadata: None,
            })
            .await;

        // Query a 10-minute window around t=1_000_000
        let query = ActivityQuery { anchor_ms: 1_000_000, window_minutes: 10, role_filter: None };
        let results = journal.query(&query).await.unwrap_or_default();
        assert_eq!(results.len(), 1, "only the middle entry should match");
        assert_eq!(results[0].content, "middle");
    }

    #[tokio::test]
    async fn role_filtering() {
        let temp_dir = tempfile::tempdir();
        assert!(temp_dir.is_ok(), "tempdir should be created");
        let temp_dir = match temp_dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let journal_path = temp_dir.path().join("activity.journal");
        let journal = FileActivityJournal::new(journal_path).await;
        assert!(journal.is_ok(), "journal should initialize");
        let journal = match journal {
            Ok(value) => value,
            Err(_) => return,
        };

        let _ = journal
            .append(ActivityEntry {
                timestamp_ms: 500_000,
                session_key: "s".into(),
                role: "user".into(),
                content: "user msg".into(),
                event_type: None,
                metadata: None,
            })
            .await;
        let _ = journal
            .append(ActivityEntry {
                timestamp_ms: 500_000,
                session_key: "s".into(),
                role: "agent".into(),
                content: "agent msg".into(),
                event_type: None,
                metadata: None,
            })
            .await;

        let query = ActivityQuery {
            anchor_ms: 500_000,
            window_minutes: 60,
            role_filter: Some("user".into()),
        };
        let results = journal.query(&query).await.unwrap_or_default();
        assert_eq!(results.len(), 1, "only user entries should match");
        assert_eq!(results[0].role, "user");
    }

    #[tokio::test]
    async fn empty_results_for_out_of_range() {
        let temp_dir = tempfile::tempdir();
        assert!(temp_dir.is_ok(), "tempdir should be created");
        let temp_dir = match temp_dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let journal_path = temp_dir.path().join("activity.journal");
        let journal = FileActivityJournal::new(journal_path).await;
        assert!(journal.is_ok(), "journal should initialize");
        let journal = match journal {
            Ok(value) => value,
            Err(_) => return,
        };

        let _ = journal
            .append(ActivityEntry {
                timestamp_ms: 1_000_000,
                session_key: "s".into(),
                role: "user".into(),
                content: "msg".into(),
                event_type: None,
                metadata: None,
            })
            .await;

        // Query far away from the entry
        let query =
            ActivityQuery { anchor_ms: 9_999_999_999, window_minutes: 1, role_filter: None };
        let results = journal.query(&query).await.unwrap_or_default();
        assert!(results.is_empty(), "should return empty for out-of-range query");
    }

    #[tokio::test]
    async fn persistence_across_instances() {
        let temp_dir = tempfile::tempdir();
        assert!(temp_dir.is_ok(), "tempdir should be created");
        let temp_dir = match temp_dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let journal_path = temp_dir.path().join("activity.journal");

        // Write with first instance.
        let first = FileActivityJournal::new(journal_path.clone()).await;
        assert!(first.is_ok(), "first journal should initialize");
        let first = match first {
            Ok(value) => value,
            Err(_) => return,
        };
        let _ = first
            .append(ActivityEntry {
                timestamp_ms: 42,
                session_key: "s".into(),
                role: "system".into(),
                content: "persisted".into(),
                event_type: Some("file_created".into()),
                metadata: None,
            })
            .await;

        // Read with second instance (simulates restart).
        let second = FileActivityJournal::new(journal_path).await;
        assert!(second.is_ok(), "second journal should initialize");
        let second = match second {
            Ok(value) => value,
            Err(_) => return,
        };

        let query = ActivityQuery { anchor_ms: 42, window_minutes: 1, role_filter: None };
        let results = second.query(&query).await.unwrap_or_default();
        assert_eq!(results.len(), 1, "entry should persist across instances");
        assert_eq!(results[0].content, "persisted");
        assert_eq!(results[0].event_type.as_deref(), Some("file_created"));
        assert_eq!(second.count().await.unwrap_or(0), 1, "count should be restored on reopen");
    }

    #[tokio::test]
    async fn count_tracks_appends() {
        let temp_dir = tempfile::tempdir();
        assert!(temp_dir.is_ok(), "tempdir should be created");
        let temp_dir = match temp_dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let journal_path = temp_dir.path().join("activity.journal");
        let journal = FileActivityJournal::new(journal_path).await;
        assert!(journal.is_ok(), "journal should initialize");
        let journal = match journal {
            Ok(value) => value,
            Err(_) => return,
        };

        assert_eq!(journal.count().await.unwrap_or(99), 0, "fresh journal should have 0 entries");

        for i in 0..5 {
            let _ = journal
                .append(ActivityEntry {
                    timestamp_ms: i,
                    session_key: "s".into(),
                    role: "user".into(),
                    content: format!("msg-{i}"),
                    event_type: None,
                    metadata: None,
                })
                .await;
        }

        assert_eq!(journal.count().await.unwrap_or(0), 5, "count should be 5 after 5 appends");
    }
}
