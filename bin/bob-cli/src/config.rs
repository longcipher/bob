//! Configuration types for the CLI agent, loaded from `agent.toml`.

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level agent configuration.
#[derive(Debug, Deserialize)]
pub(crate) struct AgentConfig {
    pub runtime: RuntimeConfig,
    #[expect(dead_code, reason = "reserved for v2 LLM retry/stream config")]
    pub llm: Option<LlmConfig>,
    pub store: Option<StoreConfig>,
    pub policy: Option<PolicyConfig>,
    pub approval: Option<ApprovalConfig>,
    pub mcp: Option<McpConfig>,
    pub skills: Option<SkillsConfig>,
    pub cost: Option<CostConfig>,
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
    /// Dispatch strategy for model action extraction.
    pub dispatch_mode: Option<RuntimeDispatchMode>,
}

/// Runtime dispatch mode.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeDispatchMode {
    PromptGuided,
    NativePreferred,
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

/// Persistent session store configuration.
#[derive(Debug, Deserialize)]
pub(crate) struct StoreConfig {
    /// Directory used for session snapshots. If omitted, in-memory store is used.
    pub path: String,
}

/// Behavioural policy overrides.
#[derive(Debug, Deserialize)]
pub(crate) struct PolicyConfig {
    /// Tool names to deny.
    pub deny_tools: Option<Vec<String>>,
    /// Tool names allowed by runtime policy.
    pub allow_tools: Option<Vec<String>>,
    /// Deny tool calls when neither runtime nor request allowlist is provided.
    pub default_deny: Option<bool>,
}

/// Approval policy configuration.
#[derive(Debug, Deserialize)]
pub(crate) struct ApprovalConfig {
    /// Approval mode.
    pub mode: Option<ApprovalMode>,
    /// Tool names denied by approval policy in allow-all mode.
    pub deny_tools: Option<Vec<String>>,
}

/// Supported approval modes.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ApprovalMode {
    AllowAll,
    DenyAll,
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

/// Cost control configuration.
#[derive(Debug, Deserialize)]
pub(crate) struct CostConfig {
    /// Optional per-session total-token ceiling.
    pub session_token_budget: Option<u64>,
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
    validate_config(&cfg)?;
    Ok(cfg)
}

