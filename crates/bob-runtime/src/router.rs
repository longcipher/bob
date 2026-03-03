//! # Slash Command Router
//!
//! Deterministic input router for the Bob Agent Framework.
//!
//! All `/`-prefixed inputs are parsed as slash commands and executed
//! **without** LLM inference — zero latency, deterministic results.
//! Everything else is treated as natural language for the LLM pipeline.
//!
//! ## Supported Commands
//!
//! | Command               | Description                          |
//! |-----------------------|--------------------------------------|
//! | `/help`               | Show available commands               |
//! | `/tools`              | List all registered tools             |
//! | `/tool.describe NAME` | Show full schema for a specific tool  |
//! | `/tape.search QUERY`  | Search conversation history           |
//! | `/tape.info`          | Show tape statistics                  |
//! | `/anchors`            | List all anchors in the tape          |
//! | `/handoff [NAME]`     | Create a context-window reset point   |
//! | `/quit`               | Exit the session                      |
//! | `/COMMAND`             | Execute as shell command (fallback)   |

/// Result of routing a user input string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteResult {
    /// A deterministic slash command (bypass LLM).
    SlashCommand(SlashCommand),
    /// Natural language input destined for the LLM pipeline.
    NaturalLanguage(String),
}

/// Recognized slash commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    /// `/help` — display available commands.
    Help,
    /// `/tools` — list all registered tools.
    Tools,
    /// `/tool.describe <name>` — describe a specific tool's schema.
    ToolDescribe { name: String },
    /// `/tape.search <query>` — full-text search the tape.
    TapeSearch { query: String },
    /// `/tape.info` — show tape entry count and last handoff info.
    TapeInfo,
    /// `/anchors` — list all anchors in the current session tape.
    Anchors,
    /// `/handoff [name]` — create a handoff anchor and reset context window.
    Handoff { name: Option<String> },
    /// `/quit` or `/exit` — signal the channel to close.
    Quit,
    /// Fallback: treat unrecognized `/cmd` as a shell command.
    Shell { command: String },
}

/// Route raw user input into either a slash command or natural language.
///
/// Only inputs starting with `/` are treated as commands. Everything else
/// goes to the LLM pipeline as natural language.
#[must_use]
pub fn route(input: &str) -> RouteResult {
    let trimmed = input.trim();

    if !trimmed.starts_with('/') {
        return RouteResult::NaturalLanguage(trimmed.to_string());
    }

    // Strip the leading `/`
    let rest = &trimmed[1..];

    // Split into command word and arguments
    let (cmd, args) = rest
        .split_once(|c: char| c.is_ascii_whitespace())
        .map_or((rest, ""), |(c, a)| (c, a.trim()));

    match cmd {
        "help" | "h" => RouteResult::SlashCommand(SlashCommand::Help),
        "tools" => RouteResult::SlashCommand(SlashCommand::Tools),
        "tool.describe" => {
            RouteResult::SlashCommand(SlashCommand::ToolDescribe { name: args.to_string() })
        }
        "tape.search" => {
            RouteResult::SlashCommand(SlashCommand::TapeSearch { query: args.to_string() })
        }
        "tape.info" => RouteResult::SlashCommand(SlashCommand::TapeInfo),
        "anchors" => RouteResult::SlashCommand(SlashCommand::Anchors),
        "handoff" => RouteResult::SlashCommand(SlashCommand::Handoff {
            name: if args.is_empty() { None } else { Some(args.to_string()) },
        }),
        "quit" | "exit" => RouteResult::SlashCommand(SlashCommand::Quit),
        _ => {
            // Unrecognized command → treat as shell command
            RouteResult::SlashCommand(SlashCommand::Shell { command: rest.to_string() })
        }
    }
}

/// Render the help text for all available slash commands.
#[must_use]
pub fn help_text() -> String {
    "\
Available commands:
  /help                 Show this help message
  /tools                List all registered tools
  /tool.describe NAME   Show full schema for a tool
  /tape.search QUERY    Search conversation history
  /tape.info            Show tape statistics
  /anchors              List all tape anchors
  /handoff [NAME]       Reset context window (create handoff point)
  /quit                 Exit the session

Natural language input (without /) goes to the AI model."
        .to_string()
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_language_passthrough() {
        let result = route("hello world");
        assert_eq!(result, RouteResult::NaturalLanguage("hello world".to_string()));
    }

    #[test]
    fn natural_language_trims_whitespace() {
        let result = route("  hello  ");
        assert_eq!(result, RouteResult::NaturalLanguage("hello".to_string()));
    }

    #[test]
    fn help_command() {
        assert_eq!(route("/help"), RouteResult::SlashCommand(SlashCommand::Help));
        assert_eq!(route("/h"), RouteResult::SlashCommand(SlashCommand::Help));
    }

    #[test]
    fn tools_command() {
        assert_eq!(route("/tools"), RouteResult::SlashCommand(SlashCommand::Tools));
    }

    #[test]
    fn tool_describe_command() {
        assert_eq!(
            route("/tool.describe file.read"),
            RouteResult::SlashCommand(SlashCommand::ToolDescribe { name: "file.read".to_string() })
        );
    }

    #[test]
    fn tape_search_command() {
        assert_eq!(
            route("/tape.search error handling"),
            RouteResult::SlashCommand(SlashCommand::TapeSearch {
                query: "error handling".to_string()
            })
        );
    }

    #[test]
    fn handoff_with_name() {
        assert_eq!(
            route("/handoff phase-2"),
            RouteResult::SlashCommand(SlashCommand::Handoff { name: Some("phase-2".to_string()) })
        );
    }

    #[test]
    fn handoff_without_name() {
        assert_eq!(
            route("/handoff"),
            RouteResult::SlashCommand(SlashCommand::Handoff { name: None })
        );
    }

    #[test]
    fn quit_and_exit_commands() {
        assert_eq!(route("/quit"), RouteResult::SlashCommand(SlashCommand::Quit));
        assert_eq!(route("/exit"), RouteResult::SlashCommand(SlashCommand::Quit));
    }

    #[test]
    fn unknown_command_becomes_shell() {
        assert_eq!(
            route("/git status"),
            RouteResult::SlashCommand(SlashCommand::Shell { command: "git status".to_string() })
        );
    }

    #[test]
    fn shell_command_preserves_full_text() {
        assert_eq!(
            route("/ls -la /tmp"),
            RouteResult::SlashCommand(SlashCommand::Shell { command: "ls -la /tmp".to_string() })
        );
    }
}
