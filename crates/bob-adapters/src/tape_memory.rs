//! # In-Memory Tape Store
//!
//! A `TapeStorePort` implementation backed by `scc::HashMap` for fast concurrent
//! access. Each session gets its own append-only `Vec<TapeEntry>`.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use bob_adapters::tape_memory::InMemoryTapeStore;
//! let tape = InMemoryTapeStore::new();
//! ```
//!
//! This implementation is suitable for development, testing, and single-process
//! deployments. For persistent tape storage, implement `TapeStorePort` with a
//! file-backed (JSONL) or database-backed adapter.

use std::sync::atomic::{AtomicU64, Ordering};

use bob_core::{
    error::StoreError,
    tape::{TapeEntry, TapeEntryKind, TapeSearchResult, now_ms},
    types::SessionId,
};

/// In-memory tape store using `scc::HashMap` for concurrent access.
#[derive(Debug)]
pub struct InMemoryTapeStore {
    tapes: scc::HashMap<SessionId, Vec<TapeEntry>>,
    next_id: AtomicU64,
}

impl InMemoryTapeStore {
    /// Create a new empty tape store.
    #[must_use]
    pub fn new() -> Self {
        Self { tapes: scc::HashMap::new(), next_id: AtomicU64::new(1) }
    }

    /// Allocate the next unique entry ID.
    fn next_entry_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for InMemoryTapeStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl bob_core::ports::TapeStorePort for InMemoryTapeStore {
    async fn append(
        &self,
        session_id: &SessionId,
        kind: TapeEntryKind,
    ) -> Result<TapeEntry, StoreError> {
        let entry = TapeEntry { id: self.next_entry_id(), kind, timestamp_ms: now_ms() };
        let entry_clone = entry.clone();

        // Insert or update the session's tape.
        let map_entry = self.tapes.entry_async(session_id.clone()).await;
        match map_entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().push(entry_clone);
            }
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(vec![entry_clone]);
            }
        }

        Ok(entry)
    }

    async fn entries_since_last_handoff(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<TapeEntry>, StoreError> {
        let result = self
            .tapes
            .read_async(session_id, |_, entries| {
                // Find the last handoff index.
                let last_handoff =
                    entries.iter().rposition(|e| matches!(e.kind, TapeEntryKind::Handoff { .. }));

                match last_handoff {
                    Some(idx) => entries[idx + 1..].to_vec(),
                    None => entries.clone(),
                }
            })
            .await;

        Ok(result.unwrap_or_default())
    }

    async fn search(
        &self,
        session_id: &SessionId,
        query: &str,
    ) -> Result<Vec<TapeSearchResult>, StoreError> {
        let query_lower = query.to_lowercase();
        let result = self
            .tapes
            .read_async(session_id, |_, entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        let text = entry_text(entry);
                        text.to_lowercase().contains(&query_lower).then(|| {
                            // Build a snippet (up to 80 chars around the first match).
                            let snippet = build_snippet(&text, &query_lower);
                            TapeSearchResult { entry: entry.clone(), snippet }
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .await;

        Ok(result.unwrap_or_default())
    }

    async fn all_entries(&self, session_id: &SessionId) -> Result<Vec<TapeEntry>, StoreError> {
        let result = self.tapes.read_async(session_id, |_, entries| entries.clone()).await;
        Ok(result.unwrap_or_default())
    }

    async fn anchors(&self, session_id: &SessionId) -> Result<Vec<TapeEntry>, StoreError> {
        let result = self
            .tapes
            .read_async(session_id, |_, entries| {
                entries
                    .iter()
                    .filter(|e| matches!(e.kind, TapeEntryKind::Anchor { .. }))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .await;

        Ok(result.unwrap_or_default())
    }
}

/// Extract searchable text from a tape entry.
fn entry_text(entry: &TapeEntry) -> String {
    match &entry.kind {
        TapeEntryKind::Message { content, .. } => content.clone(),
        TapeEntryKind::Event { event, payload } => {
            format!("{event}: {}", serde_json::to_string(payload).unwrap_or_default())
        }
        TapeEntryKind::Anchor { name, .. } => format!("anchor: {name}"),
        TapeEntryKind::Handoff { name, summary, .. } => {
            let s = summary.as_deref().unwrap_or("");
            format!("handoff: {name} {s}")
        }
    }
}

/// Build a snippet around the first occurrence of `query` in `text`.
fn build_snippet(text: &str, query: &str) -> String {
    let lower = text.to_lowercase();
    let pos = lower.find(query).unwrap_or(0);
    let start = pos.saturating_sub(40);
    let end = (pos + query.len() + 40).min(text.len());
    let mut snippet = String::new();
    if start > 0 {
        snippet.push_str("...");
    }
    snippet.push_str(&text[start..end]);
    if end < text.len() {
        snippet.push_str("...");
    }
    snippet
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use bob_core::{ports::TapeStorePort, types::Role};

    use super::*;

    #[tokio::test]
    async fn append_and_read_entries() {
        let store = InMemoryTapeStore::new();
        let sid = "session-1".to_string();

        let e1 = store
            .append(&sid, TapeEntryKind::Message { role: Role::User, content: "hello".into() })
            .await;
        assert!(e1.is_ok());

        let e2 = store
            .append(
                &sid,
                TapeEntryKind::Message { role: Role::Assistant, content: "hi there".into() },
            )
            .await;
        assert!(e2.is_ok());

        let all = store.all_entries(&sid).await;
        assert!(all.is_ok());
        if let Ok(entries) = all {
            assert_eq!(entries.len(), 2);
        }
    }

    #[tokio::test]
    async fn entries_since_handoff() {
        let store = InMemoryTapeStore::new();
        let sid = "session-2".to_string();

        // Two messages before handoff.
        let _ = store
            .append(&sid, TapeEntryKind::Message { role: Role::User, content: "msg1".into() })
            .await;
        let _ = store
            .append(&sid, TapeEntryKind::Message { role: Role::Assistant, content: "resp1".into() })
            .await;

        // Handoff.
        let _ = store
            .append(
                &sid,
                TapeEntryKind::Handoff {
                    name: "topic-switch".into(),
                    entries_before: 2,
                    summary: None,
                },
            )
            .await;

        // One message after handoff.
        let _ = store
            .append(&sid, TapeEntryKind::Message { role: Role::User, content: "msg2".into() })
            .await;

        let since = store.entries_since_last_handoff(&sid).await;
        assert!(since.is_ok());
        if let Ok(entries) = since {
            assert_eq!(entries.len(), 1);
        }
    }

    #[tokio::test]
    async fn search_finds_matching_entries() {
        let store = InMemoryTapeStore::new();
        let sid = "session-3".to_string();

        let _ = store
            .append(
                &sid,
                TapeEntryKind::Message {
                    role: Role::User,
                    content: "compile the Rust project".into(),
                },
            )
            .await;
        let _ = store
            .append(
                &sid,
                TapeEntryKind::Message {
                    role: Role::Assistant,
                    content: "Running cargo build...".into(),
                },
            )
            .await;

        let results = store.search(&sid, "rust").await;
        assert!(results.is_ok());
        if let Ok(results) = results {
            assert_eq!(results.len(), 1);
        }
    }

    #[tokio::test]
    async fn anchors_only_returns_anchor_entries() {
        let store = InMemoryTapeStore::new();
        let sid = "session-4".to_string();

        let _ = store
            .append(&sid, TapeEntryKind::Message { role: Role::User, content: "msg".into() })
            .await;
        let _ = store
            .append(
                &sid,
                TapeEntryKind::Anchor {
                    name: "phase-1".into(),
                    state: serde_json::json!({"status": "done"}),
                },
            )
            .await;

        let anchors = store.anchors(&sid).await;
        assert!(anchors.is_ok());
        if let Ok(anchors) = anchors {
            assert_eq!(anchors.len(), 1);
        }
    }
}
