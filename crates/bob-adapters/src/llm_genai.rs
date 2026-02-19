//! GenAi LLM adapter — implements [`LlmPort`] using the `genai` crate.

use bob_core::{
    error::LlmError,
    ports::LlmPort,
    types::{FinishReason, LlmRequest, LlmResponse, LlmStream, Message, Role, TokenUsage},
};
use genai::chat::{ChatMessage, ChatRequest, ChatResponse};

/// Adapter that delegates LLM inference to `genai::Client`.
pub struct GenAiLlmAdapter {
    client: genai::Client,
}

impl std::fmt::Debug for GenAiLlmAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenAiLlmAdapter").finish_non_exhaustive()
    }
}

impl GenAiLlmAdapter {
    /// Create a new adapter wrapping an existing `genai::Client`.
    #[must_use]
    pub fn new(client: genai::Client) -> Self {
        Self { client }
    }
}

// ── Mapping helpers ──────────────────────────────────────────────────

/// Convert our internal `Message` to a `genai::ChatMessage`.
fn to_chat_message(msg: &Message) -> ChatMessage {
    match msg.role {
        Role::System => ChatMessage::system(&msg.content),
        Role::User => ChatMessage::user(&msg.content),
        Role::Assistant => ChatMessage::assistant(&msg.content),
        // Tool results are sent as user messages with a tool-result preamble.
        Role::Tool => ChatMessage::user(format!("[Tool result] {}", msg.content)),
    }
}

/// Convert our `LlmRequest` to a `genai::ChatRequest`.
fn to_chat_request(req: &LlmRequest) -> ChatRequest {
    let messages: Vec<ChatMessage> = req.messages.iter().map(to_chat_message).collect();
    ChatRequest::new(messages)
}

/// Extract `TokenUsage` from the genai `ChatResponse`.
fn extract_usage(resp: &ChatResponse) -> TokenUsage {
    TokenUsage {
        prompt_tokens: resp.usage.prompt_tokens.unwrap_or(0).try_into().unwrap_or(0),
        completion_tokens: resp.usage.completion_tokens.unwrap_or(0).try_into().unwrap_or(0),
    }
}

/// Map a `genai::Error` to our `LlmError`.
fn map_genai_error(err: genai::Error) -> LlmError {
    let msg = err.to_string();
    if msg.contains("rate") || msg.contains("429") {
        LlmError::RateLimited
    } else if msg.contains("context length") || msg.contains("maximum") {
        LlmError::ContextLengthExceeded
    } else {
        LlmError::Provider(msg)
    }
}

// ── LlmPort implementation ──────────────────────────────────────────

#[async_trait::async_trait]
impl LlmPort for GenAiLlmAdapter {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let model = &req.model;
        let chat_req = to_chat_request(&req);

        let chat_resp: ChatResponse =
            self.client.exec_chat(model, chat_req, None).await.map_err(map_genai_error)?;

        let content = chat_resp.first_text().unwrap_or("").to_string();

        let usage = extract_usage(&chat_resp);

        Ok(LlmResponse { content, usage, finish_reason: FinishReason::Stop })
    }

    async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
        // Streaming scaffolded but not fully implemented in v1.
        Err(LlmError::Provider("streaming not yet implemented for GenAi adapter".into()))
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn adapter_is_object_safe() {
        let client = genai::Client::default();
        let adapter = GenAiLlmAdapter::new(client);
        // Verify it can be stored as `Arc<dyn LlmPort>`.
        let _port: Arc<dyn LlmPort> = Arc::new(adapter);
    }

    #[test]
    fn message_mapping_covers_all_roles() {
        let system = to_chat_message(&Message { role: Role::System, content: "sys".into() });
        assert_eq!(system.role, genai::chat::ChatRole::System);

        let user = to_chat_message(&Message { role: Role::User, content: "usr".into() });
        assert_eq!(user.role, genai::chat::ChatRole::User);

        let asst = to_chat_message(&Message { role: Role::Assistant, content: "ast".into() });
        assert_eq!(asst.role, genai::chat::ChatRole::Assistant);

        // Tool messages are mapped to user role.
        let tool = to_chat_message(&Message { role: Role::Tool, content: "result".into() });
        assert_eq!(tool.role, genai::chat::ChatRole::User);
    }

    #[test]
    fn chat_request_includes_all_messages() {
        let req = LlmRequest {
            model: "test-model".into(),
            messages: vec![
                Message { role: Role::System, content: "system msg".into() },
                Message { role: Role::User, content: "user msg".into() },
            ],
            tools: vec![],
        };

        let chat_req = to_chat_request(&req);
        assert_eq!(chat_req.messages.len(), 2);
    }
}
