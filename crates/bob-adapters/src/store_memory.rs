//! # In-Memory Session Store
//!
//! In-memory session store — implements [`SessionStore`] via `scc::HashMap`.
//!
//! ## Overview
//!
//! This adapter provides a thread-safe, in-memory session store backed by
//! [`scc::HashMap`](https://docs.rs/scc/latest/scc/struct.HashMap.html).
//!
//! Suitable for:
//! - Development and testing
//! - Single-process CLI applications
//! - Scenarios where persistence across restarts is not required
//!
//! Not suitable for:
//! - Multi-process deployments
//! - Production environments requiring persistence
//! - Horizontal scaling
//!
//! ## CAS Support
//!
//! The `save_if_version` method performs an atomic compare-and-swap: the
//! session is only persisted when the stored version matches the expected
//! version. On success the version is incremented atomically.

use bob_core::{
    error::StoreError,
    ports::SessionStore,
    types::{SessionId, SessionState},
};

/// Thread-safe, in-memory session store backed by [`scc::HashMap`].
///
/// Suitable for single-process / CLI usage where persistence across
/// restarts is not required.
#[derive(Debug)]
pub struct InMemorySessionStore {
    inner: scc::HashMap<SessionId, SessionState>,
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemorySessionStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self { inner: scc::HashMap::new() }
    }
}

#[async_trait::async_trait]
impl SessionStore for InMemorySessionStore {
    async fn load(&self, id: &SessionId) -> Result<Option<SessionState>, StoreError> {
        let state = self.inner.read_async(id, |_k, v| v.clone()).await;
        Ok(state)
    }

    async fn save(&self, id: &SessionId, state: &SessionState) -> Result<(), StoreError> {
        let entry = self.inner.entry_async(id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                let new_version = occ.get().version.saturating_add(1);
                let mut updated = state.clone();
                updated.version = new_version;
                occ.get_mut().clone_from(&updated);
            }
            scc::hash_map::Entry::Vacant(vac) => {
                let mut initial = state.clone();
                initial.version = initial.version.max(1);
                let _ = vac.insert_entry(initial);
            }
        }
        Ok(())
    }

    async fn save_if_version(
        &self,
        id: &SessionId,
        state: &SessionState,
        expected_version: u64,
    ) -> Result<u64, StoreError> {
        let entry = self.inner.entry_async(id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                if occ.get().version != expected_version {
                    return Err(StoreError::VersionConflict {
                        expected: expected_version,
                        actual: occ.get().version,
                    });
                }
                let new_version = expected_version.saturating_add(1);
                let mut updated = state.clone();
                updated.version = new_version;
                occ.get_mut().clone_from(&updated);
                Ok(new_version)
            }
            scc::hash_map::Entry::Vacant(vac) => {
                if expected_version != 0 {
                    return Err(StoreError::VersionConflict {
                        expected: expected_version,
                        actual: 0,
                    });
                }
                let mut initial = state.clone();
                initial.version = 1;
                let _ = vac.insert_entry(initial);
                Ok(1)
            }
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bob_core::types::Message;

    use super::*;

    #[tokio::test]
    async fn load_missing_returns_none() {
        let store = InMemorySessionStore::new();
        let result = store.load(&"nonexistent".to_string()).await;
        assert!(result.is_ok());
        assert!(result.ok().flatten().is_none());
    }

    #[tokio::test]
    async fn roundtrip_save_load() {
        let store = InMemorySessionStore::new();
        let id = "sess-1".to_string();
        let state = SessionState {
            messages: vec![Message::text(bob_core::types::Role::User, "hello")],
            ..SessionState::default()
        };

        store.save(&id, &state).await.ok();
        let loaded = store.load(&id).await.ok().flatten();
        assert!(loaded.is_some());
        assert_eq!(loaded.as_ref().map(|s| s.messages.len()), Some(1));
        assert_eq!(loaded.as_ref().map(|s| s.version), Some(1));
    }

    #[tokio::test]
    async fn save_increments_version() {
        let store = InMemorySessionStore::new();
        let id = "sess-v".to_string();
        let state = SessionState::default();

        store.save(&id, &state).await.ok();
        let v1 = store.load(&id).await.ok().flatten().unwrap_or_default().version;
        assert_eq!(v1, 1);

        store.save(&id, &state).await.ok();
        let v2 = store.load(&id).await.ok().flatten().unwrap_or_default().version;
        assert_eq!(v2, 2);
    }

    #[tokio::test]
    async fn save_if_version_succeeds_on_match() {
        let store = InMemorySessionStore::new();
        let id = "sess-cas".to_string();
        let state = SessionState::default();

        // First save (version starts at 0 -> becomes 1)
        store.save(&id, &state).await.ok();
        let loaded = store.load(&id).await.ok().flatten().unwrap_or_default();
        assert_eq!(loaded.version, 1);

        // CAS with correct version
        let new_version = store.save_if_version(&id, &state, 1).await;
        assert!(new_version.is_ok());
        assert_eq!(new_version.unwrap_or_default(), 2);
    }

    #[tokio::test]
    async fn save_if_version_fails_on_mismatch() {
        let store = InMemorySessionStore::new();
        let id = "sess-cas-fail".to_string();
        let state = SessionState::default();

        store.save(&id, &state).await.ok();

        // CAS with stale version
        let result = store.save_if_version(&id, &state, 0).await;
        assert!(result.is_err());
        if let Err(StoreError::VersionConflict { expected, actual }) = result {
            assert_eq!(expected, 0);
            assert_eq!(actual, 1);
        } else {
            panic!("expected VersionConflict");
        }
    }

    #[tokio::test]
    async fn overwrite_existing_session() {
        let store = InMemorySessionStore::new();
        let id = "sess-2".to_string();

        let state1 = SessionState {
            messages: vec![Message::text(bob_core::types::Role::User, "first")],
            ..SessionState::default()
        };
        store.save(&id, &state1).await.ok();

        let state2 = SessionState {
            messages: vec![
                Message::text(bob_core::types::Role::User, "first"),
                Message::text(bob_core::types::Role::Assistant, "second"),
            ],
            ..SessionState::default()
        };
        store.save(&id, &state2).await.ok();

        let loaded = store.load(&id).await.ok().flatten();
        assert_eq!(loaded.as_ref().map(|s| s.messages.len()), Some(2));
        assert_eq!(loaded.as_ref().map(|s| s.version), Some(2));
    }

    #[tokio::test]
    async fn arc_dyn_session_store_works() {
        let store: Arc<dyn SessionStore> = Arc::new(InMemorySessionStore::new());
        let id = "sess-arc".to_string();
        let state = SessionState::default();
        store.save(&id, &state).await.ok();
        let loaded = store.load(&id).await.ok().flatten();
        assert!(loaded.is_some());
    }
}
