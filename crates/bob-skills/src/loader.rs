//! # Filesystem Skill Loader
//!
//! Discovers and loads skills from filesystem directories using the
//! [`parsing`] module for spec-compliant parsing and validation.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::RwLock,
};

use tracing::{debug, info, instrument, warn};

use crate::{
    error::SkillError,
    parsing,
    store::{SkillStore, SkillStoreSync},
    types::SkillEntry,
};

/// Configuration for a skill source directory.
#[derive(Debug, Clone)]
pub struct SkillSource {
    /// Path to the directory containing skills.
    pub path: PathBuf,
    /// Whether to scan subdirectories recursively.
    pub recursive: bool,
}

impl SkillSource {
    /// Create a new skill source.
    #[must_use]
    pub fn new(path: PathBuf, recursive: bool) -> Self {
        Self { path, recursive }
    }
}

/// Filesystem-based skill store with concurrent-safe caching.
///
/// Loads skills from one or more source directories and caches them
/// in a lock-free [`HashIndex`] for fast concurrent reads.
#[derive(Debug)]
pub struct FsSkillStore {
    sources: Vec<SkillSource>,
    cache: RwLock<HashMap<String, SkillEntry>>,
}

impl FsSkillStore {
    /// Create a new filesystem skill store from source configurations.
    #[must_use]
    pub fn new(sources: Vec<SkillSource>) -> Self {
        Self { sources, cache: RwLock::new(HashMap::new()) }
    }

    /// Create from a single directory path (non-recursive).
    #[must_use]
    pub fn from_path(path: PathBuf) -> Self {
        Self::new(vec![SkillSource::new(path, false)])
    }

    /// Create from a single directory path (with recursion control).
    #[must_use]
    pub fn from_path_recursive(path: PathBuf, recursive: bool) -> Self {
        Self::new(vec![SkillSource::new(path, recursive)])
    }

    /// Reload all skills from sources into the cache.
    ///
    /// This clears the existing cache and re-discovers all skill directories.
    pub fn reload(&self) -> Result<(), SkillError> {
        let entries = self.discover_all()?;
        let mut cache = self.cache.write().unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.clear();
        for entry in entries {
            cache.insert(entry.name().to_string(), entry);
        }
        info!(count = cache.len(), "skills reloaded");
        Ok(())
    }

    fn discover_all(&self) -> Result<Vec<SkillEntry>, SkillError> {
        let mut dirs = Vec::new();
        for source in &self.sources {
            collect_skill_dirs(&source.path, source.recursive, &mut dirs)?;
        }
        dirs.sort();
        dirs.dedup();

        let mut entries = Vec::with_capacity(dirs.len());
        for dir in dirs {
            match load_skill_dir(&dir) {
                Ok(entry) => {
                    debug!(name = entry.name(), path = %dir.display(), "skill loaded");
                    entries.push(entry);
                }
                Err(e) => {
                    warn!(path = %dir.display(), error = %e, "skipping invalid skill directory");
                }
            }
        }

        entries.sort_by(|a, b| a.name().cmp(b.name()));
        Ok(entries)
    }

    fn ensure_loaded(&self) -> Result<(), SkillError> {
        let is_empty = self
            .cache
            .read()
            .map_or_else(|poisoned| poisoned.into_inner().is_empty(), |lock| lock.is_empty());
        if is_empty {
            self.reload()?;
        }
        Ok(())
    }
}

impl SkillStoreSync for FsSkillStore {
    fn load_all(&self) -> Result<Vec<SkillEntry>, SkillError> {
        self.ensure_loaded()?;
        let cache = self.cache.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut entries: Vec<SkillEntry> = cache.values().cloned().collect();
        entries.sort_by(|a, b| a.name().cmp(b.name()));
        Ok(entries)
    }

    fn find(&self, name: &str) -> Result<Option<SkillEntry>, SkillError> {
        self.ensure_loaded()?;
        let cache = self.cache.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(cache.get(name).cloned())
    }

    fn list_names(&self) -> Result<Vec<String>, SkillError> {
        self.ensure_loaded()?;
        let cache = self.cache.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut names: Vec<String> = cache.keys().cloned().collect();
        names.sort();
        Ok(names)
    }

    fn count(&self) -> Result<usize, SkillError> {
        self.ensure_loaded()?;
        let cache = self.cache.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(cache.len())
    }
}

