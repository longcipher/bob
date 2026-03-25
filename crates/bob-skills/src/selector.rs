//! # Skill Selector
//!
//! Relevance-based skill selection using lightweight keyword scoring.
//!
//! The selector scores skills against user input by matching:
//! 1. Skill name (highest weight)
//! 2. Metadata tags
//! 3. Description and body keyword overlap
//!
//! Results are sorted by descending score and truncated to `max_selected`.

use crate::types::{SkillEntry, SkillPolicy};

/// Result of scoring a single skill.
#[derive(Debug, Clone)]
pub struct ScoredSkill<'a> {
    /// The skill entry.
    pub entry: &'a SkillEntry,
    /// Relevance score (higher = more relevant).
    pub score: f64,
}

/// Stateless skill selector with relevance scoring.
#[derive(Debug, Clone)]
pub struct SkillSelector {
    /// Maximum number of skills to select.
    max_selected: usize,
}

impl SkillSelector {
    /// Create a new selector with the given max selection count.
    ///
    /// The count is clamped to a minimum of 1.
    #[must_use]
    pub fn new(max_selected: usize) -> Self {
        Self { max_selected: max_selected.max(1) }
    }

    /// Select the most relevant skills for the given input.
    ///
    /// Skills are scored by keyword overlap with the input. Only skills
    /// with a positive score are considered. Results are sorted by
    /// descending score (ties broken alphabetically by name).
    #[must_use]
    pub fn select<'a>(&self, skills: &'a [SkillEntry], input: &str) -> Vec<&'a SkillEntry> {
        self.select_with_policy(skills, input, &SkillPolicy::default())
    }

    /// Select skills with tool policy filtering.
    #[must_use]
    pub fn select_with_policy<'a>(
        &self,
        skills: &'a [SkillEntry],
        input: &str,
        policy: &SkillPolicy,
    ) -> Vec<&'a SkillEntry> {
        let input_lower = input.to_ascii_lowercase();
        if input_lower.trim().is_empty() {
            return Vec::new();
        }

        let input_tokens = tokenize(&input_lower);

        let mut scored: Vec<ScoredSkill<'a>> = skills
            .iter()
            .filter(|skill| is_compatible_with_policy(skill, policy))
            .map(|skill| {
                let score = score_skill(skill, &input_lower, &input_tokens);
                ScoredSkill { entry: skill, score }
            })
            .filter(|s| s.score > 0.0)
            .collect();

        scored.sort_by(|a, b| {
            b.score.total_cmp(&a.score).then_with(|| a.entry.name().cmp(b.entry.name()))
        });

        scored.into_iter().take(self.max_selected).map(|s| s.entry).collect()
    }
}

impl Default for SkillSelector {
    fn default() -> Self {
        Self::new(3)
    }
}

// ── Scoring ───────────────────────────────────────────────────────────

fn score_skill(skill: &SkillEntry, input_lower: &str, input_tokens: &[String]) -> f64 {
    let mut score = 0.0_f64;
    let name_lower = skill.name().to_ascii_lowercase();

    // Name match (highest signal).
    if input_lower.contains(&name_lower) {
        score += 1.0;
    }

    // Tag overlap.
    let tag_hits = skill.tags().iter().filter(|tag| input_tokens.iter().any(|t| t == *tag)).count();
    if tag_hits > 0 {
        score = 0.4f64.mul_add(tag_hits as f64, score);
    }

    // Keyword hits in description + body.
    let haystack = format!(
        "{} {} {}",
        skill.name().to_ascii_lowercase(),
        skill.description().to_ascii_lowercase(),
        skill.body().to_ascii_lowercase()
    );

    let keyword_hits =
        input_tokens.iter().filter(|t| t.len() >= 3 && haystack.contains(t.as_str())).count();
    score =
        0.2f64.mul_add(f64::from(u32::try_from(keyword_hits).unwrap_or(u32::MAX).min(10)), score);

    score
}

/// Check if a skill is compatible with the current tool policy.
///
/// A skill with no `allowed-tools` is always compatible. A skill whose
/// entire `allowed-tools` list is denied by the policy is filtered out.
fn is_compatible_with_policy(skill: &SkillEntry, policy: &SkillPolicy) -> bool {
    if skill.allowed_tools().is_empty() {
        return true;
    }
    !effective_allowed_tools(skill, policy).is_empty()
}

/// Compute the effective allowed tools after applying policy filters.
pub(crate) fn effective_allowed_tools(skill: &SkillEntry, policy: &SkillPolicy) -> Vec<String> {
    if skill.allowed_tools().is_empty() {
        return Vec::new();
    }

    skill
        .allowed_tools()
        .iter()
        .filter(|tool| is_tool_allowed(tool, &policy.deny_tools, policy.allow_tools.as_deref()))
        .map(ToString::to_string)
        .collect()
}

