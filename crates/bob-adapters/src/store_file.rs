//! File-backed session store adapter.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bob_core::{
    error::StoreError,
    ports::SessionStore,
    types::{SessionId, SessionState},
};

/// Durable session store backed by per-session JSON files.
///
/// Session snapshots are cached in-memory for fast reads and also persisted on
/// every save so process restarts can recover context.
#[derive(Debug)]
pub struct FileSessionStore {
    root: PathBuf,
    cache: scc::HashMap<SessionId, SessionState>,
    write_guard: tokio::sync::Mutex<()>,
}

impl FileSessionStore {
    /// Create a file-backed store rooted at `root`.
    ///
    /// # Errors
    /// Returns a backend error when the root directory cannot be created.
    pub fn new(root: PathBuf) -> Result<Self, StoreError> {
        std::fs::create_dir_all(&root)
            .map_err(|err| StoreError::Backend(format!("failed to create store dir: {err}")))?;
        Ok(Self { root, cache: scc::HashMap::new(), write_guard: tokio::sync::Mutex::new(()) })
    }

    fn session_path(&self, session_id: &SessionId) -> PathBuf {
        self.root.join(format!("{}.json", encode_session_id(session_id)))
    }

    fn temp_path_for(final_path: &Path) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        final_path.with_extension(format!("json.tmp.{}.{}", std::process::id(), nanos))
    }

    fn quarantine_path_for(path: &Path) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let filename = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("snapshot");
        path.with_file_name(format!("{filename}.corrupt.{}.{}", std::process::id(), nanos))
    }

    async fn quarantine_corrupt_file(path: &Path) -> Result<PathBuf, StoreError> {
        let quarantine_path = Self::quarantine_path_for(path);
        tokio::fs::rename(path, &quarantine_path).await.map_err(|err| {
            StoreError::Backend(format!(
                "failed to quarantine corrupted snapshot '{}': {err}",
                path.display()
            ))
        })?;
        Ok(quarantine_path)
    }

    async fn load_from_disk(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<SessionState>, StoreError> {
        let path = self.session_path(session_id);
        let raw = match tokio::fs::read(&path).await {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(StoreError::Backend(format!(
                    "failed to read session snapshot '{}': {err}",
                    path.display()
                )));
            }
        };

        let state = if let Ok(value) = serde_json::from_slice::<SessionState>(&raw) {
            value
        } else {
            let _ = Self::quarantine_corrupt_file(&path).await?;
            return Ok(None);
        };
        Ok(Some(state))
    }

    async fn save_to_disk(
        &self,
        session_id: &SessionId,
        state: &SessionState,
    ) -> Result<(), StoreError> {
        let final_path = self.session_path(session_id);
        let temp_path = Self::temp_path_for(&final_path);
        let bytes = serde_json::to_vec_pretty(state)
            .map_err(|err| StoreError::Serialization(err.to_string()))?;

        tokio::fs::write(&temp_path, bytes).await.map_err(|err| {
            StoreError::Backend(format!(
                "failed to write temp session snapshot '{}': {err}",
                temp_path.display()
            ))
        })?;

        // Prefer direct atomic replace. Fallback to remove+rename only when needed.
        if let Err(rename_err) = tokio::fs::rename(&temp_path, &final_path).await {
            if path_exists(&final_path).await {
                tokio::fs::remove_file(&final_path).await.map_err(|remove_err| {
                    StoreError::Backend(format!(
                        "failed to replace existing session snapshot '{}' after rename error '{rename_err}': {remove_err}",
                        final_path.display()
                    ))
                })?;
                tokio::fs::rename(&temp_path, &final_path).await.map_err(|err| {
                    StoreError::Backend(format!(
                        "failed to replace session snapshot '{}' after fallback remove: {err}",
                        final_path.display()
                    ))
                })?;
            } else {
                return Err(StoreError::Backend(format!(
                    "failed to atomically replace session snapshot '{}': {rename_err}",
                    final_path.display()
                )));
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl SessionStore for FileSessionStore {
    async fn load(&self, id: &SessionId) -> Result<Option<SessionState>, StoreError> {
        if let Some(state) = self.cache.read_async(id, |_key, value| value.clone()).await {
            return Ok(Some(state));
        }

        let loaded = self.load_from_disk(id).await?;
        if let Some(ref state) = loaded {
            let entry = self.cache.entry_async(id.clone()).await;
            match entry {
                scc::hash_map::Entry::Occupied(mut occ) => occ.get_mut().clone_from(state),
                scc::hash_map::Entry::Vacant(vac) => {
                    let _ = vac.insert_entry(state.clone());
                }
            }
        }
        Ok(loaded)
    }

    async fn save(&self, id: &SessionId, state: &SessionState) -> Result<(), StoreError> {
        let _lock = self.write_guard.lock().await;
        let mut updated = state.clone();
        // Read current version from cache or disk.
        let current_version = self.cache.read_async(id, |_k, v| v.version).await.unwrap_or(0);
        updated.version = current_version.saturating_add(1);
        self.save_to_disk(id, &updated).await?;

        let entry = self.cache.entry_async(id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().clone_from(&updated);
            }
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(updated);
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
        let _lock = self.write_guard.lock().await;
        // Read current version from cache or disk.
        let current_version = if let Some(v) = self.cache.read_async(id, |_k, v| v.version).await {
            v
        } else {
            let loaded = self.load_from_disk(id).await?;
            loaded.map_or(0, |s| s.version)
        };

        if current_version != expected_version {
            return Err(StoreError::VersionConflict {
                expected: expected_version,
                actual: current_version,
            });
        }

        let new_version = expected_version.saturating_add(1);
        let mut updated = state.clone();
        updated.version = new_version;
        self.save_to_disk(id, &updated).await?;

        let entry = self.cache.entry_async(id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().clone_from(&updated);
            }
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(updated);
            }
        }
        Ok(new_version)
    }
}

fn encode_session_id(session_id: &str) -> String {
    if session_id.is_empty() {
        return "session".to_string();
    }

    // Hex-encode all bytes to avoid collisions from lossy sanitization.
    let mut encoded = String::with_capacity(session_id.len().saturating_mul(2));
    for byte in session_id.as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

async fn path_exists(path: &Path) -> bool {
    tokio::fs::metadata(path).await.is_ok()
}

#[cfg(test)]
mod tests {
    use bob_core::types::{Message, Role};

    use super::*;

    #[tokio::test]
    async fn missing_session_returns_none() {
        let temp_dir = tempfile::tempdir();
        assert!(temp_dir.is_ok(), "tempdir should be created");
        let temp_dir = match temp_dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let store = FileSessionStore::new(temp_dir.path().to_path_buf());
        assert!(store.is_ok(), "store should initialize");
        let store = match store {
            Ok(value) => value,
            Err(_) => return,
        };

        let loaded = store.load(&"missing".to_string()).await;
        assert!(loaded.is_ok(), "load should not fail for missing sessions");
        assert!(loaded.ok().flatten().is_none());
    }

    #[tokio::test]
    async fn roundtrip_persists_across_store_recreation() {
        let temp_dir = tempfile::tempdir();
        assert!(temp_dir.is_ok(), "tempdir should be created");
        let temp_dir = match temp_dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let session_id = "cli/session:1".to_string();
        let state = SessionState {
            messages: vec![Message::text(Role::User, "hello")],
            total_usage: bob_core::types::TokenUsage { prompt_tokens: 4, completion_tokens: 2 },
            ..Default::default()
        };

        let first = FileSessionStore::new(temp_dir.path().to_path_buf());
        assert!(first.is_ok(), "first store should initialize");
        let first = match first {
            Ok(value) => value,
            Err(_) => return,
        };
        let saved = first.save(&session_id, &state).await;
        assert!(saved.is_ok(), "save should succeed");

        let second = FileSessionStore::new(temp_dir.path().to_path_buf());
        assert!(second.is_ok(), "second store should initialize");
        let second = match second {
            Ok(value) => value,
            Err(_) => return,
        };
        let loaded = second.load(&session_id).await;
        assert!(loaded.is_ok(), "load should succeed after restart");
        let loaded = loaded.ok().flatten();
        assert!(loaded.is_some(), "session should exist");
        let loaded = loaded.unwrap_or_default();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.total_usage.total(), 6);
    }

    #[tokio::test]
    async fn corrupted_snapshot_is_quarantined_and_treated_as_missing() {
        let temp_dir = tempfile::tempdir();
        assert!(temp_dir.is_ok(), "tempdir should be created");
        let temp_dir = match temp_dir {
            Ok(value) => value,
            Err(_) => return,
        };

        let session_id = "broken-session".to_string();
        let encoded = encode_session_id(&session_id);
        let snapshot_path = temp_dir.path().join(format!("{encoded}.json"));
        let write = tokio::fs::write(&snapshot_path, b"{not-json").await;
        assert!(write.is_ok(), "fixture should be written");

        let store = FileSessionStore::new(temp_dir.path().to_path_buf());
        assert!(store.is_ok(), "store should initialize");
        let store = match store {
            Ok(value) => value,
            Err(_) => return,
        };
        let loaded = store.load(&session_id).await;
        assert!(loaded.is_ok(), "load should recover from corruption");
        assert!(loaded.ok().flatten().is_none(), "corrupted session should be treated as missing");
        assert!(
            !snapshot_path.exists(),
            "corrupted snapshot should be moved out of the primary location"
        );

        let mut has_quarantine = false;
        let read_dir = std::fs::read_dir(temp_dir.path());
        assert!(read_dir.is_ok());
        if let Ok(entries) = read_dir {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.contains(".corrupt.") {
                    has_quarantine = true;
                    break;
                }
            }
        }
        assert!(has_quarantine, "corrupted snapshot should be quarantined");
    }

    #[tokio::test]
    async fn session_id_path_is_sanitized() {
        let temp_dir = tempfile::tempdir();
        assert!(temp_dir.is_ok(), "tempdir should be created");
        let temp_dir = match temp_dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let store = FileSessionStore::new(temp_dir.path().to_path_buf());
        assert!(store.is_ok(), "store should initialize");
        let store = match store {
            Ok(value) => value,
            Err(_) => return,
        };
        let session_id = "../escape?*id".to_string();
        let state = SessionState::default();

        let saved = store.save(&session_id, &state).await;
        assert!(saved.is_ok(), "save should succeed");
        let path = store.session_path(&session_id);
        assert!(path.starts_with(temp_dir.path()), "path must stay in store root");
    }

    #[test]
    fn session_id_encoding_is_collision_resistant_for_common_cases() {
        let a = encode_session_id("a/b");
        let b = encode_session_id("a?b");
        assert_ne!(a, b, "different session ids should map to different filenames");
    }
}
