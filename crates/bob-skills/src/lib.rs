//! # bob-skills
//!
//! Skill management for the [Bob Agent Framework](https://github.com/longcipher/bob).
//!
//! This crate provides a complete skill lifecycle pipeline:
//!
//! - **Parse**: Spec-compliant SKILL.md parsing and validation
//! - **Load**: Filesystem discovery with recursive scanning and caching
//! - **Select**: Relevance-based skill selection using keyword scoring
//! - **Compose**: Prompt generation in XML or Markdown format with token budget enforcement
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │          SkillRegistry               │  ← High-level API
//! │  ┌──────────┐ ┌──────────────────┐   │
//! │  │ Selector │ │ PromptComposer   │   │
//! │  └──────────┘ └──────────────────┘   │
//! │              ┌──────────────────┐    │
//! │              │  FsSkillStore    │    │  ← SkillStore port impl
//! │              │  (parsing)       │    │
//! │              └──────────────────┘    │
//! └──────────────────────────────────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use bob_skills::{SkillRegistry, SkillSource};
//!
//! let registry = SkillRegistry::new(
//!     bob_skills::loader::FsSkillStore::new(vec![
//!         SkillSource::new("./skills".into(), true),
//!     ]),
//! );
//!
//! let rendered = registry.render_for_input("review this code", None)?;
//! println!("{}", rendered.prompt);
//! ```
//!
//! ## Progressive Disclosure
//!
//! Following the Agent Skills specification, skills support three layers:
//!
//! 1. **Metadata** (~100 tokens): `name` + `description` loaded at startup
//! 2. **Instructions** (<5000 tokens): Full `SKILL.md` body loaded on activation
//! 3. **Resources** (as needed): Files in `scripts/`, `references/`, `assets/`

pub mod compose;
pub mod error;
pub mod loader;
pub mod parsing;
pub mod registry;
pub mod selector;
pub mod store;
pub mod types;

pub use compose::{PromptComposer, PromptFormat};
pub use error::SkillError;
pub use loader::{FsSkillStore, SkillSource};
pub use parsing::{
    AllowedTools, Compatibility, Frontmatter, FrontmatterBuilder, LoadError, Metadata, ParseError,
    Skill, SkillDescription, SkillDescriptionError, SkillDirectory, SkillName, SkillNameError,
};
pub use registry::SkillRegistry;
pub use selector::SkillSelector;
pub use store::{SkillStore, SkillStoreSync};
pub use types::{RenderedSkills, SkillEntry, SkillPolicy, SkillSummary};
