//! Configuration types for the CLI agent, loaded from `agent.toml`.

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level agent configuration.
#[derive(Debug, Deserialize)]
pub(crate) struct AgentConfig {
    pub runtime: RuntimeConfig,
    #[expect(dead_code, reason = "reserved for v2 LLM retry/stream config")]
    pub llm: Option<LlmConfig>,
    #[expect(dead_code, reason = "reserved for v2 deny-list policy")]
    pub policy: Option<PolicyConfig>,
    pub mcp: Option<McpConfig>,
}

/// Core runtime settings.
#[derive(Debug, Deserialize)]
pub(crate) struct RuntimeConfig {
    /// Default model identifier (e.g. `"openai:gpt-4o-mini"`).
    pub default_model: String,
    /// Maximum scheduler steps per turn (overrides default 12).
    pub max_steps: Option<u32>,
    /// Turn timeout in milliseconds (overrides default 90_000).
    pub turn_timeout_ms: Option<u64>,
}

/// LLM-specific settings.
#[derive(Debug, Deserialize)]
#[expect(dead_code, reason = "parsed from TOML, wired in v2")]
pub(crate) struct LlmConfig {
    /// Max retries on transient LLM errors.
    pub retry_max: Option<u32>,
    /// Whether streaming is default.
    pub stream_default: Option<bool>,
}

/// Behavioural policy overrides.
#[derive(Debug, Deserialize)]
#[expect(dead_code, reason = "parsed from TOML, wired in v2")]
pub(crate) struct PolicyConfig {
    /// Tool names to deny.
    pub deny_tools: Option<Vec<String>>,
}

/// MCP server configuration.
#[derive(Debug, Deserialize)]
pub(crate) struct McpConfig {
    pub servers: Vec<McpServerEntry>,
}

/// A single MCP server to connect to.
#[derive(Debug, Deserialize)]
pub(crate) struct McpServerEntry {
    /// Logical name used as namespace prefix.
    pub id: String,
    /// Transport type (only `"stdio"` supported in v1).
    #[expect(dead_code, reason = "only stdio supported in v1, field reserved")]
    pub transport: String,
    /// Executable command to spawn.
    pub command: String,
    /// Arguments to pass to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables for the child process.
    pub env: Option<HashMap<String, String>>,
    /// Per-tool call timeout in ms.
    #[expect(dead_code, reason = "wired into per-call timeout in v2")]
    pub tool_timeout_ms: Option<u64>,
}

/// Load configuration from a TOML file at the given path.
///
/// # Errors
/// Returns an error if the file cannot be read or parsed.
pub(crate) fn load_config(path: &str) -> eyre::Result<AgentConfig> {
    let settings = config::Config::builder()
        .add_source(config::File::with_name(path).required(true))
        .build()?;
    let cfg: AgentConfig = settings.try_deserialize()?;
    Ok(cfg)
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_toml() {
        let toml_str = r#"
[runtime]
default_model = "openai:gpt-4o-mini"
"#;
        let cfg: AgentConfig = config::Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .and_then(|c| c.try_deserialize())
            .ok()
            .and_then(|c: AgentConfig| Some(c))
            .unwrap();

        assert_eq!(cfg.runtime.default_model, "openai:gpt-4o-mini");
        assert!(cfg.mcp.is_none());
    }

    #[test]
    fn parse_full_toml() {
        let toml_str = r#"
[runtime]
default_model = "openai:gpt-4o-mini"
max_steps = 12
turn_timeout_ms = 90000

[llm]
retry_max = 2
stream_default = false

[policy]
deny_tools = []

[[mcp.servers]]
id = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
tool_timeout_ms = 15000
"#;
        let cfg: AgentConfig = config::Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .and_then(|c| c.try_deserialize())
            .ok()
            .and_then(|c: AgentConfig| Some(c))
            .unwrap();

        assert_eq!(cfg.runtime.max_steps, Some(12));
        let servers = &cfg.mcp.as_ref().unwrap().servers;
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].id, "filesystem");
        assert_eq!(servers[0].args.len(), 3);
    }
}
