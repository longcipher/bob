//! In-memory artifact store adapter.

use bob_core::{
    error::StoreError,
    ports::ArtifactStorePort,
    types::{ArtifactRecord, SessionId},
};

/// In-memory artifact store grouped by session id.
#[derive(Debug, Default)]
pub struct InMemoryArtifactStore {
    inner: scc::HashMap<SessionId, Vec<ArtifactRecord>>,
}

impl InMemoryArtifactStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl ArtifactStorePort for InMemoryArtifactStore {
    async fn put(&self, artifact: ArtifactRecord) -> Result<(), StoreError> {
        let entry = self.inner.entry_async(artifact.session_id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().push(artifact);
            }
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(vec![artifact]);
            }
        }
        Ok(())
    }

    async fn list_by_session(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<ArtifactRecord>, StoreError> {
        Ok(self.inner.read_async(session_id, |_k, v| v.clone()).await.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stores_and_lists_artifacts() {
        let store = InMemoryArtifactStore::new();
        let put = store
            .put(ArtifactRecord {
                session_id: "s1".to_string(),
                kind: "tool_result".to_string(),
                name: "search".to_string(),
                content: serde_json::json!({"hits": 3}),
            })
            .await;
        assert!(put.is_ok());

        let listed = store.list_by_session(&"s1".to_string()).await;
        assert!(listed.is_ok());
        assert_eq!(listed.ok().map(|records| records.len()), Some(1));
    }
}
