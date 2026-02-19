# Bob — LLM-Powered Coding Agent

[![DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/longcipher/bob)
[![Context7](https://img.shields.io/badge/Website-context7.com-blue)](https://context7.com/longcipher/bob)
[![crates.io](https://img.shields.io/crates/v/bob-core.svg)](https://crates.io/crates/bob-core)
[![docs.rs](https://docs.rs/bob-core/badge.svg)](https://docs.rs/bob-core)

![bob](https://socialify.git.ci/longcipher/bob/image?font=Source+Code+Pro&language=1&name=1&owner=1&pattern=Circuit+Board&theme=Auto)

Bob is an LLM-powered coding agent built in Rust with a hexagonal (ports & adapters) architecture. It connects to language models via the [`genai`](https://crates.io/crates/genai) crate and to external tools via [MCP](https://modelcontextprotocol.io/) servers using [`rmcp`](https://crates.io/crates/rmcp).

## Architecture

```text
bin/cli-agent        — CLI composition root (config, REPL)
crates/bob-core      — Domain types and port traits (LlmPort, ToolPort, SessionStore, EventSink)
crates/bob-runtime   — Scheduler FSM, prompt builder, action parser, CompositeToolPort
crates/bob-adapters  — Concrete adapter implementations (genai, rmcp, in-memory store, tracing)
```

See [docs/design.md](docs/design.md) for the full design document.

## Quick Start

### Prerequisites

```bash
# Install Rust (nightly for formatting)
rustup install nightly

# Install dev tools
just setup
```

### Configuration

Create an `agent.toml` in the project root (see the included example):

```toml
[runtime]
default_model = "gpt-4o-mini"
max_steps = 12

# Optional: MCP tool server
# [mcp]
# [[mcp.servers]]
# id = "filesystem"
# command = "npx"
# args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
```

Set your LLM provider API key as an environment variable (e.g. `OPENAI_API_KEY`).

### Run

```bash
cargo run --bin cli-agent -- --config agent.toml
```

The REPL prints `>` when ready. Type a message and press Enter. Use `/quit` to exit.

### Development

```bash
# Format code
just format

# Run lints (typos, clippy, machete, etc.)
just lint

# Run all tests
just test

# Full CI check (lint + test + build)
just ci
```

## Workspace Configuration

### Linting Philosophy

The workspace uses strict clippy lints with the following principles:

1. **Pedantic by default**: Enable all pedantic lints, then allow specific ones that are too noisy
2. **Panic safety**: Deny `unwrap`, `expect`, and `panic` — use proper error handling
3. **No debug code**: Deny `dbg!`, `todo!`, and `unimplemented!`
4. **No stdout in libraries**: Use `tracing` instead of `println!`/`eprintln!`

### Adding Dependencies

Always use `cargo add`:

```bash
# Add to workspace
cargo add <crate> --workspace

# Add to specific crate
cargo add <crate> -p <crate-name>
```

## License

MIT
