//! # GenAi LLM Adapter
//!
//! GenAi LLM adapter — implements [`LlmPort`] using the `genai` crate.
//!
//! ## Overview
//!
//! This adapter provides unified access to multiple LLM providers through the
//! [`genai`](https://crates.io/crates/genai) crate, supporting:
//!
//! - OpenAI (GPT-4, GPT-4o-mini, etc.)
//! - Anthropic (Claude)
//! - Google (Gemini)
//! - Groq
//! - Cohere
//! - And more...
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_adapters::llm_genai::GenAiLlmAdapter;
//! use bob_core::{
//!     ports::LlmPort,
//!     types::{LlmRequest, LlmResponse},
//! };
//! use genai::Client;
//!
//! let client = Client::default();
//! let adapter = GenAiLlmAdapter::new(client);
//!
//! let request = LlmRequest {
//!     model: "openai:gpt-4o-mini".to_string(),
//!     messages: vec![],
//!     tools: vec![],
//! };
//!
//! let response = adapter.complete(request).await?;
//! ```
//!
//! ## Feature Flag
//!
//! This module is only available when the `llm-genai` feature is enabled (default).

use bob_core::{
    error::LlmError,
    ports::LlmPort,
    types::{
        FinishReason, LlmRequest, LlmResponse, LlmStream, LlmStreamChunk, Message, Role, TokenUsage,
    },
};
use futures_util::StreamExt;
use genai::chat::{ChatMessage, ChatOptions, ChatRequest, ChatResponse, ChatStreamEvent, Usage};

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
    extract_usage_from_usage(&resp.usage)
}

/// Extract [`TokenUsage`] from genai [`Usage`].
fn extract_usage_from_usage(usage: &Usage) -> TokenUsage {
    TokenUsage {
        prompt_tokens: usage.prompt_tokens.unwrap_or(0).try_into().unwrap_or(0),
        completion_tokens: usage.completion_tokens.unwrap_or(0).try_into().unwrap_or(0),
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

        Ok(LlmResponse {
            content,
            usage,
            finish_reason: FinishReason::Stop,
            tool_calls: Vec::new(),
        })
    }

    async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
        let model = &_req.model;
        let chat_req = to_chat_request(&_req);
        let options = ChatOptions::default().with_capture_usage(true).with_capture_content(true);

        let chat_stream = self
            .client
            .exec_chat_stream(model, chat_req, Some(&options))
            .await
            .map_err(map_genai_error)?;

        let mapped = chat_stream.stream.filter_map(|event| async move {
            match event {
                Ok(ChatStreamEvent::Chunk(chunk)) => {
                    if chunk.content.is_empty() {
                        None
                    } else {
                        Some(Ok(LlmStreamChunk::TextDelta(chunk.content)))
                    }
                }
                Ok(ChatStreamEvent::End(end)) => {
                    let usage = end
                        .captured_usage
                        .as_ref()
                        .map_or_else(TokenUsage::default, extract_usage_from_usage);
                    Some(Ok(LlmStreamChunk::Done { usage }))
                }
                Ok(
                    ChatStreamEvent::Start
                    | ChatStreamEvent::ReasoningChunk(_)
                    | ChatStreamEvent::ThoughtSignatureChunk(_)
                    | ChatStreamEvent::ToolCallChunk(_),
                ) => None,
                Err(err) => Some(Err(map_genai_error(err))),
            }
        });

        Ok(Box::pin(mapped))
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
