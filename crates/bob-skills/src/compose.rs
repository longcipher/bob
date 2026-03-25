//! # Skill Prompt Composer
//!
//! Generates prompt-ready content from selected skills.
//!
//! Supports two output formats:
//! - **XML**: The `<available_skills>` block format (spec-compliant)
//! - **Markdown**: Table + detail sections (bob legacy format)
//!
//! The composer handles token budget enforcement by:
//! 1. First stripping example sections from over-budget skills
//! 2. Then dropping lowest-ranked skills until within budget

use crate::{
    selector::effective_allowed_tools,
    types::{RenderedSkills, SkillEntry, SkillPolicy},
};

/// Prompt output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptFormat {
    /// XML `<available_skills>` block (spec-compliant).
    #[default]
    Xml,
    /// Markdown table + detail sections.
    Markdown,
}

/// Composes prompt text from selected skills.
#[derive(Debug, Clone)]
pub struct PromptComposer {
    format: PromptFormat,
}

impl PromptComposer {
    /// Create a composer with the default (XML) format.
    #[must_use]
    pub fn new() -> Self {
        Self { format: PromptFormat::Xml }
    }

    /// Create a composer with the specified format.
    #[must_use]
    pub fn with_format(format: PromptFormat) -> Self {
        Self { format }
    }

    /// Render selected skills into a prompt string.
    #[must_use]
    pub fn render(&self, selected: &[&SkillEntry], policy: &SkillPolicy) -> String {
        self.render_bundle(selected, policy).prompt
    }

    /// Render selected skills into a full bundle with metadata.
    #[must_use]
    pub fn render_bundle(&self, selected: &[&SkillEntry], policy: &SkillPolicy) -> RenderedSkills {
        if selected.is_empty() {
            return RenderedSkills::default();
        }

        let mut entries: Vec<RenderEntry> = selected
            .iter()
            .map(|skill| RenderEntry {
                name: skill.name().to_string(),
                description: skill.description().to_string(),
                body: skill.body().to_string(),
                location: skill.skill_md_path().display().to_string(),
                allowed_tools: effective_allowed_tools(skill, policy),
            })
            .collect();

        let mut prompt = self.render_entries(&entries);

        // Token budget enforcement.
        if policy.token_budget > 0 && estimate_tokens(&prompt) > policy.token_budget {
            for entry in &mut entries {
                entry.body = strip_examples(&entry.body);
            }
            prompt = self.render_entries(&entries);
        }

        while entries.len() > 1 &&
            policy.token_budget > 0 &&
            estimate_tokens(&prompt) > policy.token_budget
        {
            entries.pop();
            prompt = self.render_entries(&entries);
        }

        if entries.is_empty() {
            return RenderedSkills::default();
        }

        let mut allowed_tools: Vec<String> =
            entries.iter().flat_map(|e| e.allowed_tools.iter().cloned()).collect();
        allowed_tools.sort();
        allowed_tools.dedup();

        RenderedSkills {
            prompt,
            selected_names: entries.iter().map(|e| e.name.clone()).collect(),
            allowed_tools,
        }
    }

    fn render_entries(&self, entries: &[RenderEntry]) -> String {
        match self.format {
            PromptFormat::Xml => render_xml(entries),
            PromptFormat::Markdown => render_markdown(entries),
        }
    }
}

impl Default for PromptComposer {
    fn default() -> Self {
        Self::new()
    }
}

// ── Internal render entry ─────────────────────────────────────────────

struct RenderEntry {
    name: String,
    description: String,
    body: String,
    location: String,
    allowed_tools: Vec<String>,
}

// ── XML format ────────────────────────────────────────────────────────

fn render_xml(entries: &[RenderEntry]) -> String {
    let mut out = String::from("<available_skills>");
    for entry in entries {
        out.push_str("\n  <skill>");
        out.push_str(&format!("\n    <name>{}</name>", escape_xml(&entry.name)));
        out.push_str(&format!(
            "\n    <description>{}</description>",
            escape_xml(&entry.description)
        ));
        out.push_str(&format!("\n    <location>{}</location>", escape_xml(&entry.location)));
        out.push_str("\n  </skill>");
    }
    out.push_str("\n</available_skills>");

    for entry in entries {
        out.push_str(&format!(
            "\n\n### Skill `{}`\nDescription: {}\n{}",
            entry.name, entry.description, entry.body
        ));
    }
    out
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ── Markdown format ───────────────────────────────────────────────────

fn render_markdown(entries: &[RenderEntry]) -> String {
    let mut out = String::from("Use these skills when relevant:");
    out.push_str("\n\n| Skill | Description | Allowed Tools |");
    out.push_str("\n| --- | --- | --- |");
    for entry in entries {
        let tools = if entry.allowed_tools.is_empty() {
            "-".to_string()
        } else {
            entry.allowed_tools.join(", ")
        };
        out.push_str(&format!(
            "\n| `{}` | {} | {} |",
            escape_md_cell(&entry.name),
            escape_md_cell(&entry.description),
            escape_md_cell(&tools),
        ));
    }

    for entry in entries {
        out.push_str(&format!(
            "\n\n### Skill `{}`\nDescription: {}\n{}",
            entry.name, entry.description, entry.body
        ));
    }
    out
}

fn escape_md_cell(value: &str) -> String {
    value.replace('|', r"\\|")
}

// ── Token budget ──────────────────────────────────────────────────────

fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count().max(1)
}

