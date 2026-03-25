//! Compatibility string validation.

/// A validated compatibility requirement.
///
/// Compatibility strings describe environment requirements for a skill,
/// such as "Requires docker" or "Linux only".
///
/// # Examples
///
/// ```
/// use bob_skills::parsing::Compatibility;
///
/// let compat = Compatibility::new("Requires docker").unwrap();
/// assert_eq!(compat.as_str(), "Requires docker");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Compatibility(String);

impl Compatibility {
    /// Creates a new compatibility string after validation.
    ///
    /// # Errors
    ///
    /// Returns `CompatibilityError` if the string is invalid.
    pub fn new(compatibility: impl Into<String>) -> Result<Self, CompatibilityError> {
        let compatibility = compatibility.into();
        validate_compatibility(&compatibility)?;
        Ok(Self(compatibility))
    }

    /// Returns the compatibility string as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Compatibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Compatibility {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Errors that can occur when validating a compatibility string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompatibilityError {
    /// Compatibility string is empty or whitespace-only.
    #[error("compatibility string cannot be empty")]
    Empty,

    /// Compatibility string is too long.
    #[error("compatibility string cannot exceed 256 characters (got {length})")]
    TooLong { length: usize },
}

fn validate_compatibility(compatibility: &str) -> Result<(), CompatibilityError> {
    let trimmed = compatibility.trim();

    if trimmed.is_empty() {
        return Err(CompatibilityError::Empty);
    }

    if compatibility.len() > 256 {
        return Err(CompatibilityError::TooLong { length: compatibility.len() });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_compatibility_accepted() {
        assert!(Compatibility::new("Requires docker").is_ok());
        assert!(Compatibility::new("Linux only").is_ok());
        assert!(Compatibility::new("Requires poppler-utils >= 0.90").is_ok());
    }

    #[test]
    fn empty_rejected() {
        assert_eq!(Compatibility::new(""), Err(CompatibilityError::Empty));
    }

    #[test]
    fn whitespace_only_rejected() {
        assert_eq!(Compatibility::new("   "), Err(CompatibilityError::Empty));
    }

    #[test]
    fn too_long_rejected() {
        let long_compat = "a".repeat(257);
        assert!(matches!(
            Compatibility::new(long_compat),
            Err(CompatibilityError::TooLong { length: 257 })
        ));
    }
}
