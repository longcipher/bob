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
//! ## Example
//!
//! ```rust,ignore
//! use bob_adapters::store_memory::InMemorySessionStore;
//! use bob_core::{
//!     ports::SessionStore,
//!     types::{SessionState, Message, Role},
//! };
//!
//! let store = InMemorySessionStore::new();
//!
//! // Save a session
//! let state = SessionState {
//!     messages: vec![Message {
//!         role: Role::User,
//!         content: "Hello".to_string(),
//!     }],
//!     ..Default::default()
//! };
//! store.save(&"session-1".to_string(), &state).await?;
//!
//! // Load the session
//! let loaded = store.load(&"session-1".to_string()).await?;
//! ```
//!
//! ## Thread Safety
//!
//! The store uses `scc::HashMap` which provides lock-free concurrent access,
//! making it safe to share across multiple threads.

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
        // entry_async: insert if absent, overwrite if present.
        let entry = self.inner.entry_async(id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().clone_from(state);
            }
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(state.clone());
            }
        }
        Ok(())
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
            messages: vec![Message { role: bob_core::types::Role::User, content: "hello".into() }],
            ..SessionState::default()
        };

        store.save(&id, &state).await.ok();
        let loaded = store.load(&id).await.ok().flatten();
        assert!(loaded.is_some());
        assert_eq!(loaded.as_ref().map(|s| s.messages.len()), Some(1));
    }

    #[tokio::test]
    async fn overwrite_existing_session() {
        let store = InMemorySessionStore::new();
        let id = "sess-2".to_string();

        let state1 = SessionState {
            messages: vec![Message { role: bob_core::types::Role::User, content: "first".into() }],
            ..SessionState::default()
        };
        store.save(&id, &state1).await.ok();

        let state2 = SessionState {
            messages: vec![
                Message { role: bob_core::types::Role::User, content: "first".into() },
                Message { role: bob_core::types::Role::Assistant, content: "second".into() },
            ],
            ..SessionState::default()
        };
        store.save(&id, &state2).await.ok();

        let loaded = store.load(&id).await.ok().flatten();
        assert_eq!(loaded.as_ref().map(|s| s.messages.len()), Some(2));
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
