//! Emoji types with well-known mappings and custom emoji support.
//!
//! Provides type-safe, cross-platform emoji with platform-specific rendering
//! for Discord, Telegram, and CLI environments.

use std::{
    fmt,
    hash::{Hash, Hasher},
};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// WellKnownEmoji
// ---------------------------------------------------------------------------

/// A well-known emoji with predefined platform-specific mappings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WellKnownEmoji {
    ThumbsUp,
    ThumbsDown,
    Heart,
    Smile,
    Laugh,
    Thinking,
    Fire,
    Star,
    Sparkles,
    Check,
    X,
    Warning,
    Rocket,
    Eyes,
    Wave,
    Clap,
    Pray,
    PartyPopper,
    HundredPoints,
    Tada,
    Robot,
    Brain,
    Lightning,
    Globe,
    Hammer,
    Wrench,
    Gear,
    Lock,
    Unlock,
    Pin,
    Memo,
    Bug,
    Bulb,
    Trophy,
    Medal,
}

impl WellKnownEmoji {
    /// Returns the snake_case canonical name of this emoji.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ThumbsUp => "thumbs_up",
            Self::ThumbsDown => "thumbs_down",
            Self::Heart => "heart",
            Self::Smile => "smile",
            Self::Laugh => "laugh",
            Self::Thinking => "thinking",
            Self::Fire => "fire",
            Self::Star => "star",
            Self::Sparkles => "sparkles",
            Self::Check => "check",
            Self::X => "x",
            Self::Warning => "warning",
            Self::Rocket => "rocket",
            Self::Eyes => "eyes",
            Self::Wave => "wave",
            Self::Clap => "clap",
            Self::Pray => "pray",
            Self::PartyPopper => "party_popper",
            Self::HundredPoints => "hundred_points",
            Self::Tada => "tada",
            Self::Robot => "robot",
            Self::Brain => "brain",
            Self::Lightning => "lightning",
            Self::Globe => "globe",
            Self::Hammer => "hammer",
            Self::Wrench => "wrench",
            Self::Gear => "gear",
            Self::Lock => "lock",
            Self::Unlock => "unlock",
            Self::Pin => "pin",
            Self::Memo => "memo",
            Self::Bug => "bug",
            Self::Bulb => "bulb",
            Self::Trophy => "trophy",
            Self::Medal => "medal",
        }
    }
}

