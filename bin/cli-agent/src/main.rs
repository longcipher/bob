//! # Bob CLI Agent
//!
//! Command-line interface for the [Bob Agent Framework](https://github.com/longcipher/bob).
//!
//! ## Overview
//!
//! `cli-agent` is a command-line interface for Bob, an LLM-powered coding assistant. It provides:
//!
//! - **Interactive REPL**: Chat with the AI agent through a terminal interface
//! - **Multi-Model Support**: Works with OpenAI, Anthropic, Google, and other LLM providers
//! - **Tool Integration**: Connect to MCP servers for file operations, shell commands, and more
//! - **Skill System**: Load and apply predefined skills for specialized tasks
//!
//! ## Usage
//!
//! ```bash
//! # Start the agent
//! cargo run --bin cli-agent -- --config agent.toml
//!
//! # Or with a custom config path
//! cargo run --bin cli-agent -- --config /path/to/config.toml
//! ```
//!
//! ## Configuration
//!
//! The agent reads configuration from a TOML file (default: `agent.toml`):
//!
//! ```toml
//! [runtime]
//! default_model = "openai:gpt-4o-mini"
//! max_steps = 12
//!
//! [mcp]
//! [[mcp.servers]]
//! id = "filesystem"
//! command = "npx"
//! args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
//! ```
//!
//! ## Environment Variables
//!
//! Set your LLM provider API key:
//!
//! ```bash
//! export OPENAI_API_KEY="sk-..."
//! export ANTHROPIC_API_KEY="sk-ant-..."
//! ```
//!
//! ## REPL Commands
//!
//! - Type your message and press **Enter** to send
//! - `/quit` or `/exit` to exit the agent
//!
//! ## Architecture
//!
//! The CLI agent is the composition root that:
//! 1. Loads configuration from `agent.toml`
//! 2. Wires up adapters (LLM, tools, storage, events)
//! 3. Creates the runtime
//! 4. Runs the REPL loop
//!
//! ## Related Crates
//!
//! - [`bob_core`] - Domain types and ports
//! - [`bob_runtime`] - Runtime orchestration
//! - [`bob_adapters`] - Adapter implementations
//!
//! [`bob_core`]: https://docs.rs/bob-core
//! [`bob_runtime`]: https://docs.rs/bob-runtime
//! [`bob_adapters`]: https://docs.rs/bob-adapters

mod config;

use std::{collections::HashMap, sync::Arc};

use bob_adapters::{
    observe::TracingEventSink,
    skills_agent::{SkillPromptComposer, SkillSelectionPolicy, SkillSourceConfig},
    store_memory::InMemorySessionStore,
};
use bob_runtime::{AgentRuntime, DefaultAgentRuntime, composite::CompositeToolPort};
use clap::Parser;
use eyre::WrapErr;

use crate::config::{AgentConfig, resolve_env_placeholders};

/// Bob CLI Agent — an LLM-powered coding assistant.
#[derive(Parser, Debug)]
#[command(name = "bob", version, about)]
struct Cli {
    /// Path to the agent configuration file.
    #[arg(short, long, default_value = "agent.toml")]
    config: String,
}

const DEFAULT_TOOL_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_SKILLS_TOKEN_BUDGET: usize = 1_800;
const DEFAULT_MODEL_CONTEXT_TOKENS: usize = 128_000;

#[derive(Debug, Clone)]
struct SkillsRuntimeContext {
    composer: SkillPromptComposer,
    selection_policy: SkillSelectionPolicy,
}

/// A no-op tool port that advertises zero tools and rejects all calls.
#[derive(Debug)]
struct NoOpToolPort;

#[async_trait::async_trait]
impl bob_adapters::core::ports::ToolPort for NoOpToolPort {
    async fn list_tools(
        &self,
    ) -> Result<Vec<bob_adapters::core::types::ToolDescriptor>, bob_adapters::core::error::ToolError>
    {
        Ok(vec![])
    }

    async fn call_tool(
        &self,
        call: bob_adapters::core::types::ToolCall,
    ) -> Result<bob_adapters::core::types::ToolResult, bob_adapters::core::error::ToolError> {
        Err(bob_adapters::core::error::ToolError::Execution(format!(
            "no tool port configured, cannot call '{}'",
            call.name
        )))
    }
}

/// Applies per-server tool timeout policies around an inner tool port.
struct TimedToolPort {
    inner: Arc<dyn bob_adapters::core::ports::ToolPort>,
    timeout_ms: u64,
}

impl std::fmt::Debug for TimedToolPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimedToolPort")
            .field("timeout_ms", &self.timeout_ms)
            .finish_non_exhaustive()
    }
}

