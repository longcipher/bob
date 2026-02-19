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
//! - **`llm-genai`** (default): LLM adapter using the [`genai`](https://crates.io/crates/genai)
//!   crate
//! - **`mcp-rmcp`** (default): Tool adapter for MCP servers via [`rmcp`](https://crates.io/crates/rmcp)
//! - **`skills-agent`** (default): Skill loading and composition via [`agent-skills`](https://crates.io/crates/agent-skills)
//! - **`store-memory`** (default): In-memory session storage
//! - **`observe-tracing`** (default): Event sink using [`tracing`](https://crates.io/crates/tracing)
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_adapters::{
//!     llm_genai::GenAiLlmAdapter,
//!     mcp_rmcp::McpToolAdapter,
//!     store_memory::InMemorySessionStore,
//!     observe::TracingEventSink,
//! };
//! use genai::Client;
//!
//! // LLM adapter
//! let client = Client::default();
//! let llm = GenAiLlmAdapter::new(client);
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
//! ### LLM Adapters (`llm-genai`)
//!
//! Connects to LLM providers through the `genai` crate, supporting:
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

#[cfg(feature = "llm-genai")]
pub mod llm_genai;

#[cfg(feature = "mcp-rmcp")]
pub mod mcp_rmcp;

#[cfg(feature = "skills-agent")]
pub mod skills_agent;

pub mod store_memory;

#[cfg(feature = "observe-tracing")]
pub mod observe;
