//! # Bob Core
//!
//! Domain types and port traits for the [Bob Agent Framework](https://github.com/longcipher/bob).
//!
//! ## Overview
//!
//! This crate defines the hexagonal boundary of the Bob Agent Framework using the
//! ports and adapters (hexagonal) architecture pattern. It contains:
//!
//! - **Domain Types**: Core data structures used throughout the framework
//! - **Port Traits**: Abstract interfaces that adapters must implement
//! - **Error Types**: Comprehensive error definitions for all components
//!
//! **No concrete implementations live here** — only contracts.
//!
//! ## Architecture
//!
//! The crate defines four primary port traits:
//!
//! 1. [`ports::LlmPort`] - Interface for language model interactions
//! 2. [`ports::ToolPort`] - Interface for tool/system operations
//! 3. [`ports::SessionStore`] - Interface for session state persistence
//! 4. [`ports::EventSink`] - Interface for event observation and logging
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_core::{
//!     ports::LlmPort,
//!     types::{LlmRequest, LlmResponse},
//!     error::LlmError,
//! };
//! use async_trait::async_trait;
//!
//! struct MyCustomLlm;
//!
//! #[async_trait]
//! impl LlmPort for MyCustomLlm {
//!     async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
//!         // Your implementation here
//!         todo!("implement LLM completion")
//!     }
//!
//!     async fn complete_stream(&self, req: LlmRequest) -> Result<LlmStream, LlmError> {
//!         // Your implementation here
//!         todo!("implement streaming completion")
//!     }
//! }
//! ```
//!
//! ## Features
//!
//! - **Zero-cost abstractions**: All traits use `async_trait` for async support
//! - **Type-safe**: Strong typing throughout with comprehensive error handling
//! - **Serializable**: All domain types implement `serde::Serialize` and `Deserialize`
//! - **Thread-safe**: All traits require `Send + Sync`
//!
//! ## Related Crates
//!
//! - [`bob_runtime`] - Runtime orchestration layer
//! - [`bob_adapters`] - Concrete adapter implementations
//!
//! [`bob_runtime`]: https://docs.rs/bob-runtime
//! [`bob_adapters`]: https://docs.rs/bob-adapters

// ── Modules ──────────────────────────────────────────────────────────

pub mod error;
pub mod ports;
pub mod tool_policy;
pub mod types;

// ── Re-exports ───────────────────────────────────────────────────────

pub use error::{AgentError, LlmError, StoreError, ToolError};
pub use ports::{EventSink, LlmPort, SessionStore, ToolPort};
pub use tool_policy::{
    intersect_allowlists, is_tool_allowed, merge_allowlists, normalize_tool_list, tools_match,
};
pub use types::*;
