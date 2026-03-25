//! Skill description validation.

/// A validated skill description.
///
/// Descriptions must:
/// - Be 1-512 characters long
/// - Not be empty or whitespace-only
///
/// # Examples
///
/// ```
/// use bob_skills::parsing::SkillDescription;
///
/// let desc = SkillDescription::new("Does something useful.").unwrap();
/// assert_eq!(desc.as_str(), "Does something useful.");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillDescription(String);

impl SkillDescription {
    /// Creates a new skill description after validation.
    ///
    /// # Errors
    ///
    /// Returns `SkillDescriptionError` if the description is invalid.
    pub fn new(description: impl Into<String>) -> Result<Self, SkillDescriptionError> {
        let description = description.into();
        validate_description(&description)?;
        Ok(Self(description))
    }

    /// Returns the description as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SkillDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SkillDescription {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Errors that can occur when validating a skill description.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillDescriptionError {
    /// Description is empty or whitespace-only.
    #[error("skill description cannot be empty")]
    Empty,

    /// Description is too long.
    #[error("skill description cannot exceed 512 characters (got {length})")]
    TooLong { length: usize },
}

fn validate_description(description: &str) -> Result<(), SkillDescriptionError> {
    let trimmed = description.trim();

    if trimmed.is_empty() {
        return Err(SkillDescriptionError::Empty);
    }

    if description.len() > 512 {
        return Err(SkillDescriptionError::TooLong { length: description.len() });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_descriptions_accepted() {
        assert!(SkillDescription::new("Short.").is_ok());
        assert!(SkillDescription::new("A longer description with more details.").is_ok());
    }

    #[test]
    fn empty_rejected() {
        assert_eq!(SkillDescription::new(""), Err(SkillDescriptionError::Empty));
    }

    #[test]
    fn whitespace_only_rejected() {
        assert_eq!(SkillDescription::new("   "), Err(SkillDescriptionError::Empty));
        assert_eq!(SkillDescription::new("\t\n"), Err(SkillDescriptionError::Empty));
    }

    #[test]
    fn too_long_rejected() {
        let long_desc = "a".repeat(513);
        assert!(matches!(
            SkillDescription::new(long_desc),
            Err(SkillDescriptionError::TooLong { length: 513 })
        ));
    }
}
