//! # Skill Domain Types
//!
//! Core domain types for skill management in the Bob Agent Framework.
//!
//! [`SkillEntry`] is the central type that wraps a [`Skill`](crate::parsing::Skill) with
//! bob-specific metadata such as the source directory path and pre-computed
//! tag/tool vectors for efficient selection and filtering.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::parsing;

/// A loaded skill enriched with bob-specific metadata.
///
/// This is the primary type used throughout the bob-skills crate. It wraps the
/// parsed [`parsing::Skill`] and adds the filesystem context and
/// pre-computed vectors needed for selection and prompt composition.
#[derive(Debug, Clone)]
pub struct SkillEntry {
    /// The parsed skill from the parsing module.
    skill: parsing::Skill,
    /// Absolute path to the skill's root directory.
    source_dir: PathBuf,
    /// Pre-computed tags extracted from metadata (lowercased, split).
    tags: Vec<String>,
    /// Pre-computed allowed tools list.
    allowed_tools: Vec<String>,
}

impl SkillEntry {
    /// Create a `SkillEntry` from a parsed skill and its source directory.
    pub fn from_skill(skill: parsing::Skill, source_dir: PathBuf) -> Self {
        let tags = extract_tags(&skill);
        let allowed_tools = extract_allowed_tools(&skill);
        Self { skill, source_dir, tags, allowed_tools }
    }

