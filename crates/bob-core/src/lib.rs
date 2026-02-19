//! Bob Core — domain types and port traits
//!
//! This crate defines the hexagonal boundary of the Bob Agent Framework:
//! domain types, error types, and the 4 v1 port traits that adapters implement.
//!
//! **No concrete implementations live here** — only contracts.

// ── Modules ──────────────────────────────────────────────────────────

pub mod error;
pub mod ports;
pub mod types;

// ── Re-exports ───────────────────────────────────────────────────────

pub use error::{AgentError, LlmError, StoreError, ToolError};
pub use ports::{EventSink, LlmPort, SessionStore, ToolPort};
pub use types::*;
