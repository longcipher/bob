//! Bob CLI Agent — composition root
//!
//! Loads `agent.toml`, wires adapters into [`DefaultAgentRuntime`],
//! and runs a stdin/stdout REPL loop.

mod config;

use std::{collections::HashMap, sync::Arc};

use bob_adapters::{observe::TracingEventSink, store_memory::InMemorySessionStore};
use bob_runtime::{AgentRuntime, DefaultAgentRuntime, composite::CompositeToolPort};
use clap::Parser;
use eyre::WrapErr;

use crate::config::AgentConfig;

/// Bob CLI Agent — an LLM-powered coding assistant.
#[derive(Parser, Debug)]
#[command(name = "bob", version, about)]
struct Cli {
    /// Path to the agent configuration file.
    #[arg(short, long, default_value = "agent.toml")]
    config: String,
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

/// Build the runtime from the loaded config.
async fn build_runtime(cfg: &AgentConfig) -> eyre::Result<Arc<dyn AgentRuntime>> {
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
                let env_vec: Vec<(String, String)> = entry
                    .env
                    .as_ref()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                let adapter = bob_adapters::mcp_rmcp::McpToolAdapter::connect_stdio(
                    &entry.id,
                    &entry.command,
                    &entry.args,
                    &env_vec,
                )
                .await
                .wrap_err_with(|| format!("failed to connect MCP server '{}'", entry.id))?;
                Arc::new(adapter)
            } else {
                // Multiple servers → CompositeToolPort
                let mut ports: Vec<(String, Arc<dyn bob_adapters::core::ports::ToolPort>)> =
                    Vec::with_capacity(mcp_cfg.servers.len());
                for entry in &mcp_cfg.servers {
                    let env_vec: Vec<(String, String)> = entry
                        .env
                        .as_ref()
                        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                        .unwrap_or_default();
                    let adapter = bob_adapters::mcp_rmcp::McpToolAdapter::connect_stdio(
                        &entry.id,
                        &entry.command,
                        &entry.args,
                        &env_vec,
                    )
                    .await
                    .wrap_err_with(|| format!("failed to connect MCP server '{}'", entry.id))?;
                    ports.push((entry.id.clone(), Arc::new(adapter)));
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
    let policy = bob_adapters::core::types::TurnPolicy {
        max_steps: cfg.runtime.max_steps.unwrap_or(12),
        turn_timeout_ms: cfg.runtime.turn_timeout_ms.unwrap_or(90_000),
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

    Ok(Arc::new(rt))
}

/// Run the interactive REPL loop.
#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI REPL must use stdout/stderr for user interaction"
)]
async fn repl(runtime: Arc<dyn AgentRuntime>, model: &str) {
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

        let req = bob_adapters::core::types::AgentRequest {
            input,
            session_id: session_id.clone(),
            model: None,
            metadata: HashMap::new(),
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

    let runtime = build_runtime(&cfg).await?;

    repl(runtime, &cfg.runtime.default_model).await;

    Ok(())
}
