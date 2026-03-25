//! Skill name validation.

/// A validated skill name.
///
/// Skill names must:
/// - Be 1-64 characters long
/// - Contain only lowercase letters, digits, and hyphens
/// - Start and end with a letter or digit
/// - Not have consecutive hyphens
///
/// # Examples
///
/// ```
/// use bob_skills::parsing::SkillName;
///
/// let name = SkillName::new("my-skill").unwrap();
/// assert_eq!(name.as_str(), "my-skill");
///
/// assert!(SkillName::new("Invalid-Name").is_err()); // uppercase
/// assert!(SkillName::new("-starts-with-hyphen").is_err());
/// assert!(SkillName::new("ends-with-hyphen-").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SkillName(String);

impl SkillName {
    /// Creates a new skill name after validation.
    ///
    /// # Errors
    ///
    /// Returns `SkillNameError` if the name is invalid.
    pub fn new(name: impl Into<String>) -> Result<Self, SkillNameError> {
        let name = name.into();
        validate_skill_name(&name)?;
        Ok(Self(name))
    }

    /// Returns the name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SkillName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SkillName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Errors that can occur when validating a skill name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillNameError {
    /// Name is empty.
    #[error("skill name cannot be empty")]
    Empty,

    /// Name is too long.
    #[error("skill name cannot exceed 64 characters (got {length})")]
    TooLong { length: usize },

    /// Name contains invalid characters.
    #[error("skill name can only contain lowercase letters, digits, and hyphens")]
    InvalidCharacters,

    /// Name starts with a hyphen.
    #[error("skill name must start with a letter or digit")]
    StartsWithHyphen,

    /// Name ends with a hyphen.
    #[error("skill name must end with a letter or digit")]
    EndsWithHyphen,

    /// Name contains consecutive hyphens.
    #[error("skill name cannot contain consecutive hyphens")]
    ConsecutiveHyphens,
}

fn validate_skill_name(name: &str) -> Result<(), SkillNameError> {
    if name.is_empty() {
        return Err(SkillNameError::Empty);
    }

    if name.len() > 64 {
        return Err(SkillNameError::TooLong { length: name.len() });
    }

    if name.starts_with('-') {
        return Err(SkillNameError::StartsWithHyphen);
    }

    if name.ends_with('-') {
        return Err(SkillNameError::EndsWithHyphen);
    }

    if name.contains("--") {
        return Err(SkillNameError::ConsecutiveHyphens);
    }

    if !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(SkillNameError::InvalidCharacters);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names_accepted() {
        assert!(SkillName::new("skill").is_ok());
        assert!(SkillName::new("my-skill").is_ok());
        assert!(SkillName::new("skill-123").is_ok());
        assert!(SkillName::new("a").is_ok());
        assert!(SkillName::new("1").is_ok());
    }

    #[test]
    fn empty_rejected() {
        assert_eq!(SkillName::new(""), Err(SkillNameError::Empty));
    }

    #[test]
    fn too_long_rejected() {
        let long_name = "a".repeat(65);
        assert!(matches!(SkillName::new(long_name), Err(SkillNameError::TooLong { length: 65 })));
    }

    #[test]
    fn uppercase_rejected() {
        assert_eq!(SkillName::new("My-Skill"), Err(SkillNameError::InvalidCharacters));
    }

    #[test]
    fn starts_with_hyphen_rejected() {
        assert_eq!(SkillName::new("-skill"), Err(SkillNameError::StartsWithHyphen));
    }

    #[test]
    fn ends_with_hyphen_rejected() {
        assert_eq!(SkillName::new("skill-"), Err(SkillNameError::EndsWithHyphen));
    }

    #[test]
    fn consecutive_hyphens_rejected() {
        assert_eq!(SkillName::new("my--skill"), Err(SkillNameError::ConsecutiveHyphens));
    }

    #[test]
    fn special_chars_rejected() {
        assert_eq!(SkillName::new("my_skill"), Err(SkillNameError::InvalidCharacters));
        assert_eq!(SkillName::new("my.skill"), Err(SkillNameError::InvalidCharacters));
    }
}
