//! Tool policy helpers shared across runtime and adapter layers.
//!
//! These helpers centralize tool-name matching and allow/deny evaluation so
//! all crates apply the same policy semantics.

/// Returns `true` when two tool identifiers refer to the same logical tool.
///
/// Matching is case-insensitive and ignores trailing signature-like suffixes
/// such as `tool(arg)` by comparing only the canonical key.
#[must_use]
pub fn tools_match(lhs: &str, rhs: &str) -> bool {
    let lhs_lower = lhs.to_ascii_lowercase();
    let rhs_lower = rhs.to_ascii_lowercase();
    lhs_lower == rhs_lower || tool_key(lhs) == tool_key(rhs)
}

/// Normalizes a list of tool identifiers:
/// - trims whitespace
/// - drops empty entries
/// - deduplicates by [`tools_match`]
/// - sorts by canonical key for deterministic behavior
#[must_use]
pub fn normalize_tool_list<I, S>(tools: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut normalized: Vec<String> = Vec::new();
    for raw in tools {
        let candidate = raw.as_ref().trim();
        if candidate.is_empty() {
            continue;
        }
        if normalized.iter().any(|existing| tools_match(existing, candidate)) {
            continue;
        }
        normalized.push(candidate.to_string());
    }
    normalized.sort_by_key(|tool| tool_key(tool));
    normalized
}

/// Computes the intersection of two allowlists using [`tools_match`] semantics.
#[must_use]
pub fn intersect_allowlists(lhs: &[String], rhs: &[String]) -> Vec<String> {
    let lhs = normalize_tool_list(lhs.iter().map(String::as_str));
    let rhs = normalize_tool_list(rhs.iter().map(String::as_str));
    normalize_tool_list(
        lhs.iter()
            .filter(|lhs_tool| rhs.iter().any(|rhs_tool| tools_match(lhs_tool, rhs_tool)))
            .map(String::as_str),
    )
}

/// Resolves two optional allowlists.
///
/// - both present  => intersection
/// - one present   => normalized copy of that list
/// - both absent   => `None`
#[must_use]
pub fn merge_allowlists(lhs: Option<&[String]>, rhs: Option<&[String]>) -> Option<Vec<String>> {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => Some(intersect_allowlists(lhs, rhs)),
        (Some(lhs), None) => Some(normalize_tool_list(lhs.iter().map(String::as_str))),
        (None, Some(rhs)) => Some(normalize_tool_list(rhs.iter().map(String::as_str))),
        (None, None) => None,
    }
}

/// Returns `true` when `tool` is permitted by deny/allow policy lists.
#[must_use]
pub fn is_tool_allowed(tool: &str, deny_tools: &[String], allow_tools: Option<&[String]>) -> bool {
    if deny_tools.iter().any(|deny| tools_match(deny, tool)) {
        return false;
    }

    allow_tools.is_none_or(|allow| allow.iter().any(|entry| tools_match(entry, tool)))
}

/// Canonical key used for deterministic matching and sorting.
fn tool_key(tool: &str) -> String {
    let lower = tool.to_ascii_lowercase();
    lower.split_once('(').map_or_else(|| lower.clone(), |(prefix, _)| prefix.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_match_is_case_insensitive() {
        assert!(tools_match("LOCAL/READ_FILE", "local/read_file"));
    }

    #[test]
    fn tools_match_ignores_signature_suffix() {
        assert!(tools_match("mcp/fs/read_file(path)", "mcp/fs/read_file"));
    }

    #[test]
    fn normalize_tool_list_deduplicates_equivalent_entries() {
        let normalized =
            normalize_tool_list(["local/read_file", "LOCAL/READ_FILE", " local/read_file() "]);
        assert_eq!(normalized.len(), 1);
    }

    #[test]
    fn merge_allowlists_intersects_when_both_present() {
        let lhs = vec!["local/read_file".to_string(), "local/write_file".to_string()];
        let rhs = vec!["local/read_file".to_string()];
        let merged = merge_allowlists(Some(&lhs), Some(&rhs));
        assert!(merged.is_some(), "merge should produce intersection");
        assert_eq!(merged.unwrap_or_default(), vec!["local/read_file".to_string()]);
    }

    #[test]
    fn deny_list_takes_precedence_over_allow_list() {
        let deny = vec!["local/shell_exec".to_string()];
        let allow = vec!["local/shell_exec".to_string(), "local/read_file".to_string()];
        assert!(!is_tool_allowed("local/shell_exec", &deny, Some(&allow)));
        assert!(is_tool_allowed("local/read_file", &deny, Some(&allow)));
    }
}
