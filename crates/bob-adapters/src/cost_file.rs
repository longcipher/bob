//! File-backed cost meter adapter.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bob_core::{
    error::{CostError, StoreError},
    ports::CostMeterPort,
    types::{SessionId, TokenUsage, ToolResult},
};

#[derive(Debug, Clone, Copy, Default)]
struct SessionCost {
    total_tokens: u64,
    tool_calls: u64,
}

impl SessionCost {
    fn from_json_slice(raw: &[u8]) -> Result<Self, StoreError> {
        let value = serde_json::from_slice::<serde_json::Value>(raw)
            .map_err(|err| StoreError::Serialization(err.to_string()))?;
        let object = value
            .as_object()
            .ok_or_else(|| StoreError::Serialization("expected JSON object".to_string()))?;
        let total_tokens =
            object.get("total_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let tool_calls = object.get("tool_calls").and_then(serde_json::Value::as_u64).unwrap_or(0);
        Ok(Self { total_tokens, tool_calls })
    }

    fn to_json_vec(self) -> Result<Vec<u8>, StoreError> {
        serde_json::to_vec_pretty(&serde_json::json!({
            "total_tokens": self.total_tokens,
            "tool_calls": self.tool_calls,
        }))
        .map_err(|err| StoreError::Serialization(err.to_string()))
    }
}

/// Durable cost meter with optional per-session token budget.
#[derive(Debug)]
pub struct FileCostMeter {
    root: PathBuf,
    session_token_budget: Option<u64>,
    cache: scc::HashMap<SessionId, SessionCost>,
    write_guard: tokio::sync::Mutex<()>,
}

impl FileCostMeter {
    /// Create a file-backed cost meter rooted at `root`.
    ///
    /// # Errors
    /// Returns a backend error when the root directory cannot be created.
    pub fn new(root: PathBuf, session_token_budget: Option<u64>) -> Result<Self, CostError> {
        std::fs::create_dir_all(&root)
            .map_err(|err| CostError::Backend(format!("failed to create cost dir: {err}")))?;
        Ok(Self {
            root,
            session_token_budget,
            cache: scc::HashMap::new(),
            write_guard: tokio::sync::Mutex::new(()),
        })
    }

    fn cost_path(&self, session_id: &SessionId) -> PathBuf {
        self.root.join(format!("{}.json", encode_session_id(session_id)))
    }

    fn temp_path_for(final_path: &Path) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        final_path.with_extension(format!("json.tmp.{}.{}", std::process::id(), nanos))
    }

