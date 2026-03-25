//! Error types for skill parsing and loading.

use super::{CompatibilityError, SkillDescriptionError, SkillNameError};

/// Errors that can occur when parsing a SKILL.md file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The content does not start with a frontmatter delimiter.
    #[error("missing frontmatter delimiter (must start with ---)")]
    MissingFrontmatter,

    /// The frontmatter delimiter is not closed.
    #[error("unterminated frontmatter (missing closing ---)")]
    UnterminatedFrontmatter,

    /// A required field is missing.
    #[error("missing required field: {field}")]
    MissingField { field: &'static str },

    /// The YAML is invalid.
    #[error("invalid YAML: {message}")]
    InvalidYaml { message: String },

    /// The skill name is invalid.
    #[error("invalid skill name: {0}")]
    InvalidName(#[from] SkillNameError),

    /// The skill description is invalid.
    #[error("invalid skill description: {0}")]
    InvalidDescription(#[from] SkillDescriptionError),

    /// The compatibility string is invalid.
    #[error("invalid compatibility: {0}")]
    InvalidCompatibility(#[from] CompatibilityError),
}

/// Errors that can occur when loading a skill from a directory.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoadError {
    /// The directory does not exist.
    #[error("skill directory not found: {path}")]
    DirectoryNotFound { path: String },

    /// The SKILL.md file does not exist.
    #[error("SKILL.md not found in: {path}")]
    SkillFileNotFound { path: String },

    /// The skill name does not match the directory name.
    #[error(
        "skill name mismatch: directory is '{directory_name}' but skill declares '{skill_name}'"
    )]
    NameMismatch { directory_name: String, skill_name: String },

    /// A file was not found.
    #[error("file not found: {path}")]
    FileNotFound { path: String },

    /// An I/O error occurred.
    #[error("I/O error on '{path}': {message}")]
    IoError { path: String, kind: std::io::ErrorKind, message: String },

    /// A parse error occurred.
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
}

impl From<std::io::Error> for LoadError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError { path: String::new(), kind: e.kind(), message: e.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_error_display() {
        assert_eq!(
            ParseError::MissingFrontmatter.to_string(),
            "missing frontmatter delimiter (must start with ---)"
        );
        assert_eq!(
            ParseError::MissingField { field: "name" }.to_string(),
            "missing required field: name"
        );
    }

    #[test]
    fn load_error_display() {
        let err = LoadError::DirectoryNotFound { path: "/tmp/skills".into() };
        assert!(err.to_string().contains("/tmp/skills"));

        let err = LoadError::NameMismatch {
            directory_name: "my-skill".into(),
            skill_name: "other-name".into(),
        };
        assert!(err.to_string().contains("my-skill"));
        assert!(err.to_string().contains("other-name"));
    }
}
