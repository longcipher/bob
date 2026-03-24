//! # Character / Persona Model
//!
//! Structured agent persona definitions that improve prompt engineering
//! by separating identity, behavior guidelines, and knowledge from
//! runtime plumbing.
//!
//! A [`Character`] bundles biography, style guidelines, topic expertise,
//! and system-prompt template into a single serializable config artifact
//! that can be loaded from TOML/JSON files or constructed in code.

use serde::{Deserialize, Serialize};

// ── Character ────────────────────────────────────────────────────────

/// Structured agent persona.
///
/// Characters are loaded from configuration files (TOML/JSON) and
/// injected into [`RequestContext`](crate::types::RequestContext)
/// via [`to_system_prompt`](Self::to_system_prompt).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Character {
    /// Display name (e.g. `"Bob"`, `"CodeReviewBot"`).
    pub name: String,

    /// Optional username / handle.
    #[serde(default)]
    pub username: Option<String>,

    /// Short biography lines. Rendered as a bulleted list in the prompt.
    #[serde(default)]
    pub bio: Vec<String>,

    /// Background lore or extended description.
    #[serde(default)]
    pub lore: Vec<String>,

    /// Areas of expertise. Used for routing and capability matching.
    #[serde(default)]
    pub topics: Vec<String>,

    /// Style guidelines applied to the assistant's responses.
    #[serde(default)]
    pub style: CharacterStyle,

    /// Inline knowledge snippets injected into the system prompt.
    #[serde(default)]
    pub knowledge: Vec<String>,

    /// Few-shot example conversations (user → assistant pairs).
    #[serde(default)]
    pub message_examples: Vec<MessageExample>,

    /// Descriptive adjectives for the character.
    #[serde(default)]
    pub adjectives: Vec<String>,
}

// ── Style ────────────────────────────────────────────────────────────

/// Style guidelines for response generation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterStyle {
    /// General writing style rules (e.g. "Be concise", "Use technical language").
    #[serde(default)]
    pub general: Vec<String>,

    /// Rules specific to chat responses.
    #[serde(default)]
    pub chat: Vec<String>,

    /// Rules specific to long-form posts.
    #[serde(default)]
    pub post: Vec<String>,
}

// ── Message Example ──────────────────────────────────────────────────

/// A single few-shot example pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageExample {
    /// User message.
    pub user: String,
    /// Expected assistant response.
    pub assistant: String,
}

// ── Prompt Rendering ─────────────────────────────────────────────────

impl Character {
    /// Render the character into a system prompt string.
    ///
    /// The output follows a structured format suitable for injection
    /// as the system message in an LLM conversation.
    #[must_use]
    pub fn to_system_prompt(&self) -> String {
        let mut parts = Vec::new();

        // Identity
        if !self.name.is_empty() {
            parts.push(format!("You are {}.", self.name));
        }

        // Bio
        if !self.bio.is_empty() {
            parts.push(String::new());
            parts.push("About you:".to_string());
            for line in &self.bio {
                parts.push(format!("- {line}"));
            }
        }

        // Lore
        if !self.lore.is_empty() {
            parts.push(String::new());
            parts.push("Background:".to_string());
            for line in &self.lore {
                parts.push(format!("- {line}"));
            }
        }

        // Topics
        if !self.topics.is_empty() {
            parts.push(String::new());
            parts.push(format!("Your areas of expertise: {}", self.topics.join(", ")));
        }

        // Style
        let has_style = !self.style.general.is_empty() ||
            !self.style.chat.is_empty() ||
            !self.style.post.is_empty();
        if has_style {
            parts.push(String::new());
            parts.push("Style guidelines:".to_string());
            for rule in &self.style.general {
                parts.push(format!("- {rule}"));
            }
            for rule in &self.style.chat {
                parts.push(format!("- [chat] {rule}"));
            }
            for rule in &self.style.post {
                parts.push(format!("- [post] {rule}"));
            }
        }

        // Adjectives
        if !self.adjectives.is_empty() {
            parts.push(String::new());
            parts.push(format!("You are: {}", self.adjectives.join(", ")));
        }

        // Knowledge
        if !self.knowledge.is_empty() {
            parts.push(String::new());
            parts.push("Reference knowledge:".to_string());
            for snippet in &self.knowledge {
                parts.push(format!("- {snippet}"));
            }
        }

        parts.join("\n")
    }

    /// Returns `true` if the character has no meaningful content.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_empty() &&
            self.bio.is_empty() &&
            self.lore.is_empty() &&
            self.topics.is_empty() &&
            self.style.general.is_empty() &&
            self.style.chat.is_empty() &&
            self.style.post.is_empty() &&
            self.knowledge.is_empty() &&
            self.adjectives.is_empty()
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_character() -> Character {
        Character {
            name: "CodeBot".to_string(),
            username: Some("codebot".to_string()),
            bio: vec![
                "An AI assistant specialized in code review.".to_string(),
                "Focuses on Rust best practices.".to_string(),
            ],
            topics: vec!["rust".to_string(), "code-review".to_string()],
            style: CharacterStyle {
                general: vec!["Be concise and technical.".to_string()],
                chat: vec!["Use code blocks for examples.".to_string()],
                post: vec![],
            },
            adjectives: vec!["helpful".to_string(), "precise".to_string()],
            knowledge: vec!["Always check for unwrap() in library code.".to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn renders_system_prompt() {
        let character = sample_character();
        let prompt = character.to_system_prompt();

        assert!(prompt.contains("You are CodeBot."));
        assert!(prompt.contains("About you:"));
        assert!(prompt.contains("An AI assistant specialized in code review."));
        assert!(prompt.contains("Your areas of expertise: rust, code-review"));
        assert!(prompt.contains("Style guidelines:"));
        assert!(prompt.contains("Be concise and technical."));
        assert!(prompt.contains("[chat] Use code blocks for examples."));
        assert!(prompt.contains("You are: helpful, precise"));
        assert!(prompt.contains("Reference knowledge:"));
        assert!(prompt.contains("Always check for unwrap() in library code."));
    }

    #[test]
    fn empty_character_is_detected() {
        let empty = Character::default();
        assert!(empty.is_empty());

        let with_name = Character { name: "Bot".to_string(), ..Default::default() };
        assert!(!with_name.is_empty());
    }

    #[test]
    fn serializes_to_json() {
        let character = sample_character();
        let json = serde_json::to_string(&character).expect("should serialize");
        let roundtrip: Character = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(roundtrip.name, "CodeBot");
        assert_eq!(roundtrip.topics.len(), 2);
    }

    #[test]
    fn partial_fields_default() {
        let json = r#"{"name": "Test"}"#;
        let character: Character = serde_json::from_str(json).expect("should deserialize");
        assert!(character.bio.is_empty());
        assert!(character.topics.is_empty());
        assert!(character.style.general.is_empty());
    }
}