    /// The skill name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.skill.name().as_str()
    }

    /// The skill description.
    #[must_use]
    pub fn description(&self) -> &str {
        self.skill.description().as_str()
    }

    /// The trimmed instruction body.
    #[must_use]
    pub fn body(&self) -> &str {
        self.skill.body_trimmed()
    }

    /// The full raw body (including leading/trailing whitespace).
    #[must_use]
    pub fn body_raw(&self) -> &str {
        self.skill.body()
    }

    /// Reference to the underlying parsed skill.
    #[must_use]
    pub fn skill(&self) -> &parsing::Skill {
        &self.skill
    }

    /// The frontmatter of the skill.
    #[must_use]
    pub fn frontmatter(&self) -> &parsing::Frontmatter {
        self.skill.frontmatter()
    }

    /// Source directory path.
    #[must_use]
    pub fn source_dir(&self) -> &Path {
        &self.source_dir
    }

    /// Pre-computed tags (lowercased, from metadata).
    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Pre-computed allowed tools list.
    #[must_use]
    pub fn allowed_tools(&self) -> &[String] {
        &self.allowed_tools
    }

    /// Path to the SKILL.md file.
    #[must_use]
    pub fn skill_md_path(&self) -> PathBuf {
        self.source_dir.join("SKILL.md")
    }

    /// Whether the skill has a scripts/ subdirectory.
    #[must_use]
    pub fn has_scripts(&self) -> bool {
        self.source_dir.join("scripts").is_dir()
    }

    /// Whether the skill has a references/ subdirectory.
    #[must_use]
    pub fn has_references(&self) -> bool {
        self.source_dir.join("references").is_dir()
    }

    /// Whether the skill has an assets/ subdirectory.
    #[must_use]
    pub fn has_assets(&self) -> bool {
        self.source_dir.join("assets").is_dir()
    }

    /// Read a file from the skill directory by relative path.
    ///
    /// Returns `None` if the file does not exist.
    pub fn read_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>, std::io::Error> {
        let path = self.source_dir.join(rel_path);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Read a file from the skill directory as a string.
    ///
    /// Returns `None` if the file does not exist.
    pub fn read_file_string(&self, rel_path: &str) -> Result<Option<String>, std::io::Error> {
        let path = self.source_dir.join(rel_path);
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Metadata summary for a skill (lightweight, used in listings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSummary {
    /// Skill name.
    pub name: String,
    /// Skill description.
    pub description: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Allowed tools.
    pub allowed_tools: Vec<String>,
    /// Source directory path (string form).
    pub source_dir: String,
    /// License (if specified).
    pub license: Option<String>,
    /// Compatibility (if specified).
    pub compatibility: Option<String>,
}

impl SkillSummary {
    /// Build a summary from a [`SkillEntry`].
    #[must_use]
    pub fn from_entry(entry: &SkillEntry) -> Self {
        Self {
            name: entry.name().to_string(),
            description: entry.description().to_string(),
            tags: entry.tags().to_vec(),
            allowed_tools: entry.allowed_tools().to_vec(),
            source_dir: entry.source_dir().display().to_string(),
            license: entry.frontmatter().license().map(ToString::to_string),
            compatibility: entry.frontmatter().compatibility().map(ToString::to_string),
        }
    }
}

/// Tool policy snapshot for skill selection and rendering.
#[derive(Debug, Clone, Default)]
pub struct SkillPolicy {
    /// Denied tool names.
    pub deny_tools: Vec<String>,
    /// Optional allow list. `None` means allow all not denied.
    pub allow_tools: Option<Vec<String>>,
    /// Token budget for rendered prompt output.
    pub token_budget: usize,
}

impl SkillPolicy {
    /// Create a policy with default token budget (1800 tokens).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the deny list.
    #[must_use]
    pub fn with_deny_tools(mut self, deny: Vec<String>) -> Self {
        self.deny_tools = deny;
        self
    }

    /// Set the allow list.
    #[must_use]
    pub fn with_allow_tools(mut self, allow: Vec<String>) -> Self {
        self.allow_tools = Some(allow);
        self
    }

    /// Set the token budget.
    #[must_use]
    pub fn with_token_budget(mut self, budget: usize) -> Self {
        self.token_budget = budget;
        self
    }
}

/// Result of skill selection and prompt rendering.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RenderedSkills {
    /// The rendered prompt text.
    pub prompt: String,
    /// Names of selected skills.
    pub selected_names: Vec<String>,
    /// Effective allowed tools after policy filtering.
    pub allowed_tools: Vec<String>,
}

// ── Internal helpers ──────────────────────────────────────────────────

fn extract_tags(skill: &parsing::Skill) -> Vec<String> {
    skill
        .frontmatter()
        .metadata()
        .and_then(|meta| meta.get("tags"))
        .map(|raw| {
            raw.split(|ch: char| ch == ',' || ch.is_whitespace())
                .filter(|part| !part.is_empty())
                .map(|part| part.to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

fn extract_allowed_tools(skill: &parsing::Skill) -> Vec<String> {
    skill
        .frontmatter()
        .allowed_tools()
        .map(|tools| tools.iter().map(ToString::to_string).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn create_test_skill(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: A test skill.\nmetadata:\n  tags: test demo\n---\n\n# {name}\n\nInstructions here.\n"
            ),
        )
        .unwrap();
        dir
    }

    #[test]
    fn entry_extracts_tags_from_metadata() {
        let temp = TempDir::new().unwrap();
        let dir = create_test_skill(temp.path(), "my-skill");

        let skill_dir = parsing::SkillDirectory::load(&dir).unwrap();
        let entry = SkillEntry::from_skill(skill_dir.skill().clone(), dir);

        assert_eq!(entry.name(), "my-skill");
        assert_eq!(entry.description(), "A test skill.");
        assert_eq!(entry.tags(), &["test", "demo"]);
        assert!(entry.allowed_tools().is_empty());
    }

    #[test]
    fn entry_provides_subdir_queries() {
        let temp = TempDir::new().unwrap();
        let dir = create_test_skill(temp.path(), "with-scripts");
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(dir.join("scripts/run.sh"), "#!/bin/bash").unwrap();

        let skill_dir = parsing::SkillDirectory::load(&dir).unwrap();
        let entry = SkillEntry::from_skill(skill_dir.skill().clone(), dir);

        assert!(entry.has_scripts());
        assert!(!entry.has_references());
        assert!(!entry.has_assets());

        let content = entry.read_file_string("scripts/run.sh").unwrap();
        assert_eq!(content, Some("#!/bin/bash".to_string()));

        let missing = entry.read_file_string("nope.txt").unwrap();
        assert_eq!(missing, None);
    }

    #[test]
    fn summary_roundtrips() {
        let temp = TempDir::new().unwrap();
        let dir = create_test_skill(temp.path(), "sum-test");

        let skill_dir = parsing::SkillDirectory::load(&dir).unwrap();
        let entry = SkillEntry::from_skill(skill_dir.skill().clone(), dir.clone());
        let summary = SkillSummary::from_entry(&entry);

        assert_eq!(summary.name, "sum-test");
        assert_eq!(summary.description, "A test skill.");
        assert_eq!(summary.tags, vec!["test", "demo"]);
        assert_eq!(summary.source_dir, dir.display().to_string());
        assert!(summary.license.is_none());
    }

    #[test]
    fn policy_builder_sets_fields() {
        let policy = SkillPolicy::new()
            .with_deny_tools(vec!["Bash".into()])
            .with_allow_tools(vec!["Read".into()])
            .with_token_budget(500);

        assert_eq!(policy.deny_tools, vec!["Bash"]);
        assert_eq!(policy.allow_tools, Some(vec!["Read".to_string()]));
        assert_eq!(policy.token_budget, 500);
    }
}
