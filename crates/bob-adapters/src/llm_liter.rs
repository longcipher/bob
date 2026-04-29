//! # Liter LLM Adapter
//!
//! Liter LLM adapter — implements [`LlmPort`] using the `liter-llm` crate.
//!
//! ## Overview
//!
//! This adapter provides unified access to multiple LLM providers through the
//! [`liter-llm`](https://crates.io/crates/liter-llm) crate, supporting:
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
//! use bob_adapters::llm_liter::LiterLlmAdapter;
//! use bob_core::{
//!     ports::LlmPort,
//!     types::{LlmRequest, LlmResponse},
//! };
//! use liter_llm::{ClientConfig, DefaultClient, LlmClient};
//! use std::sync::Arc;
//!
//! let config = ClientConfig::new(std::env::var("OPENAI_API_KEY").unwrap_or_default());
//! let client = DefaultClient::new(config, None).unwrap();
//! let adapter = LiterLlmAdapter::new(Arc::new(client));
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
//! This module is only available when the `llm-liter` feature is enabled (default).

use std::sync::Arc;

use bob_core::{
    error::LlmError,
    ports::LlmPort,
    types::{
        FinishReason, LlmCapabilities, LlmRequest, LlmResponse, LlmStream, LlmStreamChunk, Message,
        Role, TokenUsage, ToolCall as BobToolCall, ToolDescriptor,
    },
};
use futures_util::{StreamExt, stream};
use liter_llm::{
    AssistantMessage, ChatCompletionRequest, ChatCompletionResponse, ChatCompletionTool,
    FinishReason as LiterFinishReason, FunctionCall, FunctionDefinition, LlmClient,
    Message as LitMessage, SystemMessage, ToolCall as LitToolCall, ToolMessage, ToolType, Usage,
    UserContent, UserMessage,
};

/// Adapter that delegates LLM inference to `liter_llm::DefaultClient`.
///
/// The client is wrapped in `Arc` so it can be shared across tasks and used
/// in the streaming implementation (which spawns a background task).
pub struct LiterLlmAdapter {
    client: Arc<liter_llm::DefaultClient>,
}

impl std::fmt::Debug for LiterLlmAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiterLlmAdapter").finish_non_exhaustive()
    }
}

impl LiterLlmAdapter {
    /// Create a new adapter wrapping an existing `liter_llm::DefaultClient`.
    #[must_use]
    pub fn new(client: Arc<liter_llm::DefaultClient>) -> Self {
        Self { client }
    }
}

// ── Mapping helpers ──────────────────────────────────────────────────

/// Convert an internal tool descriptor to a `liter_llm` tool definition.
fn to_liter_tool(tool: &ToolDescriptor) -> ChatCompletionTool {
    let parameters =
        if tool.input_schema.is_null() { None } else { Some(tool.input_schema.clone()) };
    let description =
        if tool.description.trim().is_empty() { None } else { Some(tool.description.clone()) };
    ChatCompletionTool {
        tool_type: ToolType::Function,
        function: FunctionDefinition {
            name: tool.id.clone(),
            description,
            parameters,
            strict: None,
        },
    }
}

fn synthetic_call_id(message_index: usize, tool_index: usize) -> String {
    format!("assistant-tool-call-{message_index}-{tool_index}")
}

/// Convert an internal tool call to a provider-native tool call.
fn to_liter_tool_call(call: &BobToolCall, message_index: usize, tool_index: usize) -> LitToolCall {
    LitToolCall {
        id: call.call_id.clone().unwrap_or_else(|| synthetic_call_id(message_index, tool_index)),
        call_type: ToolType::Function,
        function: FunctionCall {
            name: call.name.clone(),
            arguments: serde_json::to_string(&call.arguments).unwrap_or_default(),
        },
    }
}

/// Convert a provider-native tool call back to Bob's internal representation.
fn from_liter_tool_call(call: LitToolCall) -> BobToolCall {
    let arguments: serde_json::Value =
        serde_json::from_str(&call.function.arguments).unwrap_or(serde_json::Value::Null);
    BobToolCall::new(call.function.name, arguments).with_call_id(call.id)
}

