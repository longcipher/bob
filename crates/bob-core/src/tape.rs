//! # Tape Types
//!
//! Append-only conversation recording types for the Bob Agent Framework.
//!
//! The tape system provides a persistent, searchable log of all interactions:
//!
//! - **Messages**: User and assistant conversation entries
//! - **Events**: Tool calls, LLM calls, and other runtime events
//! - **Anchors**: Semantic markers for task phases and milestones
//! - **Handoffs**: Context window reset points for topic switching
//!
//! ## Design
//!
//! The tape is **append-only** — entries are never modified or deleted.
//! This ensures auditability and allows reliable replay and search.
//!
//! ```text
//! ┌────────┐  ┌────────┐  ┌────────┐  ┌─────────┐  ┌────────┐
//! │ msg:U  │→ │ msg:A  │→ │ event  │→ │ anchor  │→ │handoff │→ ...
//! └────────┘  └────────┘  └────────┘  └─────────┘  └────────┘
//! ```
//!
//! ## Handoff
//!
//! When a conversation grows long or the user switches tasks, a **handoff**
//! entry resets the context window. The LLM only sees entries **after** the
//! most recent handoff, while the full history remains in the tape for search.

use serde::{Deserialize, Serialize};

use crate::types::Role;

/// Unique identifier for a tape entry.
pub type TapeEntryId = u64;

/// A single record in the append-only tape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TapeEntry {
    /// Monotonically increasing entry identifier.
    pub id: TapeEntryId,
    /// What kind of entry this is.
    pub kind: TapeEntryKind,
    /// Unix epoch milliseconds when this entry was recorded.
    pub timestamp_ms: u64,
}

/// Discriminated union of tape entry types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TapeEntryKind {
    /// A conversation message (user, assistant, tool, or system).
    Message { role: Role, content: String },
    /// A runtime event (tool call, LLM call, etc.).
    Event { event: String, payload: serde_json::Value },
    /// A semantic bookmark marking a milestone or phase boundary.
    Anchor { name: String, state: serde_json::Value },
    /// A context-window reset point.
    ///
    /// Entries before the most recent handoff are excluded from the LLM
    /// context window but remain in the tape for search.
    Handoff {
        name: String,
        /// Number of tape entries that existed before this handoff.
        entries_before: u64,
        /// Optional human-readable summary of the preceding context.
        summary: Option<String>,
    },
}

/// A search hit within the tape.
#[derive(Debug, Clone)]
pub struct TapeSearchResult {
    /// The matching entry.
    pub entry: TapeEntry,
    /// A short snippet highlighting the matching text.
    pub snippet: String,
}

/// Returns the current time as Unix epoch milliseconds.
///
/// Falls back to `0` if the system clock is before the Unix epoch.
#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}
