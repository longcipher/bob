//! # Secrets Management
//!
//! Utilities for safely loading and filtering environment variables
//! as agent configuration secrets.
//!
//! The primary entry point is [`load_safe_env_vars`], which returns only
//! non-sensitive environment variables. Sensitive variables (those matching
//! known credential patterns) are filtered out to prevent accidental leakage
//! into configuration files or logs.
//!
//! For intentionally loading specific credentials, use [`is_env_var_safe`]
//! to verify a variable name before loading.

// ── Blocked Patterns ─────────────────────────────────────────────────

/// Exact environment variable names that are always blocked.
const BLOCKED_ENV_VARS: &[&str] = &[
    // Cloud credentials
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "AZURE_CLIENT_SECRET",
    // CI/CD tokens
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "GITLAB_TOKEN",
    "NPM_TOKEN",
    // Docker / registry
    "DOCKER_PASSWORD",
    "REGISTRY_PASSWORD",
    // System / user info
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "HISTFILE",
    // Database URLs
    "DATABASE_URL",
    "REDIS_URL",
    "MONGODB_URI",
    "POSTGRES_PASSWORD",
    "MYSQL_ROOT_PASSWORD",
    // Generic secrets
    "JWT_SECRET",
    "SESSION_SECRET",
    "ENCRYPTION_KEY",
    "MASTER_KEY",
    "PRIVATE_KEY",
    "SECRET_KEY",
];

/// Prefixes that block any variable starting with them (case-insensitive).
const BLOCKED_ENV_PREFIXES: &[&str] =
    &["AWS_", "AZURE_", "GCP_", "GOOGLE_", "GITHUB_", "GITLAB_", "SSH_", "GPG_"];

/// Substrings that block any variable containing them (case-insensitive).
const BLOCKED_KEYWORDS: &[&str] =
    &["PASSWORD", "SECRET", "TOKEN", "KEY", "CREDENTIAL", "AUTH", "PRIVATE"];

// ── Public API ───────────────────────────────────────────────────────

/// Returns `true` if the environment variable name is considered safe
/// (i.e. does not match any blocked pattern).
///
/// Use this to verify a variable name before loading it.
#[must_use]
pub fn is_env_var_safe(name: &str) -> bool {
    !is_blocked(name)
}

/// Collect all safe environment variables into a `HashMap`.
///
/// Variables matching blocked patterns are excluded.
#[must_use]
pub fn load_safe_env_vars() -> std::collections::HashMap<String, String> {
    std::env::vars().filter(|(name, _)| !is_blocked(name)).collect()
}

/// Load a single environment variable by name if it is safe.
///
/// Returns `None` if the variable does not exist or is blocked.
#[must_use]
pub fn load_env_var_safe(name: &str) -> Option<String> {
    if is_blocked(name) {
        return None;
    }
    std::env::var(name).ok()
}

// ── Internal ─────────────────────────────────────────────────────────

fn is_blocked(name: &str) -> bool {
    // Exact match (case-insensitive).
    if BLOCKED_ENV_VARS.iter().any(|blocked| blocked.eq_ignore_ascii_case(name)) {
        return true;
    }

    // Prefix match (case-insensitive).
    let upper = name.to_ascii_uppercase();
    if BLOCKED_ENV_PREFIXES.iter().any(|prefix| upper.starts_with(prefix)) {
        return true;
    }

    // Keyword substring match (case-insensitive).
    if BLOCKED_KEYWORDS.iter().any(|keyword| upper.contains(keyword)) {
        return true;
    }

    false
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::*;

    #[test]
    fn blocks_known_sensitive_vars() {
        assert!(!is_env_var_safe("AWS_SECRET_ACCESS_KEY"));
        assert!(!is_env_var_safe("GITHUB_TOKEN"));
        assert!(!is_env_var_safe("DATABASE_URL"));
        assert!(!is_env_var_safe("HOME"));
        assert!(!is_env_var_safe("SSH_AUTH_SOCK"));
    }

    #[test]
    fn blocks_prefix_match() {
        assert!(!is_env_var_safe("AWS_REGION"));
        assert!(!is_env_var_safe("AZURE_SUBSCRIPTION_ID"));
        assert!(!is_env_var_safe("GCP_PROJECT"));
        assert!(!is_env_var_safe("GOOGLE_CLOUD_KEY"));
    }

    #[test]
    fn blocks_keyword_match() {
        assert!(!is_env_var_safe("MY_PASSWORD"));
        assert!(!is_env_var_safe("API_SECRET"));
        assert!(!is_env_var_safe("ACCESS_TOKEN"));
        assert!(!is_env_var_safe("ENCRYPTION_KEY"));
        assert!(!is_env_var_safe("CUSTOM_CREDENTIAL"));
    }

    #[test]
    fn allows_safe_vars() {
        assert!(is_env_var_safe("RUST_LOG"));
        assert!(is_env_var_safe("CARGO_HOME"));
        assert!(is_env_var_safe("LANG"));
        assert!(is_env_var_safe("TERM"));
        assert!(is_env_var_safe("BOB_MODEL"));
    }

    #[test]
    fn case_insensitive() {
        assert!(!is_env_var_safe("github_token"));
        assert!(!is_env_var_safe("Database_Url"));
        assert!(!is_env_var_safe("aws_secret_access_key"));
    }

    // NOTE: std::env::set_var/remove_var require unsafe blocks in Rust 2024 edition.
    #[test]
    fn load_safe_env_vars_filters() {
        // SAFETY: These tests run serially and use unique variable names
        // that do not conflict with other threads.
        unsafe {
            std::env::set_var("BOB_TEST_SAFE_VAR", "safe_value");
            std::env::set_var("BOB_TEST_PASSWORD", "secret_value");
        }

        let vars = load_safe_env_vars();
        assert!(vars.contains_key("BOB_TEST_SAFE_VAR"));
        assert!(!vars.contains_key("BOB_TEST_PASSWORD"));

        // SAFETY: Removing test-only variables.
        unsafe {
            std::env::remove_var("BOB_TEST_SAFE_VAR");
            std::env::remove_var("BOB_TEST_PASSWORD");
        }
    }

    #[test]
    fn load_env_var_safe_blocks_sensitive() {
        // SAFETY: Test-only variable with unique name.
        unsafe { std::env::set_var("MY_API_TOKEN", "secret") };
        assert!(load_env_var_safe("MY_API_TOKEN").is_none());
        // SAFETY: Cleaning up test-only variable.
        unsafe { std::env::remove_var("MY_API_TOKEN") };
    }

    #[test]
    fn load_env_var_safe_returns_safe() {
        // SAFETY: Test-only variable with unique name.
        unsafe { std::env::set_var("BOB_TEST_LOADED", "value") };
        assert_eq!(load_env_var_safe("BOB_TEST_LOADED"), Some("value".to_string()));
        // SAFETY: Cleaning up test-only variable.
        unsafe { std::env::remove_var("BOB_TEST_LOADED") };
    }
}
