//! # Agent Loop
//!
//! High-level orchestration loop that unifies slash command routing, tape
//! recording, and LLM pipeline execution.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐       ┌────────────┐       ┌─────────────────┐
//! │  Channel     │ ───→  │ AgentLoop  │ ───→  │  AgentRuntime   │
//! │ (transport)  │ ←───  │ (routing)  │ ←───  │  (LLM pipeline) │
//! └─────────────┘       └────────────┘       └─────────────────┘
//!                           │
//!                           ├─→ Router (slash commands)
//!                           ├─→ TapeStorePort (recording)
//!                           └─→ ToolPort (tool listing for /tools)
//! ```
//!
//! The `AgentLoop` wraps an `AgentRuntime` and adds:
//!
//! 1. **Slash command routing** — deterministic commands bypass the LLM
//! 2. **Tape recording** — conversation history is persisted to the tape
//! 3. **System prompt override** — load custom system prompts from workspace files

use std::sync::Arc;

use bob_core::{
    error::AgentError,
    ports::{EventSink, ToolPort},
    tape::{TapeEntryKind, TapeSearchResult},
    types::{AgentRequest, AgentRunResult, RequestContext},
};

// Re-export for convenience.
pub use crate::router::help_text;
use crate::{
    AgentRuntime,
    router::{self, RouteResult, SlashCommand},
};

/// Output from the agent loop after processing a single input.
#[derive(Debug)]
pub enum AgentLoopOutput {
    /// A response from the LLM pipeline (normal conversation).
    Response(AgentRunResult),
    /// A deterministic command response (no LLM involved).
    CommandOutput(String),
    /// The user requested to quit the session.
    Quit,
}

/// High-level agent orchestration loop.
///
/// Wraps an `AgentRuntime` with slash command routing, tape recording,
/// and optional system prompt overrides.
///
/// ## Construction
///
/// ```rust,ignore
/// let agent_loop = AgentLoop::new(runtime, tools)
///     .with_tape(tape_store)
///     .with_system_prompt("You are a helpful assistant.".to_string());
/// ```
pub struct AgentLoop {
    runtime: Arc<dyn AgentRuntime>,
    tools: Arc<dyn ToolPort>,
    tape: Option<Arc<dyn bob_core::ports::TapeStorePort>>,
    events: Option<Arc<dyn EventSink>>,
    system_prompt_override: Option<String>,
}

impl std::fmt::Debug for AgentLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentLoop")
            .field("has_tape", &self.tape.is_some())
            .field("has_system_prompt_override", &self.system_prompt_override.is_some())
            .finish_non_exhaustive()
    }
}

impl AgentLoop {
    /// Create a new agent loop wrapping the given runtime and tool port.
    #[must_use]
    pub fn new(runtime: Arc<dyn AgentRuntime>, tools: Arc<dyn ToolPort>) -> Self {
        Self { runtime, tools, tape: None, events: None, system_prompt_override: None }
    }

    /// Attach a tape store for persistent conversation recording.
    #[must_use]
    pub fn with_tape(mut self, tape: Arc<dyn bob_core::ports::TapeStorePort>) -> Self {
        self.tape = Some(tape);
        self
    }

    /// Attach an event sink for observability.
    #[must_use]
    pub fn with_events(mut self, events: Arc<dyn EventSink>) -> Self {
        self.events = Some(events);
        self
    }

