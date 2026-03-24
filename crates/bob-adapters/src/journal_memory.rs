//! # In-Memory Journal Implementations
//!
//! In-memory implementations of journal ports backed by `scc::HashMap` and `Vec`.
//!
//! Suitable for development and single-process deployments.

use std::sync::atomic::{AtomicU64, Ordering};

use bob_core::{
    error::StoreError,
    journal::{JournalEntry, ToolJournalPort},
    types::{ActivityEntry, ActivityQuery, SessionId},
};

/// In-memory tool call journal.
///
/// Entries are stored per-session in a lock-free concurrent map.
#[derive(Debug)]
pub struct InMemoryToolJournal {
    inner: scc::HashMap<SessionId, Vec<JournalEntry>>,
}

impl Default for InMemoryToolJournal {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryToolJournal {
    /// Create an empty journal.
    #[must_use]
    pub fn new() -> Self {
        Self { inner: scc::HashMap::new() }
    }
}

#[async_trait::async_trait]
impl ToolJournalPort for InMemoryToolJournal {
    async fn append(&self, entry: JournalEntry) -> Result<(), StoreError> {
        let session_id = entry.session_id.clone();
        let map_entry = self.inner.entry_async(session_id).await;
        match map_entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().push(entry);
            }
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(vec![entry]);
            }
        }
        Ok(())
    }

    async fn lookup(
        &self,
        session_id: &SessionId,
        fingerprint: &str,
    ) -> Result<Option<JournalEntry>, StoreError> {
        let found = self
            .inner
            .read_async(session_id, |_k, entries| {
                entries.iter().find(|e| e.call_fingerprint == fingerprint).cloned()
            })
            .await;
        Ok(found.flatten())
    }

    async fn entries(&self, session_id: &SessionId) -> Result<Vec<JournalEntry>, StoreError> {
        let entries =
            self.inner.read_async(session_id, |_k, v| v.clone()).await.unwrap_or_default();
        Ok(entries)
    }
}

// ── In-Memory Activity Journal ────────────────────────────────────────

/// In-memory activity journal for testing and development.
///
/// Entries are stored in a `Vec` behind a `tokio::sync::Mutex` for
/// simplicity. Time-window queries filter over the full vector.
#[derive(Debug)]
pub struct MemoryActivityJournal {
    entries: tokio::sync::Mutex<Vec<ActivityEntry>>,
    count: AtomicU64,
}

