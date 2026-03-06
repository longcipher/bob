//! # Bob CLI Agent
//!
//! Thin CLI entrypoint for wiring config + runtime builder + REPL.

mod bootstrap;
mod config;
mod request_context;

use bob_runtime::agent_loop::{AgentLoop, AgentLoopOutput, help_text};
use clap::Parser;
use eyre::WrapErr;

use crate::{
    bootstrap::{CliRuntimeHandles, SkillsRuntimeContext, build_runtime},
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReplCommand {
    Help,
    NewSession,
}

fn parse_repl_command(input: &str) -> Option<ReplCommand> {
    let trimmed = input.trim();
    match trimmed {
        "/help" | "/h" => Some(ReplCommand::Help),
        "/new" | "/reset" => Some(ReplCommand::NewSession),
        _ => None,
    }
}

#[expect(clippy::print_stdout, reason = "help text is intentionally printed for REPL UX")]
fn print_help() {
    println!(
        "\
CLI session commands:
  /help, /h             Show this help message
  /new, /reset          Start a new session context

{}
",
        help_text()
    );
}

/// Run the interactive REPL loop.
#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI REPL must use stdout/stderr for user interaction"
)]
async fn repl(
    agent_loop: AgentLoop,
    model: &str,
    skills_context: Option<&SkillsRuntimeContext>,
    policy: Option<&config::PolicyConfig>,
) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    let mut session_seq: u64 = 1;
    let mut session_id = format!("cli-session-{session_seq}");

    eprintln!("Bob agent ready  (model: {model})");
    eprintln!("Type a message and press Enter. /help for commands.\n");

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

        if let Some(command) = parse_repl_command(&input) {
            match command {
                ReplCommand::Help => {
                    print_help();
                    continue;
                }
                ReplCommand::NewSession => {
                    session_seq = session_seq.saturating_add(1);
                    session_id = format!("cli-session-{session_seq}");
                    eprintln!("Started new session: {session_id}");
                    continue;
                }
            }
        }

        let context = build_request_context(&input, skills_context, policy);
        match agent_loop.handle_input_with_context(&input, &session_id, context).await {
            Ok(AgentLoopOutput::Response(bob_runtime::core::types::AgentRunResult::Finished(
                resp,
            ))) => {
                println!("{}", resp.content);
                println!(
                    "\n[usage] prompt={} completion={} total={}",
                    resp.usage.prompt_tokens,
                    resp.usage.completion_tokens,
                    resp.usage.total()
                );
            }
            Ok(AgentLoopOutput::CommandOutput(output)) => {
                println!("{output}");
            }
            Ok(AgentLoopOutput::Quit) => break,
            Err(err) => {
                eprintln!("Error: {err}");
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

    let CliRuntimeHandles { runtime, tools, store, tape, skills_context } =
        build_runtime(&cfg).await?;
    let agent_loop =
        AgentLoop::new(runtime.clone(), tools.clone()).with_store(store).with_tape(tape);
    repl(agent_loop, &cfg.runtime.default_model, skills_context.as_ref(), cfg.policy.as_ref())
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ReplCommand, parse_repl_command};

    #[test]
    fn parse_help_command() {
        assert_eq!(parse_repl_command("/help"), Some(ReplCommand::Help));
        assert_eq!(parse_repl_command("/h"), Some(ReplCommand::Help));
    }

    #[test]
    fn parse_new_session_commands() {
        assert_eq!(parse_repl_command("/new"), Some(ReplCommand::NewSession));
        assert_eq!(parse_repl_command("/reset"), Some(ReplCommand::NewSession));
    }

    #[test]
    fn non_command_input_returns_none() {
        assert_eq!(parse_repl_command("hello"), None);
        assert_eq!(parse_repl_command("/tools"), None);
    }
}
