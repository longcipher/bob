//! # Bob CLI Agent
//!
//! Thin CLI entrypoint for wiring config + runtime builder + REPL.

mod bootstrap;
mod config;
mod request_context;

use std::sync::Arc;

use bob_runtime::AgentRuntime;
use clap::Parser;
use eyre::WrapErr;

use crate::{
    bootstrap::{SkillsRuntimeContext, build_runtime},
    request_context::build_request_context,
};

/// Bob CLI Agent — a general-purpose AI assistant CLI.
#[derive(Parser, Debug)]
#[command(name = "bob", version, about)]
struct Cli {
    /// Path to the agent configuration file.
    #[arg(short, long, default_value = "agent.toml")]
    config: String,
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
        let _ = tokio::io::AsyncWriteExt::flush(&mut tokio::io::stderr()).await;

        let line = match lines.next_line().await {
            Ok(Some(l)) => l,
            Ok(None) => break,
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

        let context = build_request_context(&input, skills_context, policy);

        let req = bob_runtime::core::types::AgentRequest {
            input,
            session_id: session_id.clone(),
            model: None,
            context,
            cancel_token: None,
        };

        match runtime.run(req).await {
            Ok(bob_runtime::core::types::AgentRunResult::Finished(resp)) => {
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