/// Convert our internal `Message` to a `liter_llm::Message`.
fn to_lit_message(msg: &Message, message_index: usize) -> LitMessage {
    match msg.role {
        Role::System => {
            LitMessage::System(SystemMessage { content: msg.content.clone(), name: None })
        }
        Role::User => LitMessage::User(UserMessage {
            content: UserContent::Text(msg.content.clone()),
            name: None,
        }),
        Role::Assistant => {
            let tool_calls = if msg.tool_calls.is_empty() {
                None
            } else {
                Some(
                    msg.tool_calls
                        .iter()
                        .enumerate()
                        .map(|(tool_index, call)| {
                            to_liter_tool_call(call, message_index, tool_index)
                        })
                        .collect(),
                )
            };
            let content = if msg.content.is_empty() && tool_calls.is_some() {
                None
            } else {
                Some(msg.content.clone())
            };
            LitMessage::Assistant(AssistantMessage {
                content,
                name: None,
                tool_calls,
                refusal: None,
                function_call: None,
            })
        }
        Role::Tool => {
            let tool_call_id =
                msg.tool_call_id.clone().unwrap_or_else(|| synthetic_call_id(message_index, 0));
            LitMessage::Tool(ToolMessage { content: msg.content.clone(), tool_call_id, name: None })
        }
    }
}

/// Convert our `LlmRequest` to a `liter_llm::ChatCompletionRequest`.
fn to_chat_request(req: &LlmRequest) -> ChatCompletionRequest {
    let messages: Vec<LitMessage> = req
        .messages
        .iter()
        .enumerate()
        .map(|(message_index, msg)| to_lit_message(msg, message_index))
        .collect();
    let tools = if req.tools.is_empty() {
        None
    } else {
        Some(req.tools.iter().map(to_liter_tool).collect())
    };
    let mut chat_req = ChatCompletionRequest::default();
    chat_req.model.clone_from(&req.model);
    chat_req.messages = messages;
    chat_req.tools = tools;
    chat_req
}

/// Extract `TokenUsage` from the liter_llm `ChatCompletionResponse`.
fn extract_usage(resp: &ChatCompletionResponse) -> TokenUsage {
    resp.usage.as_ref().map_or_else(TokenUsage::default, extract_usage_from_usage)
}

/// Extract [`TokenUsage`] from liter_llm [`Usage`].
fn extract_usage_from_usage(usage: &Usage) -> TokenUsage {
    TokenUsage {
        prompt_tokens: usage.prompt_tokens.try_into().unwrap_or(0),
        completion_tokens: usage.completion_tokens.try_into().unwrap_or(0),
    }
}

/// Map a `liter_llm::LiterLlmError` to our `LlmError`.
fn map_liter_error(err: liter_llm::LiterLlmError) -> LlmError {
    let msg = err.to_string();
    if msg.contains("rate") || msg.contains("429") {
        LlmError::RateLimited
    } else if msg.contains("context length") || msg.contains("maximum") {
        LlmError::ContextLengthExceeded
    } else {
        LlmError::Provider(msg)
    }
}

/// Map a `liter_llm::FinishReason` to our `FinishReason`.
fn map_finish_reason(reason: &Option<LiterFinishReason>) -> FinishReason {
    match reason {
        Some(LiterFinishReason::ToolCalls) => FinishReason::ToolCall,
        _ => FinishReason::Stop,
    }
}

// ── LlmPort implementation ──────────────────────────────────────────

#[async_trait::async_trait]
impl LlmPort for LiterLlmAdapter {
    fn capabilities(&self) -> LlmCapabilities {
        LlmCapabilities { native_tool_calling: true, streaming: true }
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let chat_req = to_chat_request(&req);

        let chat_resp: ChatCompletionResponse =
            self.client.chat(chat_req).await.map_err(map_liter_error)?;

        let content =
            chat_resp.choices.first().and_then(|c| c.message.content.clone()).unwrap_or_default();
        let usage = extract_usage(&chat_resp);
        let tool_calls: Vec<BobToolCall> = chat_resp
            .choices
            .first()
            .and_then(|c| c.message.tool_calls.as_ref())
            .map(|calls| calls.iter().cloned().map(from_liter_tool_call).collect())
            .unwrap_or_default();
        let finish_reason = chat_resp
            .choices
            .first()
            .map_or(FinishReason::Stop, |c| map_finish_reason(&c.finish_reason));

        Ok(LlmResponse { content, usage, finish_reason, tool_calls })
    }

