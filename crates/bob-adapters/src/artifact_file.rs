//! File-backed artifact store adapter.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bob_core::{
    error::StoreError,
    ports::ArtifactStorePort,
    types::{ArtifactRecord, SessionId},
};

/// Durable artifact store backed by per-session JSON snapshots.
#[derive(Debug)]
pub struct FileArtifactStore {
    root: PathBuf,
    cache: scc::HashMap<SessionId, Vec<ArtifactRecord>>,
    write_guard: tokio::sync::Mutex<()>,
}

impl FileArtifactStore {
    /// Create a file-backed artifact store rooted at `root`.
    ///
    /// # Errors
    /// Returns a backend error when the root directory cannot be created.
    pub fn new(root: PathBuf) -> Result<Self, StoreError> {
        std::fs::create_dir_all(&root)
            .map_err(|err| StoreError::Backend(format!("failed to create artifact dir: {err}")))?;
        Ok(Self { root, cache: scc::HashMap::new(), write_guard: tokio::sync::Mutex::new(()) })
    }

    fn artifact_path(&self, session_id: &SessionId) -> PathBuf {
        self.root.join(format!("{}.json", encode_session_id(session_id)))
    }

    fn temp_path_for(final_path: &Path) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        final_path.with_extension(format!("json.tmp.{}.{}", std::process::id(), nanos))
    }

    fn quarantine_path_for(path: &Path) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let filename = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("artifacts");
        path.with_file_name(format!("{filename}.corrupt.{}.{}", std::process::id(), nanos))
    }

    async fn quarantine_corrupt_file(path: &Path) -> Result<PathBuf, StoreError> {
        let quarantine_path = Self::quarantine_path_for(path);
        tokio::fs::rename(path, &quarantine_path).await.map_err(|err| {
            StoreError::Backend(format!(
                "failed to quarantine corrupted artifacts '{}': {err}",
                path.display()
            ))
        })?;
        Ok(quarantine_path)
    }

    async fn load_from_disk(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<ArtifactRecord>, StoreError> {
        let path = self.artifact_path(session_id);
        let raw = match tokio::fs::read(&path).await {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                return Err(StoreError::Backend(format!(
                    "failed to read artifacts '{}': {err}",
                    path.display()
                )));
            }
        };

        let artifacts = if let Ok(value) = serde_json::from_slice::<Vec<ArtifactRecord>>(&raw) {
            value
        } else {
            let _ = Self::quarantine_corrupt_file(&path).await?;
            return Ok(Vec::new());
        };
        Ok(artifacts)
    }

    async fn save_to_disk(
        &self,
        session_id: &SessionId,
        records: &[ArtifactRecord],
    ) -> Result<(), StoreError> {
        let final_path = self.artifact_path(session_id);
        let temp_path = Self::temp_path_for(&final_path);
        let bytes = serde_json::to_vec_pretty(records)
            .map_err(|err| StoreError::Serialization(err.to_string()))?;

        tokio::fs::write(&temp_path, bytes).await.map_err(|err| {
            StoreError::Backend(format!(
                "failed to write temp artifacts '{}': {err}",
                temp_path.display()
            ))
        })?;

        if let Err(rename_err) = tokio::fs::rename(&temp_path, &final_path).await {
            if path_exists(&final_path).await {
                tokio::fs::remove_file(&final_path).await.map_err(|remove_err| {
                    StoreError::Backend(format!(
                        "failed to replace existing artifacts '{}' after rename error '{rename_err}': {remove_err}",
                        final_path.display()
                    ))
                })?;
                tokio::fs::rename(&temp_path, &final_path).await.map_err(|err| {
                    StoreError::Backend(format!(
                        "failed to replace artifacts '{}' after fallback remove: {err}",
                        final_path.display()
                    ))
                })?;
            } else {
                return Err(StoreError::Backend(format!(
                    "failed to persist artifacts '{}': {rename_err}",
                    final_path.display()
                )));
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl ArtifactStorePort for FileArtifactStore {
    async fn put(&self, artifact: ArtifactRecord) -> Result<(), StoreError> {
        let session_id = artifact.session_id.clone();
        let _lock = self.write_guard.lock().await;

        let mut records = if let Some(cached) =
            self.cache.read_async(&session_id, |_k, value| value.clone()).await
        {
            cached
        } else {
            self.load_from_disk(&session_id).await?
        };
        records.push(artifact);
        self.save_to_disk(&session_id, &records).await?;

        let entry = self.cache.entry_async(session_id).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => occ.get_mut().clone_from(&records),
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(records);
            }
        }
        Ok(())
    }

    async fn list_by_session(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<ArtifactRecord>, StoreError> {
        if let Some(cached) = self.cache.read_async(session_id, |_k, value| value.clone()).await {
            return Ok(cached);
        }

        let records = self.load_from_disk(session_id).await?;
        let entry = self.cache.entry_async(session_id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => occ.get_mut().clone_from(&records),
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(records.clone());
            }
        }
        Ok(records)
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
    use super::*;

    #[tokio::test]
    async fn list_missing_session_returns_empty() {
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let dir = match dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let store = FileArtifactStore::new(dir.path().to_path_buf());
        assert!(store.is_ok());
        let store = match store {
            Ok(value) => value,
            Err(_) => return,
        };

        let listed = store.list_by_session(&"missing".to_string()).await;
        assert!(listed.is_ok());
        assert!(listed.ok().is_some_and(|items| items.is_empty()));
    }

    #[tokio::test]
    async fn roundtrip_persists_across_store_recreation() {
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let dir = match dir {
            Ok(value) => value,
            Err(_) => return,
        };

        let first = FileArtifactStore::new(dir.path().to_path_buf());
        assert!(first.is_ok());
        let first = match first {
            Ok(value) => value,
            Err(_) => return,
        };
        let inserted = first
            .put(ArtifactRecord {
                session_id: "s/1".to_string(),
                kind: "tool_result".to_string(),
                name: "search".to_string(),
                content: serde_json::json!({"hits": 3}),
            })
            .await;
        assert!(inserted.is_ok());

        let second = FileArtifactStore::new(dir.path().to_path_buf());
        assert!(second.is_ok());
        let second = match second {
            Ok(value) => value,
            Err(_) => return,
        };
        let listed = second.list_by_session(&"s/1".to_string()).await;
        assert!(listed.is_ok());
        assert_eq!(listed.ok().map(|items| items.len()), Some(1));
    }

    #[tokio::test]
    async fn corrupted_artifact_snapshot_is_quarantined_and_treated_as_empty() {
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let dir = match dir {
            Ok(value) => value,
            Err(_) => return,
        };

        let session_id = "broken-artifacts".to_string();
        let encoded = encode_session_id(&session_id);
        let artifact_path = dir.path().join(format!("{encoded}.json"));
        let write = tokio::fs::write(&artifact_path, b"{not-json").await;
        assert!(write.is_ok());

        let store = FileArtifactStore::new(dir.path().to_path_buf());
        assert!(store.is_ok());
        let store = match store {
            Ok(value) => value,
            Err(_) => return,
        };
        let listed = store.list_by_session(&session_id).await;
        assert!(listed.is_ok());
        assert!(listed.ok().is_some_and(|records| records.is_empty()));
        assert!(!artifact_path.exists());

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
