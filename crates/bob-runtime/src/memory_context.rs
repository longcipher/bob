//! # Memory Context Injection
//!
//! Memory context injection logic for injecting session memory summaries
//! into system prompts on a per-request basis.
//!
//! ## Overview
//!
//! This module provides functionality to inject memory summaries from
//! `SessionState` into system instructions, enabling persistent context
//! across conversation turns.
//!
//! ## Components
//!
//! - **Memory Injection**: Appends memory summary to system instructions
//! - **Per-request Support**: Works with AgentLoop's RequestContext system
//!
//! ## Usage
//!
//! ```rust,ignore
//! use bob_runtime::memory_context::inject_memory_prompt;
//!
//! let system_instructions = "You are a helpful assistant.";
//! let memory_summary = Some("Previous conversation about Rust programming".to_string());
//!
//! let injected = inject_memory_prompt(system_instructions, memory_summary.as_deref());
//! ```

/// Injects memory summary into system instructions if available.
///
/// This function appends memory context to the base system instructions,
/// enabling the agent to maintain persistent context across conversation turns.
///
/// # Arguments
///
/// * `system_instructions` - The base system instructions
/// * `memory_summary` - Optional memory summary from session state
///
/// # Returns
///
/// System instructions with memory context injected, or original instructions if no memory
pub(crate) fn inject_memory_prompt(
    system_instructions: &str,
    memory_summary: Option<&str>,
) -> String {
    let Some(memory) = memory_summary else {
        return system_instructions.to_string();
    };

    if memory.trim().is_empty() {
        return system_instructions.to_string();
    }

    format!("{}\n\nMemory Context:\n{}", system_instructions, memory.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_memory_prompt_with_memory() {
        let base = "You are a helpful assistant.";
        let memory = "Previous conversation about Rust programming.";

        let result = inject_memory_prompt(base, Some(memory));

        assert!(result.contains(base));
        assert!(result.contains("Memory Context:"));
        assert!(result.contains(memory));
    }

    #[test]
    fn inject_memory_prompt_without_memory() {
        let base = "You are a helpful assistant.";

        let result = inject_memory_prompt(base, None);

        assert_eq!(result, base);
    }

    #[test]
    fn inject_memory_prompt_with_empty_memory() {
        let base = "You are a helpful assistant.";

        let result = inject_memory_prompt(base, Some(""));

        assert_eq!(result, base);
    }

    #[test]
    fn inject_memory_prompt_with_whitespace_memory() {
        let base = "You are a helpful assistant.";

        let result = inject_memory_prompt(base, Some("   \n\t   "));

        assert_eq!(result, base);
    }

    #[test]
    fn inject_memory_prompt_preserves_formatting() {
        let base = "You are a helpful assistant.\nThink step by step.";
        let memory = "User prefers detailed explanations.";

        let result = inject_memory_prompt(base, Some(memory));

        assert!(result.starts_with(base));
        assert!(result.contains("\n\nMemory Context:\n"));
        assert!(result.ends_with(memory));
    }
}
