//! # In-Memory Tool Journal
//!
//! In-memory implementation of [`ToolJournalPort`] backed by `scc::HashMap`.
//!
//! Suitable for development and single-process deployments.

use bob_core::{
    error::StoreError,
    journal::{JournalEntry, ToolJournalPort},
    types::SessionId,
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bob_core::journal::JournalEntry;

    use super::*;

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
}
