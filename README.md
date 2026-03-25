# Bob — Minimal Hexagonal AI Agent Framework

[![DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/longcipher/bob)
[![Context7](https://img.shields.io/badge/Website-context7.com-blue)](https://context7.com/longcipher/bob)
[![crates.io](https://img.shields.io/crates/v/bob-core.svg)](https://crates.io/crates/bob-core)
[![docs.rs](https://docs.rs/bob-core/badge.svg)](https://docs.rs/bob-core)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE.md)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org/)

**Bob** is a minimal AI agent framework built in Rust with a hexagonal (ports & adapters) architecture. It connects to language models via [`genai`](https://crates.io/crates/genai) and to external tools via MCP servers using [`rmcp`](https://crates.io/crates/rmcp).

## Design Philosophy

Bob prioritizes **extreme developer experience (DX)** and **compile-time safety**:

- **Zero custom macros**: All abstractions use native Rust generics and traits
- **Type-driven design**: TypeState patterns enforce valid state transitions at compile time
- **Hexagonal purity**: `bob-runtime` NEVER imports `bob-adapters`; `bob-core` has ZERO internal dependencies
- **Tower ecosystem**: Middleware composition via `tower::Service` instead of custom chains
- **Extension traits**: Blanket implementations provide fluent APIs without inheritance

```text
Compile-Time Safety
├── TypeState Builder:  Incomplete → Described → Complete
├── TypedToolAdapter:   Input/Output types bound at compile time
├── Extension Traits:   ToolPortExt.chain().with_timeout()
└── Service Composition: tower::Service + tower middleware
```

## Features

- 🤖 **Multi-Model Support**: OpenAI, Anthropic, Google, Groq, and more
- 🔧 **Tool Integration**: MCP servers for filesystem, shell, and custom tools
- 🎯 **Skill System**: Load and apply predefined skills for specialized tasks
- 👥 **Subagent Support**: Spawn independent background agents
- 💬 **Interactive REPL**: Slash commands (`/tools`, `/usage`, `/handoff`)
- 🔄 **Streaming Responses**: Real-time LLM response streaming
- 📊 **Observability**: Tracing + OpenTelemetry event sinks

## Crates

| Crate | Description |
|-------|-------------|
| **[bob-core](https://crates.io/crates/bob-core)** | Domain types, port traits (`LlmPort`, `ToolPort`, `SessionStore`, `EventSink`) |
| **[bob-runtime](https://crates.io/crates/bob-runtime)** | Runtime orchestration: 6-state turn FSM, prompt builder, action parser |
| **[bob-adapters](https://crates.io/crates/bob-adapters)** | Concrete implementations: genai LLM, MCP tools, file/memory stores, OpenTelemetry |
| **[bob-chat](https://crates.io/crates/bob-chat)** | Chat channel types and streaming abstractions for multi-platform integration |
| **[bob-skills](https://crates.io/crates/bob-skills)** | Skill loading, parsing, selection, and registry with frontmatter metadata |
| **bob-cli** | CLI application with interactive REPL and skill management |

### bob-core

Defines the hexagonal boundary with **zero concrete implementations** — only contracts.

**Key patterns:**

- **Port Traits**: `LlmPort`, `ToolPort`, `SessionStore`, `EventSink`
- **TypeState Builder**: `TypedToolBuilder<S>` — `build()` only available in `Complete` state
- **Typed Tool Adapter**: `TypedToolAdapter` with associated `Input`/`Output` types
- **Extension Traits**: `ToolPortExt` for `.chain()`, `.with_timeout()`, `.filter()`

```rust
// Compile-time validation: build() only in Complete state
let descriptor = TypedToolBuilder::new("search")
    .with_description("Search the web")
    .with_schema(json!({"type": "object"}))
    .build();

// Typed tool with associated types
impl TypedToolAdapter for SearchTool {
    type Input = SearchInput;
    type Output = SearchOutput;
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, ToolError> { ... }
}
```

### bob-runtime

Orchestration layer with 6-state turn FSM: Start → BuildPrompt → LlmInfer → ParseAction → CallTool → Done.

```rust
// Tower service composition
let service = tools.into_tool_service()
    .with_timeout(Duration::from_secs(15))
    .with_rate_limit(10, Duration::from_secs(1));

// Type-safe runtime builder
let runtime = TypedRuntimeBuilder::new()
    .with_llm(llm)
    .with_default_model("gpt-4o-mini")
    .build();
```

### bob-skills

Skill management system with deterministic selection and frontmatter metadata:

- **Loader**: Parse `SKILL.md` files with YAML frontmatter
- **Registry**: Store and query skills by name, tags, and compatibility
- **Selector**: Choose skills based on context and token budget
- **Compose**: Merge multiple skills into a single prompt

## Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                     CLI Agent (bin)                         │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              DefaultAgentRuntime                    │    │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────────────┐   │    │
│  │  │Scheduler │→ │Prompt    │→ │Action Parser     │   │    │
│  │  │  FSM     │  │Builder   │  │                  │   │    │
│  │  └──────────┘  └──────────┘  └──────────────────┘   │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
            ↓ uses ports (traits) from bob-core
┌─────────────────────────────────────────────────────────────┐
│                  Adapters (bob-adapters)                    │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐     │
│  │GenAI LLM │  │MCP Tools │  │In-Memory │  │ Tracing  │     │
│  │          │  │          │  │  Store   │  │  Events  │     │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘     │
└─────────────────────────────────────────────────────────────┘
```

**Dependency rule**: `bob-runtime` NEVER imports `bob-adapters`. `bob-core` has ZERO internal workspace dependencies.

## Quick Start

### Installation

```bash
# From source
git clone https://github.com/longcipher/bob.git && cd bob
cargo build --release
cargo run --release --bin bob-cli -- --config agent.toml

# Or install directly
cargo install --git https://github.com/longcipher/bob --package cli-agent --bin bob-cli
```

### Configuration

Create `agent.toml`:

```toml
[runtime]
default_model = "openai:gpt-4o-mini"
max_steps = 12
turn_timeout_ms = 90000

# MCP tool servers
[mcp]
[[mcp.servers]]
id = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]

# Skill sources
[skills]
max_selected = 3
[[skills.sources]]
type = "directory"
path = "./skills"

# Session persistence
[store]
path = "./.bob/sessions"
```

### Environment Variables

```bash
export OPENAI_API_KEY="sk-..."      # OpenAI
export ANTHROPIC_API_KEY="sk-ant-..." # Anthropic
export GEMINI_API_KEY="..."          # Google
```

### Run

```bash
cargo run --bin bob-cli -- --config agent.toml
```

## CLI Usage

### Global Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--config` | `-c` | `agent.toml` | Path to configuration file |

### Subcommands

#### `repl` — Interactive REPL

```bash
bob-cli repl
```

**REPL Commands:**

| Command | Alias | Description |
|---------|-------|-------------|
| `/help` | `/h` | Show available commands |
| `/new` | `/reset` | Start new session |
| `/quit` | `/q` | Exit REPL |
| `/tools` | — | List available tools |
| `/tool <name>` | — | Describe a specific tool |
| `/tape search <query>` | — | Search tape history |
| `/tape info` | — | Show tape statistics |
| `/handoff [name] | — | Create handoff checkpoint |
| `/usage` | — | Show token usage |

#### `skills` — Skill Management

```bash
# List skills
bob-cli skills list ./skills --recursive --check

# Validate a skill
bob-cli skills validate ./my-skill

# Read properties (json/yaml/toml)
bob-cli skills read-properties ./my-skill --format yaml

# Generate XML prompt block
bob-cli skills to-prompt ./skill1 ./skill2
```

**`skills list` Options:**

| Option | Short | Description |
|--------|-------|-------------|
| `--recursive` | `-r` | Search subdirectories |
| `--check` | `-c` | Validate each skill |
| `--failed` | `-f` | Show only invalid skills |
| `--long` | `-l` | Show full details |
| `--paths` | `-p` | Show only paths |
| `--names` | `-n` | Show only names |
| `--json` | — | Output as JSON |

## Development

```bash
just format    # Format code
just lint      # Run lints
just test      # Run tests
just ci        # Full CI check
```

## License

Apache-2.0. See [LICENSE.md](LICENSE.md).
