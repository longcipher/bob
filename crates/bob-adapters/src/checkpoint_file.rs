//! File-backed turn checkpoint store adapter.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bob_core::{
    error::StoreError,
    ports::TurnCheckpointStorePort,
    types::{SessionId, TurnCheckpoint},
};

/// Durable checkpoint store backed by per-session JSON snapshots.
#[derive(Debug)]
pub struct FileCheckpointStore {
    root: PathBuf,
    cache: scc::HashMap<SessionId, TurnCheckpoint>,
    write_guard: tokio::sync::Mutex<()>,
}

impl FileCheckpointStore {
    /// Create a file-backed checkpoint store rooted at `root`.
    ///
    /// # Errors
    /// Returns a backend error when the root directory cannot be created.
    pub fn new(root: PathBuf) -> Result<Self, StoreError> {
        std::fs::create_dir_all(&root).map_err(|err| {
            StoreError::Backend(format!("failed to create checkpoint dir: {err}"))
        })?;
        Ok(Self { root, cache: scc::HashMap::new(), write_guard: tokio::sync::Mutex::new(()) })
    }

    fn checkpoint_path(&self, session_id: &SessionId) -> PathBuf {
        self.root.join(format!("{}.json", encode_session_id(session_id)))
    }

    fn temp_path_for(final_path: &Path) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        final_path.with_extension(format!("json.tmp.{}.{}", std::process::id(), nanos))
    }

    fn quarantine_path_for(path: &Path) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let filename = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("checkpoint");
        path.with_file_name(format!("{filename}.corrupt.{}.{}", std::process::id(), nanos))
    }

    async fn quarantine_corrupt_file(path: &Path) -> Result<PathBuf, StoreError> {
        let quarantine_path = Self::quarantine_path_for(path);
        tokio::fs::rename(path, &quarantine_path).await.map_err(|err| {
            StoreError::Backend(format!(
                "failed to quarantine corrupted checkpoint '{}': {err}",
                path.display()
            ))
        })?;
        Ok(quarantine_path)
    }

    async fn load_from_disk(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<TurnCheckpoint>, StoreError> {
        let path = self.checkpoint_path(session_id);
        let raw = match tokio::fs::read(&path).await {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(StoreError::Backend(format!(
                    "failed to read checkpoint '{}': {err}",
                    path.display()
                )));
            }
        };

        let checkpoint = if let Ok(value) = serde_json::from_slice::<TurnCheckpoint>(&raw) {
            value
        } else {
            let _ = Self::quarantine_corrupt_file(&path).await?;
            return Ok(None);
        };
        Ok(Some(checkpoint))
    }

    async fn save_to_disk(
        &self,
        session_id: &SessionId,
        checkpoint: &TurnCheckpoint,
    ) -> Result<(), StoreError> {
        let final_path = self.checkpoint_path(session_id);
        let temp_path = Self::temp_path_for(&final_path);
        let bytes = serde_json::to_vec_pretty(checkpoint)
            .map_err(|err| StoreError::Serialization(err.to_string()))?;

        tokio::fs::write(&temp_path, bytes).await.map_err(|err| {
            StoreError::Backend(format!(
                "failed to write temp checkpoint '{}': {err}",
                temp_path.display()
            ))
        })?;

        if let Err(rename_err) = tokio::fs::rename(&temp_path, &final_path).await {
            if path_exists(&final_path).await {
                tokio::fs::remove_file(&final_path).await.map_err(|remove_err| {
                    StoreError::Backend(format!(
                        "failed to replace existing checkpoint '{}' after rename error '{rename_err}': {remove_err}",
                        final_path.display()
                    ))
                })?;
                tokio::fs::rename(&temp_path, &final_path).await.map_err(|err| {
                    StoreError::Backend(format!(
                        "failed to replace checkpoint '{}' after fallback remove: {err}",
                        final_path.display()
                    ))
                })?;
            } else {
                return Err(StoreError::Backend(format!(
                    "failed to persist checkpoint '{}': {rename_err}",
                    final_path.display()
                )));
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl TurnCheckpointStorePort for FileCheckpointStore {
    async fn save_checkpoint(&self, checkpoint: &TurnCheckpoint) -> Result<(), StoreError> {
        let _lock = self.write_guard.lock().await;
        self.save_to_disk(&checkpoint.session_id, checkpoint).await?;
        let entry = self.cache.entry_async(checkpoint.session_id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => occ.get_mut().clone_from(checkpoint),
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(checkpoint.clone());
            }
        }
        Ok(())
    }

    async fn load_latest(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<TurnCheckpoint>, StoreError> {
        if let Some(value) = self.cache.read_async(session_id, |_k, v| v.clone()).await {
            return Ok(Some(value));
        }

        let loaded = self.load_from_disk(session_id).await?;
        if let Some(ref checkpoint) = loaded {
            let entry = self.cache.entry_async(session_id.clone()).await;
            match entry {
                scc::hash_map::Entry::Occupied(mut occ) => occ.get_mut().clone_from(checkpoint),
                scc::hash_map::Entry::Vacant(vac) => {
                    let _ = vac.insert_entry(checkpoint.clone());
                }
            }
        }
        Ok(loaded)
    }
}

fn encode_session_id(session_id: &str) -> String {
    if session_id.is_empty() {
        return "session".to_string();
    }

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
    use bob_core::types::TokenUsage;

    use super::*;

    #[tokio::test]
    async fn missing_checkpoint_returns_none() {
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let dir = match dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let store = FileCheckpointStore::new(dir.path().to_path_buf());
        assert!(store.is_ok());
        let store = match store {
            Ok(value) => value,
            Err(_) => return,
        };

        let loaded = store.load_latest(&"missing".to_string()).await;
        assert!(loaded.is_ok());
        assert!(loaded.ok().flatten().is_none());
    }

    #[tokio::test]
    async fn roundtrip_persists_across_store_recreation() {
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let dir = match dir {
            Ok(value) => value,
            Err(_) => return,
        };

        let checkpoint = TurnCheckpoint {
            session_id: "s/1".to_string(),
            step: 3,
            tool_calls: 2,
            usage: TokenUsage { prompt_tokens: 5, completion_tokens: 6 },
        };

        let first = FileCheckpointStore::new(dir.path().to_path_buf());
        assert!(first.is_ok());
        let first = match first {
            Ok(value) => value,
            Err(_) => return,
        };
        let saved = first.save_checkpoint(&checkpoint).await;
        assert!(saved.is_ok());

        let second = FileCheckpointStore::new(dir.path().to_path_buf());
        assert!(second.is_ok());
        let second = match second {
            Ok(value) => value,
            Err(_) => return,
        };
        let loaded = second.load_latest(&"s/1".to_string()).await;
        assert!(loaded.is_ok());
        let loaded = loaded.ok().flatten();
        assert!(loaded.is_some());
        assert_eq!(loaded.as_ref().map(|cp| cp.step), Some(3));
    }

    #[tokio::test]
    async fn corrupted_checkpoint_is_quarantined_and_treated_as_missing() {
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let dir = match dir {
            Ok(value) => value,
            Err(_) => return,
        };

        let session_id = "broken-checkpoint".to_string();
        let encoded = encode_session_id(&session_id);
        let checkpoint_path = dir.path().join(format!("{encoded}.json"));
        let write = tokio::fs::write(&checkpoint_path, b"{not-json").await;
        assert!(write.is_ok());

        let store = FileCheckpointStore::new(dir.path().to_path_buf());
        assert!(store.is_ok());
        let store = match store {
            Ok(value) => value,
            Err(_) => return,
        };

        let loaded = store.load_latest(&session_id).await;
        assert!(loaded.is_ok());
        assert!(loaded.ok().flatten().is_none());
        assert!(!checkpoint_path.exists());

        let mut has_quarantine = false;
        let read_dir = std::fs::read_dir(dir.path());
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
        assert!(has_quarantine);
    }
}
