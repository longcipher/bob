//! # ICE Classifier / Router
//!
//! Implements the Interpreter-Classifier-Executor cognitive architecture
//! inspired by OpenAgent's cortex design.
//!
//! ## Overview
//!
//! Before sending a large prompt to the LLM, the classifier:
//!
//! 1. **Interprets** the user's intent from their message
//! 2. **Classifies** the task type (coding, research, file ops, etc.)
//! 3. **Routes** only the relevant tools into the LLM context
//!
//! This reduces token usage and prevents "tool overload" where having
//! too many tools degrades LLM performance.
//!
//! ## Architecture
//!
//! ```text
//! User Input
//!     │
//!     ▼
//! ┌──────────────┐
//! │ Interpreter  │  Extract intent + entities
//! └──────┬───────┘
//!        │
//!        ▼
//! ┌──────────────┐
//! │ Classifier   │  Map intent → task category
//! └──────┬───────┘
//!        │
//!        ▼
//! ┌──────────────┐
//! │ Router       │  Filter tools for category
//! └──────────────┘
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_runtime::classifier::{Classifier, HeuristicClassifier};
//!
//! let classifier = HeuristicClassifier::new();
//! let routing = classifier.classify("read the file src/main.rs", &tools).await;
//! // routing.filtered_tools only contains file-related tools
//! ```

use bob_core::types::ToolDescriptor;

/// Task categories for classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskCategory {
    /// Code reading, writing, refactoring.
    Coding,
    /// Web search, research, information gathering.
    Research,
    /// File system operations (read, write, list).
    FileOps,
    /// Shell command execution.
    Shell,
    /// Git operations.
    Git,
    /// Data analysis, transformation.
    DataProcessing,
    /// General conversation, Q&A.
    General,
    /// Unknown / cannot classify.
    Unknown,
}

impl TaskCategory {
    /// All known categories.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::Coding,
            Self::Research,
            Self::FileOps,
            Self::Shell,
            Self::Git,
            Self::DataProcessing,
            Self::General,
        ]
    }

    /// Keywords that strongly indicate this category.
    #[must_use]
    pub fn keywords(&self) -> &'static [&'static str] {
        match self {
            Self::Coding => &[
                "code",
                "function",
                "class",
                "implement",
                "refactor",
                "compile",
                "build",
                "test",
                "debug",
                "error",
                "bug",
                "fix",
                "syntax",
                "variable",
                "type",
                "trait",
                "struct",
                "enum",
                "module",
                "import",
                "export",
                "api",
                "endpoint",
                "handler",
                "library",
                "dependency",
                "crate",
                "package",
                ".rs",
                ".ts",
                ".py",
                ".go",
                ".java",
                ".cpp",
                ".js",
                ".jsx",
                ".tsx",
            ],
            Self::Research => &[
                "search",
                "find",
                "look up",
                "research",
                "what is",
                "how does",
                "explain",
                "documentation",
                "docs",
                "learn",
                "understand",
                "compare",
                "difference",
                "pros and cons",
                "alternative",
                "best practice",
                "recommendation",
            ],
            Self::FileOps => &[
                "read file",
                "write file",
                "create file",
                "delete file",
                "list files",
                "list directory",
                "directory",
                "folder",
                "copy file",
                "move file",
                "rename",
                "file content",
                "cat ",
                "ls ",
                "mkdir",
                "touch",
                "rm ",
                "cp ",
                "mv ",
                "read the file",
                "write the file",
                "open file",
                "open the file",
                "read src",
                "read the src",
                ".rs",
                ".toml",
                ".json",
                ".yaml",
                ".yml",
                ".md",
                ".txt",
                ".csv",
                ".log",
            ],
            Self::Shell => &[
                "run", "execute", "command", "shell", "bash", "terminal", "script", "install",
                "apt", "brew", "npm", "pip", "cargo", "docker", "kubectl", "ssh",
            ],
            Self::Git => &[
                "git",
                "commit",
                "push",
                "pull",
                "branch",
                "merge",
                "rebase",
                "diff",
                "log",
                "status",
                "checkout",
                "clone",
                "repository",
                "repo",
                "pr",
                "pull request",
            ],
            Self::DataProcessing => &[
                "parse",
                "transform",
                "convert",
                "json",
                "csv",
                "xml",
                "sql",
                "database",
                "query",
                "aggregate",
                "filter",
                "sort",
                "map",
                "reduce",
                "process data",
            ],
            Self::General => &[
                "hello",
                "hi",
                "hey",
                "help",
                "thanks",
                "please",
                "can you",
                "could you",
                "would you",
                "tell me",
            ],
            Self::Unknown => &[],
        }
    }

    /// Tool name prefixes that belong to this category.
    #[must_use]
    pub fn tool_prefixes(&self) -> &'static [&'static str] {
        match self {
            Self::Coding => &["code.", "lint.", "format.", "test.", "build."],
            Self::Research => &["web.", "search.", "browse.", "fetch."],
            Self::FileOps => &["file.", "read_file", "write_file", "list_files"],
            Self::Shell => &["shell.", "exec.", "command."],
            Self::Git => &["git.", "gh."],
            Self::DataProcessing => &["data.", "parse.", "transform.", "sql."],
            Self::General => &[],
            Self::Unknown => &[],
        }
    }
}

