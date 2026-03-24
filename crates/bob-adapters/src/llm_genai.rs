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
        FinishReason, LlmCapabilities, LlmRequest, LlmResponse, LlmStream, LlmStreamChunk, Message,
        Role, TokenUsage, ToolCall as BobToolCall, ToolDescriptor,
    },
};
use futures_util::StreamExt;
use genai::chat::{
    ChatMessage, ChatOptions, ChatRequest, ChatResponse, ChatStreamEvent, ContentPart,
    MessageContent, Tool, ToolCall as GenAiToolCall, ToolResponse, Usage,
};

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

/// Convert an internal tool descriptor to a `genai` tool definition.
fn to_genai_tool(tool: &ToolDescriptor) -> Tool {
    let mut mapped = Tool::new(tool.id.clone());
    if !tool.description.trim().is_empty() {
        mapped = mapped.with_description(tool.description.clone());
    }
    if !tool.input_schema.is_null() {
        mapped = mapped.with_schema(tool.input_schema.clone());
    }
    mapped
}

fn synthetic_call_id(message_index: usize, tool_index: usize) -> String {
    format!("assistant-tool-call-{message_index}-{tool_index}")
}

/// Convert an internal tool call to a provider-native tool call.
fn to_genai_tool_call(
    call: &BobToolCall,
    message_index: usize,
    tool_index: usize,
) -> GenAiToolCall {
    GenAiToolCall {
        call_id: call
            .call_id
            .clone()
            .unwrap_or_else(|| synthetic_call_id(message_index, tool_index)),
        fn_name: call.name.clone(),
        fn_arguments: call.arguments.clone(),
        thought_signatures: None,
    }
}

/// Convert a provider-native tool call back to Bob's internal representation.
fn from_genai_tool_call(call: GenAiToolCall) -> BobToolCall {
    BobToolCall::new(call.fn_name, call.fn_arguments).with_call_id(call.call_id)
}

/// Convert our internal `Message` to a `genai::ChatMessage`.
fn to_chat_message(msg: &Message, message_index: usize) -> ChatMessage {
    match msg.role {
        Role::System => ChatMessage::system(&msg.content),
        Role::User => ChatMessage::user(&msg.content),
        Role::Assistant => {
            if msg.tool_calls.is_empty() {
                ChatMessage::assistant(&msg.content)
            } else {
                let mut parts =
                    Vec::with_capacity(usize::from(!msg.content.is_empty()) + msg.tool_calls.len());
                if !msg.content.is_empty() {
                    parts.push(ContentPart::Text(msg.content.clone()));
                }
                parts.extend(msg.tool_calls.iter().enumerate().map(|(tool_index, call)| {
                    ContentPart::ToolCall(to_genai_tool_call(call, message_index, tool_index))
                }));
                ChatMessage::assistant(MessageContent::from_parts(parts))
            }
        }
        Role::Tool => {
            if let Some(call_id) = msg.tool_call_id.as_ref() {
                ChatMessage::from(ToolResponse::new(call_id.clone(), msg.content.clone()))
            } else {
                ChatMessage::user(format!("[Tool result] {}", msg.content))
            }
        }
    }
}

