# cli-agent

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

CLI agent for the [Bob Agent Framework](https://github.com/longcipher/bob).

## Overview

`cli-agent` is a command-line interface for Bob, an LLM-powered coding assistant. It provides:

- **Interactive REPL**: Chat with the AI agent through a terminal interface
- **Multi-Model Support**: Works with OpenAI, Anthropic, Google, and other LLM providers
- **Tool Integration**: Connect to MCP servers for file operations, shell commands, and more
- **Skill System**: Load and apply predefined skills for specialized tasks

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/longcipher/bob
cd bob

# Build and run
cargo run --bin cli-agent -- --config agent.toml
```

### Binary Release

```bash
# Download from GitHub releases
# (Coming soon)
```

## Configuration

Create an `agent.toml` file in the project root:

```toml
[runtime]
default_model = "openai:gpt-4o-mini"
max_steps = 12
turn_timeout_ms = 90000
dispatch_mode = "native_preferred"

# Optional: Configure MCP servers
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

`dispatch_mode` supports `native_preferred` and `prompt_guided`.

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

## Usage

### Starting the Agent

```bash
cargo run --bin cli-agent
```

The agent will start an interactive REPL:

```text
Bob agent ready  (model: openai:gpt-4o-mini)
Type a message and press Enter. /quit to exit.

> Write a hello world program in Rust
```

### REPL Commands

- Type your message and press **Enter** to send
- `/quit` or `/exit` to exit the agent
- `/help` for available commands (coming soon)

### Example Session

```text
> Read the main.rs file and explain what it does

I'll read the main.rs file for you...

[uses filesystem tool to read the file]

The main.rs file implements...

> Now add error handling to that function

[agent modifies the code]

I've added error handling to the function. The changes include...
```

## Features

### Multi-Model Support

Works with any LLM provider supported by `genai`:

- OpenAI: `openai:gpt-4o`, `openai:gpt-4o-mini`
- Anthropic: `anthropic:claude-3-5-sonnet-20241022`
- Google: `google:gemini-2.0-flash-exp`
- Groq: `groq:llama-3.3-70b-versatile`
- And more...

### Tool Integration

Connect to MCP servers for extended capabilities:

- **Filesystem**: Read, write, and manage files
- **Shell**: Execute shell commands
- **Database**: Query databases
- **Custom**: Build your own MCP servers

### Skill System

Apply predefined skills for specialized tasks:

- Code review
- Documentation generation
- Test writing
- Refactoring

### Session Persistence

Sessions are persisted in-memory (development) or can be configured for persistent storage.

## Development

```bash
# Run in development mode
cargo run --bin cli-agent

# Build release binary
cargo build --bin cli-agent --release

# Run tests
cargo test -p cli-agent
```

## Architecture

The CLI agent is the composition root that:

1. Loads configuration from `agent.toml`
2. Wires up adapters (LLM, tools, storage, events)
3. Creates the runtime
4. Runs the REPL loop

See the [main.rs](src/main.rs) for the implementation.

## Related Crates

- **[bob-core](https://crates.io/crates/bob-core)** - Domain types and ports
- **[bob-runtime](https://crates.io/crates/bob-runtime)** - Runtime orchestration
- **[bob-adapters](https://crates.io/crates/bob-adapters)** - Adapter implementations

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../../LICENSE.md) for details.