    /// Set a system prompt that overrides the default runtime prompt.
    ///
    /// This is typically loaded from a workspace file (e.g. `.agent/system-prompt.md`)
    /// at the composition root. The content is passed as a pre-loaded string to
    /// keep the runtime free from filesystem dependencies.
    #[must_use]
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt_override = Some(prompt);
        self
    }

    /// Process a single user input and return the appropriate output.
    ///
    /// Slash commands are handled deterministically. Natural language input
    /// is forwarded to the LLM pipeline.
    pub async fn handle_input(
        &self,
        input: &str,
        session_id: &str,
    ) -> Result<AgentLoopOutput, AgentError> {
        let sid = session_id.to_string();

        // Record user input to tape (if configured).
        if let Some(ref tape) = self.tape {
            let _ = tape
                .append(
                    &sid,
                    TapeEntryKind::Message {
                        role: bob_core::types::Role::User,
                        content: input.to_string(),
                    },
                )
                .await;
        }

        match router::route(input) {
            RouteResult::SlashCommand(cmd) => self.execute_command(cmd, &sid).await,
            RouteResult::NaturalLanguage(text) => self.execute_llm(&text, &sid).await,
        }
    }

    /// Execute a deterministic slash command.
    async fn execute_command(
        &self,
        cmd: SlashCommand,
        session_id: &String,
    ) -> Result<AgentLoopOutput, AgentError> {
        match cmd {
            SlashCommand::Help => Ok(AgentLoopOutput::CommandOutput(help_text())),

            SlashCommand::Tools => {
                let tools = self.tools.list_tools().await?;
                let mut out = String::from("Registered tools:\n");
                for tool in &tools {
                    out.push_str(&format!("  - {}: {}\n", tool.id, tool.description));
                }
                if tools.is_empty() {
                    out.push_str("  (none)\n");
                }
                Ok(AgentLoopOutput::CommandOutput(out))
            }

            SlashCommand::ToolDescribe { name } => {
                let tools = self.tools.list_tools().await?;
                let found = tools.iter().find(|t| t.id == name);
                let out = match found {
                    Some(tool) => {
                        format!(
                            "Tool: {}\nDescription: {}\nSource: {:?}\nSchema:\n{}",
                            tool.id,
                            tool.description,
                            tool.source,
                            serde_json::to_string_pretty(&tool.input_schema).unwrap_or_default()
                        )
                    }
                    None => {
                        format!("Tool '{}' not found. Use /tools to list available tools.", name)
                    }
                };
                Ok(AgentLoopOutput::CommandOutput(out))
            }

            SlashCommand::TapeSearch { query } => {
                let out = if let Some(ref tape) = self.tape {
                    let results = tape.search(session_id, &query).await?;
                    format_search_results(&results)
                } else {
                    "Tape not configured.".to_string()
                };
                Ok(AgentLoopOutput::CommandOutput(out))
            }

            SlashCommand::TapeInfo => {
                let out = if let Some(ref tape) = self.tape {
                    let entries = tape.all_entries(session_id).await?;
                    let anchors = tape.anchors(session_id).await?;
                    format!("Tape: {} entries, {} anchors", entries.len(), anchors.len())
                } else {
                    "Tape not configured.".to_string()
                };
                Ok(AgentLoopOutput::CommandOutput(out))
            }

            SlashCommand::Anchors => {
                let out = if let Some(ref tape) = self.tape {
                    let anchors = tape.anchors(session_id).await?;
                    if anchors.is_empty() {
                        "No anchors in tape.".to_string()
                    } else {
                        let mut buf = String::from("Anchors:\n");
                        for a in &anchors {
                            if let TapeEntryKind::Anchor { ref name, .. } = a.kind {
                                buf.push_str(&format!("  [{}] {}\n", a.id, name));
                            }
                        }
                        buf
                    }
                } else {
                    "Tape not configured.".to_string()
                };
                Ok(AgentLoopOutput::CommandOutput(out))
            }

            SlashCommand::Handoff { name } => {
                let handoff_name = name.unwrap_or_else(|| "manual".to_string());
                if let Some(ref tape) = self.tape {
                    let all = tape.all_entries(session_id).await?;
                    let _ = tape
                        .append(
                            session_id,
                            TapeEntryKind::Handoff {
                                name: handoff_name.clone(),
                                entries_before: all.len() as u64,
                                summary: None,
                            },
                        )
                        .await;
                    Ok(AgentLoopOutput::CommandOutput(format!(
                        "Handoff '{}' created. Context window reset.",
                        handoff_name
                    )))
                } else {
                    Ok(AgentLoopOutput::CommandOutput("Tape not configured.".to_string()))
                }
            }

            SlashCommand::Quit => Ok(AgentLoopOutput::Quit),

            SlashCommand::Shell { command } => {
                // Shell execution is a convenience fallback.
                // In production, this should be gated by approval policies.
                Ok(AgentLoopOutput::CommandOutput(format!(
                    "Shell execution not yet implemented: {}",
                    command
                )))
            }
        }
    }

    /// Forward natural language input to the LLM pipeline.
    async fn execute_llm(
        &self,
        text: &str,
        session_id: &String,
    ) -> Result<AgentLoopOutput, AgentError> {
        let mut context = RequestContext::default();
        if let Some(ref prompt) = self.system_prompt_override {
            context.system_prompt = Some(prompt.clone());
        }

        let req = AgentRequest {
            input: text.to_string(),
            session_id: session_id.clone(),
            model: None,
            context,
            cancel_token: None,
        };

        let result = self.runtime.run(req).await?;

        // Record assistant response to tape.
        if let Some(ref tape) = self.tape {
            let AgentRunResult::Finished(ref resp) = result;
            let _ = tape
                .append(
                    session_id,
                    TapeEntryKind::Message {
                        role: bob_core::types::Role::Assistant,
                        content: resp.content.clone(),
                    },
                )
                .await;
        }

        Ok(AgentLoopOutput::Response(result))
    }
}

/// Format tape search results into a human-readable string.
fn format_search_results(results: &[TapeSearchResult]) -> String {
    if results.is_empty() {
        return "No results found.".to_string();
    }
    let mut buf = format!("{} result(s):\n", results.len());
    for r in results {
        buf.push_str(&format!("  [{}] {}\n", r.entry.id, r.snippet));
    }
    buf
}