impl fmt::Display for WellKnownEmoji {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Default emoji map
// ---------------------------------------------------------------------------

/// Default platform-specific emoji mappings.
///
/// Each entry is `(emoji, discord_format, telegram_format, cli_format)`.
const DEFAULT_EMOJI_MAP: &[(WellKnownEmoji, &str, &str, &str)] = &[
    (WellKnownEmoji::ThumbsUp, "👍", "👍", "[thumbs_up]"),
    (WellKnownEmoji::ThumbsDown, "👎", "👎", "[thumbs_down]"),
    (WellKnownEmoji::Heart, "❤️", "❤️", "[heart]"),
    (WellKnownEmoji::Smile, "😊", "😊", "[smile]"),
    (WellKnownEmoji::Laugh, "😂", "😂", "[laugh]"),
    (WellKnownEmoji::Thinking, "🤔", "🤔", "[thinking]"),
    (WellKnownEmoji::Fire, "🔥", "🔥", "[fire]"),
    (WellKnownEmoji::Star, "⭐", "⭐", "[star]"),
    (WellKnownEmoji::Sparkles, "✨", "✨", "[sparkles]"),
    (WellKnownEmoji::Check, "✅", "✅", "[check]"),
    (WellKnownEmoji::X, "❌", "❌", "[x]"),
    (WellKnownEmoji::Warning, "⚠️", "⚠️", "[warning]"),
    (WellKnownEmoji::Rocket, "🚀", "🚀", "[rocket]"),
    (WellKnownEmoji::Eyes, "👀", "👀", "[eyes]"),
    (WellKnownEmoji::Wave, "👋", "👋", "[wave]"),
    (WellKnownEmoji::Clap, "👏", "👏", "[clap]"),
    (WellKnownEmoji::Pray, "🙏", "🙏", "[pray]"),
    (WellKnownEmoji::PartyPopper, "🎉", "🎉", "[party_popper]"),
    (WellKnownEmoji::HundredPoints, "💯", "💯", "[hundred_points]"),
    (WellKnownEmoji::Tada, "🎊", "🎊", "[tada]"),
    (WellKnownEmoji::Robot, "🤖", "🤖", "[robot]"),
    (WellKnownEmoji::Brain, "🧠", "🧠", "[brain]"),
    (WellKnownEmoji::Lightning, "⚡", "⚡", "[lightning]"),
    (WellKnownEmoji::Globe, "🌍", "🌍", "[globe]"),
    (WellKnownEmoji::Hammer, "🔨", "🔨", "[hammer]"),
    (WellKnownEmoji::Wrench, "🔧", "🔧", "[wrench]"),
    (WellKnownEmoji::Gear, "⚙️", "⚙️", "[gear]"),
    (WellKnownEmoji::Lock, "🔒", "🔒", "[lock]"),
    (WellKnownEmoji::Unlock, "🔓", "🔓", "[unlock]"),
    (WellKnownEmoji::Pin, "📌", "📌", "[pin]"),
    (WellKnownEmoji::Memo, "📝", "📝", "[memo]"),
    (WellKnownEmoji::Bug, "🐛", "🐛", "[bug]"),
    (WellKnownEmoji::Bulb, "💡", "💡", "[bulb]"),
    (WellKnownEmoji::Trophy, "🏆", "🏆", "[trophy]"),
    (WellKnownEmoji::Medal, "🏅", "🏅", "[medal]"),
];

/// Looks up platform format strings for a well-known emoji.
fn lookup_formats(emoji: WellKnownEmoji) -> Option<EmojiFormats> {
    DEFAULT_EMOJI_MAP.iter().find(|(e, _, _, _)| *e == emoji).map(|(_, discord, telegram, cli)| {
        EmojiFormats {
            discord: (*discord).to_owned(),
            telegram: (*telegram).to_owned(),
            cli: (*cli).to_owned(),
        }
    })
}

/// Resolves a platform-specific string from the default map by name and platform.
fn resolve_from_default_map(name: &str, platform: &str) -> Option<String> {
    DEFAULT_EMOJI_MAP.iter().find(|(emoji, _, _, _)| emoji.as_str() == name).and_then(
        |(_, discord, telegram, cli)| match platform {
            "discord" => Some((*discord).to_owned()),
            "telegram" => Some((*telegram).to_owned()),
            "cli" => Some((*cli).to_owned()),
            _ => None,
        },
    )
}

// ---------------------------------------------------------------------------
// EmojiFormats
// ---------------------------------------------------------------------------

/// Platform-specific emoji format strings.
#[derive(Debug, Clone)]
pub struct EmojiFormats {
    /// Discord representation (Unicode emoji or custom syntax like `:thumbsup:`).
    pub discord: String,
    /// Telegram representation (typically Unicode emoji).
    pub telegram: String,
    /// CLI representation (e.g. `[thumbs_up]`).
    pub cli: String,
}

// ---------------------------------------------------------------------------
// EmojiValue
// ---------------------------------------------------------------------------

/// A resolved emoji value with a name and platform-specific formats.
///
/// Equality and hashing are based solely on the `name` field, so two
/// `EmojiValue` instances with the same name are considered identical
/// regardless of their format strings.
#[derive(Debug, Clone)]
pub struct EmojiValue {
    /// The canonical snake_case name of this emoji.
    pub name: String,
    /// Platform-specific format strings.
    formats: EmojiFormats,
}

impl EmojiValue {
    /// Creates an `EmojiValue` from a well-known emoji using the default map.
    #[must_use]
    pub fn from_well_known(emoji: WellKnownEmoji) -> Self {
        let name = emoji.as_str().to_owned();
        let formats = lookup_formats(emoji).unwrap_or_else(|| EmojiFormats {
            discord: name.clone(),
            telegram: name.clone(),
            cli: format!("[{name}]"),
        });
        Self { name, formats }
    }

    /// Resolves this emoji's representation for the given platform.
    ///
    /// Returns `None` if the platform is not recognised.
    #[must_use]
    pub fn resolve(&self, platform: &str) -> Option<String> {
        match platform {
            "discord" => Some(self.formats.discord.clone()),
            "telegram" => Some(self.formats.telegram.clone()),
            "cli" => Some(self.formats.cli.clone()),
            _ => None,
        }
    }
}

impl PartialEq for EmojiValue {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for EmojiValue {}

impl Hash for EmojiValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl fmt::Display for EmojiValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, ":{}:", self.name)
    }
}

// ---------------------------------------------------------------------------
// EmojiResolver trait
// ---------------------------------------------------------------------------

/// Trait for resolving emoji names to platform-specific strings.
///
/// Implementations may consult built-in tables, remote services, or
/// user-defined custom emoji sets.
pub trait EmojiResolver: Send + Sync {
    /// Resolves an emoji by its canonical name for the given platform.
    ///
    /// Returns `None` if the emoji name is unknown or the platform is
    /// not supported.
    fn resolve(&self, name: &str, platform: &str) -> Option<String>;
}

// ---------------------------------------------------------------------------
// DefaultEmojiResolver
// ---------------------------------------------------------------------------

/// Default emoji resolver backed by the built-in emoji map.
#[derive(Debug, Clone, Copy)]
pub struct DefaultEmojiResolver;

impl EmojiResolver for DefaultEmojiResolver {
    fn resolve(&self, name: &str, platform: &str) -> Option<String> {
        resolve_from_default_map(name, platform)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };

    use super::*;

    #[test]
    fn thumbs_up_resolves_for_discord() {
        let resolver = DefaultEmojiResolver;
        let result = resolver.resolve("thumbs_up", "discord");
        assert_eq!(result, Some("👍".to_owned()));
    }

    #[test]
    fn thumbs_up_resolves_for_telegram() {
        let resolver = DefaultEmojiResolver;
        let result = resolver.resolve("thumbs_up", "telegram");
        assert_eq!(result, Some("👍".to_owned()));
    }

