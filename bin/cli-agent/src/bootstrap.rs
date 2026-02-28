use std::sync::Arc;

use bob_adapters::{
    approval_static::{StaticApprovalMode, StaticApprovalPort},
    artifact_memory::InMemoryArtifactStore,
    checkpoint_memory::InMemoryCheckpointStore,
    cost_simple::SimpleCostMeter,
    observe::TracingEventSink,
    policy_static::StaticToolPolicyPort,
    skills_agent::{SkillPromptComposer, SkillSelectionPolicy, SkillSourceConfig},
    store_memory::InMemorySessionStore,
};
use bob_runtime::{
    AgentBootstrap, AgentRuntime, NoOpToolPort, RuntimeBuilder, TimeoutToolLayer, ToolLayer,
    composite::CompositeToolPort,
};
use eyre::WrapErr;

use crate::config::{
    AgentConfig, ApprovalMode, McpServerEntry, McpTransport, RuntimeConfig, RuntimeDispatchMode,
    SkillSourceType, SkillsConfig, resolve_env_placeholders,
};

pub(crate) const DEFAULT_TOOL_TIMEOUT_MS: u64 = 15_000;
pub(crate) const DEFAULT_SKILLS_TOKEN_BUDGET: usize = 1_800;
pub(crate) const DEFAULT_MODEL_CONTEXT_TOKENS: usize = 128_000;

#[derive(Debug, Clone)]
pub(crate) struct SkillsRuntimeContext {
    pub composer: SkillPromptComposer,
    pub selection_policy: SkillSelectionPolicy,
}

/// Build the runtime from a loaded config.
pub(crate) async fn build_runtime(
    cfg: &AgentConfig,
) -> eyre::Result<(Arc<dyn AgentRuntime>, Option<SkillsRuntimeContext>)> {
    let client = genai::Client::default();
    let llm: Arc<dyn bob_adapters::core::ports::LlmPort> =
        Arc::new(bob_adapters::llm_genai::GenAiLlmAdapter::new(client));

    let tools = build_tool_port(cfg).await?;
    let store: Arc<dyn bob_adapters::core::ports::SessionStore> =
        Arc::new(InMemorySessionStore::new());
    let events: Arc<dyn bob_adapters::core::ports::EventSink> = Arc::new(TracingEventSink::new());

    let tool_timeout_ms = cfg.mcp.as_ref().map_or(DEFAULT_TOOL_TIMEOUT_MS, |mcp_cfg| {
        mcp_cfg
            .servers
            .iter()
            .map(|server| server.tool_timeout_ms.unwrap_or(DEFAULT_TOOL_TIMEOUT_MS))
            .max()
            .unwrap_or(DEFAULT_TOOL_TIMEOUT_MS)
    });

    let policy = bob_adapters::core::types::TurnPolicy {
        max_steps: cfg.runtime.max_steps.unwrap_or(12),
        turn_timeout_ms: cfg.runtime.turn_timeout_ms.unwrap_or(90_000),
        tool_timeout_ms,
        ..bob_adapters::core::types::TurnPolicy::default()
    };

    let runtime = RuntimeBuilder::new()
        .with_llm(llm)
        .with_tools(tools)
        .with_store(store)
        .with_events(events)
        .with_default_model(cfg.runtime.default_model.clone())
        .with_policy(policy)
        .with_tool_policy(build_tool_policy_port(cfg))
        .with_approval(build_approval_port(cfg))
        .with_dispatch_mode(resolve_dispatch_mode(&cfg.runtime))
        .with_checkpoint_store(Arc::new(InMemoryCheckpointStore::new()))
        .with_artifact_store(Arc::new(InMemoryArtifactStore::new()))
        .with_cost_meter(Arc::new(SimpleCostMeter::new(
            cfg.cost.as_ref().and_then(|cost| cost.session_token_budget),
        )))
        .build()
        .wrap_err("failed to build runtime")?;

    let skills_context = build_skills_composer(cfg)?;
    Ok((runtime, skills_context))
}

async fn build_tool_port(
    cfg: &AgentConfig,
) -> eyre::Result<Arc<dyn bob_adapters::core::ports::ToolPort>> {
    let Some(mcp_cfg) = cfg.mcp.as_ref() else {
        return Ok(Arc::new(NoOpToolPort));
    };
    if mcp_cfg.servers.is_empty() {
        return Ok(Arc::new(NoOpToolPort));
    }

    if mcp_cfg.servers.len() == 1 {
        return build_single_tool_port(&mcp_cfg.servers[0]).await;
    }

    let mut ports: Vec<(String, Arc<dyn bob_adapters::core::ports::ToolPort>)> =
        Vec::with_capacity(mcp_cfg.servers.len());
    for entry in &mcp_cfg.servers {
        let port = build_single_tool_port(entry).await?;
        ports.push((entry.id.clone(), port));
    }
    Ok(Arc::new(CompositeToolPort::new(ports)))
}

