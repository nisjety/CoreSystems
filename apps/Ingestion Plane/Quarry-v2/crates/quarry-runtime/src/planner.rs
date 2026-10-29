//! Planner trait — Model Plane abstraction for agentic browsing.
//!
//! The Planner receives BrowserObservations and produces AgentActionRequests.
//! Quarry ships an in-process MockPlanner for testing; the real implementation
//! lives in Model Plane and calls an LLM to decide the next action.

use async_trait::async_trait;

use quarry_core::contracts::{AgentActionRequest, BrowserObservation};
use quarry_core::error::QuarryResult;

#[async_trait]
pub trait Planner: Send + Sync {
    async fn next_actions(&self, observation: &BrowserObservation)
        -> QuarryResult<PlannerDecision>;
}

#[derive(Debug, Clone)]
pub enum PlannerDecision {
    Continue(Vec<AgentActionRequest>),
    Done,
}

pub struct MockPlanner {
    steps: std::sync::Mutex<std::collections::VecDeque<Vec<AgentActionRequest>>>,
}

impl MockPlanner {
    pub fn new(steps: Vec<Vec<AgentActionRequest>>) -> Self {
        Self {
            steps: std::sync::Mutex::new(steps.into()),
        }
    }
}

#[async_trait]
impl Planner for MockPlanner {
    async fn next_actions(
        &self,
        _observation: &BrowserObservation,
    ) -> QuarryResult<PlannerDecision> {
        let mut steps = self.steps.lock().unwrap();
        match steps.pop_front() {
            Some(actions) => Ok(PlannerDecision::Continue(actions)),
            None => Ok(PlannerDecision::Done),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use quarry_core::contracts::{AgentAction, AgentConstraints};
    use quarry_core::ids::kinds::RunKind;
    use quarry_core::ids::Id;
    use quarry_core::zdr::ZdrMode;

    fn mock_observation() -> BrowserObservation {
        BrowserObservation {
            run_id: Id::new(),
            step: 0,
            url: "https://example.com".into(),
            title: Some("Example".into()),
            snapshot: None,
            dom_summary: None,
            screenshot_artifact_id: None,
            visual_observation_artifact_id: None,
            evidence_delta_artifact_id: None,
            console_summary: vec![],
            network_summary: vec![],
            egress_receipts: vec![],
            dialogs: vec![],
            policy_denials: vec![],
            action_outcome: quarry_core::contracts::ActionOutcome::default(),
            observation_delta: None,
            challenge: None,
            extraction_profile: None,
            extraction_result: None,
            proof_bundle: None,
            target_resolution: None,
            telemetry: quarry_core::contracts::BrowserTelemetry::default(),
            observed_at: Utc::now(),
        }
    }

    fn mock_action_request(run_id: &RunKind, action: AgentAction) -> AgentActionRequest {
        AgentActionRequest {
            run_id: run_id.clone(),
            lease_id: Id::new(),
            action,
            instruction: None,
            constraints: AgentConstraints {
                max_steps: 10,
                allowed_domains: vec![],
                max_runtime_s: None,
                max_cost_usd: None,
            },
            zdr: ZdrMode::Off,
            extraction_profile: None,
        }
    }

    #[tokio::test]
    async fn mock_planner_returns_steps_then_done() {
        let run_id: RunKind = Id::new();
        let planner = MockPlanner::new(vec![
            vec![mock_action_request(
                &run_id,
                AgentAction::Navigate {
                    url: "https://example.com".into(),
                },
            )],
            vec![mock_action_request(
                &run_id,
                AgentAction::Click {
                    selector: "#btn".into(),
                },
            )],
        ]);

        let obs = mock_observation();

        let d1 = planner.next_actions(&obs).await.unwrap();
        assert!(matches!(d1, PlannerDecision::Continue(ref v) if v.len() == 1));

        let d2 = planner.next_actions(&obs).await.unwrap();
        assert!(matches!(d2, PlannerDecision::Continue(ref v) if v.len() == 1));

        let d3 = planner.next_actions(&obs).await.unwrap();
        assert!(matches!(d3, PlannerDecision::Done));
    }

    #[tokio::test]
    async fn mock_planner_empty_returns_done_immediately() {
        let planner = MockPlanner::new(vec![]);
        let obs = mock_observation();
        let decision = planner.next_actions(&obs).await.unwrap();
        assert!(matches!(decision, PlannerDecision::Done));
    }
}
