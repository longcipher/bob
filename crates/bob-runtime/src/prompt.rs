//! # Prompt Builder
//!
//! Prompt builder — assembles `LlmRequest` from session state, tool
//! descriptors, and system instructions.
//!
//! ## Overview
//!
//! The prompt builder constructs the complete LLM request by combining:
//!
//! 1. **System instructions**: Core instructions + action schema + tool schemas
//! 2. **Session history**: Message history (truncated to most recent 50 non-system messages)
//! 3. **Tool definitions**: Available tools and their schemas
//!
//! ## Components
//!
//! - **Action Schema**: JSON schema for the action protocol
//! - **Tool Schema Block**: Formatted list of available tools
//! - **History Truncation**: Keeps most recent messages to fit context limits
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_runtime::prompt::build_llm_request;
//! use bob_core::types::{SessionState, ToolDescriptor};
//!
//! let session = SessionState::default();
//! let tools = vec![];
//! let request = build_llm_request("openai:gpt-4o-mini", &session, &tools, "You are helpful.");
//! ```

use bob_core::types::{LlmRequest, Message, Role, SessionState, ToolDescriptor};

/// Maximum number of non-system history messages to keep.
const MAX_HISTORY: usize = 50;

/// Options controlling prompt shape for different dispatch strategies.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PromptBuildOptions {
    pub include_action_schema: bool,
    pub include_tool_schema: bool,
}

impl Default for PromptBuildOptions {
    fn default() -> Self {
        Self { include_action_schema: true, include_tool_schema: true }
    }
}

/// Returns the JSON action-schema contract text (design doc §8.3.1).
pub(crate) fn action_schema_prompt() -> String {
    r#"You must respond with exactly one JSON object and no extra text.
Schema:
{
  "type": "final" | "tool_call" | "ask_user",
  "content": "string (required when type=final)",
  "name": "string (required when type=tool_call)",
  "arguments": "object (required when type=tool_call)",
  "question": "string (required when type=ask_user)"
}"#
    .to_string()
}

/// Renders tool names, descriptions, and input schemas as a text block.
///
/// Returns an empty string when no tools are available.
pub(crate) fn tool_schema_block(tools: &[ToolDescriptor]) -> String {
    if tools.is_empty() {
        return String::new();
    }

    let mut buf = String::from("Available tools:\n");
    for tool in tools {
        buf.push_str(&format!(
            "\n- **{}**: {}\n  Input schema: {}\n",
            tool.id,
            tool.description,
            serde_json::to_string_pretty(&tool.input_schema).unwrap_or_default(),
        ));
    }
    buf
}

/// Assembles a complete `LlmRequest`:
///   1. System message = core instructions + action schema + tool schemas
///   2. Session history (truncated to most recent 50 non-system messages)
///   3. `LlmRequest { model, messages, tools }`
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "compatibility wrapper retained for callers that use default prompt build options"
    )
)]
pub(crate) fn build_llm_request(
    model: &str,
    session: &SessionState,
    tools: &[ToolDescriptor],
    system_instructions: &str,
) -> LlmRequest {
    build_llm_request_with_options(
        model,
        session,
        tools,
        system_instructions,
        PromptBuildOptions::default(),
    )
}

/// Assembles an `LlmRequest` with configurable schema/tool prompt sections.
pub(crate) fn build_llm_request_with_options(
    model: &str,
    session: &SessionState,
    tools: &[ToolDescriptor],
    system_instructions: &str,
    options: PromptBuildOptions,
) -> LlmRequest {
    // -- system message --------------------------------------------------
    let mut system_content = system_instructions.to_string();
    if options.include_action_schema {
        system_content.push_str("\n\n");
        system_content.push_str(&action_schema_prompt());
    }

    let tool_block =
        if options.include_tool_schema { tool_schema_block(tools) } else { String::new() };
    if !tool_block.is_empty() {
        system_content.push_str("\n\n");
        system_content.push_str(&tool_block);
    }

    let system_msg = Message { role: Role::System, content: system_content };

    // -- history (truncated) ---------------------------------------------
    let history = truncate_history(&session.messages, MAX_HISTORY);

    // -- assemble --------------------------------------------------------
    let mut messages = Vec::with_capacity(1 + history.len());
    messages.push(system_msg);
    messages.extend(history);

    LlmRequest { model: model.to_string(), messages, tools: tools.to_vec() }
}

