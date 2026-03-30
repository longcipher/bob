//! # Bob Adapters
//!
//! Adapter implementations for the [Bob Agent Framework](https://github.com/longcipher/bob) ports.
//!
//! ## Overview
//!
//! This crate provides concrete implementations of the port traits defined in [`bob_core`]:
//!
//! - **LLM Adapters**: Connect to language models via various providers
//! - **Tool Adapters**: Integrate with external tools and MCP servers
//! - **Storage Adapters**: Persist session state
//! - **Observability Adapters**: Log and monitor agent events
//!
//! ## Features
//!
//! All adapters are feature-gated to minimize dependencies:
//!
//! - **`llm-liter`** (default): LLM adapter using the [`liter-llm`](https://crates.io/crates/liter-llm)
//!   crate
//! - **`mcp-rmcp`** (default): Tool adapter for MCP servers via [`rmcp`](https://crates.io/crates/rmcp)
//! - **`skills-agent`** (default): Skill loading and composition via [`bob-skills`](https://crates.io/crates/bob-skills)
//! - **`store-memory`** (default): In-memory session storage
//! - **`observe-tracing`** (default): Event sink using [`tracing`](https://crates.io/crates/tracing)
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_adapters::{
//!     llm_liter::LiterLlmAdapter,
//!     mcp_rmcp::McpToolAdapter,
//!     store_memory::InMemorySessionStore,
//!     observe::TracingEventSink,
//! };
//! use liter_llm::{ClientConfig, DefaultClient, LlmClient};
//! use std::sync::Arc;
//!
//! // LLM adapter
//! let config = ClientConfig::new(std::env::var("OPENAI_API_KEY").unwrap_or_default());
//! let client = Arc::new(DefaultClient::new(config, None).unwrap());
//! let llm = LiterLlmAdapter::new(client);
//!
//! // Tool adapter (MCP server)
//! let tools = McpToolAdapter::connect_stdio(
//!     "filesystem",
//!     "npx",
//!     &["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
//!     &[],
//! ).await?;
//!
//! // Session store
//! let store = InMemorySessionStore::new();
//!
//! // Event sink
//! let events = TracingEventSink::new();
//! ```
//!
//! ## Adapters
//!
//! ### LLM Adapters (`llm-liter`)
//!
//! Connects to LLM providers through the `liter-llm` crate, supporting:
//! - OpenAI (GPT-4, GPT-4o-mini, etc.)
//! - Anthropic (Claude)
//! - Google (Gemini)
//! - Groq
//! - And more...
//!
//! ### Tool Adapters (`mcp-rmcp`)
//!
//! Connects to [MCP (Model Context Protocol)](https://modelcontextprotocol.io/) servers:
//! - Filesystem operations
//! - Shell commands
//! - Database queries
//! - Custom tools
//!
//! ### Storage Adapters (`store-memory`)
//!
//! In-memory session storage for development and testing:
//! - Fast in-process storage
//! - No external dependencies
//! - Suitable for single-instance deployments
//!
//! ### Observability (`observe-tracing`)
//!
//! Event sink using the `tracing` ecosystem:
//! - Structured logging
//! - Integration with observability tools
//! - Configurable log levels
//!
//! ## Related Crates
//!
//! - [`bob_core`] - Domain types and ports
//! - [`bob_runtime`] - Runtime orchestration
//!
//! [`bob_core`]: https://docs.rs/bob-core
//! [`bob_runtime`]: https://docs.rs/bob-runtime

pub use bob_core as core;

pub mod builtin_tools;
pub mod journal_file;
pub mod journal_memory;
pub mod openai_schema;
pub mod tape_memory;

#[cfg(feature = "llm-liter")]
pub mod llm_liter;

#[cfg(feature = "mcp-rmcp")]
pub mod mcp_rmcp;

#[cfg(feature = "skills-agent")]
pub mod skills_agent;

pub mod access_control;
pub mod approval_static;
pub mod artifact_file;
pub mod artifact_memory;
pub mod checkpoint_file;
pub mod checkpoint_memory;
pub mod cost_file;
pub mod cost_simple;
pub mod policy_static;
pub mod store_file;
pub mod store_memory;

#[cfg(feature = "observe-tracing")]
pub mod observe;

#[cfg(feature = "observe-tracing")]
pub mod provider_router;

#[cfg(feature = "observe-otel")]
pub mod observe_otel;
