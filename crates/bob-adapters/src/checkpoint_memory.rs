//! In-memory turn checkpoint store adapter.

use bob_core::{
    error::StoreError,
    ports::TurnCheckpointStorePort,
    types::{SessionId, TurnCheckpoint},
};

/// In-memory checkpoint store keyed by session id.
#[derive(Debug, Default)]
pub struct InMemoryCheckpointStore {
    inner: scc::HashMap<SessionId, TurnCheckpoint>,
}

impl InMemoryCheckpointStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl TurnCheckpointStorePort for InMemoryCheckpointStore {
    async fn save_checkpoint(&self, checkpoint: &TurnCheckpoint) -> Result<(), StoreError> {
        let entry = self.inner.entry_async(checkpoint.session_id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().clone_from(checkpoint);
            }
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
        Ok(self.inner.read_async(session_id, |_k, v| v.clone()).await)
    }
}

#[cfg(test)]
mod tests {
    use bob_core::types::TokenUsage;

    use super::*;

    #[tokio::test]
    async fn roundtrip_checkpoint() {
        let store = InMemoryCheckpointStore::new();
        let checkpoint = TurnCheckpoint {
            session_id: "s1".to_string(),
            step: 2,
            tool_calls: 1,
            usage: TokenUsage { prompt_tokens: 10, completion_tokens: 5 },
        };
        let saved = store.save_checkpoint(&checkpoint).await;
        assert!(saved.is_ok());

        let loaded = store.load_latest(&"s1".to_string()).await;
        assert!(loaded.is_ok());
        assert_eq!(loaded.ok().flatten().map(|c| c.step), Some(2));
    }
}