/// Check if a single tool is allowed given deny/allow lists.
fn is_tool_allowed(tool: &str, deny_tools: &[String], allow_tools: Option<&[String]>) -> bool {
    let tool_lower = tool.to_ascii_lowercase();

    if deny_tools.iter().any(|d| tool_matches(&tool_lower, &d.to_ascii_lowercase())) {
        return false;
    }

    match allow_tools {
        Some(allowed) => allowed.iter().any(|a| tool_matches(&tool_lower, &a.to_ascii_lowercase())),
        None => true,
    }
}

/// Simple tool pattern matching supporting `Bash(git:*)` style globs.
fn tool_matches(tool: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        tool.starts_with(prefix)
    } else if let Some(prefix) = pattern.strip_suffix("*)") {
        // Handle "Bash(git:*)" where pattern is "bash(git:*)"
        if let Some(paren_idx) = tool.find('(') {
            let tool_prefix = &tool[..paren_idx];
            let pat_prefix = &pattern[..pattern.find('(').unwrap_or(pattern.len())];
            if tool_prefix == pat_prefix {
                let tool_args = &tool[paren_idx + 1..].trim_end_matches(')');
                let pat_args_prefix =
                    prefix[pattern.find('(').map_or(0, |i| i + 1)..].trim_end_matches(')');
                return tool_args.starts_with(pat_args_prefix);
            }
        }
        tool.starts_with(prefix)
    } else {
        tool == pattern
    }
}

// ── Tokenization ──────────────────────────────────────────────────────

fn tokenize(input: &str) -> Vec<String> {
    input
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::TempDir;

    use super::*;
    use crate::parsing;

    fn make_entry(root: &Path, name: &str, description: &str, body: &str) -> SkillEntry {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\n{body}\n"),
        )
        .unwrap();
        let skill_dir = parsing::SkillDirectory::load(&dir).unwrap();
        SkillEntry::from_skill(skill_dir.skill().clone(), dir)
    }

    fn make_entry_with_tags(root: &Path, name: &str, description: &str, tags: &str) -> SkillEntry {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: {description}\nmetadata:\n  tags: {tags}\n---\n\n# {name}\n\nBody.\n"
            ),
        )
        .unwrap();
        let skill_dir = parsing::SkillDirectory::load(&dir).unwrap();
        SkillEntry::from_skill(skill_dir.skill().clone(), dir)
    }

    #[test]
    fn selects_by_name_match() {
        let temp = TempDir::new().unwrap();
        let entries = vec![
            make_entry(temp.path(), "rust-review", "Review Rust code.", "Check safety."),
            make_entry(temp.path(), "sql-tuning", "Tune SQL queries.", "Inspect indexes."),
        ];

        let selector = SkillSelector::new(1);
        let selected = selector.select(&entries, "please do rust-review on this module");

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name(), "rust-review");
    }

    #[test]
    fn selects_by_tag_match() {
        let temp = TempDir::new().unwrap();
        let entries = vec![make_entry_with_tags(
            temp.path(),
            "db-advisor",
            "Generic advisor.",
            "postgres migration",
        )];

        let selector = SkillSelector::new(3);
        let selected = selector.select(&entries, "need postgres migration plan");

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name(), "db-advisor");
    }

    #[test]
    fn returns_empty_for_empty_input() {
        let temp = TempDir::new().unwrap();
        let entries = vec![make_entry(temp.path(), "test", "Test.", "Body.")];
        let selector = SkillSelector::default();

        assert!(selector.select(&entries, "").is_empty());
        assert!(selector.select(&entries, "   ").is_empty());
    }

    #[test]
    fn policy_filters_deny_tools() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join("shell-skill");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: shell-skill\ndescription: Runs shell.\nallowed-tools: Bash(git:*) Read\n---\n\nBody.\n",
        )
        .unwrap();
        let skill_dir = parsing::SkillDirectory::load(&dir).unwrap();
        let entry = SkillEntry::from_skill(skill_dir.skill().clone(), dir);
        let entries = vec![entry];

        let selector = SkillSelector::new(3);
        let policy = SkillPolicy::new().with_deny_tools(vec!["Bash".into()]);

        // Bash is denied, but Read should still work — but effective tools is filtered
        let selected = selector.select_with_policy(&entries, "shell access", &policy);
        // The skill's allowed-tools include Read, so it should still be compatible
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn respects_max_selected() {
        let temp = TempDir::new().unwrap();
        let entries = vec![
            make_entry(temp.path(), "alpha", "Alpha skill.", "keyword shared."),
            make_entry(temp.path(), "beta", "Beta skill.", "keyword shared."),
            make_entry(temp.path(), "gamma", "Gamma skill.", "keyword shared."),
        ];

        let selector = SkillSelector::new(2);
        let selected = selector.select(&entries, "keyword");
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn tool_matching_supports_prefix_glob() {
        assert!(tool_matches("bash(git:status)", "bash(git:*)"));
        assert!(!tool_matches("bash(rm:*)", "bash(git:*)"));
        assert!(tool_matches("read", "read"));
        assert!(!tool_matches("write", "read"));
    }
}
