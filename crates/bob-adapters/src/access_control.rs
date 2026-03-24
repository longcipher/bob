//! Static access control adapter based on per-channel allowlists.

use bob_core::{
    ports::AccessControlPort,
    types::{AccessDecision, ChannelAccessPolicy},
};

/// Static [`AccessControlPort`] backed by a list of [`ChannelAccessPolicy`].
///
/// Empty `allow_from` on a policy means "allow all" (default open).
/// Unknown channels fall back to the default policy (allow all).
#[derive(Debug, Clone, Default)]
pub struct StaticAccessControl {
    policies: Vec<ChannelAccessPolicy>,
}

impl StaticAccessControl {
    /// Create a new instance from a list of channel policies.
    #[must_use]
    pub fn new(policies: Vec<ChannelAccessPolicy>) -> Self {
        Self { policies }
    }
}

impl AccessControlPort for StaticAccessControl {
    fn check_access(&self, channel: &str, sender_id: &str) -> AccessDecision {
        match self.policies.iter().find(|p| p.channel == channel) {
            Some(policy) if !policy.allow_from.is_empty() => {
                if policy.allow_from.iter().any(|id| id == sender_id) {
                    AccessDecision::Allow
                } else {
                    AccessDecision::Deny
                }
            }
            // Known channel with empty allow_from, or unknown channel: allow all.
            _ => AccessDecision::Allow,
        }
    }

    fn policies(&self) -> &[ChannelAccessPolicy] {
        &self.policies
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_sender_passes() {
        let ac = StaticAccessControl::new(vec![ChannelAccessPolicy {
            channel: "telegram".into(),
            allow_from: vec!["alice".into(), "bob".into()],
        }]);
        assert_eq!(ac.check_access("telegram", "alice"), AccessDecision::Allow);
        assert_eq!(ac.check_access("telegram", "bob"), AccessDecision::Allow);
    }

    #[test]
    fn denied_sender_blocked() {
        let ac = StaticAccessControl::new(vec![ChannelAccessPolicy {
            channel: "telegram".into(),
            allow_from: vec!["alice".into()],
        }]);
        assert_eq!(ac.check_access("telegram", "eve"), AccessDecision::Deny);
    }

    #[test]
    fn empty_allow_from_allows_all() {
        let ac = StaticAccessControl::new(vec![ChannelAccessPolicy {
            channel: "cli".into(),
            allow_from: vec![],
        }]);
        assert_eq!(ac.check_access("cli", "anyone"), AccessDecision::Allow);
    }

    #[test]
    fn unknown_channel_allows_all() {
        let ac = StaticAccessControl::new(vec![ChannelAccessPolicy {
            channel: "telegram".into(),
            allow_from: vec!["alice".into()],
        }]);
        assert_eq!(ac.check_access("discord", "random"), AccessDecision::Allow);
    }

    #[test]
    fn multiple_channels_different_policies() {
        let ac = StaticAccessControl::new(vec![
            ChannelAccessPolicy { channel: "telegram".into(), allow_from: vec!["alice".into()] },
            ChannelAccessPolicy {
                channel: "discord".into(),
                allow_from: vec!["bob".into(), "carol".into()],
            },
        ]);
        assert_eq!(ac.check_access("telegram", "alice"), AccessDecision::Allow);
        assert_eq!(ac.check_access("telegram", "bob"), AccessDecision::Deny);
        assert_eq!(ac.check_access("discord", "bob"), AccessDecision::Allow);
        assert_eq!(ac.check_access("discord", "carol"), AccessDecision::Allow);
        assert_eq!(ac.check_access("discord", "alice"), AccessDecision::Deny);
    }

    #[test]
    fn no_policies_allows_everything() {
        let ac = StaticAccessControl::default();
        assert_eq!(ac.check_access("anything", "anyone"), AccessDecision::Allow);
        assert_eq!(ac.policies().len(), 0);
    }

    #[test]
    fn policies_accessor_returns_configured_list() {
        let policies = vec![ChannelAccessPolicy {
            channel: "telegram".into(),
            allow_from: vec!["alice".into()],
        }];
        let ac = StaticAccessControl::new(policies.clone());
        assert_eq!(ac.policies().len(), 1);
        assert_eq!(ac.policies()[0].channel, "telegram");
    }
}