/// Classification result with confidence and routing information.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    /// The primary task category.
    pub category: TaskCategory,
    /// Secondary categories (if multi-category input).
    pub secondary_categories: Vec<TaskCategory>,
    /// Confidence score (0.0 - 1.0).
    pub confidence: f64,
    /// Extracted keywords that led to classification.
    pub matched_keywords: Vec<String>,
    /// Tools filtered for this classification.
    pub filtered_tools: Vec<ToolDescriptor>,
}

/// Trait for task classifiers.
///
/// Classifiers analyze user input and determine the task category
/// to enable intelligent tool routing.
#[async_trait::async_trait]
pub trait Classifier: Send + Sync {
    /// Classify user input and return routing information.
    async fn classify(
        &self,
        input: &str,
        available_tools: &[ToolDescriptor],
    ) -> ClassificationResult;

    /// Whether this classifier can handle multi-category classification.
    fn supports_multi_category(&self) -> bool {
        false
    }
}

/// Heuristic keyword-based classifier.
///
/// Fast, deterministic classification using keyword matching.
/// No LLM calls required — pure string matching.
///
/// ## Example
///
/// ```rust,ignore
/// use bob_runtime::classifier::HeuristicClassifier;
///
/// let classifier = HeuristicClassifier::new();
/// let result = classifier.classify("read the file src/main.rs", &tools).await;
/// assert_eq!(result.category, TaskCategory::FileOps);
/// ```
#[derive(Debug, Clone, Default)]
pub struct HeuristicClassifier {
    /// Minimum confidence threshold to accept classification.
    min_confidence: f64,
}

impl HeuristicClassifier {
    /// Create a new heuristic classifier.
    #[must_use]
    pub fn new() -> Self {
        Self { min_confidence: 0.05 }
    }

    /// Create with custom minimum confidence threshold.
    #[must_use]
    pub fn with_min_confidence(mut self, threshold: f64) -> Self {
        self.min_confidence = threshold.clamp(0.0, 1.0);
        self
    }
}

#[async_trait::async_trait]
impl Classifier for HeuristicClassifier {
    async fn classify(
        &self,
        input: &str,
        available_tools: &[ToolDescriptor],
    ) -> ClassificationResult {
        let input_lower = input.to_lowercase();

        // Score each category
        let mut scores: Vec<(TaskCategory, f64, Vec<String>)> = TaskCategory::all()
            .iter()
            .map(|cat| {
                let mut score = 0.0;
                let mut matched = Vec::new();

                for keyword in cat.keywords() {
                    if input_lower.contains(keyword) {
                        score += 1.0;
                        matched.push(keyword.to_string());
                    }
                }

                // Normalize by keyword count
                let normalized = if cat.keywords().is_empty() {
                    0.0
                } else {
                    score / cat.keywords().len() as f64
                };

                (*cat, normalized, matched)
            })
            .collect();

        // Sort by score descending
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let (primary, primary_score, primary_keywords) =
            scores.first().cloned().unwrap_or((TaskCategory::Unknown, 0.0, Vec::new()));

        // Secondary categories (score > 50% of primary)
        let secondary_threshold = primary_score * 0.5;
        let secondary: Vec<TaskCategory> = scores
            .iter()
            .skip(1)
            .filter(|(_, score, _)| *score >= secondary_threshold && *score > 0.0)
            .map(|(cat, _, _)| *cat)
            .take(2)
            .collect();

        // Filter tools based on primary category
        let filtered_tools =
            if primary_score >= self.min_confidence && primary != TaskCategory::Unknown {
                filter_tools_for_category(primary, available_tools)
            } else {
                // Unknown or low confidence: return all tools
                available_tools.to_vec()
            };

        ClassificationResult {
            category: if primary_score >= self.min_confidence {
                primary
            } else {
                TaskCategory::Unknown
            },
            secondary_categories: secondary,
            confidence: primary_score,
            matched_keywords: primary_keywords,
            filtered_tools,
        }
    }

    fn supports_multi_category(&self) -> bool {
        true
    }
}

/// No-op classifier that returns all tools without filtering.
///
/// Useful as a fallback or when classification is not desired.
#[derive(Debug, Clone, Copy, Default)]
pub struct PassThroughClassifier;

