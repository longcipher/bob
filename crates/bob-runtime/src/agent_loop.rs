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
    ports::{EventSink, SessionStore, ToolPort},
    tape::{TapeEntryKind, TapeSearchResult},
    types::{AgentRequest, AgentRunResult, RequestContext, TokenUsage},
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
    store: Option<Arc<dyn SessionStore>>,
    tape: Option<Arc<dyn bob_core::ports::TapeStorePort>>,
    events: Option<Arc<dyn EventSink>>,
    system_prompt_override: Option<String>,
}

impl std::fmt::Debug for AgentLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentLoop")
            .field("has_store", &self.store.is_some())
            .field("has_tape", &self.tape.is_some())
            .field("has_system_prompt_override", &self.system_prompt_override.is_some())
            .finish_non_exhaustive()
    }
}

impl AgentLoop {
    /// Create a new agent loop wrapping the given runtime and tool port.
    #[must_use]
    pub fn new(runtime: Arc<dyn AgentRuntime>, tools: Arc<dyn ToolPort>) -> Self {
        Self { runtime, tools, store: None, tape: None, events: None, system_prompt_override: None }
    }

    /// Attach a session store for slash commands that inspect persisted state.
    #[must_use]
    pub fn with_store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.store = Some(store);
        self
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
        self.handle_input_with_context(input, session_id, RequestContext::default()).await
    }

    /// Process a single user input using an explicit request context.
    pub async fn handle_input_with_context(
        &self,
        input: &str,
        session_id: &str,
        context: RequestContext,
    ) -> Result<AgentLoopOutput, AgentError> {
        let sid = session_id.to_string();

        match router::route(input) {
            RouteResult::SlashCommand(cmd) => self.execute_command(cmd, &sid).await,
            RouteResult::NaturalLanguage(text) => {
                if let Some(ref tape) = self.tape {
                    let _ = tape
                        .append(
                            &sid,
                            TapeEntryKind::Message {
                                role: bob_core::types::Role::User,
                                content: text.clone(),
                            },
                        )
                        .await;
                }
                self.execute_llm(&text, &sid, context).await
            }
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
                let reset_applied = if let Some(ref store) = self.store {
                    let retained_usage = store
                        .load(session_id)
                        .await?
                        .map_or_else(TokenUsage::default, |state| state.total_usage);
                    store
                        .save(
                            session_id,
                            &bob_core::types::SessionState {
                                messages: Vec::new(),
                                total_usage: retained_usage,
                            },
                        )
                        .await?;
                    true
                } else {
                    false
                };

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
                    let message = if reset_applied {
                        format!("Handoff '{}' created. Context window reset.", handoff_name)
                    } else {
                        format!(
                            "Handoff '{}' recorded, but session store is not configured so context was not reset.",
                            handoff_name
                        )
                    };
                    Ok(AgentLoopOutput::CommandOutput(message))
                } else if reset_applied {
                    Ok(AgentLoopOutput::CommandOutput(format!(
                        "Context window reset for handoff '{}'. Tape not configured.",
                        handoff_name
                    )))
                } else {
                    Ok(AgentLoopOutput::CommandOutput(
                        "Handoff requires a session store or tape configuration.".to_string(),
                    ))
                }
            }

            SlashCommand::Usage => {
                let out = if let Some(ref store) = self.store {
                    let session = store.load(session_id).await?;
                    format_usage_summary(session.as_ref().map(|state| &state.total_usage))
                } else {
                    "Session store not configured.".to_string()
                };
                Ok(AgentLoopOutput::CommandOutput(out))
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
        mut context: RequestContext,
    ) -> Result<AgentLoopOutput, AgentError> {
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

fn format_usage_summary(usage: Option<&TokenUsage>) -> String {
    let usage = usage.cloned().unwrap_or_default();
    format!(
        "Session usage:\n  Prompt tokens: {}\n  Completion tokens: {}\n  Total tokens: {}",
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.total(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use bob_core::{
        error::{AgentError, StoreError, ToolError},
        ports::{TapeStorePort, ToolPort},
        tape::{TapeEntry, TapeEntryKind, TapeSearchResult},
        types::{
            AgentEventStream, AgentResponse, FinishReason, RuntimeHealth, SessionId, SessionState,
            ToolCall, ToolDescriptor, ToolResult,
        },
    };

    use super::*;

    struct StubRuntime;

    #[async_trait::async_trait]
    impl AgentRuntime for StubRuntime {
        async fn run(&self, _req: AgentRequest) -> Result<AgentRunResult, AgentError> {
            Ok(AgentRunResult::Finished(AgentResponse {
                content: "stub".to_string(),
                tool_transcript: Vec::new(),
                usage: TokenUsage::default(),
                finish_reason: FinishReason::Stop,
            }))
        }

        async fn run_stream(&self, _req: AgentRequest) -> Result<AgentEventStream, AgentError> {
            Err(AgentError::Config("unused in test".to_string()))
        }

        async fn health(&self) -> RuntimeHealth {
            RuntimeHealth {
                status: bob_core::types::HealthStatus::Healthy,
                llm_ready: true,
                mcp_pool_ready: true,
            }
        }
    }

    struct StubTools;

    #[async_trait::async_trait]
    impl ToolPort for StubTools {
        async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
            Ok(Vec::new())
        }

        async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
            Err(ToolError::NotFound { name: call.name })
        }
    }

    struct StaticSessionStore {
        state: SessionState,
    }

    #[async_trait::async_trait]
    impl SessionStore for StaticSessionStore {
        async fn load(&self, _id: &SessionId) -> Result<Option<SessionState>, StoreError> {
            Ok(Some(self.state.clone()))
        }

        async fn save(&self, _id: &SessionId, _state: &SessionState) -> Result<(), StoreError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemorySessionStore {
        state: Mutex<Option<SessionState>>,
    }

    #[async_trait::async_trait]
    impl SessionStore for MemorySessionStore {
        async fn load(&self, _id: &SessionId) -> Result<Option<SessionState>, StoreError> {
            Ok(self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone())
        }

        async fn save(&self, _id: &SessionId, state: &SessionState) -> Result<(), StoreError> {
            *self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
                Some(state.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryTapeStore {
        entries: Mutex<Vec<TapeEntry>>,
    }

    #[async_trait::async_trait]
    impl TapeStorePort for MemoryTapeStore {
        async fn append(
            &self,
            _session_id: &SessionId,
            kind: TapeEntryKind,
        ) -> Result<TapeEntry, StoreError> {
            let mut entries = self.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let entry = TapeEntry { id: entries.len() as u64 + 1, kind, timestamp_ms: 0 };
            entries.push(entry.clone());
            Ok(entry)
        }

        async fn entries_since_last_handoff(
            &self,
            _session_id: &SessionId,
        ) -> Result<Vec<TapeEntry>, StoreError> {
            Ok(Vec::new())
        }

        async fn search(
            &self,
            _session_id: &SessionId,
            _query: &str,
        ) -> Result<Vec<TapeSearchResult>, StoreError> {
            Ok(Vec::new())
        }

        async fn all_entries(&self, _session_id: &SessionId) -> Result<Vec<TapeEntry>, StoreError> {
            Ok(self.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone())
        }

        async fn anchors(&self, _session_id: &SessionId) -> Result<Vec<TapeEntry>, StoreError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn usage_command_reads_total_usage_from_store() {
        let store = Arc::new(StaticSessionStore {
            state: SessionState {
                messages: Vec::new(),
                total_usage: TokenUsage { prompt_tokens: 12, completion_tokens: 8 },
            },
        });
        let loop_ = AgentLoop::new(Arc::new(StubRuntime), Arc::new(StubTools)).with_store(store);

        let output = loop_.handle_input("/usage", "session-1").await;

        match output {
            Ok(AgentLoopOutput::CommandOutput(body)) => {
                assert!(body.contains("Prompt tokens: 12"));
                assert!(body.contains("Completion tokens: 8"));
                assert!(body.contains("Total tokens: 20"));
            }
            Ok(other) => panic!("expected usage command output, got {other:?}"),
            Err(err) => panic!("usage command failed: {err}"),
        }
    }

    #[tokio::test]
    async fn slash_commands_do_not_append_user_messages_to_tape() {
        let store = Arc::new(StaticSessionStore {
            state: SessionState {
                messages: Vec::new(),
                total_usage: TokenUsage { prompt_tokens: 12, completion_tokens: 8 },
            },
        });
        let tape = Arc::new(MemoryTapeStore::default());
        let loop_ = AgentLoop::new(Arc::new(StubRuntime), Arc::new(StubTools))
            .with_store(store)
            .with_tape(tape.clone());

        let output = loop_.handle_input("/usage", "session-1").await;

        match output {
            Ok(AgentLoopOutput::CommandOutput(body)) => {
                assert!(body.contains("Total tokens: 20"));
            }
            Ok(other) => panic!("expected usage command output, got {other:?}"),
            Err(err) => panic!("usage command failed: {err}"),
        }

        let entries = tape.all_entries(&"session-1".to_string()).await;
        let entries = match entries {
            Ok(entries) => entries,
            Err(err) => panic!("failed to read tape entries: {err}"),
        };
        assert!(entries.is_empty(), "slash commands should not be recorded as tape messages");
    }

    #[tokio::test]
    async fn natural_language_turns_still_append_user_and_assistant_messages_to_tape() {
        let tape = Arc::new(MemoryTapeStore::default());
        let loop_ =
            AgentLoop::new(Arc::new(StubRuntime), Arc::new(StubTools)).with_tape(tape.clone());

        let output = loop_.handle_input("hello world", "session-1").await;

        match output {
            Ok(AgentLoopOutput::Response(AgentRunResult::Finished(resp))) => {
                assert_eq!(resp.content, "stub");
            }
            Ok(other) => panic!("expected LLM response output, got {other:?}"),
            Err(err) => panic!("natural language turn failed: {err}"),
        }

        let entries = tape.all_entries(&"session-1".to_string()).await;
        let entries = match entries {
            Ok(entries) => entries,
            Err(err) => panic!("failed to read tape entries: {err}"),
        };
        assert_eq!(
            entries.len(),
            2,
            "natural language turns should record both user and assistant"
        );
        assert!(matches!(
            entries.first().map(|entry| &entry.kind),
            Some(TapeEntryKind::Message { role: bob_core::types::Role::User, content })
                if content == "hello world"
        ));
        assert!(matches!(
            entries.get(1).map(|entry| &entry.kind),
            Some(TapeEntryKind::Message { role: bob_core::types::Role::Assistant, content })
                if content == "stub"
        ));
    }

    #[tokio::test]
    async fn handoff_resets_session_messages_but_keeps_usage() {
        let store = Arc::new(MemorySessionStore {
            state: Mutex::new(Some(SessionState {
                messages: vec![
                    bob_core::types::Message::text(bob_core::types::Role::User, "before"),
                    bob_core::types::Message::text(bob_core::types::Role::Assistant, "answer"),
                ],
                total_usage: TokenUsage { prompt_tokens: 21, completion_tokens: 13 },
            })),
        });
        let tape = Arc::new(MemoryTapeStore::default());
        let loop_ = AgentLoop::new(Arc::new(StubRuntime), Arc::new(StubTools))
            .with_store(store.clone())
            .with_tape(tape.clone());

        let output = loop_.handle_input("/handoff phase-2", "session-1").await;

        match output {
            Ok(AgentLoopOutput::CommandOutput(body)) => {
                assert!(body.contains("Context window reset"));
            }
            Ok(other) => panic!("expected handoff command output, got {other:?}"),
            Err(err) => panic!("handoff command failed: {err}"),
        }

        let saved = store.load(&"session-1".to_string()).await;
        let saved = match saved {
            Ok(Some(state)) => state,
            other => panic!("expected saved session state, got {other:?}"),
        };
        assert!(saved.messages.is_empty(), "handoff should clear session messages");
        assert_eq!(saved.total_usage.total(), 34, "handoff should preserve cumulative usage");

        let entries = tape.all_entries(&"session-1".to_string()).await;
        let entries = match entries {
            Ok(entries) => entries,
            Err(err) => panic!("failed to read tape entries: {err}"),
        };
        assert_eq!(entries.len(), 1, "handoff should not leave a slash-command message on tape");
        assert!(
            entries.iter().any(|entry| matches!(
                entry.kind,
                TapeEntryKind::Handoff { ref name, .. } if name == "phase-2"
            )),
            "handoff should be recorded to the tape",
        );
    }
}