impl TimedToolPort {
    fn new(inner: Arc<dyn bob_adapters::core::ports::ToolPort>, timeout_ms: u64) -> Self {
        Self { inner, timeout_ms }
    }
}

#[async_trait::async_trait]
impl bob_adapters::core::ports::ToolPort for TimedToolPort {
    async fn list_tools(
        &self,
    ) -> Result<Vec<bob_adapters::core::types::ToolDescriptor>, bob_adapters::core::error::ToolError>
    {
        self.inner.list_tools().await
    }

    async fn call_tool(
        &self,
        call: bob_adapters::core::types::ToolCall,
    ) -> Result<bob_adapters::core::types::ToolResult, bob_adapters::core::error::ToolError> {
        let tool_name = call.name.clone();
        match tokio::time::timeout(
            std::time::Duration::from_millis(self.timeout_ms),
            self.inner.call_tool(call),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(bob_adapters::core::error::ToolError::Timeout { name: tool_name }),
        }
    }
}

/// Build the runtime from the loaded config.
async fn build_runtime(
    cfg: &AgentConfig,
) -> eyre::Result<(Arc<dyn AgentRuntime>, Option<SkillsRuntimeContext>)> {
    // LLM adapter
    let client = genai::Client::default();
    let llm: Arc<dyn bob_adapters::core::ports::LlmPort> =
        Arc::new(bob_adapters::llm_genai::GenAiLlmAdapter::new(client));

    // Tool adapter
    let tools: Arc<dyn bob_adapters::core::ports::ToolPort> =
        if let Some(mcp_cfg) = cfg.mcp.as_ref() {
            if mcp_cfg.servers.is_empty() {
                Arc::new(NoOpToolPort)
            } else if mcp_cfg.servers.len() == 1 {
                let entry = &mcp_cfg.servers[0];
                let env_vec = resolve_mcp_env(entry.env.as_ref())?;
                let adapter = bob_adapters::mcp_rmcp::McpToolAdapter::connect_stdio(
                    &entry.id,
                    &entry.command,
                    &entry.args,
                    &env_vec,
                )
                .await
                .wrap_err_with(|| format!("failed to connect MCP server '{}'", entry.id))?;
                let inner: Arc<dyn bob_adapters::core::ports::ToolPort> = Arc::new(adapter);
                Arc::new(TimedToolPort::new(
                    inner,
                    entry.tool_timeout_ms.unwrap_or(DEFAULT_TOOL_TIMEOUT_MS),
                ))
            } else {
                // Multiple servers → CompositeToolPort
                let mut ports: Vec<(String, Arc<dyn bob_adapters::core::ports::ToolPort>)> =
                    Vec::with_capacity(mcp_cfg.servers.len());
                for entry in &mcp_cfg.servers {
                    let env_vec = resolve_mcp_env(entry.env.as_ref())?;
                    let adapter = bob_adapters::mcp_rmcp::McpToolAdapter::connect_stdio(
                        &entry.id,
                        &entry.command,
                        &entry.args,
                        &env_vec,
                    )
                    .await
                    .wrap_err_with(|| format!("failed to connect MCP server '{}'", entry.id))?;
                    let inner: Arc<dyn bob_adapters::core::ports::ToolPort> = Arc::new(adapter);
                    let timed = Arc::new(TimedToolPort::new(
                        inner,
                        entry.tool_timeout_ms.unwrap_or(DEFAULT_TOOL_TIMEOUT_MS),
                    ));
                    ports.push((entry.id.clone(), timed));
                }
                Arc::new(CompositeToolPort::new(ports))
            }
        } else {
            Arc::new(NoOpToolPort)
        };

    // Session store
    let store: Arc<dyn bob_adapters::core::ports::SessionStore> =
        Arc::new(InMemorySessionStore::new());

    // Event sink
    let events: Arc<dyn bob_adapters::core::ports::EventSink> = Arc::new(TracingEventSink::new());

    // Turn policy
    let tool_timeout_ms = cfg.mcp.as_ref().map_or(DEFAULT_TOOL_TIMEOUT_MS, |mcp_cfg| {
        mcp_cfg
            .servers
            .iter()
            .map(|server| server.tool_timeout_ms.unwrap_or(DEFAULT_TOOL_TIMEOUT_MS))
            .max()
            .unwrap_or(DEFAULT_TOOL_TIMEOUT_MS)
    });

    let policy = bob_adapters::core::types::TurnPolicy {
        max_steps: cfg.runtime.max_steps.unwrap_or(12),
        turn_timeout_ms: cfg.runtime.turn_timeout_ms.unwrap_or(90_000),
        tool_timeout_ms,
        ..bob_adapters::core::types::TurnPolicy::default()
    };

    let rt = DefaultAgentRuntime {
        llm,
        tools,
        store,
        events,
        default_model: cfg.runtime.default_model.clone(),
        policy,
    };

    let skills_composer = build_skills_composer(cfg)?;

    Ok((Arc::new(rt), skills_composer))
}