/// Convert our `LlmRequest` to a `genai::ChatRequest`.
fn to_chat_request(req: &LlmRequest) -> ChatRequest {
    let messages: Vec<ChatMessage> = req
        .messages
        .iter()
        .enumerate()
        .map(|(message_index, msg)| to_chat_message(msg, message_index))
        .collect();
    let mut chat_req = ChatRequest::new(messages);
    if !req.tools.is_empty() {
        chat_req = chat_req.with_tools(req.tools.iter().map(to_genai_tool));
    }
    chat_req
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
    fn capabilities(&self) -> LlmCapabilities {
        LlmCapabilities { native_tool_calling: true, streaming: true }
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let model = &req.model;
        let chat_req = to_chat_request(&req);

        let chat_resp: ChatResponse =
            self.client.exec_chat(model, chat_req, None).await.map_err(map_genai_error)?;

        let content = chat_resp.first_text().unwrap_or("").to_string();
        let usage = extract_usage(&chat_resp);
        let tool_calls: Vec<BobToolCall> =
            chat_resp.into_tool_calls().into_iter().map(from_genai_tool_call).collect();
        let finish_reason =
            if tool_calls.is_empty() { FinishReason::Stop } else { FinishReason::ToolCall };

        Ok(LlmResponse { content, usage, finish_reason, tool_calls })
    }

    async fn complete_stream(&self, req: LlmRequest) -> Result<LlmStream, LlmError> {
        let model = &req.model;
        let chat_req = to_chat_request(&req);
        let options = ChatOptions::default()
            .with_capture_usage(true)
            .with_capture_content(true)
            .with_capture_tool_calls(true);

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
                    ChatStreamEvent::Start |
                    ChatStreamEvent::ReasoningChunk(_) |
                    ChatStreamEvent::ThoughtSignatureChunk(_) |
                    ChatStreamEvent::ToolCallChunk(_),
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
        let system = to_chat_message(&Message::text(Role::System, "sys"), 0);
        assert_eq!(system.role, genai::chat::ChatRole::System);

        let user = to_chat_message(&Message::text(Role::User, "usr"), 1);
        assert_eq!(user.role, genai::chat::ChatRole::User);

        let asst = to_chat_message(&Message::text(Role::Assistant, "ast"), 2);
        assert_eq!(asst.role, genai::chat::ChatRole::Assistant);

        // Tool messages are mapped to user role.
        let tool = to_chat_message(&Message::text(Role::Tool, "result"), 3);
        assert_eq!(tool.role, genai::chat::ChatRole::User);
    }

    #[test]
    fn tool_response_message_uses_provider_tool_role_when_call_id_exists() {
        let tool = to_chat_message(&Message::tool_result("search", Some("call-1".into()), "{}"), 0);
        assert_eq!(tool.role, genai::chat::ChatRole::Tool);
    }

    #[test]
    fn chat_request_includes_all_messages() {
        let req = LlmRequest {
            model: "test-model".into(),
            messages: vec![
                Message::text(Role::System, "system msg"),
                Message::text(Role::User, "user msg"),
            ],
            tools: vec![],
            output_schema: None,
        };

        let chat_req = to_chat_request(&req);
        assert_eq!(chat_req.messages.len(), 2);
    }

    #[test]
    fn adapter_declares_native_tool_calling_capability() {
        let client = genai::Client::default();
        let adapter = GenAiLlmAdapter::new(client);

        let capabilities = adapter.capabilities();
        assert!(
            capabilities.native_tool_calling,
            "genai adapter should expose native tool calling"
        );
        assert!(capabilities.streaming, "streaming should remain enabled");
    }

    #[test]
    fn chat_request_includes_declared_tools() {
        let req = LlmRequest {
            model: "test-model".into(),
            messages: vec![Message::text(Role::User, "find rust docs")],
            tools: vec![
                bob_core::types::ToolDescriptor::new("search", "Search indexed docs")
                    .with_input_schema(serde_json::json!({
                        "type": "object",
                        "properties": {"q": {"type": "string"}}
                    })),
            ],
            output_schema: None,
        };

        let chat_req = to_chat_request(&req);
        let tools = chat_req.tools.expect("tools should be forwarded to genai");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name.as_str(), "search");
        assert_eq!(tools[0].description.as_deref(), Some("Search indexed docs"));
    }

    #[test]
    fn maps_genai_tool_call_preserving_call_id() {
        let call = genai::chat::ToolCall {
            call_id: "call-1".into(),
            fn_name: "search".into(),
            fn_arguments: serde_json::json!({"q": "rust"}),
            thought_signatures: None,
        };

        let mapped = from_genai_tool_call(call);
        assert_eq!(mapped.call_id.as_deref(), Some("call-1"));
        assert_eq!(mapped.name, "search");
        assert_eq!(mapped.arguments, serde_json::json!({"q": "rust"}));
    }
}
