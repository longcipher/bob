//! # Message Bus
//!
//! Tokio-based [`MessageBusPort`] implementation using `tokio::sync::mpsc` channels.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────┐  InboundMessage   ┌──────────────┐
//! │ Bot /    │ ────────────────→ │              │
//! │ Adapter  │                   │ TokioMessage │
//! │          │ ←──────────────── │     Bus      │
//! └──────────┘  OutboundMessage  └──────────────┘
//!                                  ↑         ↓
//!                           recv()         send()
//!                        (agent side)   (bot side)
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use bob_runtime::message_bus::MessageBusHandle;
//!
//! let handle = MessageBusHandle::new(64);
//! let inbound = handle.inbound_port();
//! let outbound = handle.outbound_port();
//!
//! // Bot side pushes inbound messages
//! inbound.push(msg).await?;
//!
//! // Agent side consumes inbound messages
//! let msg = outbound.recv().await?;
//! ```

use bob_core::{
    error::AgentError,
    ports::MessageBusPort,
    types::{InboundMessage, OutboundMessage},
};
use tokio::sync::{Mutex, mpsc};

/// Concrete [`MessageBusPort`] backed by `tokio::sync::mpsc` channels.
///
/// Each `TokioMessageBus` instance holds one direction of communication.
/// Use [`MessageBusHandle`] to create a matched pair.
#[derive(Debug)]
pub struct TokioMessageBus {
    /// Sender for outbound messages (agent → bot).
    outbound_tx: mpsc::Sender<OutboundMessage>,
    /// Receiver for inbound messages (bot → agent), wrapped in `Mutex` for
    /// interior mutability since `MessageBusPort::recv` takes `&self`.
    inbound_rx: Mutex<mpsc::Receiver<InboundMessage>>,
}

#[async_trait::async_trait]
impl MessageBusPort for TokioMessageBus {
    async fn send(&self, msg: OutboundMessage) -> Result<(), AgentError> {
        self.outbound_tx
            .send(msg)
            .await
            .map_err(|_| AgentError::Config("message bus outbound channel closed".into()))
    }

    async fn recv(&self) -> Result<InboundMessage, AgentError> {
        self.inbound_rx
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| AgentError::Config("message bus inbound channel closed".into()))
    }
}

/// The bot-side port for pushing inbound messages and consuming outbound messages.
#[derive(Debug)]
pub struct BotSideBus {
    /// Sender for inbound messages (bot → agent).
    inbound_tx: mpsc::Sender<InboundMessage>,
    /// Receiver for outbound messages (agent → bot).
    outbound_rx: mpsc::Receiver<OutboundMessage>,
}

impl BotSideBus {
    /// Push an inbound message onto the bus for the agent to consume.
    pub async fn push(&self, msg: InboundMessage) -> Result<(), AgentError> {
        self.inbound_tx
            .send(msg)
            .await
            .map_err(|_| AgentError::Config("message bus inbound channel closed".into()))
    }

    /// Receive the next outbound message from the agent.
    pub async fn recv_outbound(&mut self) -> Option<OutboundMessage> {
        self.outbound_rx.recv().await
    }
}

/// Builder for configuring a [`MessageBusHandle`].
#[derive(Debug)]
#[must_use]
pub struct MessageBusBuilder {
    buffer_size: usize,
}

impl MessageBusBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self { buffer_size: 64 }
    }

    /// Set the channel buffer size (number of messages that can be buffered).
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Build the message bus handle.
    pub fn build(self) -> MessageBusHandle {
        let (inbound_tx, inbound_rx) = mpsc::channel(self.buffer_size);
        let (outbound_tx, outbound_rx) = mpsc::channel(self.buffer_size);

        let agent_port = TokioMessageBus { outbound_tx, inbound_rx: Mutex::new(inbound_rx) };
        let bot_port = BotSideBus { inbound_tx, outbound_rx };

        MessageBusHandle { agent_port, bot_port }
    }
}

impl Default for MessageBusBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle that owns both sides of a message bus channel pair.
///
/// Destructure into [`TokioMessageBus`] (agent side) and [`BotSideBus`]
/// (bot side) for independent use.
#[derive(Debug)]
pub struct MessageBusHandle {
    /// Agent-side port implementing [`MessageBusPort`].
    agent_port: TokioMessageBus,
    /// Bot-side port for pushing inbound and receiving outbound messages.
    bot_port: BotSideBus,
}

impl MessageBusHandle {
    /// Create a new message bus with the given buffer size.
    pub fn new(buffer_size: usize) -> Self {
        MessageBusBuilder::new().with_buffer_size(buffer_size).build()
    }

    /// Consume the handle and return both ports.
    pub fn split(self) -> (TokioMessageBus, BotSideBus) {
        (self.agent_port, self.bot_port)
    }

    /// Get a reference to the agent-side port.
    #[must_use]
    pub fn agent_port(&self) -> &TokioMessageBus {
        &self.agent_port
    }

