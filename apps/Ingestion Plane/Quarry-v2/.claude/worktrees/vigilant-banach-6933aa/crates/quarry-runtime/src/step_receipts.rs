//! Step receipts — append-only audit trail for agent actions.
//!
//! Every `AgentAction` executed by `AgentLoop` produces a receipt that's
//! either:
//! - Emitted as an `action.completed` / `action.failed` event (existing path)
//! - Persisted via [`StepReceiptStore`] (this module) for replay, audit, and
//!   debugging
//!
//! Stagehand-style "step receipts" are the contract that lets reviewers and
//! oversight tools answer "what exactly did the agent do?" without replaying
//! the whole loop. Quarry collects them; Model Plane reasons about them; App
//! Shell renders them.
//!
//! Persistence is pluggable — `InMemoryStore` for dev/test, `S3Store` for
//! production (artifact-bucket flavor), or a control-plane HTTP store for
//! organization-scoped storage. All stores are append-only; never mutate or
//! delete a receipt. To override a step, append a corrective receipt with
//! `correction_of: <prior_id>`.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use quarry_core::contracts::{AgentAction, BrowserObservation};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::RunKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepReceipt {
    pub receipt_id: String,
    pub run_id: String,
    pub step: u32,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub action: AgentAction,
    pub outcome: StepOutcome,
    /// Snapshot of the post-action observation, if any. Stored so reviewers
    /// can see the world state the agent reacted to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<BrowserObservation>,
    /// When the agent retried after an error, points back at the original.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correction_of: Option<String>,
    /// Cost spent by this step in micro-USD (so receipts can be summed
    /// without floating-point drift).
    pub cost_micro_usd: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StepOutcome {
    Completed { latency_ms: u64 },
    Failed { error_code: String, message: String },
    Skipped { reason: String },
}

#[async_trait]
pub trait StepReceiptStore: Send + Sync {
    /// Append a receipt. MUST be idempotent on `receipt_id`.
    async fn append(&self, receipt: StepReceipt) -> QuarryResult<()>;

    /// Fetch all receipts for a run, ordered by `step` ascending.
    async fn list(&self, run_id: &RunKind) -> QuarryResult<Vec<StepReceipt>>;

    /// Fetch a single receipt by ID.
    async fn get(&self, receipt_id: &str) -> QuarryResult<Option<StepReceipt>>;
}

/// In-process append-only store for tests and single-node dev. Receipts are
/// keyed by `run_id` so `list()` is a single hash lookup.
#[derive(Default, Clone)]
pub struct InMemoryStepReceiptStore {
    by_run: Arc<RwLock<HashMap<String, Vec<StepReceipt>>>>,
    by_id: Arc<RwLock<HashMap<String, StepReceipt>>>,
}

impl InMemoryStepReceiptStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn count(&self) -> usize {
        self.by_id.read().await.len()
    }
}

#[async_trait]
impl StepReceiptStore for InMemoryStepReceiptStore {
    async fn append(&self, receipt: StepReceipt) -> QuarryResult<()> {
        let id = receipt.receipt_id.clone();
        let run = receipt.run_id.clone();

        // Idempotent: skip if we've seen this receipt_id before.
        let mut by_id = self.by_id.write().await;
        if by_id.contains_key(&id) {
            return Ok(());
        }
        by_id.insert(id.clone(), receipt.clone());
        drop(by_id);

        let mut by_run = self.by_run.write().await;
        by_run.entry(run).or_default().push(receipt);
        Ok(())
    }

    async fn list(&self, run_id: &RunKind) -> QuarryResult<Vec<StepReceipt>> {
        let by_run = self.by_run.read().await;
        let mut receipts = by_run.get(&run_id.to_string()).cloned().unwrap_or_default();
        receipts.sort_by_key(|r| r.step);
        Ok(receipts)
    }

    async fn get(&self, receipt_id: &str) -> QuarryResult<Option<StepReceipt>> {
        Ok(self.by_id.read().await.get(receipt_id).cloned())
    }
}

/// Builder helper — agent loops use this to construct receipts inline without
/// hand-rolling timestamps.
pub struct ReceiptBuilder {
    run_id: String,
    step: u32,
    started_at: DateTime<Utc>,
    correction_of: Option<String>,
}

impl ReceiptBuilder {
    pub fn start(run_id: impl Into<String>, step: u32) -> Self {
        Self {
            run_id: run_id.into(),
            step,
            started_at: Utc::now(),
            correction_of: None,
        }
    }

    pub fn correction_of(mut self, prior_receipt_id: impl Into<String>) -> Self {
        self.correction_of = Some(prior_receipt_id.into());
        self
    }

    pub fn complete(
        self,
        action: AgentAction,
        observation: Option<BrowserObservation>,
        cost_usd: f64,
    ) -> StepReceipt {
        let finished_at = Utc::now();
        let latency_ms = (finished_at - self.started_at).num_milliseconds().max(0) as u64;
        StepReceipt {
            receipt_id: format!("rcpt_{}", ulid::Ulid::new()),
            run_id: self.run_id,
            step: self.step,
            started_at: self.started_at,
            finished_at,
            action,
            outcome: StepOutcome::Completed { latency_ms },
            observation,
            correction_of: self.correction_of,
            cost_micro_usd: (cost_usd * 1_000_000.0).max(0.0) as u64,
        }
    }

