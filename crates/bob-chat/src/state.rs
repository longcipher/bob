//! # Type-Safe State Container
//!
//! A lightweight, type-safe dependency injection container for sharing state
//! across chat event handlers without manual `Arc` cloning.
//!
//! Inspired by `actix-web::Extensions` and `http::Extensions`.
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_chat::state::AppState;
//!
//! let mut state = AppState::new();
//! state.insert(DatabasePool::new());
//! state.insert(Config::load());
//!
//! // In a handler:
//! let db = state.get::<DatabasePool>().unwrap();
//! ```

use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

/// Type-safe state container using `TypeId` as key.
///
/// Stores one value per type. Inserting a value of type `T` replaces
/// any previous value of the same type.
#[derive(Default)]
pub struct AppState {
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let type_names: Vec<_> = self.map.keys().map(|id| format!("{id:?}")).collect();
        f.debug_struct("AppState").field("types", &type_names).finish()
    }
}

impl AppState {
    /// Create an empty state container.
    #[must_use]
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    /// Insert a value of type `T`. Replaces any previous value of the same type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, val: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(val));
    }

    /// Get an immutable reference to the value of type `T`.
    #[must_use]
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.map.get(&TypeId::of::<T>()).and_then(|boxed| boxed.downcast_ref())
    }

    /// Get a mutable reference to the value of type `T`.
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.map.get_mut(&TypeId::of::<T>()).and_then(|boxed| boxed.downcast_mut())
    }

    /// Remove and return the value of type `T`.
    pub fn remove<T: 'static>(&mut self) -> Option<T> {
        self.map
            .remove(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast().ok())
            .map(|boxed| *boxed)
    }

    /// Check if a value of type `T` is present.
    #[must_use]
    pub fn contains<T: 'static>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }

    /// Return the number of stored values.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Return `true` if no values are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Clear all stored values.
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct DbPool(String);

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Config {
        max_connections: u32,
    }

    #[test]
    fn insert_and_get() {
        let mut state = AppState::new();
        state.insert(DbPool("postgres://localhost".into()));

        let pool = state.get::<DbPool>().unwrap();
        assert_eq!(pool.0, "postgres://localhost");
    }

    #[test]
    fn different_types_coexist() {
        let mut state = AppState::new();
        state.insert(DbPool("pg".into()));
        state.insert(Config { max_connections: 10 });

        assert_eq!(state.get::<DbPool>().unwrap().0, "pg");
        assert_eq!(state.get::<Config>().unwrap().max_connections, 10);
    }

    #[test]
    fn insert_replaces_previous() {
        let mut state = AppState::new();
        state.insert(DbPool("old".into()));
        state.insert(DbPool("new".into()));

        assert_eq!(state.get::<DbPool>().unwrap().0, "new");
    }

    #[test]
    fn get_missing_returns_none() {
        let state = AppState::new();
        assert!(state.get::<DbPool>().is_none());
    }

    #[test]
    fn remove_returns_value() {
        let mut state = AppState::new();
        state.insert(DbPool("pg".into()));

        let removed = state.remove::<DbPool>().unwrap();
        assert_eq!(removed.0, "pg");
        assert!(state.get::<DbPool>().is_none());
    }

    #[test]
    fn contains_check() {
        let mut state = AppState::new();
        assert!(!state.contains::<DbPool>());

        state.insert(DbPool("pg".into()));
        assert!(state.contains::<DbPool>());
    }

    #[test]
    fn len_and_is_empty() {
        let mut state = AppState::new();
        assert!(state.is_empty());
        assert_eq!(state.len(), 0);

        state.insert(DbPool("pg".into()));
        state.insert(Config { max_connections: 5 });
        assert_eq!(state.len(), 2);
        assert!(!state.is_empty());
    }

    #[test]
    fn clear_removes_all() {
        let mut state = AppState::new();
        state.insert(DbPool("pg".into()));
        state.insert(Config { max_connections: 5 });

        state.clear();
        assert!(state.is_empty());
    }

    #[test]
    fn get_mut_allows_mutation() {
        let mut state = AppState::new();
        state.insert(Config { max_connections: 5 });

        state.get_mut::<Config>().unwrap().max_connections = 20;
        assert_eq!(state.get::<Config>().unwrap().max_connections, 20);
    }

    // AppState must be Send + Sync for use across async tasks.
    const _: () = {
        const fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AppState>();
    };
}
