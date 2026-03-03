//! # Progressive Tool View
//!
//! Token-efficient tool definition management for LLM requests.
//!
//! ## Problem
//!
//! Sending full JSON Schema definitions for all tools on every LLM request
//! wastes significant tokens. A workspace with 10+ MCP tools can easily
//! consume 700+ tokens on schema alone — even for simple chat messages
//! that don't need any tools.
//!
//! ## Solution
//!
//! `ProgressiveToolView` implements a **"menu first, recipe on demand"** strategy:
//!
//! 1. **Initial state** (~50 tokens): system prompt includes only tool names and one-line
//!    descriptions.
//! 2. **On activation**: when the LLM mentions `$tool_name` or actually calls a tool, that tool's
//!    full schema is included in subsequent requests.
//!
//! This typically saves **90%+** of tool-related token usage for simple
//! conversations, and progressively reveals schemas as the agent needs them.

use std::collections::HashSet;

use bob_core::types::ToolDescriptor;

/// Token-efficient tool view that progressively reveals tool schemas.
///
/// Only activated tools have their full JSON Schema included in LLM requests.
/// Inactive tools appear only as compact name + description entries in the
/// system prompt.
#[derive(Debug)]
pub struct ProgressiveToolView {
    /// All available tools (full descriptors).
    all_tools: Vec<ToolDescriptor>,
    /// Set of tool IDs that have been activated (full schema should be sent).
    activated: HashSet<String>,
}

impl ProgressiveToolView {
    /// Create a new progressive view from the full tool list.
    ///
    /// All tools start as inactive (compact view only).
    #[must_use]
    pub fn new(tools: Vec<ToolDescriptor>) -> Self {
        Self { all_tools: tools, activated: HashSet::new() }
    }

    /// Activate a specific tool by ID (its full schema will be sent).
    pub fn activate(&mut self, tool_id: &str) {
        self.activated.insert(tool_id.to_string());
    }

    /// Scan LLM output text for `$tool_name` hints and activate mentioned tools.
    ///
    /// This allows the LLM to signal interest in a tool before actually calling it,
    /// so the full schema is available on the next turn.
    pub fn activate_hints(&mut self, text: &str) {
        for tool in &self.all_tools {
            let hint = format!("${}", tool.id);
            if text.contains(&hint) {
                self.activated.insert(tool.id.clone());
            }
        }
    }

    /// Returns `true` if the given tool has been activated.
    #[must_use]
    pub fn is_activated(&self, tool_id: &str) -> bool {
        self.activated.contains(tool_id)
    }

    /// Returns the number of currently activated tools.
    #[must_use]
    pub fn activated_count(&self) -> usize {
        self.activated.len()
    }

    /// Returns the total number of tools in the view.
    #[must_use]
    pub fn total_count(&self) -> usize {
        self.all_tools.len()
    }

    /// Returns a compact summary prompt listing all tools by name and
    /// description, suitable for injection into the system prompt.
    ///
    /// Returns an empty string when no tools are available.
    #[must_use]
    pub fn summary_prompt(&self) -> String {
        if self.all_tools.is_empty() {
            return String::new();
        }

        let mut buf =
            String::from("<tool_view>\nAvailable tools (use $name to request full schema):\n");
        for tool in &self.all_tools {
            let marker = if self.activated.contains(&tool.id) { " [active]" } else { "" };
            buf.push_str(&format!("  - {}: {}{}\n", tool.id, tool.description, marker));
        }
        buf.push_str("</tool_view>");
        buf
    }

    /// Returns full descriptors for activated tools only.
    ///
    /// These are the tools whose complete JSON Schema should be sent to the LLM
    /// (either in the prompt or via native tool calling API).
    #[must_use]
    pub fn activated_tools(&self) -> Vec<ToolDescriptor> {
        self.all_tools.iter().filter(|t| self.activated.contains(&t.id)).cloned().collect()
    }

    /// Returns all tool descriptors regardless of activation state.
    #[must_use]
    pub fn all_tools(&self) -> &[ToolDescriptor] {
        &self.all_tools
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use bob_core::types::ToolSource;
    use serde_json::json;

    use super::*;

    fn make_tool(id: &str, desc: &str) -> ToolDescriptor {
        ToolDescriptor {
            id: id.to_string(),
            description: desc.to_string(),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            source: ToolSource::Local,
        }
    }

    #[test]
    fn new_view_has_no_activated_tools() {
        let view = ProgressiveToolView::new(vec![
            make_tool("file.read", "Read a file"),
            make_tool("shell.exec", "Run a command"),
        ]);

        assert_eq!(view.activated_count(), 0);
        assert_eq!(view.total_count(), 2);
        assert!(view.activated_tools().is_empty());
    }

    #[test]
    fn activate_adds_tool_to_active_set() {
        let mut view = ProgressiveToolView::new(vec![
            make_tool("file.read", "Read a file"),
            make_tool("shell.exec", "Run a command"),
        ]);

        view.activate("file.read");

        assert_eq!(view.activated_count(), 1);
        assert!(view.is_activated("file.read"));
        assert!(!view.is_activated("shell.exec"));

        let active = view.activated_tools();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "file.read");
    }

    #[test]
    fn activate_hints_detects_dollar_prefix() {
        let mut view = ProgressiveToolView::new(vec![
            make_tool("file.read", "Read a file"),
            make_tool("shell.exec", "Run a command"),
        ]);

        view.activate_hints("I'll use $file.read to check the config");

        assert!(view.is_activated("file.read"));
        assert!(!view.is_activated("shell.exec"));
    }

    #[test]
    fn summary_prompt_lists_all_tools() {
        let mut view = ProgressiveToolView::new(vec![
            make_tool("file.read", "Read a file"),
            make_tool("shell.exec", "Run a command"),
        ]);

        view.activate("file.read");
        let summary = view.summary_prompt();

        assert!(summary.contains("file.read"));
        assert!(summary.contains("shell.exec"));
        assert!(summary.contains("[active]"));
        assert!(summary.contains("<tool_view>"));
    }

    #[test]
    fn empty_tool_list_produces_empty_summary() {
        let view = ProgressiveToolView::new(vec![]);
        assert!(view.summary_prompt().is_empty());
    }

    #[test]
    fn duplicate_activation_is_idempotent() {
        let mut view = ProgressiveToolView::new(vec![make_tool("file.read", "Read a file")]);

        view.activate("file.read");
        view.activate("file.read");

        assert_eq!(view.activated_count(), 1);
    }
}