    /// Get a mutable reference to the bot-side port.
    pub fn bot_port(&mut self) -> &mut BotSideBus {
        &mut self.bot_port
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bob_core::ports::MessageBusPort;

    use super::*;

    fn sample_inbound(content: &str) -> InboundMessage {
        InboundMessage {
            channel: "slack".into(),
            sender_id: "user-1".into(),
            chat_id: "chat-1".into(),
            content: content.into(),
            timestamp_ms: 1_700_000_000_000,
        }
    }

    fn sample_outbound(content: &str) -> OutboundMessage {
        OutboundMessage {
            channel: "slack".into(),
            chat_id: "chat-1".into(),
            content: content.into(),
            is_progress: false,
            reply_to: None,
        }
    }

    #[tokio::test]
    async fn send_and_recv_single_message() {
        let handle = MessageBusHandle::new(8);
        let (agent, bot) = handle.split();

        bot.push(sample_inbound("hello")).await.expect("push should succeed");

        let received = agent.recv().await.expect("recv should succeed");
        assert_eq!(received.content, "hello");
        assert_eq!(received.channel, "slack");
        assert_eq!(received.sender_id, "user-1");
    }

    #[tokio::test]
    async fn multiple_messages_received_in_order() {
        let handle = MessageBusHandle::new(16);
        let (agent, bot) = handle.split();

        for i in 0..5 {
            bot.push(sample_inbound(&format!("msg-{i}"))).await.expect("push should succeed");
        }

        for i in 0..5 {
            let received = agent.recv().await.expect("recv should succeed");
            assert_eq!(received.content, format!("msg-{i}"));
        }
    }

    #[tokio::test]
    async fn outbound_send_through_agent_port() {
        let handle = MessageBusHandle::new(8);
        let (agent, mut bot) = handle.split();

        agent.send(sample_outbound("response")).await.expect("send should succeed");

        let received = bot.recv_outbound().await.expect("should receive outbound");
        assert_eq!(received.content, "response");
        assert!(!received.is_progress);
    }

    #[tokio::test]
    async fn outbound_progress_messages() {
        let handle = MessageBusHandle::new(8);
        let (agent, mut bot) = handle.split();

        agent
            .send(OutboundMessage {
                channel: "discord".into(),
                chat_id: "ch-1".into(),
                content: "thinking...".into(),
                is_progress: true,
                reply_to: None,
            })
            .await
            .expect("send should succeed");

        let received = bot.recv_outbound().await.expect("should receive outbound");
        assert!(received.is_progress);
        assert_eq!(received.content, "thinking...");
    }

    #[tokio::test]
    async fn recv_returns_error_when_sender_dropped() {
        let handle = MessageBusHandle::new(8);
        let (agent, bot) = handle.split();

        // Drop the bot side (which holds the inbound sender).
        drop(bot);

        let result = agent.recv().await;
        assert!(result.is_err(), "recv should fail when sender is dropped");
    }

    #[tokio::test]
    async fn send_returns_error_when_receiver_dropped() {
        let handle = MessageBusHandle::new(8);
        let (agent, bot) = handle.split();

        // Drop the bot side's outbound receiver by dropping the whole bot.
        // We need to extract and drop just the receiver — but since BotSideBus
        // owns both, we drop the whole thing.
        drop(bot);

        let result = agent.send(sample_outbound("x")).await;
        assert!(result.is_err(), "send should fail when receiver is dropped");
    }

    #[tokio::test]
    async fn builder_pattern_default_buffer() {
        let handle = MessageBusBuilder::new().build();
        let (agent, bot) = handle.split();

        bot.push(sample_inbound("test")).await.expect("push should succeed");
        let received = agent.recv().await.expect("recv should succeed");
        assert_eq!(received.content, "test");
    }

    #[tokio::test]
    async fn builder_pattern_custom_buffer() {
        let handle = MessageBusBuilder::new().with_buffer_size(2).build();
        let (agent, bot) = handle.split();

        // Fill the small buffer.
        bot.push(sample_inbound("a")).await.expect("push should succeed");
        bot.push(sample_inbound("b")).await.expect("push should succeed");

        // Drain in order.
        assert_eq!(agent.recv().await.unwrap_or_else(|_| unreachable!()).content, "a");
        assert_eq!(agent.recv().await.unwrap_or_else(|_| unreachable!()).content, "b");
    }

    #[tokio::test]
    async fn agent_port_as_dyn_message_bus() {
        let handle = MessageBusHandle::new(8);
        let (agent, bot) = handle.split();

        let bus: Arc<dyn MessageBusPort> = Arc::new(agent);

        bot.push(sample_inbound("via-dyn")).await.expect("push should succeed");

        let received = bus.recv().await.expect("dyn recv should succeed");
        assert_eq!(received.content, "via-dyn");
    }

    #[tokio::test]
    async fn bidirectional_communication() {
        let handle = MessageBusHandle::new(8);
        let (agent, mut bot) = handle.split();

        // Bot sends inbound.
        bot.push(sample_inbound("question")).await.expect("push should succeed");

        // Agent receives inbound and sends outbound.
        let inbound = agent.recv().await.expect("recv should succeed");
        assert_eq!(inbound.content, "question");

        agent
            .send(OutboundMessage {
                channel: inbound.channel,
                chat_id: inbound.chat_id,
                content: format!("answer to: {}", inbound.content),
                is_progress: false,
                reply_to: None,
            })
            .await
            .expect("send should succeed");

        // Bot receives outbound.
        let outbound = bot.recv_outbound().await.expect("should receive outbound");
        assert_eq!(outbound.content, "answer to: question");
    }
}