fn resolve_mcp_env(
    env: Option<&std::collections::HashMap<String, String>>,
) -> eyre::Result<Vec<(String, String)>> {
    let Some(env) = env else {
        return Ok(Vec::new());
    };

    let mut resolved = Vec::with_capacity(env.len());
    for (key, value) in env {
        let parsed = resolve_env_placeholders(value)
            .wrap_err_with(|| format!("failed to resolve env placeholder for key '{key}'"))?;
        resolved.push((key.clone(), parsed));
    }
    Ok(resolved)
}

fn build_skills_composer(cfg: &AgentConfig) -> eyre::Result<Option<SkillsRuntimeContext>> {
    let Some(skills_cfg) = cfg.skills.as_ref() else {
        return Ok(None);
    };

    if skills_cfg.sources.is_empty() {
        return Ok(None);
    }

    let mut sources = Vec::with_capacity(skills_cfg.sources.len());
    for source in &skills_cfg.sources {
        if source.source_type != "directory" {
            return Err(eyre::eyre!(
                "unsupported skills source type '{}', only 'directory' is supported",
                source.source_type
            ));
        }

        sources.push(SkillSourceConfig {
            path: std::path::PathBuf::from(&source.path),
            recursive: source.recursive.unwrap_or(false),
        });
    }

    let composer =
        SkillPromptComposer::from_sources(&sources, skills_cfg.max_selected.unwrap_or(3))
            .wrap_err("failed to load skills from configured sources")?;

    let (deny_tools, allow_tools) = cfg.policy.as_ref().map_or_else(
        || (Vec::new(), None),
        |policy| (policy.deny_tools.clone().unwrap_or_default(), policy.allow_tools.clone()),
    );
    let token_budget_tokens = resolve_skills_token_budget(&cfg.runtime, skills_cfg)?;
    let selection_policy = SkillSelectionPolicy { deny_tools, allow_tools, token_budget_tokens };

    Ok(Some(SkillsRuntimeContext { composer, selection_policy }))
}

fn resolve_skills_token_budget(
    runtime: &config::RuntimeConfig,
    skills: &config::SkillsConfig,
) -> eyre::Result<usize> {
    if let Some(tokens) = skills.token_budget_tokens {
        return Ok(tokens.max(1));
    }

    if let Some(ratio) = skills.token_budget_ratio {
        if !(0.0..=1.0).contains(&ratio) || ratio == 0.0 {
            return Err(eyre::eyre!(
                "invalid skills.token_budget_ratio '{ratio}', expected 0.0 < ratio <= 1.0"
            ));
        }

        let context_tokens = runtime.model_context_tokens.unwrap_or(DEFAULT_MODEL_CONTEXT_TOKENS);
        let budget = (ratio * context_tokens as f64).round() as usize;
        return Ok(budget.max(1));
    }

    Ok(DEFAULT_SKILLS_TOKEN_BUDGET)
}

/// Run the interactive REPL loop.
#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI REPL must use stdout/stderr for user interaction"
)]
async fn repl(
    runtime: Arc<dyn AgentRuntime>,
    model: &str,
    skills_context: Option<&SkillsRuntimeContext>,
    policy: Option<&config::PolicyConfig>,
) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    let session_id = "cli-session".to_string();

    eprintln!("Bob agent ready  (model: {model})");
    eprintln!("Type a message and press Enter. /quit to exit.\n");

    loop {
        eprint!("> ");
        // Flush stderr so prompt appears before input.
        let _ = tokio::io::AsyncWriteExt::flush(&mut tokio::io::stderr()).await;

        let line = match lines.next_line().await {
            Ok(Some(l)) => l,
            Ok(None) => break, // EOF
            Err(e) => {
                eprintln!("stdin error: {e}");
                break;
            }
        };

        let input = line.trim().to_string();

        if input.is_empty() {
            continue;
        }
        if input.eq_ignore_ascii_case("/quit") || input.eq_ignore_ascii_case("/exit") {
            break;
        }

        let metadata = build_request_metadata(&input, skills_context, policy);

        let req = bob_adapters::core::types::AgentRequest {
            input,
            session_id: session_id.clone(),
            model: None,
            metadata,
            cancel_token: None,
        };

        match runtime.run(req).await {
            Ok(bob_adapters::core::types::AgentRunResult::Finished(resp)) => {
                println!("{}", resp.content);
            }
            Err(e) => {
                eprintln!("Error: {e}");
            }
        }
    }

    eprintln!("\nGoodbye!");
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // Initialise tracing subscriber with env filter support.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    let cfg = config::load_config(&cli.config)
        .wrap_err_with(|| format!("failed to load config from '{}'", cli.config))?;

    let (runtime, skills_context) = build_runtime(&cfg).await?;

    repl(runtime, &cfg.runtime.default_model, skills_context.as_ref(), cfg.policy.as_ref()).await;

    Ok(())
}

