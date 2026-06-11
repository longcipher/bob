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

#[cfg(test)]
mod proptests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn tools_match_is_reflexive(tool in "[a-z_/]{1,30}") {
            prop_assert!(tools_match(&tool, &tool));
        }

        #[test]
        fn tools_match_is_symmetric(a in "[a-z_/]{1,30}", b in "[a-z_/]{1,30}") {
            prop_assert_eq!(tools_match(&a, &b), tools_match(&b, &a));
        }

        #[test]
        fn tools_match_is_case_insensitive_prop(tool in "[a-z_/]{1,30}") {
            let upper = tool.to_uppercase();
            prop_assert!(tools_match(&tool, &upper));
        }

        #[test]
        fn normalize_tool_list_never_contains_empty(tools in prop::collection::vec("[a-z_]{0,20}", 0..20)) {
            let normalized = normalize_tool_list(tools.iter().map(String::as_str));
            prop_assert!(!normalized.iter().any(String::is_empty));
        }

        #[test]
        fn normalize_tool_list_deduplicates(tools in prop::collection::vec("[a-z_]{1,10}", 0..20)) {
            let normalized = normalize_tool_list(tools.iter().map(String::as_str));
            // Check that no two elements match
            for i in 0..normalized.len() {
                for j in (i+1)..normalized.len() {
                    prop_assert!(!tools_match(&normalized[i], &normalized[j]),
                        "found duplicates: {} and {}", normalized[i], normalized[j]);
                }
            }
        }

        #[test]
        fn normalize_tool_list_is_sorted(tools in prop::collection::vec("[a-z_]{1,10}", 0..20)) {
            let normalized = normalize_tool_list(tools.iter().map(String::as_str));
            for window in normalized.windows(2) {
                prop_assert!(tool_key(&window[0]) <= tool_key(&window[1]),
                    "not sorted: {} > {}", window[0], window[1]);
            }
        }

        #[test]
        fn intersect_is_subset_of_lhs(lhs in prop::collection::vec("[a-z_]{1,10}", 0..10), rhs in prop::collection::vec("[a-z_]{1,10}", 0..10)) {
            let result = intersect_allowlists(&lhs, &rhs);
            for item in &result {
                prop_assert!(lhs.iter().any(|l| tools_match(l, item)),
                    "result item {} not in lhs", item);
            }
        }

        #[test]
        fn intersect_is_subset_of_rhs(lhs in prop::collection::vec("[a-z_]{1,10}", 0..10), rhs in prop::collection::vec("[a-z_]{1,10}", 0..10)) {
            let result = intersect_allowlists(&lhs, &rhs);
            for item in &result {
                prop_assert!(rhs.iter().any(|r| tools_match(r, item)),
                    "result item {} not in rhs", item);
            }
        }

        #[test]
        fn is_tool_allowed_deny_overrides_allow(tool in "[a-z_]{1,10}") {
            let deny = vec![tool.clone()];
            let allow = vec![tool.clone()];
            prop_assert!(!is_tool_allowed(&tool, &deny, Some(&allow)));
        }

        #[test]
        fn is_tool_allowed_no_allow_list_allows(tool in "[a-z_]{1,10}") {
            let deny: Vec<String> = vec![];
            prop_assert!(is_tool_allowed(&tool, &deny, None));
        }

        #[test]
        fn merge_both_none_returns_none(a in prop::option::of(prop::collection::vec("[a-z_]{1,10}", 0..5)), b in prop::option::of(prop::collection::vec("[a-z_]{1,10}", 0..5))) {
            // Only test when both are None
            if a.is_none() && b.is_none() {
                prop_assert!(merge_allowlists(None, None).is_none());
            }
        }
    }
}
