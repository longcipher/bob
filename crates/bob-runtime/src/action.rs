//! # Action Parser
//!
//! Action parser — extracts [`AgentAction`] from raw LLM text output.
//!
//! ## Overview
//!
//! The parser handles the conversion from raw LLM text output to structured
//! [`AgentAction`] enum variants. It supports:
//!
//! - Plain JSON objects
//! - JSON wrapped in markdown code fences (`` ```json ``)
//! - Automatic whitespace trimming
//!
//! ## Action Types
//!
//! The parser recognizes three action types:
//!
//! - **`final`**: Final response to the user
//! - **`tool_call`**: Request to execute a tool
//! - **`ask_user`**: Request for user input
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_runtime::action::parse_action;
//! use bob_core::types::AgentAction;
//!
//! let json = r#"{"type": "final", "content": "Hello!"}"#;
//! let action = parse_action(json)?;
//!
//! match action {
//!     AgentAction::Final { content } => println!("Response: {}", content),
//!     _ => println!("Other action"),
//! }
//! ```

use bob_core::types::AgentAction;

// ── Error Type ───────────────────────────────────────────────────────

/// Errors that can occur when parsing an [`AgentAction`] from LLM output.
#[derive(Debug, thiserror::Error)]
pub enum ActionParseError {
    /// The input could not be parsed as valid JSON.
    #[error("invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    /// A required field is missing from the JSON object.
    #[error("missing required field: {0}")]
    MissingField(String),

    /// The `type` field contains an unrecognised variant.
    #[error("unknown action type: {0}")]
    UnknownType(String),
}

// ── Public API ───────────────────────────────────────────────────────

/// Parse a raw LLM text response into an [`AgentAction`].
///
/// Handles optional markdown code fences (`` ```json `` / `` ``` ``) and
/// leading/trailing whitespace.
pub fn parse_action(content: &str) -> Result<AgentAction, ActionParseError> {
    let stripped = strip_code_fences(content);

    // First pass: ensure it is valid JSON and is an object.
    let value: serde_json::Value = serde_json::from_str(stripped)?;

    let obj = value.as_object().ok_or_else(|| ActionParseError::MissingField("type".to_owned()))?;

    // Check for the mandatory `type` discriminator.
    let type_val = obj
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ActionParseError::MissingField("type".to_owned()))?;

    // Validate that `type` is a known variant before full deserialization so we
    // can distinguish "unknown type" from other serde errors.
    const KNOWN_TYPES: &[&str] = &["final", "tool_call", "ask_user"];
    if !KNOWN_TYPES.contains(&type_val) {
        return Err(ActionParseError::UnknownType(type_val.to_owned()));
    }

    // Full deserialization — will surface missing variant-specific fields as
    // `InvalidJson` via the `From<serde_json::Error>` impl.
    let action: AgentAction = serde_json::from_value(value)?;
    Ok(action)
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Strip optional markdown code fences and surrounding whitespace.
fn strip_code_fences(input: &str) -> &str {
    let trimmed = input.trim();
    // Handle ```json ... ``` and ``` ... ```
    let without_opening = trimmed.strip_prefix("```json").or_else(|| trimmed.strip_prefix("```"));
    match without_opening {
        Some(rest) => rest.strip_suffix("```").unwrap_or(rest).trim(),
        None => trimmed,
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // ── Happy-path: each variant ─────────────────────────────────────

    #[test]
    fn parse_final_variant() {
        let input = json!({"type": "final", "content": "Hello!"}).to_string();
        let action = parse_action(&input);
        assert!(
            matches!(action.as_ref(), Ok(AgentAction::Final { content }) if content == "Hello!")
        );
    }

    #[test]
    fn parse_tool_call_variant() {
        let input =
            json!({"type": "tool_call", "name": "search", "arguments": {"q": "rust"}}).to_string();
        let action = parse_action(&input);
        assert!(matches!(
            action.as_ref(),
            Ok(AgentAction::ToolCall { name, arguments })
                if name == "search" && *arguments == json!({"q": "rust"})
        ));
    }

    #[test]
    fn parse_ask_user_variant() {
        let input = json!({"type": "ask_user", "question": "Which file?"}).to_string();
        let action = parse_action(&input);
        assert!(matches!(
            action.as_ref(),
            Ok(AgentAction::AskUser { question }) if question == "Which file?"
        ));
    }

    // ── Error cases ──────────────────────────────────────────────────

    #[test]
    fn reject_missing_type_field() {
        let input = json!({"content": "Hello!"}).to_string();
        let err = parse_action(&input);
        assert!(
            matches!(err, Err(ActionParseError::MissingField(_))),
            "expected MissingField, got {err:?}",
        );
    }

    #[test]
    fn reject_unknown_type() {
        let input = json!({"type": "explode", "payload": 42}).to_string();
        let err = parse_action(&input);
        assert!(
            matches!(err, Err(ActionParseError::UnknownType(_))),
            "expected UnknownType, got {err:?}",
        );
    }

    #[test]
    fn reject_non_json_text() {
        let err = parse_action("this is not json");
        assert!(
            matches!(err, Err(ActionParseError::InvalidJson(_))),
            "expected InvalidJson, got {err:?}",
        );
    }

    #[test]
    fn reject_missing_required_field_for_variant() {
        // ToolCall requires `name` and `arguments`
        let input = json!({"type": "tool_call", "name": "search"}).to_string();
        let err = parse_action(&input);
        assert!(
            matches!(
                err,
                Err(ActionParseError::InvalidJson(_) | ActionParseError::MissingField(_))
            ),
            "expected InvalidJson or MissingField, got {err:?}",
        );
    }

    // ── Edge cases ───────────────────────────────────────────────────

    #[test]
    fn handle_json_code_fence() {
        let input = format!("```json\n{}\n```", json!({"type": "final", "content": "done"}));
        let action = parse_action(&input);
        assert!(matches!(action, Ok(AgentAction::Final { .. })));
    }

    #[test]
    fn handle_plain_code_fence() {
        let input = format!("```\n{}\n```", json!({"type": "ask_user", "question": "yes?"}));
        let action = parse_action(&input);
        assert!(matches!(action, Ok(AgentAction::AskUser { .. })));
    }

    #[test]
    fn handle_extra_whitespace() {
        let input = format!("  \n\n  {}  \n\n  ", json!({"type": "final", "content": "hi"}));
        let action = parse_action(&input);
        assert!(matches!(action, Ok(AgentAction::Final { .. })));
    }
}
