//! Metadata key-value pairs.

use std::collections::HashMap;

/// Arbitrary key-value metadata for a skill.
///
/// Metadata can contain any string key-value pairs, typically used for
/// tags, author, version, etc.
///
/// # Examples
///
/// ```
/// use bob_skills::parsing::Metadata;
///
/// let metadata = Metadata::from_pairs([("author", "example-org"), ("version", "1.0")]);
///
/// assert_eq!(metadata.get("author"), Some("example-org"));
/// assert_eq!(metadata.get("version"), Some("1.0"));
/// assert_eq!(metadata.get("missing"), None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Metadata(HashMap<String, String>);

impl Metadata {
    /// Creates metadata from key-value pairs.
    ///
    /// Keys are trimmed and lowercased.
    #[must_use]
    pub fn from_pairs(
        pairs: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let map: HashMap<String, String> =
            pairs.into_iter().map(|(k, v)| (k.into().trim().to_lowercase(), v.into())).collect();
        Self(map)
    }

    /// Returns the value for a key, or `None` if not present.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key.to_lowercase().as_str()).map(String::as_str)
    }

    /// Returns an iterator over all key-value pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Returns the number of key-value pairs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if there are no key-value pairs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<K, V> FromIterator<(K, V)> for Metadata
where
    K: Into<String>,
    V: Into<String>,
{
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self::from_pairs(iter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_from_pairs() {
        let metadata = Metadata::from_pairs([("author", "test"), ("version", "1.0")]);
        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata.get("author"), Some("test"));
        assert_eq!(metadata.get("version"), Some("1.0"));
    }

    #[test]
    fn keys_are_lowercased() {
        let metadata = Metadata::from_pairs([("Author", "test"), ("VERSION", "1.0")]);
        assert_eq!(metadata.get("author"), Some("test"));
        assert_eq!(metadata.get("version"), Some("1.0"));
    }

    #[test]
    fn get_missing_returns_none() {
        let metadata = Metadata::from_pairs([("key", "value")]);
        assert_eq!(metadata.get("missing"), None);
    }

    #[test]
    fn iterates_all_pairs() {
        let metadata = Metadata::from_pairs([("a", "1"), ("b", "2")]);
        let mut pairs: Vec<_> = metadata.iter().collect();
        pairs.sort();
        assert_eq!(pairs, vec![("a", "1"), ("b", "2")]);
    }

    #[test]
    fn is_empty_works() {
        let empty: Metadata = Metadata::from_pairs(Vec::<(&str, &str)>::new());
        assert!(empty.is_empty());

        let not_empty = Metadata::from_pairs([("key", "value")]);
        assert!(!not_empty.is_empty());
    }
}