    async fn complete_stream(&self, req: LlmRequest) -> Result<LlmStream, LlmError> {
        let chat_req = to_chat_request(&req);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<LlmStreamChunk, LlmError>>();
        let client = Arc::clone(&self.client);

        tokio::spawn(async move {
            let chat_stream = match client.chat_stream(chat_req).await {
                Ok(s) => s,
                Err(err) => {
                    let _ = tx.send(Err(map_liter_error(err)));
                    return;
                }
            };

            let mut chat_stream = chat_stream;
            while let Some(event) = chat_stream.next().await {
                match event {
                    Ok(chunk) => {
                        let content = chunk.choices.first().and_then(|c| c.delta.content.clone());
                        let is_done =
                            chunk.choices.first().is_some_and(|c| c.finish_reason.is_some());
                        let usage = if is_done {
                            chunk
                                .usage
                                .as_ref()
                                .map_or_else(TokenUsage::default, extract_usage_from_usage)
                        } else {
                            TokenUsage::default()
                        };
                        if let Some(text) = content &&
                            !text.is_empty() &&
                            tx.send(Ok(LlmStreamChunk::TextDelta(text))).is_err()
                        {
                            return;
                        }
                        if is_done && tx.send(Ok(LlmStreamChunk::Done { usage })).is_err() {
                            return;
                        }
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let mapped_err = if msg.contains("rate") || msg.contains("429") {
                            LlmError::RateLimited
                        } else if msg.contains("context length") || msg.contains("maximum") {
                            LlmError::ContextLengthExceeded
                        } else {
                            LlmError::Provider(msg)
                        };
                        if tx.send(Err(mapped_err)).is_err() {
                            return;
                        }
                    }
                }
            }
        });

        let mapped =
            stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|item| (item, rx)) });

        Ok(Box::pin(mapped))
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> Arc<liter_llm::DefaultClient> {
        let config = liter_llm::ClientConfig::new("test-key");
        Arc::new(liter_llm::DefaultClient::new(config, None).expect("client creation"))
    }

    #[test]
    fn adapter_is_object_safe() {
        let adapter = LiterLlmAdapter::new(test_client());
        // Verify it can be stored as `Arc<dyn LlmPort>`.
        let _port: Arc<dyn bob_core::ports::LlmPort> = Arc::new(adapter);
    }

    #[test]
    fn message_mapping_covers_all_roles() {
        let system = to_lit_message(&Message::text(Role::System, "sys"), 0);
        assert!(matches!(system, LitMessage::System(_)));

        let user = to_lit_message(&Message::text(Role::User, "usr"), 1);
        assert!(matches!(user, LitMessage::User(_)));

        let asst = to_lit_message(&Message::text(Role::Assistant, "ast"), 2);
        assert!(matches!(asst, LitMessage::Assistant(_)));

        // Tool messages are mapped to tool role.
        let tool =
            to_lit_message(&Message::tool_result("search", Some("call-1".into()), "result"), 3);
        assert!(matches!(tool, LitMessage::Tool(_)));
    }

    #[test]
    fn tool_response_message_uses_provider_tool_role_when_call_id_exists() {
        let tool = to_lit_message(&Message::tool_result("search", Some("call-1".into()), "{}"), 0);
        assert!(matches!(tool, LitMessage::Tool(_)));
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
        let adapter = LiterLlmAdapter::new(test_client());

        let capabilities = adapter.capabilities();
        assert!(
            capabilities.native_tool_calling,
            "liter_llm adapter should expose native tool calling"
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
        let tools = chat_req.tools.expect("tools should be forwarded to liter_llm");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.name.as_str(), "search");
        assert_eq!(tools[0].function.description.as_deref(), Some("Search indexed docs"));
    }

    #[test]
    fn maps_liter_tool_call_preserving_call_id() {
        let call = LitToolCall {
            id: "call-1".into(),
            call_type: ToolType::Function,
            function: FunctionCall {
                name: "search".into(),
                arguments: serde_json::json!({"q": "rust"}).to_string(),
            },
        };

        let mapped = from_liter_tool_call(call);
        assert_eq!(mapped.call_id.as_deref(), Some("call-1"));
        assert_eq!(mapped.name, "search");
        assert_eq!(mapped.arguments, serde_json::json!({"q": "rust"}));
    }
}
