//! # Agent Skills
//!
//! Agent-skills loader and prompt composer.
//!
//! ## Overview
//!
//! This module provides skill loading and composition capabilities using the
//! [`agent-skills`](https://crates.io/crates/agent-skills) crate.
//!
//! Skills are predefined prompts and instructions that can be dynamically
//! selected and injected into the conversation based on the user's input.
//!
//! ## Features
//!
//! - Load skills from directories
//! - Recursive directory scanning
//! - Skill selection based on user input
//! - Tool filtering (deny/allow lists)
//! - Token budget management
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_adapters::skills_agent::{
//!     SkillPromptComposer,
//!     SkillSourceConfig,
//!     SkillSelectionPolicy,
//! };
//!
//! // Load skills from a directory
//! let sources = vec![SkillSourceConfig {
//!     path: "./skills".into(),
//!     recursive: true,
//! }];
//!
//! let composer = SkillPromptComposer::from_sources(&sources, 3)?;
//!
//! // Render skills for a user input
//! let policy = SkillSelectionPolicy::default();
//! let rendered = composer.render_bundle_for_input_with_policy(
//!     "review this code",
//!     &policy,
//! );
//!
//! println!("Selected skills: {:?}", rendered.selected_skill_names);
//! println!("Prompt: {}", rendered.prompt);
//! ```
//!
//! ## Feature Flag
//!
//! This module is only available when the `skills-agent` feature is enabled (default).

use std::{
    fs,
    path::{Path, PathBuf},
};

use bob_core::{is_tool_allowed, normalize_tool_list};

/// One skill source entry.
#[derive(Debug, Clone)]
pub struct SkillSourceConfig {
    pub path: PathBuf,
    pub recursive: bool,
}

/// Loaded skill data used by prompt composition.
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub tags: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub source_dir: PathBuf,
}

/// Result of rendering the skills prompt for one user input.
#[derive(Debug, Clone, Default)]
pub struct RenderedSkillsPrompt {
    pub prompt: String,
    pub selected_skill_names: Vec<String>,
    pub selected_allowed_tools: Vec<String>,
}

/// Runtime policy affecting skill selection and rendering.
#[derive(Debug, Clone)]
pub struct SkillSelectionPolicy {
    pub deny_tools: Vec<String>,
    pub allow_tools: Option<Vec<String>>,
    pub token_budget_tokens: usize,
}

impl Default for SkillSelectionPolicy {
    fn default() -> Self {
        Self { deny_tools: Vec::new(), allow_tools: None, token_budget_tokens: 1_800 }
    }
}

/// Errors produced by skill loading/composition.
#[derive(Debug, thiserror::Error)]
pub enum SkillsAgentError {
    #[error("skill source path does not exist: {path}")]
    SourceNotFound { path: String },
    #[error("failed to list directory '{path}': {message}")]
    ReadDir { path: String, message: String },
    #[error("failed to load skill directory '{path}': {message}")]
    LoadSkill { path: String, message: String },
}

/// Load all skills from configured sources.
pub fn load_skills_from_sources(
    sources: &[SkillSourceConfig],
) -> Result<Vec<LoadedSkill>, SkillsAgentError> {
    let mut dirs = Vec::new();
    for source in sources {
        collect_skill_dirs(&source.path, source.recursive, &mut dirs)?;
    }

    dirs.sort();
    dirs.dedup();

    let mut loaded = Vec::with_capacity(dirs.len());
    for dir in dirs {
        let skill_dir = agent_skills::SkillDirectory::load(&dir).map_err(|err| {
            SkillsAgentError::LoadSkill {
                path: dir.display().to_string(),
                message: err.to_string(),
            }
        })?;

        let skill = skill_dir.skill();
        let tags = skill
            .frontmatter()
            .metadata()
            .and_then(|meta| meta.get("tags"))
            .map(parse_tags)
            .unwrap_or_default();
        let allowed_tools = skill
            .frontmatter()
            .allowed_tools()
            .map(|tools| tools.iter().map(ToString::to_string).collect())
            .unwrap_or_default();
        loaded.push(LoadedSkill {
            name: skill.name().as_str().to_string(),
            description: skill.description().as_str().to_string(),
            body: skill.body_trimmed().to_string(),
            tags,
            allowed_tools,
            source_dir: dir,
        });
    }

    loaded.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(loaded)
}