fn validate_config(cfg: &AgentConfig) -> eyre::Result<()> {
    if cfg.runtime.default_model.trim().is_empty() {
        return Err(eyre::eyre!("runtime.default_model must not be empty"));
    }

    if matches!(cfg.runtime.max_steps, Some(0)) {
        return Err(eyre::eyre!("runtime.max_steps must be greater than 0"));
    }
    if matches!(cfg.runtime.turn_timeout_ms, Some(0)) {
        return Err(eyre::eyre!("runtime.turn_timeout_ms must be greater than 0"));
    }
    if matches!(cfg.runtime.model_context_tokens, Some(0)) {
        return Err(eyre::eyre!("runtime.model_context_tokens must be greater than 0"));
    }

    if let Some(ref mcp) = cfg.mcp {
        let mut ids = std::collections::HashSet::with_capacity(mcp.servers.len());
        for server in &mcp.servers {
            if server.id.trim().is_empty() {
                return Err(eyre::eyre!("mcp.servers[].id must not be empty"));
            }
            if !ids.insert(server.id.clone()) {
                return Err(eyre::eyre!("duplicate mcp server id '{}'", server.id));
            }
            if server.command.trim().is_empty() {
                return Err(eyre::eyre!("mcp server '{}' command must not be empty", server.id));
            }
            if matches!(server.tool_timeout_ms, Some(0)) {
                return Err(eyre::eyre!(
                    "mcp server '{}' tool_timeout_ms must be greater than 0",
                    server.id
                ));
            }
        }
    }

    if let Some(ref store) = cfg.store &&
        store.path.trim().is_empty()
    {
        return Err(eyre::eyre!("store.path must not be empty when store is configured"));
    }

    if let Some(ref skills) = cfg.skills {
        if matches!(skills.max_selected, Some(0)) {
            return Err(eyre::eyre!("skills.max_selected must be greater than 0"));
        }
        if matches!(skills.token_budget_tokens, Some(0)) {
            return Err(eyre::eyre!("skills.token_budget_tokens must be greater than 0"));
        }
        if let Some(ratio) = skills.token_budget_ratio &&
            (!(0.0..=1.0).contains(&ratio) || ratio == 0.0)
        {
            return Err(eyre::eyre!("skills.token_budget_ratio must satisfy 0.0 < ratio <= 1.0"));
        }
    }

    if matches!(cfg.cost.as_ref().and_then(|cost| cost.session_token_budget), Some(0)) {
        return Err(eyre::eyre!("cost.session_token_budget must be greater than 0"));
    }

    Ok(())
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
dispatch_mode = "prompt_guided"

[llm]
retry_max = 2
stream_default = false

[store]
path = "./.bob/sessions"

[policy]
deny_tools = []
allow_tools = ["local/read_file"]
default_deny = false

[approval]
mode = "allow_all"
deny_tools = ["local/shell_exec"]

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

[cost]
session_token_budget = 10000

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
        assert_eq!(cfg.runtime.dispatch_mode, Some(RuntimeDispatchMode::PromptGuided));
        let mcp = cfg.mcp.as_ref().ok_or_else(|| eyre::eyre!("mcp config should exist"))?;
        let servers = &mcp.servers;
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].id, "filesystem");
        assert_eq!(servers[0].transport, McpTransport::Stdio);
        assert_eq!(servers[0].args.len(), 3);
        assert_eq!(cfg.store.as_ref().map(|s| s.path.as_str()), Some("./.bob/sessions"));
        assert_eq!(
            cfg.policy.as_ref().and_then(|p| p.allow_tools.clone()),
            Some(vec!["local/read_file".to_string()])
        );
        assert_eq!(cfg.policy.as_ref().and_then(|p| p.default_deny), Some(false));
        assert_eq!(cfg.approval.as_ref().and_then(|p| p.mode), Some(ApprovalMode::AllowAll));
        assert_eq!(
            cfg.approval.as_ref().and_then(|p| p.deny_tools.clone()),
            Some(vec!["local/shell_exec".to_string()])
        );
        assert_eq!(cfg.skills.as_ref().and_then(|s| s.max_selected), Some(2));
        assert_eq!(cfg.skills.as_ref().and_then(|s| s.token_budget_tokens), Some(1200));
        assert_eq!(cfg.skills.as_ref().and_then(|s| s.token_budget_ratio), Some(0.10));
        assert_eq!(cfg.cost.as_ref().and_then(|c| c.session_token_budget), Some(10_000));
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

    #[test]
    fn reject_zero_runtime_limits() -> eyre::Result<()> {
        let toml_str = r#"
[runtime]
default_model = "openai:gpt-4o-mini"
max_steps = 0
turn_timeout_ms = 0
"#;
        let cfg: AgentConfig = config::Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .and_then(|c| c.try_deserialize())?;

        let err = validate_config(&cfg);
        assert!(err.is_err(), "zero runtime limits should be rejected");
        let msg = err.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("max_steps"));
        Ok(())
    }

    #[test]
    fn reject_duplicate_mcp_server_ids() -> eyre::Result<()> {
        let toml_str = r#"
[runtime]
default_model = "openai:gpt-4o-mini"

[mcp]
[[mcp.servers]]
id = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]

[[mcp.servers]]
id = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
"#;
        let cfg: AgentConfig = config::Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .and_then(|c| c.try_deserialize())?;

        let err = validate_config(&cfg);
        assert!(err.is_err(), "duplicate MCP server ids should be rejected");
        let msg = err.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("duplicate mcp server id"));
        Ok(())
    }

    #[test]
    fn reject_empty_store_path() -> eyre::Result<()> {
        let toml_str = r#"
[runtime]
default_model = "openai:gpt-4o-mini"

[store]
path = ""
"#;
        let cfg: AgentConfig = config::Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .and_then(|c| c.try_deserialize())?;

        let err = validate_config(&cfg);
        assert!(err.is_err(), "empty store path should be rejected");
        let msg = err.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("store.path"));
        Ok(())
    }
}
