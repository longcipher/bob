//! # Bob CLI
//!
//! Command-line interface for the Bob Agent Framework.
//!
//! # Subcommands
//!
//! - `repl` — Interactive REPL for chatting with the agent
//! - `skills list` — List skills in a directory
//! - `skills validate` — Validate a skill directory
//! - `skills read-properties` — Read skill properties as JSON/YAML/TOML
//! - `skills to-prompt` — Generate XML block for agent prompts

mod bootstrap;
mod config;
mod request_context;

use std::path::{Path, PathBuf};

use bob_runtime::agent_loop::{AgentLoop, AgentLoopOutput, help_text};
use clap::{Parser, Subcommand};
use eyre::WrapErr;

use crate::{
    bootstrap::{CliRuntimeHandles, SkillsRuntimeContext, build_runtime},
    request_context::build_request_context,
};

/// Bob CLI — a general-purpose AI agent framework CLI.
#[derive(Parser, Debug)]
#[command(name = "bob", version, about)]
struct Cli {
    /// Path to the agent configuration file.
    #[arg(short, long, default_value = "agent.toml")]
    config: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start an interactive REPL session with the agent.
    Repl,

    /// Skill management commands.
    #[command(subcommand)]
    Skills(SkillsCommands),
}

#[derive(Subcommand, Debug)]
enum SkillsCommands {
    /// List skills in a directory.
    #[command(visible_alias = "ls")]
    List {
        /// Directory to search for skills.
        #[arg(default_value = ".")]
        directory: PathBuf,

        /// Search recursively.
        #[arg(short, long)]
        recursive: bool,

        /// Validate each skill and show status.
        #[arg(short, long)]
        check: bool,

        /// Show only skills that fail validation (implies --check).
        #[arg(short, long)]
        failed: bool,

        /// Show full details (path and complete description).
        #[arg(short, long, conflicts_with_all = ["paths", "names"])]
        long: bool,

        /// Show only paths (for scripting).
        #[arg(short, long, conflicts_with_all = ["long", "names"])]
        paths: bool,

        /// Show only names (for scripting).
        #[arg(short, long, conflicts_with_all = ["long", "paths"])]
        names: bool,

        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Validate a skill directory.
    #[command(visible_alias = "check")]
    Validate {
        /// Path to skill directory or SKILL.md file.
        #[arg(default_value = ".")]
        skill_path: String,
    },

    /// Read skill properties as JSON/YAML/TOML.
    #[command(name = "read-properties", visible_alias = "props")]
    ReadProperties {
        /// Path to skill directory or SKILL.md file.
        #[arg(default_value = ".")]
        skill_path: String,

        /// Output format.
        #[arg(short, long, default_value = "json")]
        format: String,

        /// Compact output (no pretty printing, JSON only).
        #[arg(long)]
        compact: bool,
    },

    /// Generate `available_skills` XML block for agent prompts.
    #[command(name = "to-prompt", visible_alias = "prompt")]
    ToPrompt {
        /// Paths to skill directories or SKILL.md files.
        #[arg(required = true)]
        skill_paths: Vec<String>,
    },
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

// ── Skills Commands ──────────────────────────────────────────────────

/// Output mode for list command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListOutputMode {
    Short,
    Long,
    PathsOnly,
    NamesOnly,
}

/// Information about a discovered skill.
#[derive(serde::Serialize)]
struct SkillInfo {
    name: String,
    description: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    valid: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Skill properties for read-properties command.
#[derive(serde::Serialize)]
struct SkillProperties {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<String>,
}

/// Resolves a skill path, handling SKILL.md files.
fn resolve_skill_path(path: &Path) -> Result<PathBuf, eyre::Report> {
    if !path.exists() {
        return Err(eyre::eyre!("path not found: '{}'", path.display()));
    }

    if path.is_file() &&
        path.file_name().is_some_and(|name| name == "SKILL.md") &&
        let Some(parent) = path.parent()
    {
        return Ok(parent.to_path_buf());
    }

    Ok(path.to_path_buf())
}

/// Truncates a description to the specified maximum length.
fn truncate_description(description: &str, max_len: usize) -> String {
    if description.len() <= max_len {
        return description.to_string();
    }

    let target_len = max_len.saturating_sub(3);
    let truncate_at = description[..target_len].rfind(' ').unwrap_or(target_len);
    format!("{}...", &description[..truncate_at])
}

/// Escape XML special characters.
fn escape_xml(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => {
                escaped.push('\u{26}');
                escaped.push_str("amp;");
            }
            '<' => {
                escaped.push('\u{3c}');
                escaped.push_str("lt;");
            }
            '>' => {
                escaped.push('\u{3e}');
                escaped.push_str("gt;");
            }
            '"' => {
                escaped.push('\u{22}');
                escaped.push_str("quot;");
            }
            '\'' => {
                escaped.push('\u{27}');
                escaped.push_str("apos;");
            }
            _ => escaped.push(c),
        }
    }
    escaped
}

