# bob-adapters

[![crates.io](https://img.shields.io/crates/v/bob-adapters.svg)](https://crates.io/crates/bob-adapters)
[![docs.rs](https://docs.rs/bob-adapters/badge.svg)](https://docs.rs/bob-adapters)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Adapter implementations for the [Bob Agent Framework](https://github.com/longcipher/bob) ports.

## Overview

`bob-adapters` provides concrete implementations of the port traits defined in `bob-core`:

- **LLM Adapters**: Connect to language models via various providers
- **Tool Adapters**: Integrate with external tools and MCP servers
- **Storage Adapters**: Persist session state
- **Runtime Extension Adapters**: Checkpoints, artifacts, and cost metering
- **Runtime Guardrail Adapters**: Tool policy and approval adapters
- **Observability Adapters**: Log and monitor agent events

## Features

All adapters are feature-gated to minimize dependencies:

- **`llm-genai`** (default): LLM adapter using the [`genai`](https://crates.io/crates/genai) crate
- **`mcp-rmcp`** (default): Tool adapter for MCP servers via [`rmcp`](https://crates.io/crates/rmcp)
- **`skills-agent`** (default): Skill loading and composition via [`agent-skills`](https://crates.io/crates/agent-skills)
- **`store-memory`** (default): In-memory session storage
- **`observe-tracing`** (default): Event sink using [`tracing`](https://crates.io/crates/tracing)

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
bob-adapters = "0.1"
```

To disable default features and select specific adapters:

```toml
[dependencies.bob-adapters]
version = "0.1"
default-features = false
features = ["llm-genai", "mcp-rmcp"]
```

### Example: Creating Adapters

```rust
use bob_adapters::{
    llm_genai::GenAiLlmAdapter,
    mcp_rmcp::McpToolAdapter,
    store_memory::InMemorySessionStore,
    observe::TracingEventSink,
};
use genai::Client;

// LLM adapter
let client = Client::default();
let llm = GenAiLlmAdapter::new(client);

// Tool adapter (MCP server)
let tools = McpToolAdapter::connect_stdio(
    "filesystem",
    "npx",
    &["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
    &[], // environment variables
).await?;

// Session store
let store = InMemorySessionStore::new();

// Event sink
let events = TracingEventSink::new();
```

### Example: Using Skills

```rust
use bob_adapters::skills_agent::{SkillPromptComposer, SkillSourceConfig};

let sources = vec![SkillSourceConfig {
    path: "./skills".into(),
    recursive: true,
}];

let composer = SkillPromptComposer::from_sources(&sources, 3)?;
let rendered = composer.render_bundle_for_input("summarize this incident report");
```

## Adapters

### LLM Adapters (`llm-genai`)

Connects to LLM providers through the `genai` crate, supporting:

- OpenAI (GPT-4, GPT-4o-mini, etc.)
- Anthropic (Claude)
- Google (Gemini)
- Groq
- And more...

### Tool Adapters (`mcp-rmcp`)

Connects to [MCP (Model Context Protocol)](https://modelcontextprotocol.io/) servers:

- Filesystem operations
- Shell commands
- Database queries
- Custom tools

### Storage Adapters (`store-memory`)

In-memory session storage for development and testing:

- Fast in-process storage
- No external dependencies
- Suitable for single-instance deployments

### Runtime Extension Adapters

- `checkpoint_memory::InMemoryCheckpointStore`: per-session turn checkpoints
- `artifact_memory::InMemoryArtifactStore`: per-session tool-result artifacts
- `cost_simple::SimpleCostMeter`: optional per-session token budget enforcement

### Runtime Guardrail Adapters

- `policy_static::StaticToolPolicyPort`: merges runtime/request allow/deny lists
- `approval_static::StaticApprovalPort`: supports `allow_all` or `deny_all` plus tool denylist

### Observability (`observe-tracing`)

Event sink using the `tracing` ecosystem:

- Structured logging
- Integration with observability tools
- Configurable log levels

## Documentation

Full API documentation is available at [docs.rs/bob-adapters](https://docs.rs/bob-adapters).

## Related Crates

- **[bob-core](https://crates.io/crates/bob-core)** - Domain types and ports
- **[bob-runtime](https://crates.io/crates/bob-runtime)** - Runtime orchestration
- **[bob-cli](https://github.com/longcipher/bob)** - CLI application

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../../LICENSE.md) for details.