async fn build_single_tool_port(
    entry: &McpServerEntry,
) -> eyre::Result<Arc<dyn bob_adapters::core::ports::ToolPort>> {
    let env_vec = resolve_mcp_env(entry.env.as_ref())?;
    let adapter = match entry.transport {
        McpTransport::Stdio => bob_adapters::mcp_rmcp::McpToolAdapter::connect_stdio(
            &entry.id,
            &entry.command,
            &entry.args,
            &env_vec,
        )
        .await
        .wrap_err_with(|| format!("failed to connect MCP server '{}'", entry.id))?,
    };
    let inner: Arc<dyn bob_adapters::core::ports::ToolPort> = Arc::new(adapter);
    let timeout_layer =
        TimeoutToolLayer::new(entry.tool_timeout_ms.unwrap_or(DEFAULT_TOOL_TIMEOUT_MS));
    Ok(timeout_layer.wrap(inner))
}

fn resolve_mcp_env(
    env: Option<&std::collections::HashMap<String, String>>,
) -> eyre::Result<Vec<(String, String)>> {
    let Some(env) = env else {
        return Ok(Vec::new());
    };

    let mut resolved = Vec::with_capacity(env.len());
    for (key, value) in env {
        let parsed = resolve_env_placeholders(value)
            .wrap_err_with(|| format!("failed to resolve env placeholder for key '{key}'"))?;
        resolved.push((key.clone(), parsed));
    }
    Ok(resolved)
}

pub(crate) fn build_skills_composer(
    cfg: &AgentConfig,
) -> eyre::Result<Option<SkillsRuntimeContext>> {
    let Some(skills_cfg) = cfg.skills.as_ref() else {
        return Ok(None);
    };
    if skills_cfg.sources.is_empty() {
        return Ok(None);
    }

    let sources = skills_cfg
        .sources
        .iter()
        .map(|source| match source.source_type {
            SkillSourceType::Directory => SkillSourceConfig {
                path: std::path::PathBuf::from(&source.path),
                recursive: source.recursive.unwrap_or(false),
            },
        })
        .collect::<Vec<_>>();

    let composer =
        SkillPromptComposer::from_sources(&sources, skills_cfg.max_selected.unwrap_or(3))
            .wrap_err("failed to load skills from configured sources")?;

    let (deny_tools, allow_tools) = cfg.policy.as_ref().map_or_else(
        || (Vec::new(), None),
        |policy| (policy.deny_tools.clone().unwrap_or_default(), policy.allow_tools.clone()),
    );
    let token_budget_tokens = resolve_skills_token_budget(&cfg.runtime, skills_cfg)?;
    let selection_policy = SkillSelectionPolicy { deny_tools, allow_tools, token_budget_tokens };

    Ok(Some(SkillsRuntimeContext { composer, selection_policy }))
}

pub(crate) fn resolve_skills_token_budget(
    runtime: &crate::config::RuntimeConfig,
    skills: &SkillsConfig,
) -> eyre::Result<usize> {
    if let Some(tokens) = skills.token_budget_tokens {
        return Ok(tokens.max(1));
    }

    if let Some(ratio) = skills.token_budget_ratio {
        if !(0.0..=1.0).contains(&ratio) || ratio == 0.0 {
            return Err(eyre::eyre!(
                "invalid skills.token_budget_ratio '{ratio}', expected 0.0 < ratio <= 1.0"
            ));
        }

        let context_tokens = runtime.model_context_tokens.unwrap_or(DEFAULT_MODEL_CONTEXT_TOKENS);
        let budget = (ratio * context_tokens as f64).round() as usize;
        return Ok(budget.max(1));
    }

    Ok(DEFAULT_SKILLS_TOKEN_BUDGET)
}

pub(crate) fn resolve_dispatch_mode(runtime: &RuntimeConfig) -> bob_runtime::DispatchMode {
    match runtime.dispatch_mode {
        Some(RuntimeDispatchMode::PromptGuided) => bob_runtime::DispatchMode::PromptGuided,
        Some(RuntimeDispatchMode::NativePreferred) | None => {
            bob_runtime::DispatchMode::NativePreferred
        }
    }
}

pub(crate) fn build_tool_policy_port(
    cfg: &AgentConfig,
) -> Arc<dyn bob_adapters::core::ports::ToolPolicyPort> {
    let deny_tools =
        cfg.policy.as_ref().and_then(|policy| policy.deny_tools.clone()).unwrap_or_default();
    let allow_tools = cfg.policy.as_ref().and_then(|policy| policy.allow_tools.clone());
    let default_deny = cfg.policy.as_ref().and_then(|policy| policy.default_deny).unwrap_or(false);

    Arc::new(StaticToolPolicyPort::new(deny_tools, allow_tools, default_deny))
}