#[async_trait::async_trait]
impl Classifier for PassThroughClassifier {
    async fn classify(
        &self,
        _input: &str,
        available_tools: &[ToolDescriptor],
    ) -> ClassificationResult {
        ClassificationResult {
            category: TaskCategory::Unknown,
            secondary_categories: Vec::new(),
            confidence: 0.0,
            matched_keywords: Vec::new(),
            filtered_tools: available_tools.to_vec(),
        }
    }
}

/// Filter tools that match a task category's prefixes.
fn filter_tools_for_category(
    category: TaskCategory,
    tools: &[ToolDescriptor],
) -> Vec<ToolDescriptor> {
    let prefixes = category.tool_prefixes();

    if prefixes.is_empty() {
        return tools.to_vec();
    }

    // Also include tools that match secondary categories
    let matched: Vec<ToolDescriptor> = tools
        .iter()
        .filter(|tool| prefixes.iter().any(|prefix| tool.id.starts_with(prefix)))
        .cloned()
        .collect();

    // If filtering produces too few tools, fall back to all tools
    if matched.is_empty() { tools.to_vec() } else { matched }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool(id: &str, desc: &str) -> ToolDescriptor {
        ToolDescriptor::new(id, desc)
    }

    fn sample_tools() -> Vec<ToolDescriptor> {
        vec![
            make_tool("file.read", "Read file contents"),
            make_tool("file.write", "Write file contents"),
            make_tool("file.list", "List directory"),
            make_tool("shell.exec", "Execute shell command"),
            make_tool("git.status", "Git status"),
            make_tool("web.search", "Search the web"),
            make_tool("code.lint", "Lint code"),
        ]
    }

    // ── Heuristic Classifier ────────────────────────────────────────

    #[tokio::test]
    async fn classify_file_operation() {
        let classifier = HeuristicClassifier::new();
        let tools = sample_tools();

        let result = classifier.classify("read the file src/main.rs", &tools).await;
        assert_eq!(result.category, TaskCategory::FileOps);
        assert!(!result.filtered_tools.is_empty());
    }

    #[tokio::test]
    async fn classify_coding_task() {
        let classifier = HeuristicClassifier::new();
        let tools = sample_tools();

        let result = classifier.classify("refactor this function to fix the bug", &tools).await;
        assert_eq!(result.category, TaskCategory::Coding);
    }

    #[tokio::test]
    async fn classify_shell_task() {
        let classifier = HeuristicClassifier::new();
        let tools = sample_tools();

        let result = classifier.classify("run the install command", &tools).await;
        assert_eq!(result.category, TaskCategory::Shell);
    }

    #[tokio::test]
    async fn classify_git_task() {
        let classifier = HeuristicClassifier::new();
        let tools = sample_tools();

        let result = classifier.classify("show me the git status", &tools).await;
        assert_eq!(result.category, TaskCategory::Git);
    }

    #[tokio::test]
    async fn classify_research_task() {
        let classifier = HeuristicClassifier::new();
        let tools = sample_tools();

        let result = classifier.classify("search for documentation on rust traits", &tools).await;
        assert_eq!(result.category, TaskCategory::Research);
    }

    #[tokio::test]
    async fn classify_general_greeting() {
        let classifier = HeuristicClassifier::new();
        let tools = sample_tools();

        let result = classifier.classify("hello, can you help me?", &tools).await;
        assert_eq!(result.category, TaskCategory::General);
    }

    #[tokio::test]
    async fn classify_unknown_returns_all_tools() {
        let classifier = HeuristicClassifier::new();
        let tools = sample_tools();

        let result = classifier.classify("xyzzy plugh", &tools).await;
        assert_eq!(result.category, TaskCategory::Unknown);
        assert_eq!(result.filtered_tools.len(), tools.len());
    }

    #[tokio::test]
    async fn filtered_tools_match_category() {
        let classifier = HeuristicClassifier::new();
        let tools = sample_tools();

        let result = classifier.classify("read the file config.toml", &tools).await;
        assert_eq!(result.category, TaskCategory::FileOps);
        assert!(
            result.filtered_tools.iter().any(|t| t.id.starts_with("file.")),
            "should include file-related tools"
        );
    }

    // ── PassThrough Classifier ──────────────────────────────────────

    #[tokio::test]
    async fn passthrough_returns_all_tools() {
        let classifier = PassThroughClassifier;
        let tools = sample_tools();

        let result = classifier.classify("anything", &tools).await;
        assert_eq!(result.filtered_tools.len(), tools.len());
        assert_eq!(result.category, TaskCategory::Unknown);
    }

    // ── Multi-category ──────────────────────────────────────────────

    #[test]
    fn heuristic_supports_multi_category() {
        let classifier = HeuristicClassifier::new();
        assert!(classifier.supports_multi_category());
    }
}