    fn quarantine_path_for(path: &Path) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let filename = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("cost");
        path.with_file_name(format!("{filename}.corrupt.{}.{}", std::process::id(), nanos))
    }

    async fn quarantine_corrupt_file(path: &Path) -> Result<PathBuf, CostError> {
        let quarantine_path = Self::quarantine_path_for(path);
        tokio::fs::rename(path, &quarantine_path).await.map_err(|err| {
            CostError::Backend(format!(
                "failed to quarantine corrupted cost snapshot '{}': {err}",
                path.display()
            ))
        })?;
        Ok(quarantine_path)
    }

    async fn load_from_disk(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<SessionCost>, CostError> {
        let path = self.cost_path(session_id);
        let raw = match tokio::fs::read(&path).await {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(CostError::Backend(format!(
                    "failed to read cost snapshot '{}': {err}",
                    path.display()
                )));
            }
        };

        if let Ok(cost) = SessionCost::from_json_slice(&raw) {
            return Ok(Some(cost));
        }

        let _ = Self::quarantine_corrupt_file(&path).await?;
        Ok(None)
    }

    async fn save_to_disk(
        &self,
        session_id: &SessionId,
        cost: SessionCost,
    ) -> Result<(), CostError> {
        let final_path = self.cost_path(session_id);
        let temp_path = Self::temp_path_for(&final_path);
        let bytes = cost.to_json_vec().map_err(|err| {
            CostError::Backend(format!("failed to serialize cost snapshot: {err}"))
        })?;

        tokio::fs::write(&temp_path, bytes).await.map_err(|err| {
            CostError::Backend(format!(
                "failed to write temp cost snapshot '{}': {err}",
                temp_path.display()
            ))
        })?;

        if let Err(rename_err) = tokio::fs::rename(&temp_path, &final_path).await {
            if path_exists(&final_path).await {
                tokio::fs::remove_file(&final_path).await.map_err(|remove_err| {
                    CostError::Backend(format!(
                        "failed to replace existing cost snapshot '{}' after rename error '{rename_err}': {remove_err}",
                        final_path.display()
                    ))
                })?;
                tokio::fs::rename(&temp_path, &final_path).await.map_err(|err| {
                    CostError::Backend(format!(
                        "failed to replace cost snapshot '{}' after fallback remove: {err}",
                        final_path.display()
                    ))
                })?;
            } else {
                return Err(CostError::Backend(format!(
                    "failed to persist cost snapshot '{}': {rename_err}",
                    final_path.display()
                )));
            }
        }
        Ok(())
    }

    fn ensure_session_budget(
        &self,
        session_id: &SessionId,
        total_tokens: u64,
    ) -> Result<(), CostError> {
        let Some(limit) = self.session_token_budget else {
            return Ok(());
        };
        if total_tokens > limit {
            return Err(CostError::BudgetExceeded(format!(
                "session '{session_id}' exceeded token budget ({total_tokens}>{limit})"
            )));
        }
        Ok(())
    }

    async fn read_session_cost(&self, session_id: &SessionId) -> Result<SessionCost, CostError> {
        if let Some(cost) = self.cache.read_async(session_id, |_k, value| *value).await {
            return Ok(cost);
        }

        let loaded = self.load_from_disk(session_id).await?.unwrap_or_default();
        let entry = self.cache.entry_async(session_id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                *occ.get_mut() = loaded;
            }
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(loaded);
            }
        }
        Ok(loaded)
    }

    async fn write_session_cost(
        &self,
        session_id: &SessionId,
        session_cost: SessionCost,
    ) -> Result<(), CostError> {
        self.save_to_disk(session_id, session_cost).await?;
        let entry = self.cache.entry_async(session_id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                *occ.get_mut() = session_cost;
            }
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(session_cost);
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl CostMeterPort for FileCostMeter {
    async fn check_budget(&self, session_id: &SessionId) -> Result<(), CostError> {
        let Some(limit) = self.session_token_budget else {
            return Ok(());
        };
        let session_cost = self.read_session_cost(session_id).await?;
        if session_cost.total_tokens >= limit {
            return Err(CostError::BudgetExceeded(format!(
                "session '{session_id}' reached token budget ({}>={limit})",
                session_cost.total_tokens
            )));
        }
        Ok(())
    }

    async fn record_llm_usage(
        &self,
        session_id: &SessionId,
        _model: &str,
        usage: &TokenUsage,
    ) -> Result<(), CostError> {
        let _lock = self.write_guard.lock().await;
        let mut session_cost = self.read_session_cost(session_id).await?;
        session_cost.total_tokens =
            session_cost.total_tokens.saturating_add(u64::from(usage.total()));
        self.write_session_cost(session_id, session_cost).await?;
        self.ensure_session_budget(session_id, session_cost.total_tokens)
    }

    async fn record_tool_result(
        &self,
        session_id: &SessionId,
        _tool_result: &ToolResult,
    ) -> Result<(), CostError> {
        let _lock = self.write_guard.lock().await;
        let mut session_cost = self.read_session_cost(session_id).await?;
        session_cost.tool_calls = session_cost.tool_calls.saturating_add(1);
        self.write_session_cost(session_id, session_cost).await
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
    async fn no_budget_never_blocks() {
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let dir = match dir {
            Ok(value) => value,
            Err(_) => return,
        };

        let meter = FileCostMeter::new(dir.path().to_path_buf(), None);
        assert!(meter.is_ok());
        let meter = match meter {
            Ok(value) => value,
            Err(_) => return,
        };
        let session = "s1".to_string();
        assert!(meter.check_budget(&session).await.is_ok());
        assert!(
            meter
                .record_llm_usage(
                    &session,
                    "test-model",
                    &TokenUsage { prompt_tokens: 10, completion_tokens: 5 }
                )
                .await
                .is_ok()
        );
        assert!(meter.check_budget(&session).await.is_ok());
    }

    #[tokio::test]
    async fn usage_persists_across_recreation() {
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let dir = match dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let session = "s1".to_string();

        let first = FileCostMeter::new(dir.path().to_path_buf(), Some(50));
        assert!(first.is_ok());
        let first = match first {
            Ok(value) => value,
            Err(_) => return,
        };
        let usage = first
            .record_llm_usage(
                &session,
                "test-model",
                &TokenUsage { prompt_tokens: 30, completion_tokens: 0 },
            )
            .await;
        assert!(usage.is_ok());

        let second = FileCostMeter::new(dir.path().to_path_buf(), Some(50));
        assert!(second.is_ok());
        let second = match second {
            Ok(value) => value,
            Err(_) => return,
        };
        let budget = second.check_budget(&session).await;
        assert!(budget.is_ok(), "persisted usage below budget should pass");
        let overflow = second
            .record_llm_usage(
                &session,
                "test-model",
                &TokenUsage { prompt_tokens: 25, completion_tokens: 0 },
            )
            .await;
        assert!(overflow.is_err(), "persisted and new usage should trigger budget");

        let third = FileCostMeter::new(dir.path().to_path_buf(), Some(50));
        assert!(third.is_ok());
        let third = match third {
            Ok(value) => value,
            Err(_) => return,
        };
        let budget = third.check_budget(&session).await;
        assert!(budget.is_err(), "budget state should survive process restart");
    }

    #[tokio::test]
    async fn corrupted_snapshot_is_quarantined_and_treated_as_empty() {
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let dir = match dir {
            Ok(value) => value,
            Err(_) => return,
        };
        let session = "broken-cost".to_string();
        let encoded = encode_session_id(&session);
        let path = dir.path().join(format!("{encoded}.json"));
        let write = tokio::fs::write(&path, b"{not-json").await;
        assert!(write.is_ok());

        let meter = FileCostMeter::new(dir.path().to_path_buf(), Some(10));
        assert!(meter.is_ok());
        let meter = match meter {
            Ok(value) => value,
            Err(_) => return,
        };
        let budget = meter.check_budget(&session).await;
        assert!(budget.is_ok(), "corrupt snapshot should not block runtime start");
        assert!(!path.exists(), "corrupt file should be quarantined");

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