/// Keeps at most `max` non-system messages, dropping the oldest first.
/// System messages are never dropped.
pub(crate) fn truncate_history(messages: &[Message], max: usize) -> Vec<Message> {
    let non_system_count = messages.iter().filter(|m| m.role != Role::System).count();

    if non_system_count <= max {
        return messages.to_vec();
    }

    let to_drop = non_system_count - max;
    let mut dropped = 0usize;
    let mut result = Vec::with_capacity(messages.len() - to_drop);

    for m in messages {
        if m.role == Role::System {
            // System messages are always kept.
            result.push(m.clone());
        } else if dropped < to_drop {
            // Drop the oldest non-system messages.
            dropped += 1;
        } else {
            result.push(m.clone());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use bob_core::types::{SessionState, TokenUsage, ToolSource};
    use serde_json::json;

    use super::*;

    // ── Helpers ──────────────────────────────────────────────────────

    fn make_tool(id: &str) -> ToolDescriptor {
        ToolDescriptor {
            id: id.to_string(),
            description: format!("{id} description"),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            source: ToolSource::Local,
        }
    }

    fn msg(role: Role, content: &str) -> Message {
        Message { role, content: content.to_string() }
    }

    // ── action_schema_prompt ─────────────────────────────────────────

    #[test]
    fn prompt_action_schema_contains_required_types() {
        let schema = action_schema_prompt();
        assert!(schema.contains("final"), "must mention 'final' action type");
        assert!(schema.contains("tool_call"), "must mention 'tool_call' action type");
        assert!(schema.contains("ask_user"), "must mention 'ask_user' action type");
    }

    #[test]
    fn prompt_action_schema_mentions_json() {
        let schema = action_schema_prompt();
        assert!(schema.contains("JSON"), "must instruct the LLM to respond with JSON");
    }

    // ── tool_schema_block ────────────────────────────────────────────

    #[test]
    fn prompt_tool_schema_empty() {
        let block = tool_schema_block(&[]);
        // Empty tool list should produce a meaningful "no tools" indicator or empty block.
        assert!(block.is_empty() || block.contains("No tools"), "empty tools produce no block");
    }

    #[test]
    fn prompt_tool_schema_renders_names_and_descriptions() {
        let tools = vec![make_tool("read_file"), make_tool("write_file")];
        let block = tool_schema_block(&tools);
        assert!(block.contains("read_file"), "must include tool name");
        assert!(block.contains("read_file description"), "must include description");
        assert!(block.contains("write_file"), "must include second tool");
    }

    #[test]
    fn prompt_tool_schema_renders_input_schema() {
        let tools = vec![make_tool("grep")];
        let block = tool_schema_block(&tools);
        assert!(block.contains("path"), "must include input_schema fields");
    }

    // ── truncate_history ─────────────────────────────────────────────

    #[test]
    fn prompt_truncate_noop_when_under_limit() {
        let msgs = vec![msg(Role::User, "a"), msg(Role::Assistant, "b")];
        let result = truncate_history(&msgs, 50);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn prompt_truncate_drops_oldest_non_system() {
        let mut msgs: Vec<Message> =
            (0..60).map(|i| msg(Role::User, &format!("msg-{i}"))).collect();
        // Prepend a system message.
        msgs.insert(0, msg(Role::System, "sys"));
        let result = truncate_history(&msgs, 50);
        // System message is kept, plus the 50 most recent non-system messages.
        assert_eq!(result.len(), 51);
        assert_eq!(result[0].role, Role::System);
        // The oldest kept non-system should be msg-10 (dropped 0..10).
        assert!(result[1].content.contains("msg-10"));
    }

    #[test]
    fn prompt_truncate_keeps_all_system_messages() {
        let msgs = vec![
            msg(Role::System, "sys-1"),
            msg(Role::User, "u1"),
            msg(Role::System, "sys-2"),
            msg(Role::User, "u2"),
            msg(Role::Assistant, "a1"),
        ];
        let result = truncate_history(&msgs, 2);
        // Both system messages kept + 2 most recent non-system (u2, a1).
        assert_eq!(result.len(), 4);
        let system_count = result.iter().filter(|m| m.role == Role::System).count();
        assert_eq!(system_count, 2);
    }

    #[test]
    fn prompt_truncate_preserves_order() {
        let msgs = vec![
            msg(Role::System, "sys"),
            msg(Role::User, "old"),
            msg(Role::User, "mid"),
            msg(Role::User, "new"),
        ];
        let result = truncate_history(&msgs, 2);
        assert_eq!(result.len(), 3); // sys + mid + new
        assert_eq!(result[0].content, "sys");
        assert_eq!(result[1].content, "mid");
        assert_eq!(result[2].content, "new");
    }

    #[test]
    fn prompt_truncate_empty_history() {
        let result = truncate_history(&[], 50);
        assert!(result.is_empty());
    }

    #[test]
    fn prompt_truncate_exactly_at_limit() {
        let msgs: Vec<Message> = (0..50).map(|i| msg(Role::User, &format!("u-{i}"))).collect();
        let result = truncate_history(&msgs, 50);
        assert_eq!(result.len(), 50, "no messages should be dropped at exact limit");
        assert_eq!(result[0].content, "u-0");
        assert_eq!(result[49].content, "u-49");
    }

    #[test]
    fn prompt_truncate_single_message() {
        let msgs = vec![msg(Role::User, "only")];
        let result = truncate_history(&msgs, 50);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "only");
    }

    #[test]
    fn prompt_truncate_all_system_messages() {
        let msgs = vec![msg(Role::System, "s1"), msg(Role::System, "s2"), msg(Role::System, "s3")];
        let result = truncate_history(&msgs, 1);
        // All system messages are kept regardless of limit.
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn prompt_truncate_limit_zero_keeps_only_system() {
        let msgs =
            vec![msg(Role::System, "sys"), msg(Role::User, "u1"), msg(Role::Assistant, "a1")];
        let result = truncate_history(&msgs, 0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, Role::System);
    }

    #[test]
    fn prompt_truncate_interleaved_system_preserves_all() {
        let msgs = vec![
            msg(Role::System, "init"),
            msg(Role::User, "u1"),
            msg(Role::System, "mid-sys"),
            msg(Role::User, "u2"),
            msg(Role::Assistant, "a1"),
            msg(Role::System, "late-sys"),
            msg(Role::User, "u3"),
        ];
        // Keep only 2 non-system messages → u2 dropped, keep a1 + u3
        // Wait — there are 4 non-system: u1, u2, a1, u3. Keep 2 → drop u1, u2.
        let result = truncate_history(&msgs, 2);
        let system_count = result.iter().filter(|m| m.role == Role::System).count();
        assert_eq!(system_count, 3, "all three system messages must survive");
        let non_system: Vec<&str> =
            result.iter().filter(|m| m.role != Role::System).map(|m| m.content.as_str()).collect();
        assert_eq!(non_system, vec!["a1", "u3"]);
    }

    // ── build_llm_request ────────────────────────────────────────────

    #[test]
    fn prompt_build_empty_session() {
        let session = SessionState::default();
        let req = build_llm_request("test-model", &session, &[], "You are Bob.");
        assert_eq!(req.model, "test-model");
        // First message must be system.
        assert_eq!(req.messages[0].role, Role::System);
        assert!(req.messages[0].content.contains("You are Bob."));
        // No history messages besides system.
        assert_eq!(req.messages.len(), 1);
        assert!(req.tools.is_empty());
    }

    #[test]
    fn prompt_build_system_contains_action_schema() {
        let session = SessionState::default();
        let req = build_llm_request("m", &session, &[], "instructions");
        assert!(req.messages[0].content.contains("JSON"));
        assert!(req.messages[0].content.contains("tool_call"));
    }

    #[test]
    fn prompt_build_includes_tools() {
        let tools = vec![make_tool("t1")];
        let session = SessionState::default();
        let req = build_llm_request("m", &session, &tools, "inst");
        assert_eq!(req.tools.len(), 1);
        assert!(req.messages[0].content.contains("t1"));
    }

    #[test]
    fn prompt_build_message_ordering() {
        let session = SessionState {
            messages: vec![msg(Role::User, "hello"), msg(Role::Assistant, "hi")],
            total_usage: TokenUsage::default(),
        };
        let req = build_llm_request("m", &session, &[], "sys");
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[1].role, Role::User);
        assert_eq!(req.messages[2].role, Role::Assistant);
    }

    #[test]
    fn prompt_build_truncates_long_history() {
        let messages: Vec<Message> = (0..60).map(|i| msg(Role::User, &format!("m-{i}"))).collect();
        let session = SessionState { messages, total_usage: TokenUsage::default() };
        let req = build_llm_request("m", &session, &[], "sys");
        // 1 system + 50 truncated history = 51
        assert_eq!(req.messages.len(), 51);
        assert_eq!(req.messages[0].role, Role::System);
    }
}
