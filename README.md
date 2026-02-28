# Bob — Minimal Hexagonal AI Agent Framework

[![DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/longcipher/bob)
[![Context7](https://img.shields.io/badge/Website-context7.com-blue)](https://context7.com/longcipher/bob)
[![crates.io](https://img.shields.io/crates/v/bob-core.svg)](https://crates.io/crates/bob-core)
[![docs.rs](https://docs.rs/bob-core/badge.svg)](https://docs.rs/bob-core)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE.md)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org/)

![bob](https://socialify.git.ci/longcipher/bob/image?font=Source+Code+Pro&language=1&name=1&owner=1&pattern=Circuit+Board&theme=Auto)

**Bob** is a minimal AI agent framework built in Rust with a hexagonal (ports & adapters) architecture. It connects to language models via the [`genai`](https://crates.io/crates/genai) crate and to external tools via [MCP](https://modelcontextprotocol.io/) servers using [`rmcp`](https://crates.io/crates/rmcp).

## Features

- 🤖 **Multi-Model Support**: Works with OpenAI, Anthropic, Google, Groq, and more
- 🔧 **Tool Integration**: Connect to MCP servers for file operations, shell commands, and custom tools
- 🎯 **Skill System**: Load and apply predefined skills for specialized tasks
- 💬 **Interactive REPL**: Chat with the AI agent through a terminal interface
- 🔄 **Streaming Responses**: Real-time streaming of LLM responses
- 📊 **Observability**: Built-in tracing and event logging
- 🏗️ **Clean Architecture**: Hexagonal (ports & adapters) design for extensibility

## Crates

This workspace contains the following crates:

| Crate | Description | Links |
|-------|-------------|-------|
| **[bob-core](https://crates.io/crates/bob-core)** | Core domain types and port traits | [![docs.rs](https://docs.rs/bob-core/badge.svg)](https://docs.rs/bob-core) |
| **[bob-runtime](https://crates.io/crates/bob-runtime)** | Runtime orchestration layer | [![docs.rs](https://docs.rs/bob-runtime/badge.svg)](https://docs.rs/bob-runtime) |
| **[bob-adapters](https://crates.io/crates/bob-adapters)** | Adapter implementations | [![docs.rs](https://docs.rs/bob-adapters/badge.svg)](https://docs.rs/bob-adapters) |
| **bob-cli** | CLI application (package: `cli-agent`) | - |

## Architecture

```text
bin/cli-agent        — CLI composition root (config, REPL)
crates/bob-core      — Domain types and port traits (LlmPort, ToolPort, SessionStore, EventSink)
crates/bob-runtime   — Scheduler FSM, prompt builder, action parser, CompositeToolPort
crates/bob-adapters  — Concrete adapter implementations (genai, rmcp, in-memory store, tracing)
```

```text
┌─────────────────────────────────────────────────────────────┐
│                     CLI Agent (bin)                         │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              DefaultAgentRuntime                     │   │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────────────┐  │   │
│  │  │Scheduler │→ │Prompt    │→ │Action Parser     │  │   │
│  │  │  FSM     │  │Builder   │  │                  │  │   │
│  │  └──────────┘  └──────────┘  └──────────────────┘  │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
            ↓ uses ports (traits) from bob-core
┌─────────────────────────────────────────────────────────────┐
│                  Adapters (bob-adapters)                    │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │GenAI LLM │  │MCP Tools │  │In-Memory │  │ Tracing  │   │
│  │          │  │          │  │  Store   │  │  Events  │   │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘   │
└─────────────────────────────────────────────────────────────┘
```

See [docs/design.md](docs/design.md) for the full design document.

## Quick Start

### Prerequisites

```bash
# Install Rust (stable)
rustup install stable

# Install dev tools
just setup
```

### Installation

#### From Source

```bash
# Clone the repository
git clone https://github.com/longcipher/bob.git
cd bob

# Build
cargo build --release

# Run
cargo run --release --bin bob-cli -- --config agent.toml
```

#### Using Cargo

```bash
# Install the CLI binary
cargo install --git https://github.com/longcipher/bob --package cli-agent --bin bob-cli

# Run
bob-cli --config agent.toml
```

### Configuration

Create an `agent.toml` in the project root:

```toml
[runtime]
default_model = "openai:gpt-4o-mini"
max_steps = 12
turn_timeout_ms = 90000
dispatch_mode = "native_preferred"
model_context_tokens = 128000

# Optional: Configure MCP tool servers
[mcp]
[[mcp.servers]]
id = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
tool_timeout_ms = 15000

# Optional: Configure skills
[skills]
max_selected = 3
token_budget_ratio = 0.1

[[skills.sources]]
type = "directory"
path = "./skills"
recursive = false

# Optional: Configure policies
[policy]
deny_tools = ["local/shell_exec"]
allow_tools = ["local/read_file", "local/write_file"]
default_deny = false

# Optional: Configure approval guardrails
[approval]
mode = "allow_all"
deny_tools = ["local/shell_exec"]

# Optional: Configure per-session token budget
[cost]
session_token_budget = 10000
```

### Environment Variables

Set your LLM provider API key:

```bash
# For OpenAI
export OPENAI_API_KEY="sk-..."

# For Anthropic
export ANTHROPIC_API_KEY="sk-ant-..."

# For Google
export GEMINI_API_KEY="..."
```

### Run

```bash
cargo run --bin bob-cli -- --config agent.toml
```

The REPL prints `>` when ready. Type a message and press Enter. Use `/quit` to exit.

### Example Session

```text
> Summarize docs/design.md in 5 bullets

I'll read docs/design.md and summarize it...

[uses filesystem tool to read the document]

Key points from docs/design.md:
- Framework follows strict ports-and-adapters boundaries
- Runtime orchestrates turn FSM and policies
- Adapters isolate provider/tool integrations
- ...

> Translate those bullets to Chinese

[agent generates translated output]

已翻译如下：
- 框架遵循严格的端口与适配器边界
- Runtime 负责回合状态机与策略控制
- ...
```

## Supported LLM Providers

Bob supports all providers available through [`genai`](https://crates.io/crates/genai):

| Provider | Model Examples | Configuration |
|----------|---------------|---------------|
| **OpenAI** | `gpt-4o`, `gpt-4o-mini` | Set `OPENAI_API_KEY` |
| **Anthropic** | `claude-3-5-sonnet-20241022` | Set `ANTHROPIC_API_KEY` |
| **Google** | `gemini-2.0-flash-exp` | Set `GEMINI_API_KEY` |
| **Groq** | `llama-3.3-70b-versatile` | Set `GROQ_API_KEY` |
| **Cohere** | `command-r-plus` | Set `COHERE_API_KEY` |

## MCP Tools

Bob integrates with [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) servers:

### Official MCP Servers

- **Filesystem**: `@modelcontextprotocol/server-filesystem`
- **GitHub**: `@modelcontextprotocol/server-github`
- **PostgreSQL**: `@modelcontextprotocol/server-postgres`
- **Slack**: `@modelcontextprotocol/server-slack`

### Custom MCP Servers

You can build custom MCP servers in any language that supports the protocol.

## Development

### Development Commands

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

### Project Structure

```text
.
├── bin/
│   └── cli-agent/          # CLI package (binary: bob-cli)
├── crates/
│   ├── bob-core/           # Domain types and ports
│   ├── bob-runtime/        # Runtime orchestration
│   └── bob-adapters/       # Adapter implementations
├── docs/
│   └── design.md           # Architecture design
├── specs/                  # Task specifications
└── .opencode/              # AI development skills
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

## Publishing

### Publishing to crates.io

Each library crate can be published independently:

```bash
# Publish bob-core
cargo publish -p bob-core

# Publish bob-runtime
cargo publish -p bob-runtime

# Publish bob-adapters
cargo publish -p bob-adapters
```

### Documentation

Documentation is automatically generated on [docs.rs](https://docs.rs) when published to crates.io:

- [bob-core docs](https://docs.rs/bob-core)
- [bob-runtime docs](https://docs.rs/bob-runtime)
- [bob-adapters docs](https://docs.rs/bob-adapters)

## Contributing

Contributions are welcome! Please read our contributing guidelines before submitting PRs.

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run `just ci` to ensure all checks pass
5. Submit a pull request

## Roadmap

- [ ] Persistent session storage (SQLite, PostgreSQL)
- [ ] Web UI for agent interaction
- [ ] Multi-agent collaboration
- [ ] Custom skill marketplace
- [ ] Agent memory and context management
- [ ] Tool composition and chaining
- [ ] More MCP server integrations

## License

Licensed under the Apache License, Version 2.0. See [LICENSE.md](LICENSE.md) for details.

## Acknowledgments

- [genai](https://crates.io/crates/genai) - Unified LLM API
- [rmcp](https://crates.io/crates/rmcp) - MCP client implementation
- [agent-skills](https://crates.io/crates/agent-skills) - Skill system
- [Model Context Protocol](https://modelcontextprotocol.io/) - Tool integration protocol

## Support

- **Documentation**: [docs.rs](https://docs.rs/bob-core)
- **Issues**: [GitHub Issues](https://github.com/longcipher/bob/issues)
- **Discussions**: [GitHub Discussions](https://github.com/longcipher/bob/discussions)

---

**Note**: This project is in active development. APIs may change between versions.
