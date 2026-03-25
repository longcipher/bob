//! # Skill Error Types
//!
//! Error types for the `bob-skills` crate.

use crate::parsing;

/// Errors produced by skill operations.
#[derive(thiserror::Error, Debug)]
pub enum SkillError {
    /// Skill source directory does not exist.
    #[error("skill source not found: {path}")]
    SourceNotFound { path: String },

    /// Failed to read a directory.
    #[error("failed to read directory '{path}': {reason}")]
    ReadDir { path: String, reason: String },

    /// Failed to load a skill from a directory.
    #[error("failed to load skill from '{path}': {reason}")]
    LoadSkill { path: String, reason: String },

    /// Skill parse error from parsing module.
    #[error("skill parse error: {0}")]
    Parse(#[from] parsing::ParseError),

    /// Skill load error from parsing module.
    #[error("skill load error: {0}")]
    Load(#[from] parsing::LoadError),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Skill not found by name.
    #[error("skill not found: {name}")]
    NotFound { name: String },
}

impl SkillError {
    /// Stable machine-readable error code for monitoring and alerting.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::SourceNotFound { .. } => "BOB_SKILL_SOURCE_NOT_FOUND",
            Self::ReadDir { .. } => "BOB_SKILL_READ_DIR",
            Self::LoadSkill { .. } => "BOB_SKILL_LOAD",
            Self::Parse(_) => "BOB_SKILL_PARSE",
            Self::Load(_) => "BOB_SKILL_LOAD_DIR",
            Self::Io(_) => "BOB_SKILL_IO",
            Self::NotFound { .. } => "BOB_SKILL_NOT_FOUND",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(
            SkillError::SourceNotFound { path: "x".into() }.code(),
            "BOB_SKILL_SOURCE_NOT_FOUND"
        );
        assert_eq!(SkillError::NotFound { name: "x".into() }.code(), "BOB_SKILL_NOT_FOUND");
        assert_eq!(
            SkillError::ReadDir { path: "x".into(), reason: "y".into() }.code(),
            "BOB_SKILL_READ_DIR"
        );
        assert_eq!(
            SkillError::LoadSkill { path: "x".into(), reason: "y".into() }.code(),
            "BOB_SKILL_LOAD"
        );
        assert_eq!(
            SkillError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "x")).code(),
            "BOB_SKILL_IO"
        );
    }

    #[test]
    fn error_display_includes_context() {
        let err = SkillError::SourceNotFound { path: "/tmp/skills".into() };
        assert!(err.to_string().contains("/tmp/skills"));

        let err = SkillError::NotFound { name: "pdf-tool".into() };
        assert!(err.to_string().contains("pdf-tool"));
    }
}