fn build_request_metadata(
    input: &str,
    skills_context: Option<&SkillsRuntimeContext>,
    policy: Option<&config::PolicyConfig>,
) -> HashMap<String, serde_json::Value> {
    let mut metadata = HashMap::new();

    if let Some(policy) = policy {
        if let Some(deny_tools) = &policy.deny_tools &&
            !deny_tools.is_empty()
        {
            metadata.insert(
                "deny_tools".to_string(),
                serde_json::Value::Array(
                    deny_tools.iter().cloned().map(serde_json::Value::String).collect(),
                ),
            );
        }

        if let Some(allow_tools) = &policy.allow_tools &&
            !allow_tools.is_empty()
        {
            metadata.insert(
                "allow_tools".to_string(),
                serde_json::Value::Array(
                    allow_tools.iter().cloned().map(serde_json::Value::String).collect(),
                ),
            );
        }
    }

    if let Some(ctx) = skills_context {
        let rendered =
            ctx.composer.render_bundle_for_input_with_policy(input, &ctx.selection_policy);
        if !rendered.prompt.is_empty() {
            metadata
                .insert("skills_prompt".to_string(), serde_json::Value::String(rendered.prompt));
            metadata.insert(
                "selected_skills".to_string(),
                serde_json::Value::Array(
                    rendered
                        .selected_skill_names
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
            if !rendered.selected_allowed_tools.is_empty() {
                metadata.insert(
                    "skill_allowed_tools".to_string(),
                    serde_json::Value::Array(
                        rendered
                            .selected_allowed_tools
                            .into_iter()
                            .map(serde_json::Value::String)
                            .collect(),
                    ),
                );
            }
        }
    }

    metadata
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PolicyConfig, RuntimeConfig, SkillSourceEntry, SkillsConfig};

    #[test]
    fn resolves_skills_budget_from_ratio() {
        let runtime = RuntimeConfig {
            default_model: "openai:gpt-4o-mini".to_string(),
            max_steps: Some(12),
            turn_timeout_ms: Some(90_000),
            model_context_tokens: Some(20_000),
        };
        let skills = SkillsConfig {
            sources: vec![SkillSourceEntry {
                source_type: "directory".to_string(),
                path: "./skills".to_string(),
                recursive: Some(false),
            }],
            max_selected: Some(3),
            token_budget_tokens: None,
            token_budget_ratio: Some(0.10),
        };

        let budget = resolve_skills_token_budget(&runtime, &skills).expect("ratio budget");
        assert_eq!(budget, 2_000);
    }

    #[test]
    fn invalid_ratio_is_rejected() {
        let runtime = RuntimeConfig {
            default_model: "openai:gpt-4o-mini".to_string(),
            max_steps: None,
            turn_timeout_ms: None,
            model_context_tokens: None,
        };
        let skills = SkillsConfig {
            sources: vec![],
            max_selected: None,
            token_budget_tokens: None,
            token_budget_ratio: Some(1.2),
        };

        let err = resolve_skills_token_budget(&runtime, &skills)
            .expect_err("ratio > 1.0 must be rejected");
        assert!(err.to_string().contains("token_budget_ratio"));
    }

    #[test]
    fn metadata_includes_policy_without_skills_context() {
        let policy = PolicyConfig {
            deny_tools: Some(vec!["local/shell_exec".to_string()]),
            allow_tools: Some(vec!["local/read_file".to_string()]),
        };

        let metadata = build_request_metadata("hello", None, Some(&policy));

        let deny = metadata
            .get("deny_tools")
            .and_then(serde_json::Value::as_array)
            .expect("deny_tools should exist");
        let allow = metadata
            .get("allow_tools")
            .and_then(serde_json::Value::as_array)
            .expect("allow_tools should exist");

        assert_eq!(deny.len(), 1);
        assert_eq!(allow.len(), 1);
        assert_eq!(deny[0], serde_json::Value::String("local/shell_exec".to_string()));
        assert_eq!(allow[0], serde_json::Value::String("local/read_file".to_string()));
    }
}