    #[test]
    fn thumbs_up_resolves_for_cli() {
        let resolver = DefaultEmojiResolver;
        let result = resolver.resolve("thumbs_up", "cli");
        assert_eq!(result, Some("[thumbs_up]".to_owned()));
    }

    #[test]
    fn unknown_emoji_returns_none() {
        let resolver = DefaultEmojiResolver;
        assert_eq!(resolver.resolve("nonexistent_emoji", "discord"), None);
    }

    #[test]
    fn unknown_platform_returns_none() {
        let resolver = DefaultEmojiResolver;
        assert_eq!(resolver.resolve("thumbs_up", "unknown_platform"), None);
    }

    #[test]
    fn emoji_value_display_formatting() {
        let value = EmojiValue::from_well_known(WellKnownEmoji::ThumbsUp);
        assert_eq!(value.to_string(), ":thumbs_up:");
    }

    #[test]
    fn emoji_value_display_formatting_multi_word() {
        let value = EmojiValue::from_well_known(WellKnownEmoji::PartyPopper);
        assert_eq!(value.to_string(), ":party_popper:");
    }

    #[test]
    fn emoji_value_partial_eq_based_on_name() {
        let a = EmojiValue::from_well_known(WellKnownEmoji::ThumbsUp);
        let b = EmojiValue::from_well_known(WellKnownEmoji::ThumbsUp);
        assert_eq!(a, b);

        let c = EmojiValue::from_well_known(WellKnownEmoji::Heart);
        assert_ne!(a, c);
    }

    #[test]
    fn emoji_value_hash_based_on_name() {
        let a = EmojiValue::from_well_known(WellKnownEmoji::ThumbsUp);
        let b = EmojiValue::from_well_known(WellKnownEmoji::ThumbsUp);

        let hash_a = {
            let mut hasher = DefaultHasher::new();
            a.hash(&mut hasher);
            hasher.finish()
        };
        let hash_b = {
            let mut hasher = DefaultHasher::new();
            b.hash(&mut hasher);
            hasher.finish()
        };
        assert_eq!(hash_a, hash_b);
    }

    #[test]
    fn emoji_value_resolve_for_platform() {
        let value = EmojiValue::from_well_known(WellKnownEmoji::Fire);
        assert_eq!(value.resolve("discord"), Some("🔥".to_owned()));
        assert_eq!(value.resolve("telegram"), Some("🔥".to_owned()));
        assert_eq!(value.resolve("cli"), Some("[fire]".to_owned()));
        assert_eq!(value.resolve("unknown"), None);
    }

    #[test]
    fn well_known_emoji_as_str() {
        assert_eq!(WellKnownEmoji::ThumbsUp.as_str(), "thumbs_up");
        assert_eq!(WellKnownEmoji::PartyPopper.as_str(), "party_popper");
        assert_eq!(WellKnownEmoji::HundredPoints.as_str(), "hundred_points");
    }

    #[test]
    fn well_known_emoji_display() {
        assert_eq!(WellKnownEmoji::Fire.to_string(), "fire");
        assert_eq!(WellKnownEmoji::X.to_string(), "x");
    }

    #[test]
    fn all_well_known_emoji_resolve_on_all_platforms() {
        let resolver = DefaultEmojiResolver;
        let variants = [
            WellKnownEmoji::ThumbsUp,
            WellKnownEmoji::ThumbsDown,
            WellKnownEmoji::Heart,
            WellKnownEmoji::Smile,
            WellKnownEmoji::Laugh,
            WellKnownEmoji::Thinking,
            WellKnownEmoji::Fire,
            WellKnownEmoji::Star,
            WellKnownEmoji::Sparkles,
            WellKnownEmoji::Check,
            WellKnownEmoji::X,
            WellKnownEmoji::Warning,
            WellKnownEmoji::Rocket,
            WellKnownEmoji::Eyes,
            WellKnownEmoji::Wave,
            WellKnownEmoji::Clap,
            WellKnownEmoji::Pray,
            WellKnownEmoji::PartyPopper,
            WellKnownEmoji::HundredPoints,
            WellKnownEmoji::Tada,
            WellKnownEmoji::Robot,
            WellKnownEmoji::Brain,
            WellKnownEmoji::Lightning,
            WellKnownEmoji::Globe,
            WellKnownEmoji::Hammer,
            WellKnownEmoji::Wrench,
            WellKnownEmoji::Gear,
            WellKnownEmoji::Lock,
            WellKnownEmoji::Unlock,
            WellKnownEmoji::Pin,
            WellKnownEmoji::Memo,
            WellKnownEmoji::Bug,
            WellKnownEmoji::Bulb,
            WellKnownEmoji::Trophy,
            WellKnownEmoji::Medal,
        ];
        for emoji in variants {
            let name = emoji.as_str();
            assert!(resolver.resolve(name, "discord").is_some(), "discord missing for {name}");
            assert!(resolver.resolve(name, "telegram").is_some(), "telegram missing for {name}");
            assert!(resolver.resolve(name, "cli").is_some(), "cli missing for {name}");
        }
    }
}