fn strip_examples(body: &str) -> String {
    let mut out = Vec::new();
    let mut in_example = false;

    for line in body.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('#') {
            let heading = trimmed.trim_start_matches('#').trim().to_ascii_lowercase();
            in_example = heading.contains("example");
            if !in_example {
                out.push(line);
            }
            continue;
        }

        if in_example {
            continue;
        }

        if trimmed.to_ascii_lowercase().starts_with("example:") {
            in_example = true;
            continue;
        }

        out.push(line);
    }

    out.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::TempDir;

    use super::*;
    use crate::{parsing, types::SkillEntry};

    fn make_entry(root: &Path, name: &str, body: &str) -> SkillEntry {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: Test skill {name}.\n---\n\n# {name}\n\n{body}\n"
            ),
        )
        .unwrap();
        let skill_dir = parsing::SkillDirectory::load(&dir).unwrap();
        SkillEntry::from_skill(skill_dir.skill().clone(), dir)
    }

    #[test]
    fn xml_format_includes_available_skills_tag() {
        let temp = TempDir::new().unwrap();
        let entry = make_entry(temp.path(), "test-skill", "Do things.");

        let composer = PromptComposer::new();
        let prompt = composer.render(&[&entry], &SkillPolicy::default());

        assert!(prompt.contains("<available_skills>"));
        assert!(prompt.contains("<name>test-skill</name>"));
        assert!(prompt.contains("<description>Test skill test-skill.</description>"));
        assert!(prompt.contains("<location>"));
        assert!(prompt.contains("SKILL.md</location>"));
        assert!(prompt.contains("</available_skills>"));
    }

    #[test]
    fn xml_escapes_special_chars() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join("special");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: special\ndescription: Use <b>bold</b> & \"quotes\"\n---\n\nBody.\n",
        )
        .unwrap();
        let skill_dir = parsing::SkillDirectory::load(&dir).unwrap();
        let entry = SkillEntry::from_skill(skill_dir.skill().clone(), dir);

        let composer = PromptComposer::new();
        let prompt = composer.render(&[&entry], &SkillPolicy::default());

        assert!(prompt.contains("<b>bold</b>"));
        assert!(prompt.contains("&"));
        assert!(prompt.contains("\"quotes\""));
    }

    #[test]
    fn markdown_format_includes_table() {
        let temp = TempDir::new().unwrap();
        let entry = make_entry(temp.path(), "md-skill", "Do markdown.");

        let composer = PromptComposer::with_format(PromptFormat::Markdown);
        let prompt = composer.render(&[&entry], &SkillPolicy::default());

        assert!(prompt.contains("| Skill |"));
        assert!(prompt.contains("md-skill"));
    }

    #[test]
    fn token_budget_strips_examples() {
        let temp = TempDir::new().unwrap();
        let body = "Checklist:\n- Focus on safety\n\n## Example\n```rust\nlet a = very_long_example_code();\nprintln!(\"{a}\");\n```";
        let entry = make_entry(temp.path(), "budget-test", body);

        let composer = PromptComposer::new();
        let policy = SkillPolicy::new().with_token_budget(15);
        let prompt = composer.render(&[&entry], &policy);

        assert!(prompt.contains("Checklist"), "core content should be preserved");
        assert!(!prompt.contains("very_long_example_code"), "example blocks should be stripped");
    }

    #[test]
    fn empty_selection_returns_empty() {
        let composer = PromptComposer::new();
        let result = composer.render(&[], &SkillPolicy::default());
        assert!(result.is_empty());
    }

    #[test]
    fn render_bundle_returns_metadata() {
        let temp = TempDir::new().unwrap();
        let entry = make_entry(temp.path(), "meta-skill", "Body.");

        let composer = PromptComposer::new();
        let bundle = composer.render_bundle(&[&entry], &SkillPolicy::default());

        assert_eq!(bundle.selected_names, vec!["meta-skill"]);
        assert!(!bundle.prompt.is_empty());
    }
}
