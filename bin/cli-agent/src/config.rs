//! Configuration types for the CLI agent, loaded from `agent.toml`.

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level agent configuration.
#[derive(Debug, Deserialize)]
pub(crate) struct AgentConfig {
    pub runtime: RuntimeConfig,
    #[expect(dead_code, reason = "reserved for v2 LLM retry/stream config")]
    pub llm: Option<LlmConfig>,
    pub policy: Option<PolicyConfig>,
    pub mcp: Option<McpConfig>,
    pub skills: Option<SkillsConfig>,
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
    /// Model context size in tokens for budget ratio calculations.
    pub model_context_tokens: Option<usize>,
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
pub(crate) struct PolicyConfig {
    /// Tool names to deny.
    pub deny_tools: Option<Vec<String>>,
    /// Tool names allowed by runtime policy.
    pub allow_tools: Option<Vec<String>>,
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
    /// Transport type (defaults to `stdio`).
    #[serde(default)]
    pub transport: McpTransport,
    /// Executable command to spawn.
    pub command: String,
    /// Arguments to pass to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables for the child process.
    pub env: Option<HashMap<String, String>>,
    /// Per-tool call timeout in ms.
    pub tool_timeout_ms: Option<u64>,
}

/// Skill loading configuration.
#[derive(Debug, Deserialize)]
pub(crate) struct SkillsConfig {
    pub sources: Vec<SkillSourceEntry>,
    pub max_selected: Option<usize>,
    pub token_budget_tokens: Option<usize>,
    pub token_budget_ratio: Option<f64>,
}

/// One skills source entry.
#[derive(Debug, Deserialize)]
pub(crate) struct SkillSourceEntry {
    #[serde(rename = "type", alias = "source_type")]
    pub source_type: SkillSourceType,
    pub path: String,
    pub recursive: Option<bool>,
}

/// Supported MCP transports.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpTransport {
    #[default]
    Stdio,
}

/// Supported skill source kinds.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SkillSourceType {
    Directory,
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

/// Resolve `${VAR_NAME}` placeholders from the current process environment.
///
/// Plain strings are returned unchanged. Missing variables are treated as
/// configuration errors.
pub(crate) fn resolve_env_placeholders(raw: &str) -> eyre::Result<String> {
    if let Some(var_name) = raw.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
        let value = std::env::var(var_name).map_err(|_| {
            eyre::eyre!("environment variable '{var_name}' is not set for placeholder '{raw}'")
        })?;
        Ok(value)
    } else {
        Ok(raw.to_string())
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_toml() -> eyre::Result<()> {
        let toml_str = r#"
[runtime]
default_model = "openai:gpt-4o-mini"
"#;
        let cfg: AgentConfig = config::Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .and_then(|c| c.try_deserialize())?;

        assert_eq!(cfg.runtime.default_model, "openai:gpt-4o-mini");
        assert!(cfg.mcp.is_none());
        Ok(())
    }

    #[test]
    fn parse_full_toml() -> eyre::Result<()> {
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
allow_tools = ["local/read_file"]

[[mcp.servers]]
id = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
tool_timeout_ms = 15000

[skills]
max_selected = 2
token_budget_tokens = 1200
token_budget_ratio = 0.10

[[skills.sources]]
type = "directory"
path = "./skills"
recursive = false
"#;
        let cfg: AgentConfig = config::Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .and_then(|c| c.try_deserialize())?;

        assert_eq!(cfg.runtime.max_steps, Some(12));
        let mcp = cfg.mcp.as_ref().ok_or_else(|| eyre::eyre!("mcp config should exist"))?;
        let servers = &mcp.servers;
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].id, "filesystem");
        assert_eq!(servers[0].transport, McpTransport::Stdio);
        assert_eq!(servers[0].args.len(), 3);
        assert_eq!(
            cfg.policy.as_ref().and_then(|p| p.allow_tools.clone()),
            Some(vec!["local/read_file".to_string()])
        );
        assert_eq!(cfg.skills.as_ref().and_then(|s| s.max_selected), Some(2));
        assert_eq!(cfg.skills.as_ref().and_then(|s| s.token_budget_tokens), Some(1200));
        assert_eq!(cfg.skills.as_ref().and_then(|s| s.token_budget_ratio), Some(0.10));
        Ok(())
    }

    #[test]
    fn parse_skills_toml() -> eyre::Result<()> {
        let toml_str = r#"
[runtime]
default_model = "openai:gpt-4o-mini"

[skills]
max_selected = 1
token_budget_tokens = 800
token_budget_ratio = 0.25

[[skills.sources]]
type = "directory"
path = "./skills"
recursive = true
"#;
        let cfg: AgentConfig = config::Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .and_then(|c| c.try_deserialize())?;

        let skills = cfg.skills.ok_or_else(|| eyre::eyre!("skills config should exist"))?;
        assert_eq!(skills.max_selected, Some(1));
        assert_eq!(skills.token_budget_tokens, Some(800));
        assert_eq!(skills.token_budget_ratio, Some(0.25));
        assert_eq!(skills.sources.len(), 1);
        assert_eq!(skills.sources[0].source_type, SkillSourceType::Directory);
        assert_eq!(skills.sources[0].path, "./skills");
        assert_eq!(skills.sources[0].recursive, Some(true));
        Ok(())
    }

    #[test]
    fn parse_skills_toml_supports_source_type_alias() -> eyre::Result<()> {
        let toml_str = r#"
[runtime]
default_model = "openai:gpt-4o-mini"

[skills]
max_selected = 1

[[skills.sources]]
source_type = "directory"
path = "./skills"
recursive = false
"#;
        let cfg: AgentConfig = config::Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .and_then(|c| c.try_deserialize())?;

        let skills = cfg.skills.ok_or_else(|| eyre::eyre!("skills config should exist"))?;
        assert_eq!(skills.sources.len(), 1);
        assert_eq!(skills.sources[0].source_type, SkillSourceType::Directory);
        Ok(())
    }

    #[test]
    fn interpolate_env_placeholder() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let resolved = resolve_env_placeholders("${HOME}");
        assert_eq!(resolved.ok(), Some(home));
    }

    #[test]
    fn interpolate_plain_string_passthrough() {
        let resolved = resolve_env_placeholders("plain-value");
        assert_eq!(resolved.ok().as_deref(), Some("plain-value"));
    }

    #[test]
    fn interpolate_missing_env_fails() {
        let resolved = resolve_env_placeholders("${__BOB_TEST_MISSING_ENV__}");
        assert!(resolved.is_err(), "missing env placeholder must fail");
    }
}
