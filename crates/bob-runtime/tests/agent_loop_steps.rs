use std::sync::Arc;

use bob_core::types::{FinishReason, LlmResponse, TokenUsage, TurnPolicy};
use bob_runtime::{AgentBootstrap, RuntimeBuilder};
use cucumber::{World as _, given, then, when};

#[derive(Debug, Clone, Default)]
struct MockLlm;

#[async_trait::async_trait]
impl bob_core::ports::LlmPort for MockLlm {
    async fn complete(
        &self,
        _req: bob_core::types::LlmRequest,
    ) -> Result<LlmResponse, bob_core::error::LlmError> {
        Ok(LlmResponse {
            content: "Hello from mock!".to_string(),
            tool_calls: vec![],
            usage: TokenUsage::default(),
            finish_reason: FinishReason::Stop,
        })
    }

    async fn complete_stream(
        &self,
        _req: bob_core::types::LlmRequest,
    ) -> Result<bob_core::types::LlmStream, bob_core::error::LlmError> {
        let response = self.complete(_req).await?;
        let stream = futures_util::stream::once(async move {
            Ok(bob_core::types::LlmStreamChunk::TextDelta(response.content.clone()))
        });
        Ok(Box::pin(stream))
    }
}

#[derive(Debug, Clone, Default)]
struct MockToolPort;

#[async_trait::async_trait]
impl bob_core::ports::ToolPort for MockToolPort {
    async fn list_tools(
        &self,
    ) -> Result<Vec<bob_core::types::ToolDescriptor>, bob_core::error::ToolError> {
        Ok(vec![])
    }

    async fn call_tool(
        &self,
        _call: bob_core::types::ToolCall,
    ) -> Result<bob_core::types::ToolResult, bob_core::error::ToolError> {
        Ok(bob_core::types::ToolResult {
            name: "mock_tool".to_string(),
            output: serde_json::json!({"result": "mock output"}),
            is_error: false,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct MockStore;

#[async_trait::async_trait]
impl bob_core::ports::SessionStore for MockStore {
    async fn load(
        &self,
        _id: &String,
    ) -> Result<Option<bob_core::types::SessionState>, bob_core::error::StoreError> {
        Ok(None)
    }

    async fn save(
        &self,
        _id: &String,
        _state: &bob_core::types::SessionState,
    ) -> Result<(), bob_core::error::StoreError> {
        Ok(())
    }

    async fn save_if_version(
        &self,
        _id: &String,
        state: &bob_core::types::SessionState,
        _expected_version: u64,
    ) -> Result<u64, bob_core::error::StoreError> {
        let _ = state;
        Ok(1)
    }
}

#[derive(Debug, Clone, Default)]
struct MockEventSink;

#[async_trait::async_trait]
impl bob_core::ports::EventSink for MockEventSink {
    fn emit(&self, _event: bob_core::types::AgentEvent) {}
}

#[derive(Debug, cucumber::World, Default)]
struct AgentWorld {
    policy: TurnPolicy,
}

#[given("a configured agent runtime")]
async fn setup_agent(world: &mut AgentWorld) {
    let llm = Arc::new(MockLlm);
    let tools = Arc::new(MockToolPort);
    let store = Arc::new(MockStore);
    let events = Arc::new(MockEventSink);

    let _runtime = RuntimeBuilder::new()
        .with_llm(llm)
        .with_tools(tools)
        .with_store(store)
        .with_events(events)
        .with_default_model("test-model".to_string())
        .with_policy(world.policy.clone())
        .build()
        .expect("failed to build runtime");
}

#[when("the agent runs a turn")]
async fn run_turn(_world: &mut AgentWorld) {
    // Placeholder - actual implementation would run the agent
}

#[then("the turn should complete successfully")]
async fn check_turn_complete(_world: &mut AgentWorld) {
    // Placeholder - actual implementation would verify completion
}

#[tokio::main]
async fn main() {
    AgentWorld::run("features").await;
}