fn collect_skill_dirs(
    path: &Path,
    recursive: bool,
    out: &mut Vec<PathBuf>,
) -> Result<(), SkillsAgentError> {
    if !path.exists() {
        return Err(SkillsAgentError::SourceNotFound { path: path.display().to_string() });
    }

    if path.join("SKILL.md").is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }

    let read_dir = fs::read_dir(path).map_err(|err| SkillsAgentError::ReadDir {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;

    for entry in read_dir {
        let entry = entry.map_err(|err| SkillsAgentError::ReadDir {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;
        let candidate = entry.path();
        if !candidate.is_dir() {
            continue;
        }

        if candidate.join("SKILL.md").is_file() {
            out.push(candidate);
            continue;
        }

        if recursive {
            collect_skill_dirs(&candidate, true, out)?;
        }
    }

    Ok(())
}

/// Stateless prompt composer for loaded skills.
#[derive(Debug, Clone)]
pub struct SkillPromptComposer {
    skills: Vec<LoadedSkill>,
    max_selected: usize,
}

impl SkillPromptComposer {
    #[must_use]
    pub fn new(skills: Vec<LoadedSkill>, max_selected: usize) -> Self {
        Self { skills, max_selected: max_selected.max(1) }
    }

    pub fn from_sources(
        sources: &[SkillSourceConfig],
        max_selected: usize,
    ) -> Result<Self, SkillsAgentError> {
        let skills = load_skills_from_sources(sources)?;
        Ok(Self::new(skills, max_selected))
    }

    #[must_use]
    pub fn skills(&self) -> &[LoadedSkill] {
        &self.skills
    }

    #[must_use]
    pub fn select_for_input<'a>(&'a self, input: &str) -> Vec<&'a LoadedSkill> {
        self.select_for_input_with_policy(input, &SkillSelectionPolicy::default())
    }

    #[must_use]
    pub fn select_for_input_with_policy<'a>(
        &'a self,
        input: &str,
        policy: &SkillSelectionPolicy,
    ) -> Vec<&'a LoadedSkill> {
        let input_lower = input.to_ascii_lowercase();
        if input_lower.trim().is_empty() {
            return Vec::new();
        }
        let input_tokens = tokenize(&input_lower);

        let mut scored: Vec<(f64, &LoadedSkill)> = self
            .skills
            .iter()
            .filter(|skill| is_skill_compatible_with_policy(skill, policy))
            .map(|skill| {
                let score = score_skill(skill, &input_lower, &input_tokens);
                (score, skill)
            })
            .filter(|(score, _)| *score > 0.0)
            .collect();

        scored.sort_by(|(a_score, a), (b_score, b)| {
            b_score.total_cmp(a_score).then_with(|| a.name.cmp(&b.name))
        });

        scored.into_iter().take(self.max_selected).map(|(_, skill)| skill).collect()
    }

    #[must_use]
    pub fn render_for_input(&self, input: &str) -> String {
        self.render_for_input_with_policy(input, &SkillSelectionPolicy::default())
    }

    #[must_use]
    pub fn render_for_input_with_policy(
        &self,
        input: &str,
        policy: &SkillSelectionPolicy,
    ) -> String {
        self.render_bundle_for_input_with_policy(input, policy).prompt
    }

    #[must_use]
    pub fn render_bundle_for_input_with_policy(
        &self,
        input: &str,
        policy: &SkillSelectionPolicy,
    ) -> RenderedSkillsPrompt {
        let selected = self.select_for_input_with_policy(input, policy);
        if selected.is_empty() {
            return RenderedSkillsPrompt::default();
        }

        let mut entries: Vec<SkillRenderEntry<'_>> =
            selected.iter().map(|skill| SkillRenderEntry::from_skill(skill, policy)).collect();

        let mut prompt = render_prompt_from_entries(&entries);
        if estimate_text_tokens(&prompt) > policy.token_budget_tokens {
            for entry in &mut entries {
                entry.body = strip_examples_from_body(&entry.body);
            }
            prompt = render_prompt_from_entries(&entries);
        }

        while entries.len() > 1 && estimate_text_tokens(&prompt) > policy.token_budget_tokens {
            entries.pop();
            prompt = render_prompt_from_entries(&entries);
        }

        if entries.is_empty() {
            return RenderedSkillsPrompt::default();
        }

        let mut selected_allowed_tools = entries
            .iter()
            .flat_map(|entry| entry.allowed_tools.iter().cloned())
            .collect::<Vec<_>>();
        selected_allowed_tools.sort();
        selected_allowed_tools.dedup();

        RenderedSkillsPrompt {
            prompt,
            selected_skill_names: entries.iter().map(|entry| entry.skill.name.clone()).collect(),
            selected_allowed_tools,
        }
    }
}

