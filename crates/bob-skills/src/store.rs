//! # Skill Store Port
//!
//! Async trait for skill persistence and retrieval in the hexagonal architecture.

use async_trait::async_trait;

use crate::{error::SkillError, types::SkillEntry};

/// Port for skill storage and retrieval.
///
/// This trait abstracts the storage backend for skills, allowing different
/// implementations (filesystem, in-memory, database, etc.) to be swapped
/// via dependency injection.
#[async_trait]
pub trait SkillStore: Send + Sync {
    /// Load all skills from the store.
    async fn load_all(&self) -> Result<Vec<SkillEntry>, SkillError>;

    /// Find a skill by name.
    async fn find(&self, name: &str) -> Result<Option<SkillEntry>, SkillError>;

    /// List skill names.
    async fn list_names(&self) -> Result<Vec<String>, SkillError>;

    /// Count skills in the store.
    async fn count(&self) -> Result<usize, SkillError>;
}

/// A synchronous skill store for contexts where async is not available.
pub trait SkillStoreSync: Send + Sync {
    /// Load all skills from the store.
    fn load_all(&self) -> Result<Vec<SkillEntry>, SkillError>;

    /// Find a skill by name.
    fn find(&self, name: &str) -> Result<Option<SkillEntry>, SkillError>;

    /// List skill names.
    fn list_names(&self) -> Result<Vec<String>, SkillError>;

    /// Count skills in the store.
    fn count(&self) -> Result<usize, SkillError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing;

    struct MockStore {
        entries: Vec<SkillEntry>,
    }

    impl SkillStoreSync for MockStore {
        fn load_all(&self) -> Result<Vec<SkillEntry>, SkillError> {
            Ok(self.entries.clone())
        }

        fn find(&self, name: &str) -> Result<Option<SkillEntry>, SkillError> {
            Ok(self.entries.iter().find(|e| e.name() == name).cloned())
        }

        fn list_names(&self) -> Result<Vec<String>, SkillError> {
            Ok(self.entries.iter().map(|e| e.name().to_string()).collect())
        }

        fn count(&self) -> Result<usize, SkillError> {
            Ok(self.entries.len())
        }
    }

    #[test]
    fn mock_store_operations() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("test-skill");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: test-skill\ndescription: A test.\n---\n\nBody.\n",
        )
        .unwrap();

        let skill_dir = parsing::SkillDirectory::load(&dir).unwrap();
        let entry = SkillEntry::from_skill(skill_dir.skill().clone(), dir);

        let store = MockStore { entries: vec![entry] };

        assert_eq!(store.count().unwrap(), 1);
        assert_eq!(store.list_names().unwrap(), vec!["test-skill"]);
        assert!(store.find("test-skill").unwrap().is_some());
        assert!(store.find("nonexistent").unwrap().is_none());
    }
}
