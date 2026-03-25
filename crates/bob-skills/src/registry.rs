//! # Skill Registry
//!
//! High-level orchestration combining skill loading, selection, and prompt
//! composition into a single convenient API.
//!
//! The registry owns a [`SkillStore`] and delegates to [`SkillSelector`] and
//! [`PromptComposer`] to provide the full skill-to-prompt pipeline.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use bob_skills::{SkillRegistry, SkillSource};
//!
//! let registry = SkillRegistry::from_sources(vec![
//!     SkillSource::new("./skills".into(), true),
//! ]);
//!
//! let rendered = registry.render_for_input("review this code", None);
//! println!("{}", rendered.prompt);
//! ```

use tracing::{debug, instrument};

use crate::{
    compose::{PromptComposer, PromptFormat},
    loader::FsSkillStore,
    selector::SkillSelector,
    store::SkillStoreSync,
    types::{RenderedSkills, SkillEntry, SkillPolicy},
};

/// High-level skill registry that orchestrates loading, selection, and composition.
#[derive(Debug)]
pub struct SkillRegistry {
    store: FsSkillStore,
    selector: SkillSelector,
    composer: PromptComposer,
}

impl SkillRegistry {
    /// Create a registry with default settings (max 3 skills, XML format).
    #[must_use]
    pub fn new(store: FsSkillStore) -> Self {
        Self { store, selector: SkillSelector::default(), composer: PromptComposer::new() }
    }

    /// Set the maximum number of skills to select.
    #[must_use]
    pub fn with_max_selected(mut self, max: usize) -> Self {
        self.selector = SkillSelector::new(max);
        self
    }

    /// Set the prompt output format.
    #[must_use]
    pub fn with_format(mut self, format: PromptFormat) -> Self {
        self.composer = PromptComposer::with_format(format);
        self
    }

    /// Set the prompt composer directly.
    #[must_use]
    pub fn with_composer(mut self, composer: PromptComposer) -> Self {
        self.composer = composer;
        self
    }

    /// Reload skills from the underlying store.
    pub fn reload(&self) -> Result<(), crate::error::SkillError> {
        self.store.reload()
    }

    /// Get all loaded skills.
    pub fn all_skills(&self) -> Result<Vec<SkillEntry>, crate::error::SkillError> {
        self.store.load_all()
    }

    /// List skill names.
    pub fn list_names(&self) -> Result<Vec<String>, crate::error::SkillError> {
        self.store.list_names()
    }

    /// Find a skill by name.
    pub fn find(&self, name: &str) -> Result<Option<SkillEntry>, crate::error::SkillError> {
        self.store.find(name)
    }

    /// Select skills for the given user input.
    pub fn select(&self, input: &str) -> Result<Vec<SkillEntry>, crate::error::SkillError> {
        self.select_with_policy(input, &SkillPolicy::default())
    }

    /// Select skills with a tool policy.
    pub fn select_with_policy(
        &self,
        input: &str,
        policy: &SkillPolicy,
    ) -> Result<Vec<SkillEntry>, crate::error::SkillError> {
        let all = self.store.load_all()?;
        let selected = self.selector.select_with_policy(&all, input, policy);
        Ok(selected.into_iter().cloned().collect())
    }

    /// Render a prompt for the given user input.
    #[instrument(level = "debug", skip(self, input, policy))]
    pub fn render_for_input(
        &self,
        input: &str,
        policy: Option<&SkillPolicy>,
    ) -> Result<RenderedSkills, crate::error::SkillError> {
        let default_policy = SkillPolicy::default();
        let policy = policy.unwrap_or(&default_policy);
        let all = self.store.load_all()?;
        let selected = self.selector.select_with_policy(&all, input, policy);
        let bundle = self.composer.render_bundle(&selected, policy);
        debug!(
            skills = ?bundle.selected_names,
            tools = ?bundle.allowed_tools,
            "rendered skills prompt"
        );
        Ok(bundle)
    }

    /// Render skills from a pre-loaded slice (avoids filesystem access).
    #[must_use]
    pub fn render_from_entries(
        &self,
        entries: &[SkillEntry],
        input: &str,
        policy: &SkillPolicy,
    ) -> RenderedSkills {
        let selected = self.selector.select_with_policy(entries, input, policy);
        self.composer.render_bundle(&selected, policy)
    }

    /// Get a reference to the underlying store.
    #[must_use]
    pub fn store(&self) -> &FsSkillStore {
        &self.store
    }

    /// Get the number of loaded skills.
    pub fn count(&self) -> Result<usize, crate::error::SkillError> {
        self.store.count()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::loader::FsSkillStore;

    fn write_skill(root: &Path, name: &str, description: &str, body: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\n{body}\n"),
        )
        .unwrap();
    }

    use std::path::Path;

    #[test]
    fn registry_full_pipeline() {
        let temp = TempDir::new().unwrap();
        write_skill(temp.path(), "rust-review", "Review Rust code.", "Check panics.");
        write_skill(temp.path(), "sql-tuning", "Tune SQL queries.", "Inspect indexes.");

        let store = FsSkillStore::from_path(temp.path().to_path_buf());
        let registry = SkillRegistry::new(store);

        assert_eq!(registry.count().unwrap(), 2);

        let names = registry.list_names().unwrap();
        assert_eq!(names, vec!["rust-review", "sql-tuning"]);

        let rendered = registry.render_for_input("do rust-review on this", None).unwrap();
        assert_eq!(rendered.selected_names, vec!["rust-review"]);
        assert!(rendered.prompt.contains("rust-review"));
    }

    #[test]
    fn registry_select_returns_entries() {
        let temp = TempDir::new().unwrap();
        write_skill(temp.path(), "alpha", "First.", "Body A.");
        write_skill(temp.path(), "beta", "Second.", "Body B.");

        let store = FsSkillStore::from_path(temp.path().to_path_buf());
        let registry = SkillRegistry::new(store).with_max_selected(1);

        let selected = registry.select("alpha").unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name(), "alpha");
    }

    #[test]
    fn registry_find_by_name() {
        let temp = TempDir::new().unwrap();
        write_skill(temp.path(), "findme", "Find me.", "Body.");

        let store = FsSkillStore::from_path(temp.path().to_path_buf());
        let registry = SkillRegistry::new(store);

        let found = registry.find("findme").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name(), "findme");

        let missing = registry.find("nope").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn registry_with_markdown_format() {
        let temp = TempDir::new().unwrap();
        write_skill(temp.path(), "md-test", "Markdown test.", "Body.");

        let store = FsSkillStore::from_path(temp.path().to_path_buf());
        let registry = SkillRegistry::new(store).with_format(PromptFormat::Markdown);

        let rendered = registry.render_for_input("md-test", None).unwrap();
        assert!(rendered.prompt.contains("| Skill |"));
    }

    #[test]
    fn registry_render_from_entries() {
        let temp = TempDir::new().unwrap();
        write_skill(temp.path(), "cached", "Cached skill.", "Body.");

        let store = FsSkillStore::from_path(temp.path().to_path_buf());
        let entries = store.load_all().unwrap();

        let registry = SkillRegistry::new(FsSkillStore::new(vec![]));
        let rendered = registry.render_from_entries(&entries, "cached", &SkillPolicy::default());
        assert_eq!(rendered.selected_names, vec!["cached"]);
    }
}