pub(crate) fn build_approval_port(
    cfg: &AgentConfig,
) -> Arc<dyn bob_adapters::core::ports::ApprovalPort> {
    let mode =
        cfg.approval.as_ref().and_then(|approval| approval.mode).unwrap_or(ApprovalMode::AllowAll);
    let mapped_mode = if mode == ApprovalMode::DenyAll {
        StaticApprovalMode::DenyAll
    } else {
        StaticApprovalMode::AllowAll
    };
    let deny_tools =
        cfg.approval.as_ref().and_then(|approval| approval.deny_tools.clone()).unwrap_or_default();
    Arc::new(StaticApprovalPort::new(mapped_mode, deny_tools))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AgentConfig, ApprovalConfig, ApprovalMode, PolicyConfig, RuntimeConfig,
        RuntimeDispatchMode, SkillSourceEntry,
    };

    fn minimal_agent_config() -> AgentConfig {
        AgentConfig {
            runtime: RuntimeConfig {
                default_model: "openai:gpt-4o-mini".to_string(),
                max_steps: None,
                turn_timeout_ms: None,
                model_context_tokens: None,
                dispatch_mode: None,
            },
            llm: None,
            policy: None,
            approval: None,
            mcp: None,
            skills: None,
            cost: None,
        }
    }

    #[test]
    fn resolves_skills_budget_from_ratio() -> eyre::Result<()> {
        let runtime = RuntimeConfig {
            default_model: "openai:gpt-4o-mini".to_string(),
            max_steps: Some(12),
            turn_timeout_ms: Some(90_000),
            model_context_tokens: Some(20_000),
            dispatch_mode: None,
        };
        let skills = SkillsConfig {
            sources: vec![SkillSourceEntry {
                source_type: SkillSourceType::Directory,
                path: "./skills".to_string(),
                recursive: Some(false),
            }],
            max_selected: Some(3),
            token_budget_tokens: None,
            token_budget_ratio: Some(0.10),
        };

        let budget = resolve_skills_token_budget(&runtime, &skills)?;
        assert_eq!(budget, 2_000);
        Ok(())
    }

    #[test]
    fn invalid_ratio_is_rejected() {
        let runtime = RuntimeConfig {
            default_model: "openai:gpt-4o-mini".to_string(),
            max_steps: None,
            turn_timeout_ms: None,
            model_context_tokens: None,
            dispatch_mode: None,
        };
        let skills = SkillsConfig {
            sources: vec![],
            max_selected: None,
            token_budget_tokens: None,
            token_budget_ratio: Some(1.2),
        };

        let result = resolve_skills_token_budget(&runtime, &skills);
        assert!(result.is_err(), "ratio > 1.0 must be rejected");
        let msg = match result {
            Err(err) => err.to_string(),
            Ok(value) => format!("unexpected budget: {value}"),
        };
        assert!(msg.contains("token_budget_ratio"));
    }

    #[test]
    fn dispatch_mode_defaults_to_native_preferred() {
        let runtime = RuntimeConfig {
            default_model: "openai:gpt-4o-mini".to_string(),
            max_steps: None,
            turn_timeout_ms: None,
            model_context_tokens: None,
            dispatch_mode: None,
        };
        let mode = resolve_dispatch_mode(&runtime);
        assert_eq!(mode, bob_runtime::DispatchMode::NativePreferred);
    }

    #[test]
    fn dispatch_mode_prompt_guided_is_resolved() {
        let runtime = RuntimeConfig {
            default_model: "openai:gpt-4o-mini".to_string(),
            max_steps: None,
            turn_timeout_ms: None,
            model_context_tokens: None,
            dispatch_mode: Some(RuntimeDispatchMode::PromptGuided),
        };
        let mode = resolve_dispatch_mode(&runtime);
        assert_eq!(mode, bob_runtime::DispatchMode::PromptGuided);
    }

    #[test]
    fn tool_policy_default_deny_blocks_without_allowlist() {
        let mut cfg = minimal_agent_config();
        cfg.policy =
            Some(PolicyConfig { deny_tools: None, allow_tools: None, default_deny: Some(true) });

        let policy_port = build_tool_policy_port(&cfg);
        assert!(!policy_port.is_tool_allowed("local/read_file", &[], None));
    }

    #[tokio::test]
    async fn approval_mode_deny_all_rejects_tool_calls() {
        let mut cfg = minimal_agent_config();
        cfg.approval = Some(ApprovalConfig { mode: Some(ApprovalMode::DenyAll), deny_tools: None });

        let approval_port = build_approval_port(&cfg);
        let decision = approval_port
            .approve_tool_call(
                &bob_runtime::core::types::ToolCall {
                    name: "local/read_file".to_string(),
                    arguments: Default::default(),
                },
                &bob_runtime::core::types::ApprovalContext {
                    session_id: "s1".to_string(),
                    turn_step: 1,
                    selected_skills: Vec::new(),
                },
            )
            .await;

        assert!(decision.is_ok());
        assert!(matches!(
            decision.ok(),
            Some(bob_runtime::core::types::ApprovalDecision::Denied { .. })
        ));
    }
}