#[async_trait::async_trait]
impl SkillStore for FsSkillStore {
    async fn load_all(&self) -> Result<Vec<SkillEntry>, SkillError> {
        SkillStoreSync::load_all(self)
    }

    async fn find(&self, name: &str) -> Result<Option<SkillEntry>, SkillError> {
        SkillStoreSync::find(self, name)
    }

    async fn list_names(&self) -> Result<Vec<String>, SkillError> {
        SkillStoreSync::list_names(self)
    }

    async fn count(&self) -> Result<usize, SkillError> {
        SkillStoreSync::count(self)
    }
}

// ── Discovery helpers ─────────────────────────────────────────────────

/// Recursively collect directories that contain a SKILL.md file.
fn collect_skill_dirs(
    path: &Path,
    recursive: bool,
    out: &mut Vec<PathBuf>,
) -> Result<(), SkillError> {
    if !path.exists() {
        return Err(SkillError::SourceNotFound { path: path.display().to_string() });
    }

    if path.join("SKILL.md").is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }

    let read_dir = fs::read_dir(path).map_err(|e| SkillError::ReadDir {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    for entry in read_dir {
        let entry = entry.map_err(|e| SkillError::ReadDir {
            path: path.display().to_string(),
            reason: e.to_string(),
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

/// Load a single skill from a directory.
#[instrument(level = "debug", skip(dir), fields(path = %dir.display()))]
fn load_skill_dir(dir: &Path) -> Result<SkillEntry, SkillError> {
    let skill_dir = parsing::SkillDirectory::load(dir)?;
    Ok(SkillEntry::from_skill(skill_dir.skill().clone(), dir.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::store::SkillStoreSync;

    fn write_skill(root: &Path, name: &str, description: &str, body: &str) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\n{body}\n"),
        )
        .unwrap();
        dir
    }

    #[test]
    fn loads_skills_non_recursive() {
        let temp = TempDir::new().unwrap();
        write_skill(temp.path(), "alpha", "First skill.", "Body A.");
        write_skill(temp.path(), "beta", "Second skill.", "Body B.");

        let store = FsSkillStore::from_path(temp.path().to_path_buf());
        let entries = SkillStoreSync::load_all(&store).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name(), "alpha");
        assert_eq!(entries[1].name(), "beta");
    }

    #[test]
    fn loads_skills_recursive() {
        let temp = TempDir::new().unwrap();
        let nested = temp.path().join("group").join("nested-skill");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("SKILL.md"),
            "---\nname: nested-skill\ndescription: Nested.\n---\n\nBody.\n",
        )
        .unwrap();

        let store = FsSkillStore::from_path_recursive(temp.path().to_path_buf(), true);
        let entries = SkillStoreSync::load_all(&store).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name(), "nested-skill");
    }

    #[test]
    fn find_returns_correct_skill() {
        let temp = TempDir::new().unwrap();
        write_skill(temp.path(), "findme", "Find this.", "Body.");
        write_skill(temp.path(), "other", "Other skill.", "Body.");

        let store = FsSkillStore::from_path(temp.path().to_path_buf());

        let found = SkillStoreSync::find(&store, "findme").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name(), "findme");

        let missing = SkillStoreSync::find(&store, "nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn skips_invalid_skill_dirs() {
        let temp = TempDir::new().unwrap();
        write_skill(temp.path(), "good", "Good skill.", "Body.");

        // Invalid: no SKILL.md
        let bad_dir = temp.path().join("bad-dir");
        fs::create_dir_all(&bad_dir).unwrap();
        fs::write(bad_dir.join("README.md"), "not a skill").unwrap();

        let store = FsSkillStore::from_path(temp.path().to_path_buf());
        let entries = SkillStoreSync::load_all(&store).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name(), "good");
    }

    #[test]
    fn source_not_found_returns_error() {
        let store = FsSkillStore::from_path(PathBuf::from("/nonexistent/path/to/skills"));
        let result = SkillStoreSync::load_all(&store);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SkillError::SourceNotFound { .. }));
    }

    #[test]
    fn list_names_sorted() {
        let temp = TempDir::new().unwrap();
        write_skill(temp.path(), "zebra", "Last.", "Body.");
        write_skill(temp.path(), "alpha", "First.", "Body.");
        write_skill(temp.path(), "mu", "Middle.", "Body.");

        let store = FsSkillStore::from_path(temp.path().to_path_buf());
        let names = SkillStoreSync::list_names(&store).unwrap();

        assert_eq!(names, vec!["alpha", "mu", "zebra"]);
    }
}
