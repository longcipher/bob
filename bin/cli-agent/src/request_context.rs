use bob_runtime::core::{
    merge_allowlists, normalize_tool_list,
    types::{RequestContext, RequestToolPolicy},
};

use crate::{bootstrap::SkillsRuntimeContext, config::PolicyConfig};

pub(crate) fn build_request_context(
    input: &str,
    skills_context: Option<&SkillsRuntimeContext>,
    policy: Option<&PolicyConfig>,
) -> RequestContext {
    let deny_tools = policy
        .and_then(|p| p.deny_tools.clone())
        .map(|tools| normalize_tool_list(tools.iter().map(String::as_str)))
        .unwrap_or_default();
    let runtime_allow_tools = policy
        .and_then(|p| p.allow_tools.clone())
        .map(|tools| normalize_tool_list(tools.iter().map(String::as_str)));

    let mut context = RequestContext {
        system_prompt: None,
        selected_skills: Vec::new(),
        tool_policy: RequestToolPolicy { deny_tools, allow_tools: runtime_allow_tools.clone() },
    };

    let Some(skills_ctx) = skills_context else {
        return context;
    };

    let rendered = skills_ctx
        .composer
        .render_bundle_for_input_with_policy(input, &skills_ctx.selection_policy);
    if rendered.prompt.is_empty() {
        return context;
    }

    context.system_prompt = Some(rendered.prompt);
    context.selected_skills = rendered.selected_skill_names;

    let skill_allow_tools = (!rendered.selected_allowed_tools.is_empty())
        .then(|| normalize_tool_list(rendered.selected_allowed_tools.iter().map(String::as_str)));
    context.tool_policy.allow_tools =
        merge_allowlists(runtime_allow_tools.as_deref(), skill_allow_tools.as_deref());

    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PolicyConfig;

    #[test]
    fn request_context_includes_runtime_policy_without_skills() {
        let policy = PolicyConfig {
            deny_tools: Some(vec!["local/shell_exec".to_string()]),
            allow_tools: Some(vec!["local/read_file".to_string()]),
        };

        let ctx = build_request_context("hello", None, Some(&policy));
        assert_eq!(ctx.tool_policy.deny_tools, vec!["local/shell_exec".to_string()]);
        assert_eq!(ctx.tool_policy.allow_tools, Some(vec!["local/read_file".to_string()]));
        assert!(ctx.system_prompt.is_none());
        assert!(ctx.selected_skills.is_empty());
    }
}