    pub fn fail(self, action: AgentAction, err: &QuarryError) -> StepReceipt {
        let finished_at = Utc::now();
        StepReceipt {
            receipt_id: format!("rcpt_{}", ulid::Ulid::new()),
            run_id: self.run_id,
            step: self.step,
            started_at: self.started_at,
            finished_at,
            action,
            outcome: StepOutcome::Failed {
                error_code: format!("{:?}", err.code).to_uppercase(),
                message: err.message.clone(),
            },
            observation: None,
            correction_of: self.correction_of,
            cost_micro_usd: 0,
        }
    }

    pub fn skip(self, action: AgentAction, reason: impl Into<String>) -> StepReceipt {
        let finished_at = Utc::now();
        StepReceipt {
            receipt_id: format!("rcpt_{}", ulid::Ulid::new()),
            run_id: self.run_id,
            step: self.step,
            started_at: self.started_at,
            finished_at,
            action,
            outcome: StepOutcome::Skipped {
                reason: reason.into(),
            },
            observation: None,
            correction_of: self.correction_of,
            cost_micro_usd: 0,
        }
    }
}

/// Helper to verify a sequence of receipts is well-ordered (step counts
/// strictly increasing) and contains no duplicate receipt_ids.
pub fn validate_sequence(receipts: &[StepReceipt]) -> QuarryResult<()> {
    let mut prev_step: i64 = -1;
    let mut seen = std::collections::HashSet::new();
    for r in receipts {
        if !seen.insert(r.receipt_id.clone()) {
            return Err(QuarryError::new(
                ErrorCode::Conflict,
                format!("duplicate receipt_id: {}", r.receipt_id),
            ));
        }
        if (r.step as i64) <= prev_step {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                format!("receipts not strictly increasing: step {} after {}", r.step, prev_step),
            ));
        }
        prev_step = r.step as i64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::contracts::AgentAction;
    use quarry_core::ids::Id;

    fn sample_action() -> AgentAction {
        AgentAction::Navigate {
            url: "https://example.com".into(),
        }
    }

    #[tokio::test]
    async fn append_and_list_returns_ordered_by_step() {
        let store = InMemoryStepReceiptStore::new();
        let run_id: RunKind = Id::new();

        let r2 = ReceiptBuilder::start(run_id.to_string(), 2).complete(sample_action(), None, 0.001);
        let r1 = ReceiptBuilder::start(run_id.to_string(), 1).complete(sample_action(), None, 0.001);
        let r3 = ReceiptBuilder::start(run_id.to_string(), 3).complete(sample_action(), None, 0.001);

        store.append(r2.clone()).await.unwrap();
        store.append(r1.clone()).await.unwrap();
        store.append(r3.clone()).await.unwrap();

        let listed = store.list(&run_id).await.unwrap();
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].step, 1);
        assert_eq!(listed[1].step, 2);
        assert_eq!(listed[2].step, 3);
    }

    #[tokio::test]
    async fn append_is_idempotent_on_receipt_id() {
        let store = InMemoryStepReceiptStore::new();
        let run_id: RunKind = Id::new();
        let r = ReceiptBuilder::start(run_id.to_string(), 1).complete(sample_action(), None, 0.0);
        store.append(r.clone()).await.unwrap();
        store.append(r.clone()).await.unwrap();
        store.append(r).await.unwrap();
        assert_eq!(store.count().await, 1);
    }

    #[tokio::test]
    async fn list_unknown_run_returns_empty() {
        let store = InMemoryStepReceiptStore::new();
        let run_id: RunKind = Id::new();
        let listed = store.list(&run_id).await.unwrap();
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn get_returns_some_when_present() {
        let store = InMemoryStepReceiptStore::new();
        let run_id: RunKind = Id::new();
        let r = ReceiptBuilder::start(run_id.to_string(), 1).complete(sample_action(), None, 0.0);
        let id = r.receipt_id.clone();
        store.append(r).await.unwrap();
        let got = store.get(&id).await.unwrap();
        assert!(got.is_some());
    }

    #[tokio::test]
    async fn fail_receipt_records_error_outcome() {
        let err = QuarryError::new(ErrorCode::Timeout, "slow page");
        let receipt = ReceiptBuilder::start("run_x", 1).fail(sample_action(), &err);
        match &receipt.outcome {
            StepOutcome::Failed { error_code, message } => {
                assert_eq!(error_code, "TIMEOUT");
                assert_eq!(message, "slow page");
            }
            _ => panic!("expected Failed outcome"),
        }
    }

    #[tokio::test]
    async fn correction_of_links_back_to_prior_receipt() {
        let prior = ReceiptBuilder::start("run_x", 1).complete(sample_action(), None, 0.0);
        let corrective = ReceiptBuilder::start("run_x", 2)
            .correction_of(prior.receipt_id.clone())
            .complete(sample_action(), None, 0.0);
        assert_eq!(corrective.correction_of.as_deref(), Some(prior.receipt_id.as_str()));
    }

    #[test]
    fn validate_sequence_rejects_out_of_order() {
        let r1 = ReceiptBuilder::start("run_x", 2).complete(sample_action(), None, 0.0);
        let r2 = ReceiptBuilder::start("run_x", 1).complete(sample_action(), None, 0.0);
        let err = validate_sequence(&[r1, r2]).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[test]
    fn validate_sequence_rejects_duplicate_id() {
        let r = ReceiptBuilder::start("run_x", 1).complete(sample_action(), None, 0.0);
        let r_dup = StepReceipt {
            receipt_id: r.receipt_id.clone(),
            ..r.clone()
        };
        let err = validate_sequence(&[r, r_dup]).unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
    }

    #[test]
    fn cost_micro_usd_rounds_correctly() {
        let receipt = ReceiptBuilder::start("run_x", 1).complete(sample_action(), None, 0.012345);
        assert_eq!(receipt.cost_micro_usd, 12_345);
    }
}
