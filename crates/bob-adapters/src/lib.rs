//! Bob Adapter Implementations
//!
//! Feature-gated adapters for `bob-core` port traits:
//! - `llm-genai`: LLM adapter via `genai` crate
//! - `mcp-rmcp`: MCP tool adapter via `rmcp` crate
//! - `skills`: Skill loading via `agent-skills` crate
//! - `store-memory`: In-memory session store
//! - `observe-tracing`: Event sink via `tracing` crate

pub use bob_core as core;

#[cfg(feature = "llm-genai")]
pub mod llm_genai;

#[cfg(feature = "mcp-rmcp")]
pub mod mcp_rmcp;

pub mod store_memory;

#[cfg(feature = "observe-tracing")]
pub mod observe;
