//! # Bob CLI Agent
//!
//! Thin CLI entrypoint for wiring config + runtime builder + REPL.

mod bootstrap;
mod config;
mod request_context;

use std::sync::Arc;

use bob_runtime::{AgentRuntime, core::ports::ToolPort};
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReplCommand {
    Help,
    Quit,
    NewSession,
    Tools,
    ToolDescribe { name: String },
}

fn parse_repl_command(input: &str) -> Option<ReplCommand> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    match trimmed {
        "/help" | "/h" => Some(ReplCommand::Help),
        "/quit" | "/exit" => Some(ReplCommand::Quit),
        "/new" | "/reset" => Some(ReplCommand::NewSession),
        "/tools" => Some(ReplCommand::Tools),
        "/tool.describe" => Some(ReplCommand::ToolDescribe { name: String::new() }),
        _ => trimmed
            .strip_prefix("/tool.describe ")
            .map(|name| ReplCommand::ToolDescribe { name: name.trim().to_string() }),
    }
}

#[expect(clippy::print_stdout, reason = "help text is intentionally printed for REPL UX")]
fn print_help() {
    println!(
        "\
Commands:
  /help, /h             Show this help message
  /tools                List available tools
  /tool.describe <name> Show a tool schema
  /new, /reset          Start a new session context
  /quit, /exit          Exit the CLI

Any other input is sent to the model."
    );
}

/// Run the interactive REPL loop.
#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI REPL must use stdout/stderr for user interaction"
)]
async fn repl(
    runtime: Arc<dyn AgentRuntime>,
    tools: Arc<dyn ToolPort>,
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
                ReplCommand::Quit => break,
                ReplCommand::NewSession => {
                    session_seq = session_seq.saturating_add(1);
                    session_id = format!("cli-session-{session_seq}");
                    eprintln!("Started new session: {session_id}");
                    continue;
                }
                ReplCommand::Tools => {
                    match tools.list_tools().await {
                        Ok(available) if available.is_empty() => println!("(no tools available)"),
                        Ok(available) => {
                            println!("Available tools:");
                            for tool in available {
                                println!("- {}: {}", tool.id, tool.description);
                            }
                        }
                        Err(err) => eprintln!("Failed to list tools: {err}"),
                    }
                    continue;
                }
                ReplCommand::ToolDescribe { name } => {
                    if name.is_empty() {
                        eprintln!("Usage: /tool.describe <tool-name>");
                        continue;
                    }
                    match tools.list_tools().await {
                        Ok(available) => {
                            if let Some(tool) = available.iter().find(|tool| tool.id == name) {
                                println!(
                                    "Tool: {}\nDescription: {}\nSource: {:?}\nSchema:\n{}",
                                    tool.id,
                                    tool.description,
                                    tool.source,
                                    serde_json::to_string_pretty(&tool.input_schema)
                                        .unwrap_or_default()
                                );
                            } else {
                                eprintln!(
                                    "Tool '{name}' not found. Use /tools to inspect choices."
                                );
                            }
                        }
                        Err(err) => eprintln!("Failed to describe tool: {err}"),
                    }
                    continue;
                }
            }
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

    let (runtime, tools, skills_context) = build_runtime(&cfg).await?;
    repl(runtime, tools, &cfg.runtime.default_model, skills_context.as_ref(), cfg.policy.as_ref())
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
    fn parse_tool_describe_command() {
        assert_eq!(
            parse_repl_command("/tool.describe mcp/filesystem/read_file"),
            Some(ReplCommand::ToolDescribe { name: "mcp/filesystem/read_file".to_string() })
        );
    }

    #[test]
    fn parse_tool_describe_requires_separator() {
        assert_eq!(parse_repl_command("/tool.describefoo"), None);
        assert_eq!(
            parse_repl_command("/tool.describe"),
            Some(ReplCommand::ToolDescribe { name: String::new() })
        );
    }

    #[test]
    fn parse_new_session_commands() {
        assert_eq!(parse_repl_command("/new"), Some(ReplCommand::NewSession));
        assert_eq!(parse_repl_command("/reset"), Some(ReplCommand::NewSession));
    }

    #[test]
    fn non_command_input_returns_none() {
        assert_eq!(parse_repl_command("hello"), None);
    }
}