/// Discover skills in a directory.
fn discover_skills(
    directory: &Path,
    recursive: bool,
    validate: bool,
) -> Result<Vec<SkillInfo>, eyre::Report> {
    let mut skills = Vec::new();
    discover_skills_in_dir(directory, recursive, validate, &mut skills)?;
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

fn discover_skills_in_dir(
    directory: &Path,
    recursive: bool,
    validate: bool,
    skills: &mut Vec<SkillInfo>,
) -> Result<(), eyre::Report> {
    let entries = std::fs::read_dir(directory)
        .map_err(|e| eyre::eyre!("failed to read directory '{}': {e}", directory.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| eyre::eyre!("failed to read directory entry: {e}"))?;
        let path = entry.path();

        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() {
                match bob_skills::SkillDirectory::load(&path) {
                    Ok(dir) => {
                        let skill = dir.skill();
                        skills.push(SkillInfo {
                            name: skill.name().as_str().to_string(),
                            description: skill.description().as_str().to_string(),
                            path: path.display().to_string(),
                            valid: validate.then_some(true),
                            error: None,
                        });
                    }
                    Err(e) if validate => {
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        skills.push(SkillInfo {
                            name,
                            description: String::new(),
                            path: path.display().to_string(),
                            valid: Some(false),
                            error: Some(e.to_string()),
                        });
                    }
                    Err(_) => {}
                }
            }

            if recursive {
                discover_skills_in_dir(&path, recursive, validate, skills)?;
            }
        }
    }

    Ok(())
}

/// Run the `skills list` command.
#[expect(clippy::print_stdout, clippy::print_stderr, reason = "CLI must print to stdout/stderr")]
fn run_skills_list(
    directory: &Path,
    recursive: bool,
    validate: bool,
    failed_only: bool,
    list_mode: ListOutputMode,
    json_output: bool,
) -> Result<(), eyre::Report> {
    if !directory.exists() {
        return Err(eyre::eyre!("path not found: '{}'", directory.display()));
    }

    let skills = discover_skills(directory, recursive, validate)?;

    let skills: Vec<_> = if failed_only {
        skills.into_iter().filter(|s| s.valid == Some(false)).collect()
    } else {
        skills
    };

    if skills.is_empty() {
        let msg = if failed_only { "No invalid skills found" } else { "No skills found" };
        eprintln!("{msg} in '{}'", directory.display());
        return Ok(());
    }

    if json_output {
        let json = serde_json::to_string_pretty(&skills)?;
        println!("{json}");
    } else {
        match list_mode {
            ListOutputMode::Short => {
                for skill in &skills {
                    let symbol = match skill.valid {
                        Some(true) => "✓ ",
                        Some(false) => "✗ ",
                        None => "",
                    };
                    let desc = truncate_description(&skill.description, 50);
                    println!("{symbol}{}  {}", skill.name, desc);
                }
            }
            ListOutputMode::Long => {
                for (i, skill) in skills.iter().enumerate() {
                    if i > 0 {
                        println!();
                    }
                    let symbol = match skill.valid {
                        Some(true) => "✓ ",
                        Some(false) => "✗ ",
                        None => "",
                    };
                    println!("{symbol}{}", skill.name);
                    println!("  Path: {}", skill.path);
                    if skill.valid == Some(false) {
                        if let Some(ref e) = skill.error {
                            println!("  Error: {e}");
                        }
                    } else {
                        println!("  {}", skill.description);
                    }
                }
            }
            ListOutputMode::PathsOnly => {
                for skill in &skills {
                    println!("{}", skill.path);
                }
            }
            ListOutputMode::NamesOnly => {
                for skill in &skills {
                    println!("{}", skill.name);
                }
            }
        }
    }

    eprintln!("Found {} skill(s)", skills.len());
    Ok(())
}

/// Run the `skills validate` command.
#[expect(clippy::print_stderr, reason = "CLI must print validation result to stderr")]
fn run_skills_validate(skill_path: &str) -> Result<(), eyre::Report> {
    let path = Path::new(skill_path);
    let skill_path = resolve_skill_path(path)?;

    bob_skills::SkillDirectory::load(&skill_path)
        .map_err(|e| eyre::eyre!("failed to load skill at '{}': {e}", skill_path.display()))?;

    let name = skill_path.file_name().and_then(|n| n.to_str()).unwrap_or("skill");
    eprintln!("✓ {name} ({})", skill_path.display());
    Ok(())
}

