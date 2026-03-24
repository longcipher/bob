//! # Tool Call Journal
//!
//! Append-only journal for recording tool call inputs and outputs, enabling
//! idempotent replay across retries and restarts.
//!
//! ## Design
//!
//! Inspired by Restate's journal-based durable execution, the journal
//! records every tool call with its result. On retry (e.g. after a crash
//! or restart), the scheduler can look up previously completed tool calls
//! and replay their results instead of re-executing them.
//!
//! ## Journal Entry
//!
//! Each entry captures:
//! - `session_id` — which session this belongs to
//! - `call_fingerprint` — deterministic hash of (tool_name + arguments)
//! - `tool_name` — the tool that was called
//! - `arguments` — the input arguments
//! - `result` — the recorded output
//! - `is_error` — whether the original call resulted in an error
//! - `timestamp_ms` — when the entry was recorded
//!
//! ## Replay Semantics
//!
//! The scheduler calls `lookup` before executing a tool call. If a matching
//! entry exists (same session + fingerprint), the cached result is returned
//! immediately, skipping the actual tool execution.

use serde::{Deserialize, Serialize};

use crate::{error::StoreError, types::SessionId};

/// A recorded tool call with its result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Session this entry belongs to.
    pub session_id: SessionId,
    /// Deterministic fingerprint of (tool_name + arguments_json).
    pub call_fingerprint: String,
    /// Name of the tool that was called.
    pub tool_name: String,
    /// Arguments passed to the tool.
    pub arguments: serde_json::Value,
    /// Recorded result output.
    pub result: serde_json::Value,
    /// Whether the original call was an error.
    pub is_error: bool,
    /// Unix epoch milliseconds.
    pub timestamp_ms: u64,
}

impl JournalEntry {
    /// Compute a deterministic fingerprint for a tool call.
    ///
    /// The fingerprint is stable across restarts and is used for
    /// deduplication and idempotent replay.
    #[must_use]
    pub fn fingerprint(tool_name: &str, arguments: &serde_json::Value) -> String {
        let args_canonical = serde_json::to_string(arguments).unwrap_or_default();
        format!("{tool_name}:{args_canonical}")
    }
}

/// Port for tool call journal persistence.
///
/// The journal is append-only: entries are never modified or deleted.
#[async_trait::async_trait]
pub trait ToolJournalPort: Send + Sync {
    /// Record a completed tool call in the journal.
    async fn append(&self, entry: JournalEntry) -> Result<(), StoreError>;

    /// Look up a previously recorded result for the same tool call.
    ///
    /// Returns `Some(entry)` if a matching entry exists for the given
    /// session and fingerprint, or `None` if this is a new call.
    async fn lookup(
        &self,
        session_id: &SessionId,
        fingerprint: &str,
    ) -> Result<Option<JournalEntry>, StoreError>;

    /// Return all journal entries for a session (for diagnostics).
    async fn entries(&self, session_id: &SessionId) -> Result<Vec<JournalEntry>, StoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_deterministic() {
        let args = serde_json::json!({"path": "/tmp/test.txt", "mode": "read"});
        let fp1 = JournalEntry::fingerprint("read_file", &args);
        let fp2 = JournalEntry::fingerprint("read_file", &args);
        assert_eq!(fp1, fp2, "same inputs should produce same fingerprint");
    }

    #[test]
    fn fingerprint_differs_for_different_tools() {
        let args = serde_json::json!({"x": 1});
        let fp1 = JournalEntry::fingerprint("tool_a", &args);
        let fp2 = JournalEntry::fingerprint("tool_b", &args);
        assert_ne!(fp1, fp2, "different tools should produce different fingerprints");
    }

    #[test]
    fn fingerprint_differs_for_different_args() {
        let args1 = serde_json::json!({"x": 1});
        let args2 = serde_json::json!({"x": 2});
        let fp1 = JournalEntry::fingerprint("tool_a", &args1);
        let fp2 = JournalEntry::fingerprint("tool_a", &args2);
        assert_ne!(fp1, fp2, "different args should produce different fingerprints");
    }
}