#[derive(Debug, Clone)]
struct SkillRenderEntry<'a> {
    skill: &'a LoadedSkill,
    body: String,
    allowed_tools: Vec<String>,
}

impl<'a> SkillRenderEntry<'a> {
    fn from_skill(skill: &'a LoadedSkill, policy: &SkillSelectionPolicy) -> Self {
        Self {
            skill,
            body: skill.body.clone(),
            allowed_tools: effective_allowed_tools(skill, policy),
        }
    }
}

fn render_prompt_from_entries(entries: &[SkillRenderEntry<'_>]) -> String {
    let mut out = String::from("Use these skills when relevant:");
    out.push_str("\n\n| Skill | Description | Tags | Allowed Tools |");
    out.push_str("\n| --- | --- | --- | --- |");
    for entry in entries {
        let skill = entry.skill;
        let tags = if skill.tags.is_empty() { "-".to_string() } else { skill.tags.join(", ") };
        let allowed_tools = if entry.allowed_tools.is_empty() {
            "-".to_string()
        } else {
            entry.allowed_tools.join(", ")
        };
        out.push_str(&format!(
            "\n| `{}` | {} | {} | {} |",
            escape_table_cell(&skill.name),
            escape_table_cell(&skill.description),
            escape_table_cell(&tags),
            escape_table_cell(&allowed_tools),
        ));
    }

    for entry in entries {
        let skill = entry.skill;
        out.push_str(&format!(
            "\n\n### Skill `{}`\nDescription: {}\n{}",
            skill.name, skill.description, entry.body
        ));
    }
    out
}

fn score_skill(skill: &LoadedSkill, input_lower: &str, input_tokens: &[String]) -> f64 {
    let mut score = 0.0_f64;
    let name_lower = skill.name.to_ascii_lowercase();

    if input_lower.contains(&name_lower) {
        score += 1.0;
    }

    let tag_overlap = skill
        .tags
        .iter()
        .map(|tag| tag.to_ascii_lowercase())
        .filter(|tag| input_tokens.iter().any(|token| token == tag))
        .count();
    if tag_overlap > 0 {
        score += 0.4 * tag_overlap as f64;
    }

    let haystack = format!(
        "{} {} {}",
        skill.name.to_ascii_lowercase(),
        skill.description.to_ascii_lowercase(),
        skill.body.to_ascii_lowercase()
    );

    let mut keyword_hits = 0_u32;
    for token in input_tokens {
        if token.len() >= 3 && haystack.contains(token) {
            keyword_hits += 1;
        }
    }
    score += 0.2 * f64::from(keyword_hits.min(10));

    score
}

fn is_skill_compatible_with_policy(skill: &LoadedSkill, policy: &SkillSelectionPolicy) -> bool {
    if skill.allowed_tools.is_empty() {
        return true;
    }

    !effective_allowed_tools(skill, policy).is_empty()
}

fn effective_allowed_tools(skill: &LoadedSkill, policy: &SkillSelectionPolicy) -> Vec<String> {
    if skill.allowed_tools.is_empty() {
        return Vec::new();
    }

    normalize_tool_list(
        skill
            .allowed_tools
            .iter()
            .filter(|tool| is_tool_allowed(tool, &policy.deny_tools, policy.allow_tools.as_deref()))
            .map(String::as_str),
    )
}

fn parse_tags(raw: &str) -> Vec<String> {
    raw.split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

fn estimate_text_tokens(text: &str) -> usize {
    text.split_whitespace().count().max(1)
}

fn strip_examples_from_body(body: &str) -> String {
    // Prefer preserving constraints/checklists; remove explicit example sections first.
    let mut out = Vec::new();
    let mut in_example_section = false;

    for line in body.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('#') {
            let heading = trimmed.trim_start_matches('#').trim().to_ascii_lowercase();
            in_example_section = heading.contains("example");
            if !in_example_section {
                out.push(line);
            }
            continue;
        }

        if in_example_section {
            continue;
        }

        if trimmed.to_ascii_lowercase().starts_with("example:") {
            in_example_section = true;
            continue;
        }

        out.push(line);
    }

    out.join("\n").trim().to_string()
}

fn escape_table_cell(value: &str) -> String {
    value.replace('|', "\\|")
}

fn tokenize(input: &str) -> Vec<String> {
    input
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn write_skill(
        root: &Path,
        name: &str,
        description: &str,
        body: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let dir = root.join(name);
        fs::create_dir_all(&dir)?;
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\n{body}\n"),
        )?;
        Ok(dir)
    }

    #[test]
    fn loads_skills_from_directory_source() -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let skills_root = temp.path().join("skills");
        fs::create_dir_all(&skills_root)?;

        write_skill(
            &skills_root,
            "rust-review",
            "Review Rust code for correctness.",
            "Focus on bug risk.",
        )?;
        write_skill(
            &skills_root,
            "sql-tuning",
            "Optimize SQL queries.",
            "Look for missing indexes.",
        )?;

        let loaded =
            load_skills_from_sources(&[SkillSourceConfig { path: skills_root, recursive: false }])?;

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "rust-review");
        assert_eq!(loaded[1].name, "sql-tuning");
        Ok(())
    }

    #[test]
    fn selects_skill_by_name_mention() {
        let skills = vec![
            LoadedSkill {
                name: "rust-review".to_string(),
                description: "Review Rust code for bugs.".to_string(),
                body: "Check panics and edge cases.".to_string(),
                tags: Vec::new(),
                allowed_tools: Vec::new(),
                source_dir: PathBuf::from("/tmp/rust-review"),
            },
            LoadedSkill {
                name: "sql-tuning".to_string(),
                description: "Tune SQL query plans.".to_string(),
                body: "Inspect indexes.".to_string(),
                tags: Vec::new(),
                allowed_tools: Vec::new(),
                source_dir: PathBuf::from("/tmp/sql-tuning"),
            },
        ];
        let composer = SkillPromptComposer::new(skills, 1);

        let selected = composer.select_for_input("please do rust-review on this module");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "rust-review");
    }

    #[test]
    fn renders_prompt_with_selected_skill_content() {
        let skills = vec![LoadedSkill {
            name: "sql-tuning".to_string(),
            description: "Tune SQL query plans.".to_string(),
            body: "Look at EXPLAIN and indexes.".to_string(),
            tags: Vec::new(),
            allowed_tools: Vec::new(),
            source_dir: PathBuf::from("/tmp/sql-tuning"),
        }];
        let composer = SkillPromptComposer::new(skills, 1);

        let prompt = composer.render_for_input("need help to tuning sql index");
        assert!(prompt.contains("Skill `sql-tuning`"));
        assert!(prompt.contains("Look at EXPLAIN"));
    }

    #[test]
    fn selects_skill_by_metadata_tags() -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let skills_root = temp.path().join("skills");
        fs::create_dir_all(&skills_root)?;

        let dir = skills_root.join("db-advisor");
        fs::create_dir_all(&dir)?;
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: db-advisor\ndescription: Generic advisor.\nmetadata:\n  tags: postgres migration\n---\n\n# db-advisor\n\nFollow checklist carefully.\n",
        )?;

        let composer = SkillPromptComposer::from_sources(
            &[SkillSourceConfig { path: skills_root, recursive: false }],
            3,
        )?;

        let selected = composer.select_for_input("need postgres migration plan");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "db-advisor");
        Ok(())
    }

    #[test]
    fn render_includes_summary_table() {
        let skills = vec![LoadedSkill {
            name: "rust-review".to_string(),
            description: "Review Rust code".to_string(),
            body: "Focus on panic-safety and edge cases.".to_string(),
            tags: Vec::new(),
            allowed_tools: Vec::new(),
            source_dir: PathBuf::from("/tmp/rust-review"),
        }];
        let composer = SkillPromptComposer::new(skills, 1);

        let prompt = composer.render_for_input("review this rust module");
        assert!(prompt.contains("| Skill |"), "prompt should include summary table header");
        assert!(prompt.contains("rust-review"), "prompt should include selected skill row");
    }

    #[test]
    fn policy_filters_skill_when_all_allowed_tools_denied() {
        let skills = vec![LoadedSkill {
            name: "danger-shell".to_string(),
            description: "Runs shell commands.".to_string(),
            body: "Only use when explicitly required.".to_string(),
            tags: vec!["shell".to_string()],
            allowed_tools: vec!["local/shell_exec".to_string()],
            source_dir: PathBuf::from("/tmp/danger-shell"),
        }];
        let composer = SkillPromptComposer::new(skills, 3);
        let policy = SkillSelectionPolicy {
            deny_tools: vec!["local/shell_exec".to_string()],
            allow_tools: None,
            token_budget_tokens: 1_800,
        };

        let selected = composer.select_for_input_with_policy("need shell access", &policy);
        assert!(selected.is_empty(), "skill should be filtered by deny_tools policy");
    }

    #[test]
    fn policy_filters_skill_when_allowlist_disjoint() {
        let skills = vec![LoadedSkill {
            name: "fs-read".to_string(),
            description: "Read repository files.".to_string(),
            body: "Use read-only tool.".to_string(),
            tags: vec!["repo".to_string()],
            allowed_tools: vec!["local/read_file".to_string()],
            source_dir: PathBuf::from("/tmp/fs-read"),
        }];
        let composer = SkillPromptComposer::new(skills, 3);
        let policy = SkillSelectionPolicy {
            deny_tools: Vec::new(),
            allow_tools: Some(vec!["local/write_file".to_string()]),
            token_budget_tokens: 1_800,
        };

        let selected = composer.select_for_input_with_policy("inspect repository files", &policy);
        assert!(selected.is_empty(), "skill should be filtered by runtime allowlist");
    }

    #[test]
    fn token_budget_drops_low_ranked_skills() {
        let skills = vec![
            LoadedSkill {
                name: "rust-review".to_string(),
                description: "Review Rust code".to_string(),
                body: "panic safety".to_string(),
                tags: vec!["rust".to_string()],
                allowed_tools: Vec::new(),
                source_dir: PathBuf::from("/tmp/rust-review"),
            },
            LoadedSkill {
                name: "sql-tuning".to_string(),
                description: "Tune SQL".to_string(),
                body: "index recommendations and query rewrite guidance".to_string(),
                tags: vec!["sql".to_string()],
                allowed_tools: Vec::new(),
                source_dir: PathBuf::from("/tmp/sql-tuning"),
            },
        ];
        let composer = SkillPromptComposer::new(skills, 3);
        let policy = SkillSelectionPolicy {
            deny_tools: Vec::new(),
            allow_tools: None,
            token_budget_tokens: 5,
        };

        let prompt = composer.render_for_input_with_policy("rust sql", &policy);
        assert!(
            prompt.contains("rust-review") || prompt.contains("sql-tuning"),
            "at least one higher-priority skill should remain under tight budget"
        );
    }

    #[test]
    fn token_budget_truncates_examples_before_drop() {
        let skills = vec![LoadedSkill {
            name: "rust-review".to_string(),
            description: "Review Rust code".to_string(),
            body: "Checklist:\n- Focus on safety\n\n## Example\n```rust\nlet a = very_long_example_code_path();\nprintln!(\"{a}\");\n```"
                .to_string(),
            tags: vec!["rust".to_string()],
            allowed_tools: Vec::new(),
            source_dir: PathBuf::from("/tmp/rust-review"),
        }];
        let composer = SkillPromptComposer::new(skills, 1);
        let policy = SkillSelectionPolicy {
            deny_tools: Vec::new(),
            allow_tools: None,
            token_budget_tokens: 12,
        };

        let prompt = composer.render_for_input_with_policy("need rust review", &policy);
        assert!(prompt.contains("Checklist"), "core checklist should be preserved");
        assert!(
            !prompt.contains("very_long_example_code_path"),
            "example blocks should be removed first under budget pressure"
        );
    }

    #[test]
    fn token_budget_keeps_non_example_code_blocks() {
        let skills = vec![LoadedSkill {
            name: "repo-review".to_string(),
            description: "Review repository state".to_string(),
            body: "## Procedure\n```bash\nrg --files\n```\n\n## Example\n```bash\necho demo this command is only an example\n```"
                .to_string(),
            tags: vec!["review".to_string()],
            allowed_tools: Vec::new(),
            source_dir: PathBuf::from("/tmp/repo-review"),
        }];
        let composer = SkillPromptComposer::new(skills, 1);
        let policy = SkillSelectionPolicy {
            deny_tools: Vec::new(),
            allow_tools: Some(vec!["local/read_file".to_string(), "local/list_dir".to_string()]),
            token_budget_tokens: 14,
        };

        let prompt = composer.render_for_input_with_policy("review repo", &policy);
        assert!(
            prompt.contains("rg --files"),
            "non-example procedural code blocks should stay intact"
        );
        assert!(
            !prompt.contains("echo demo this command is only an example"),
            "example blocks should still be removed under budget pressure"
        );
    }

    #[test]
    fn render_bundle_returns_effective_allowed_tools() {
        let skills = vec![LoadedSkill {
            name: "repo-read".to_string(),
            description: "Read repo files".to_string(),
            body: "Use read file and list dir.".to_string(),
            tags: vec!["repo".to_string()],
            allowed_tools: vec!["local/read_file".to_string(), "local/list_dir".to_string()],
            source_dir: PathBuf::from("/tmp/repo-read"),
        }];
        let composer = SkillPromptComposer::new(skills, 1);
        let policy = SkillSelectionPolicy {
            deny_tools: vec!["local/list_dir".to_string()],
            allow_tools: Some(vec!["local/read_file".to_string(), "local/write_file".to_string()]),
            token_budget_tokens: 1_800,
        };

        let rendered = composer.render_bundle_for_input_with_policy("inspect repo", &policy);
        assert_eq!(rendered.selected_skill_names, vec!["repo-read".to_string()]);
        assert_eq!(rendered.selected_allowed_tools, vec!["local/read_file".to_string()]);
    }
}