/// Run the `skills read-properties` command.
#[expect(clippy::print_stdout, reason = "CLI must print to stdout")]
fn run_skills_read_properties(
    skill_path: &str,
    format: &str,
    compact: bool,
) -> Result<(), eyre::Report> {
    let path = Path::new(skill_path);
    let skill_path = resolve_skill_path(path)?;

    let dir = bob_skills::SkillDirectory::load(&skill_path)
        .map_err(|e| eyre::eyre!("failed to load skill at '{}': {e}", skill_path.display()))?;

    let skill = dir.skill();
    let frontmatter = skill.frontmatter();

    let properties = SkillProperties {
        name: skill.name().as_str().to_string(),
        description: skill.description().as_str().to_string(),
        license: frontmatter.license().map(String::from),
        compatibility: frontmatter.compatibility().map(|c| c.as_str().to_string()),
    };

    let output = match format.to_lowercase().as_str() {
        "json" => {
            if compact {
                serde_json::to_string(&properties)?
            } else {
                serde_json::to_string_pretty(&properties)?
            }
        }
        "yaml" | "yml" => serde_yml::to_string(&properties)
            .map(|s| s.trim_end().to_string())
            .map_err(|e| eyre::eyre!("YAML serialization error: {e}"))?,
        "toml" => toml::to_string_pretty(&properties)
            .map_err(|e| eyre::eyre!("TOML serialization error: {e}"))?,
        _ => return Err(eyre::eyre!("invalid format '{format}'; valid formats: json, yaml, toml")),
    };

    println!("{output}");
    Ok(())
}

/// Run the `skills to-prompt` command.
#[expect(clippy::print_stdout, reason = "CLI must print to stdout")]
fn run_skills_to_prompt(skill_paths: &[String]) -> Result<(), eyre::Report> {
    if skill_paths.is_empty() {
        return Err(eyre::eyre!("no skill paths provided"));
    }

    let mut skills_xml = Vec::new();

    for path_str in skill_paths {
        let path = Path::new(path_str);
        let skill_path = resolve_skill_path(path)?;

        let absolute_path = std::fs::canonicalize(&skill_path)
            .map_err(|e| eyre::eyre!("failed to canonicalize '{}': {e}", skill_path.display()))?;

        let dir = bob_skills::SkillDirectory::load(&skill_path)
            .map_err(|e| eyre::eyre!("failed to load skill at '{}': {e}", skill_path.display()))?;

        let skill = dir.skill();
        let skill_md_path = absolute_path.join("SKILL.md");

        skills_xml.push(format!(
            "<skill>\n<name>{}</name>\n<description>{}</description>\n<location>{}</location>\n</skill>\n",
            escape_xml(skill.name().as_str()),
            escape_xml(skill.description().as_str()),
            escape_xml(&skill_md_path.display().to_string()),
        ));
    }

    println!("<available_skills>");
    for xml in skills_xml {
        print!("{xml}");
    }
    println!("</available_skills>");

    Ok(())
}

// ── Main ─────────────────────────────────────────────────────────────

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

    match cli.command {
        Commands::Repl => {
            let cfg = config::load_config(&cli.config)
                .wrap_err_with(|| format!("failed to load config from '{}'", cli.config))?;

            let CliRuntimeHandles { runtime, tools, store, tape, skills_context } =
                build_runtime(&cfg).await?;
            let agent_loop =
                AgentLoop::new(runtime.clone(), tools.clone()).with_store(store).with_tape(tape);
            repl(
                agent_loop,
                &cfg.runtime.default_model,
                skills_context.as_ref(),
                cfg.policy.as_ref(),
            )
            .await;
        }
        Commands::Skills(skills_cmd) => match skills_cmd {
            SkillsCommands::List {
                directory,
                recursive,
                check,
                failed,
                long,
                paths,
                names,
                json,
            } => {
                let list_mode = if paths {
                    ListOutputMode::PathsOnly
                } else if names {
                    ListOutputMode::NamesOnly
                } else if long {
                    ListOutputMode::Long
                } else {
                    ListOutputMode::Short
                };
                let validate = check || failed;
                run_skills_list(&directory, recursive, validate, failed, list_mode, json)?;
            }
            SkillsCommands::Validate { skill_path } => {
                run_skills_validate(&skill_path)?;
            }
            SkillsCommands::ReadProperties { skill_path, format, compact } => {
                run_skills_read_properties(&skill_path, &format, compact)?;
            }
            SkillsCommands::ToPrompt { skill_paths } => {
                run_skills_to_prompt(&skill_paths)?;
            }
        },
    }

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
