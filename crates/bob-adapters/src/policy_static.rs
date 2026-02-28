//! Static tool-policy adapter composed from runtime and request policies.

use bob_core::{is_tool_allowed, merge_allowlists, normalize_tool_list, ports::ToolPolicyPort};

/// Runtime-level static tool policy.
#[derive(Debug, Clone, Default)]
pub struct StaticToolPolicyPort {
    runtime_deny_tools: Vec<String>,
    runtime_allow_tools: Option<Vec<String>>,
    default_deny: bool,
}

impl StaticToolPolicyPort {
    #[must_use]
    pub fn new(
        runtime_deny_tools: Vec<String>,
        runtime_allow_tools: Option<Vec<String>>,
        default_deny: bool,
    ) -> Self {
        Self {
            runtime_deny_tools: normalize_tool_list(runtime_deny_tools.iter().map(String::as_str)),
            runtime_allow_tools: runtime_allow_tools
                .map(|tools| normalize_tool_list(tools.iter().map(String::as_str))),
            default_deny,
        }
    }
}

impl ToolPolicyPort for StaticToolPolicyPort {
    fn is_tool_allowed(
        &self,
        tool: &str,
        deny_tools: &[String],
        allow_tools: Option<&[String]>,
    ) -> bool {
        let effective_deny = normalize_tool_list(
            self.runtime_deny_tools
                .iter()
                .map(String::as_str)
                .chain(deny_tools.iter().map(String::as_str)),
        );
        let effective_allow = merge_allowlists(self.runtime_allow_tools.as_deref(), allow_tools);
        if self.default_deny && effective_allow.is_none() {
            return false;
        }
        is_tool_allowed(tool, &effective_deny, effective_allow.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_deny_blocks_tool_even_when_request_allows() {
        let policy = StaticToolPolicyPort::new(
            vec!["local/shell_exec".to_string()],
            Some(vec!["local/shell_exec".to_string(), "local/read_file".to_string()]),
            false,
        );
        let request_allow = vec!["local/shell_exec".to_string()];
        assert!(!policy.is_tool_allowed("local/shell_exec", &[], Some(request_allow.as_slice())));
    }

    #[test]
    fn default_deny_requires_effective_allowlist() {
        let policy = StaticToolPolicyPort::new(vec![], None, true);
        assert!(!policy.is_tool_allowed("local/read_file", &[], None));

        let request_allow = vec!["local/read_file".to_string()];
        assert!(policy.is_tool_allowed("local/read_file", &[], Some(request_allow.as_slice())));
    }

    #[test]
    fn runtime_and_request_allowlists_are_intersected() {
        let policy = StaticToolPolicyPort::new(
            vec![],
            Some(vec!["local/read_file".to_string(), "local/write_file".to_string()]),
            false,
        );
        let request_allow = vec!["local/read_file".to_string()];
        assert!(policy.is_tool_allowed("local/read_file", &[], Some(request_allow.as_slice())));
        assert!(!policy.is_tool_allowed("local/write_file", &[], Some(request_allow.as_slice())));
    }
}
