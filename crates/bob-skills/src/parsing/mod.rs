//! # Skill Parsing
//!
//! Low-level parsing and validation for Agent Skills.
//!
//! This module provides types and functions for working with Agent Skills
//! as defined by the [Agent Skills specification](https://agentskills.io).
//!
//! # Overview
//!
//! Agent Skills are directories containing a `SKILL.md` file with YAML frontmatter
//! and markdown instructions. This module handles:
//!
//! - Parsing `SKILL.md` files (frontmatter + markdown body)
//! - Validating skills against the specification
//! - Loading skills from directories
//! - Accessing skill metadata, instructions, and referenced files

mod allowed_tools;
mod compatibility;
mod description;
mod error;
mod frontmatter;
mod loader;
mod metadata;
mod name;
mod skill;

// Re-export main types
pub use allowed_tools::AllowedTools;
pub use compatibility::{Compatibility, CompatibilityError};
pub use description::{SkillDescription, SkillDescriptionError};
pub use error::{LoadError, ParseError};
pub use frontmatter::{Frontmatter, FrontmatterBuilder};
pub use loader::SkillDirectory;
pub use metadata::Metadata;
pub use name::{SkillName, SkillNameError};
pub use skill::Skill;