impl Default for MemoryActivityJournal {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryActivityJournal {
    /// Create an empty activity journal.
    #[must_use]
    pub fn new() -> Self {
        Self { entries: tokio::sync::Mutex::new(Vec::new()), count: AtomicU64::new(0) }
    }
}

#[async_trait::async_trait]
impl bob_core::ports::ActivityJournalPort for MemoryActivityJournal {
    async fn append(&self, entry: ActivityEntry) -> Result<(), StoreError> {
        let mut entries = self.entries.lock().await;
        entries.push(entry);
        self.count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn query(&self, query: &ActivityQuery) -> Result<Vec<ActivityEntry>, StoreError> {
        let entries = self.entries.lock().await;
        let lower = query.lower_bound_ms();
        let upper = query.upper_bound_ms();

        let results: Vec<ActivityEntry> = entries
            .iter()
            .filter(|e| e.timestamp_ms >= lower && e.timestamp_ms <= upper)
            .filter(|e| query.role_filter.as_ref().is_none_or(|role| e.role == *role))
            .cloned()
            .collect();

        Ok(results)
    }

    async fn count(&self) -> Result<u64, StoreError> {
        Ok(self.count.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bob_core::journal::JournalEntry;

    use super::*;

    // ── InMemoryToolJournal tests ─────────────────────────────────

    #[tokio::test]
    async fn append_and_lookup_roundtrip() {
        let journal = InMemoryToolJournal::new();
        let sid = "sess-1".to_string();
        let args = serde_json::json!({"path": "/tmp/a"});
        let fp = JournalEntry::fingerprint("read_file", &args);

        let entry = JournalEntry {
            session_id: sid.clone(),
            call_fingerprint: fp.clone(),
            tool_name: "read_file".to_string(),
            arguments: args.clone(),
            result: serde_json::json!({"content": "hello"}),
            is_error: false,
            timestamp_ms: 0,
        };

        journal.append(entry).await.ok();

        let found = journal.lookup(&sid, &fp).await.ok().flatten();
        assert!(found.is_some(), "should find the appended entry");
        assert_eq!(found.as_ref().map(|e| e.tool_name.as_str()), Some("read_file"));
    }

    #[tokio::test]
    async fn lookup_returns_none_for_missing() {
        let journal = InMemoryToolJournal::new();
        let found =
            journal.lookup(&"missing".to_string(), "nonexistent:fingerprint").await.ok().flatten();
        assert!(found.is_none(), "should return None for missing session");
    }

    #[tokio::test]
    async fn entries_returns_all_for_session() {
        let journal = InMemoryToolJournal::new();
        let sid = "sess-2".to_string();

        for i in 0..3 {
            let args = serde_json::json!({"i": i});
            let entry = JournalEntry {
                session_id: sid.clone(),
                call_fingerprint: JournalEntry::fingerprint("tool", &args),
                tool_name: "tool".to_string(),
                arguments: args,
                result: serde_json::json!({"i": i}),
                is_error: false,
                timestamp_ms: 0,
            };
            journal.append(entry).await.ok();
        }

        let all = journal.entries(&sid).await.ok().unwrap_or_default();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn arc_dyn_journal_works() {
        let journal: Arc<dyn ToolJournalPort> = Arc::new(InMemoryToolJournal::new());
        let sid = "sess-arc".to_string();
        let args = serde_json::json!({});
        let entry = JournalEntry {
            session_id: sid.clone(),
            call_fingerprint: JournalEntry::fingerprint("t", &args),
            tool_name: "t".to_string(),
            arguments: args,
            result: serde_json::json!(null),
            is_error: false,
            timestamp_ms: 0,
        };
        journal.append(entry).await.ok();
        let found = journal
            .lookup(&sid, &JournalEntry::fingerprint("t", &serde_json::json!({})))
            .await
            .ok()
            .flatten();
        assert!(found.is_some());
    }

    // ── MemoryActivityJournal tests ───────────────────────────────

    use bob_core::ports::ActivityJournalPort;

    #[tokio::test]
    async fn activity_append_and_query_roundtrip() {
        let journal = MemoryActivityJournal::new();

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
    async fn activity_time_window_filtering() {
        let journal = MemoryActivityJournal::new();

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

        let query = ActivityQuery { anchor_ms: 1_000_000, window_minutes: 10, role_filter: None };
        let results = journal.query(&query).await.unwrap_or_default();
        assert_eq!(results.len(), 1, "only the middle entry should match");
        assert_eq!(results[0].content, "middle");
    }

    #[tokio::test]
    async fn activity_role_filtering() {
        let journal = MemoryActivityJournal::new();

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
    async fn activity_empty_results_for_out_of_range() {
        let journal = MemoryActivityJournal::new();

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

        let query =
            ActivityQuery { anchor_ms: 9_999_999_999, window_minutes: 1, role_filter: None };
        let results = journal.query(&query).await.unwrap_or_default();
        assert!(results.is_empty(), "should return empty for out-of-range query");
    }

    #[tokio::test]
    async fn activity_count_tracks_appends() {
        let journal = MemoryActivityJournal::new();

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

    #[tokio::test]
    async fn activity_arc_dyn_journal_works() {
        let journal: Arc<dyn bob_core::ports::ActivityJournalPort> =
            Arc::new(MemoryActivityJournal::new());

        let _ = journal
            .append(ActivityEntry {
                timestamp_ms: 42,
                session_key: "s".into(),
                role: "system".into(),
                content: "arc test".into(),
                event_type: Some("file_created".into()),
                metadata: Some(serde_json::json!({"key": "value"})),
            })
            .await;

        let query = ActivityQuery { anchor_ms: 42, window_minutes: 1, role_filter: None };
        let results = journal.query(&query).await.unwrap_or_default();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type.as_deref(), Some("file_created"));
    }
}
