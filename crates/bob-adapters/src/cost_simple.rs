//! Simple in-memory cost meter adapter.

use bob_core::{
    error::CostError,
    ports::CostMeterPort,
    types::{SessionId, TokenUsage, ToolResult},
};

#[derive(Debug, Clone, Copy, Default)]
struct SessionCost {
    total_tokens: u64,
    tool_calls: u64,
}

/// In-memory cost meter with optional per-session token budget.
#[derive(Debug)]
pub struct SimpleCostMeter {
    session_token_budget: Option<u64>,
    sessions: scc::HashMap<SessionId, SessionCost>,
}

impl SimpleCostMeter {
    #[must_use]
    pub fn new(session_token_budget: Option<u64>) -> Self {
        Self { session_token_budget, sessions: scc::HashMap::new() }
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
}

#[async_trait::async_trait]
impl CostMeterPort for SimpleCostMeter {
    async fn check_budget(&self, session_id: &SessionId) -> Result<(), CostError> {
        let Some(limit) = self.session_token_budget else {
            return Ok(());
        };
        let total = self.sessions.read_async(session_id, |_k, v| v.total_tokens).await.unwrap_or(0);
        if total >= limit {
            return Err(CostError::BudgetExceeded(format!(
                "session '{session_id}' reached token budget ({total}>={limit})"
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
        let usage_tokens = u64::from(usage.total());
        let entry = self.sessions.entry_async(session_id.clone()).await;
        let total_after = match entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().total_tokens += usage_tokens;
                occ.get().total_tokens
            }
            scc::hash_map::Entry::Vacant(vac) => {
                let inserted =
                    vac.insert_entry(SessionCost { total_tokens: usage_tokens, tool_calls: 0 });
                inserted.get().total_tokens
            }
        };
        self.ensure_session_budget(session_id, total_after)
    }

    async fn record_tool_result(
        &self,
        session_id: &SessionId,
        _tool_result: &ToolResult,
    ) -> Result<(), CostError> {
        let entry = self.sessions.entry_async(session_id.clone()).await;
        match entry {
            scc::hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().tool_calls += 1;
            }
            scc::hash_map::Entry::Vacant(vac) => {
                let _ = vac.insert_entry(SessionCost { total_tokens: 0, tool_calls: 1 });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_budget_never_blocks() {
        let meter = SimpleCostMeter::new(None);
        let session = "s1".to_string();
        assert!(meter.check_budget(&session).await.is_ok());
        assert!(
            meter
                .record_llm_usage(
                    &session,
                    "test-model",
                    &TokenUsage { prompt_tokens: 30, completion_tokens: 20 }
                )
                .await
                .is_ok()
        );
        assert!(meter.check_budget(&session).await.is_ok());
    }

    #[tokio::test]
    async fn check_budget_blocks_after_limit_is_reached() {
        let meter = SimpleCostMeter::new(Some(100));
        let session = "s1".to_string();

        assert!(
            meter
                .record_llm_usage(
                    &session,
                    "test-model",
                    &TokenUsage { prompt_tokens: 60, completion_tokens: 40 }
                )
                .await
                .is_ok()
        );
        let result = meter.check_budget(&session).await;
        assert!(result.is_err());
        let message = result.err().map(|err| err.to_string()).unwrap_or_default();
        assert!(message.contains("budget"));
    }

    #[tokio::test]
    async fn record_usage_fails_when_exceeding_limit() {
        let meter = SimpleCostMeter::new(Some(50));
        let session = "s1".to_string();

        let result = meter
            .record_llm_usage(
                &session,
                "test-model",
                &TokenUsage { prompt_tokens: 40, completion_tokens: 20 },
            )
            .await;
        assert!(result.is_err());
        let message = result.err().map(|err| err.to_string()).unwrap_or_default();
        assert!(message.contains("exceeded"));
    }
}
