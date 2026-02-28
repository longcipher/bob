//! Static approval adapter for tool-call guardrails.

use bob_core::{
    ToolError, normalize_tool_list,
    ports::ApprovalPort,
    tools_match,
    types::{ApprovalContext, ApprovalDecision, ToolCall},
};

/// Approval behavior mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticApprovalMode {
    AllowAll,
    DenyAll,
}

/// Runtime static approval policy.
#[derive(Debug, Clone)]
pub struct StaticApprovalPort {
    mode: StaticApprovalMode,
    deny_tools: Vec<String>,
}

impl StaticApprovalPort {
    #[must_use]
    pub fn new(mode: StaticApprovalMode, deny_tools: Vec<String>) -> Self {
        Self { mode, deny_tools: normalize_tool_list(deny_tools.iter().map(String::as_str)) }
    }
}

#[async_trait::async_trait]
impl ApprovalPort for StaticApprovalPort {
    async fn approve_tool_call(
        &self,
        call: &ToolCall,
        _context: &ApprovalContext,
    ) -> Result<ApprovalDecision, ToolError> {
        if self.mode == StaticApprovalMode::DenyAll {
            return Ok(ApprovalDecision::Denied {
                reason: "tool calls are disabled by approval policy".to_string(),
            });
        }
        if self.deny_tools.iter().any(|tool| tools_match(tool, &call.name)) {
            return Ok(ApprovalDecision::Denied {
                reason: format!("tool '{}' denied by approval policy", call.name),
            });
        }
        Ok(ApprovalDecision::Approved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool_call(name: &str) -> ToolCall {
        ToolCall { name: name.to_string(), arguments: serde_json::json!({}) }
    }

    fn test_context() -> ApprovalContext {
        ApprovalContext { session_id: "s1".to_string(), turn_step: 1, selected_skills: Vec::new() }
    }

    #[tokio::test]
    async fn deny_all_rejects_everything() {
        let approval = StaticApprovalPort::new(StaticApprovalMode::DenyAll, vec![]);
        let decision =
            approval.approve_tool_call(&make_tool_call("local/read_file"), &test_context()).await;
        assert!(decision.is_ok());
        assert!(matches!(decision.ok(), Some(ApprovalDecision::Denied { .. })));
    }

    #[tokio::test]
    async fn deny_tools_rejects_listed_tools() {
        let approval =
            StaticApprovalPort::new(StaticApprovalMode::AllowAll, vec!["local/shell_exec".into()]);
        let decision =
            approval.approve_tool_call(&make_tool_call("local/shell_exec"), &test_context()).await;
        assert!(decision.is_ok());
        assert!(matches!(decision.ok(), Some(ApprovalDecision::Denied { .. })));
    }

    #[tokio::test]
    async fn allow_mode_allows_unlisted_tools() {
        let approval =
            StaticApprovalPort::new(StaticApprovalMode::AllowAll, vec!["local/shell_exec".into()]);
        let decision =
            approval.approve_tool_call(&make_tool_call("local/read_file"), &test_context()).await;
        assert!(decision.is_ok());
        assert!(matches!(decision.ok(), Some(ApprovalDecision::Approved)));
    }
}
