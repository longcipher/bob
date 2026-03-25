//! Allowed tools list for pre-approved tool access.

/// A list of tools that a skill is allowed to use.
///
/// Tools are specified as a space-separated string, such as "Bash(git:*) Read Write".
///
/// # Examples
///
/// ```
/// use bob_skills::parsing::AllowedTools;
///
/// let tools = AllowedTools::new("Bash(git:*) Read Write");
/// assert_eq!(tools.len(), 3);
/// assert!(tools.iter().any(|t| t == "Bash(git:*)"));
/// assert!(tools.iter().any(|t| t == "Read"));
/// assert!(tools.iter().any(|t| t == "Write"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AllowedTools(Vec<String>);

impl AllowedTools {
    /// Creates a new allowed tools list from a space-separated string.
    ///
    /// Empty strings and whitespace-only input produce an empty list.
    #[must_use]
    pub fn new(tools: &str) -> Self {
        let list: Vec<String> =
            tools.split_whitespace().filter(|s| !s.is_empty()).map(String::from).collect();
        Self(list)
    }

    /// Creates from a pre-split list of tool names.
    #[must_use]
    pub fn from_list(tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(tools.into_iter().map(Into::into).collect())
    }

    /// Returns an iterator over the tool names.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// Returns the number of tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if there are no tools.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for AllowedTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.join(" "))
    }
}

impl IntoIterator for AllowedTools {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_space_separated_tools() {
        let tools = AllowedTools::new("Bash(git:*) Read Write");
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().any(|t| t == "Bash(git:*)"));
        assert!(tools.iter().any(|t| t == "Read"));
        assert!(tools.iter().any(|t| t == "Write"));
    }

    #[test]
    fn empty_string_produces_empty_list() {
        let tools = AllowedTools::new("");
        assert!(tools.is_empty());
    }

    #[test]
    fn whitespace_only_produces_empty_list() {
        let tools = AllowedTools::new("   \t\n  ");
        assert!(tools.is_empty());
    }

    #[test]
    fn from_list_works() {
        let tools = AllowedTools::from_list(["Read", "Write"]);
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn display_formats_correctly() {
        let tools = AllowedTools::new("Read Write");
        assert_eq!(tools.to_string(), "Read Write");
    }
}
