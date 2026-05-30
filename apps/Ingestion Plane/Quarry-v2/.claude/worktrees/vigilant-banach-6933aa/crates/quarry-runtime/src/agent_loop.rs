//! Agent loop — drives an agentic browser session with constraint enforcement.
//!
//! Combines ObservationRunner (action execution + observation production) with
//! AgentConstraints (max_steps, allowed_domains, max_runtime_s, max_cost_usd).
//! Model Plane sends AgentActionRequests; Quarry returns BrowserObservations.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use quarry_browser::BrowserDriver;
use quarry_core::contracts::{AgentActionRequest, AgentConstraints, BrowserObservation};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::event::EventType;
use quarry_core::ids::kinds::RunKind;
use quarry_core::zdr::ZdrMode;
use serde_json::json;
use url::Url;

use crate::artifact_store::ArtifactStore;
use crate::event_bus::EventBus;
use crate::events::EventSink;
use crate::observation::{ObservationContext, ObservationRunner};

#[derive(Debug, Clone, PartialEq)]
pub enum LoopTermination {
    Completed,
    MaxStepsReached,
    MaxRuntimeExceeded,
    MaxCostExceeded { spent_usd: f64, limit_usd: f64 },
    DomainViolation { url: String },
    Aborted,
}

pub struct AgentLoopResult {
    pub termination: LoopTermination,
    pub observations: Vec<BrowserObservation>,
    pub steps_executed: u32,
    pub elapsed: Duration,
}

pub struct AgentLoop {
    runner: ObservationRunner,
    constraints: AgentConstraints,
    #[allow(dead_code)]
    zdr: ZdrMode,
    run_id: RunKind,
    events: Option<EventSink>,
    /// Optional EventBus (e.g. NatsEventBus) that publishes agent lifecycle
    /// events for cross-plane consumers (Model Plane orchestrator, Control Plane
    /// status views). Independent of the in-process EventSink so callers can
    /// wire either or both. When both are set, every event flows to both sinks.
    bus: Option<Arc<dyn EventBus>>,
    /// Cumulative cost in micro-USD (1e-6 USD units) so we can use AtomicU64 share.
    cost_micro_usd: Arc<AtomicU64>,
}

impl AgentLoop {
    pub fn new(
        browser: Arc<dyn BrowserDriver>,
        constraints: AgentConstraints,
        zdr: ZdrMode,
        run_id: RunKind,
    ) -> Self {
        Self {
            runner: ObservationRunner {
                browser,
                artifacts: None,
                events: None,
            },
            constraints,
            zdr,
            run_id,
            events: None,
            bus: None,
            cost_micro_usd: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn with_artifacts(mut self, store: Arc<dyn ArtifactStore>) -> Self {
        self.runner.artifacts = Some(store);
        self
    }

    pub fn with_events(mut self, events: EventSink) -> Self {
        self.runner.events = Some(events.clone());
        self.events = Some(events);
        self
    }

    /// Attach a cross-plane EventBus (e.g. `NatsEventBus`). Lifecycle events
    /// flow to both this bus AND the in-process EventSink (when set).
    /// Failures publishing to the bus are logged but never abort the agent
    /// run — local execution wins over cross-plane fan-out.
    pub fn with_event_bus(mut self, bus: Arc<dyn EventBus>) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Helper: emit an event to both EventSink and EventBus (when configured).
    async fn emit_event(
        &self,
        event_type: EventType,
        payload: serde_json::Value,
        idempotency_key: String,
    ) {
        if let Some(events) = &self.events {
            events
                .emit(
                    self.run_id.clone(),
                    event_type,
                    payload.clone(),
                    idempotency_key.clone(),
                )
                .await;
        }
        if let Some(bus) = &self.bus {
            if let Err(e) = bus
                .publish(self.run_id.clone(), event_type, payload, idempotency_key)
                .await
            {
                tracing::warn!(error = %e, "agent event bus publish failed");
            }
        }
    }

    /// Record additional cost spent on this run (e.g. after a Model Plane planner call).
    /// Negative or NaN values are ignored.
    pub fn record_cost(&self, additional_usd: f64) {
        if !additional_usd.is_finite() || additional_usd < 0.0 {
            return;
        }
        let micro = (additional_usd * 1_000_000.0).round() as u64;
        self.cost_micro_usd.fetch_add(micro, Ordering::Relaxed);
    }

    /// Cumulative spent USD on this run.
    pub fn spent_usd(&self) -> f64 {
        self.cost_micro_usd.load(Ordering::Relaxed) as f64 / 1_000_000.0
    }

    fn over_budget(&self) -> Option<(f64, f64)> {
        let limit = self.constraints.max_cost_usd?;
        let spent = self.spent_usd();
        if spent >= limit {
            Some((spent, limit))
        } else {
            None
        }
    }

    pub fn check_domain(&self, url: &str) -> QuarryResult<()> {
        if self.constraints.allowed_domains.is_empty() {
            return Ok(());
        }
        let parsed = Url::parse(url).map_err(|e| {
            QuarryError::new(ErrorCode::BadRequest, format!("invalid url: {e}"))
        })?;
        let host = parsed.host_str().unwrap_or("");
        let allowed = self.constraints.allowed_domains.iter().any(|d| {
            host == d.as_str() || host.ends_with(&format!(".{d}"))
        });
        if allowed {
            Ok(())
        } else {
            Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                format!("domain {host} not in allowed list"),
            ))
        }
    }

    pub async fn execute(
        &self,
        requests: &[AgentActionRequest],
        session: &quarry_browser::BrowserSession,
    ) -> QuarryResult<AgentLoopResult> {
        let start = Instant::now();
        let max_runtime = self
            .constraints
            .max_runtime_s
            .map(|s| Duration::from_secs(s as u64));

        let mut ctx = ObservationContext {
            step: 0,
            current_url: String::new(),
            page_hash: format!("agent:{}", self.run_id),
        };

        let mut observations = Vec::new();

        self.emit_event(
            EventType::AgentStarted,
            json!({
                "max_steps": self.constraints.max_steps,
                "max_runtime_s": self.constraints.max_runtime_s,
                "allowed_domains": self.constraints.allowed_domains,
            }),
            format!("{}:agent:started", self.run_id),
        )
        .await;

        for request in requests {
            if ctx.step >= self.constraints.max_steps {
                let result = AgentLoopResult {
                    termination: LoopTermination::MaxStepsReached,
                    observations,
                    steps_executed: ctx.step,
                    elapsed: start.elapsed(),
                };
                self.emit_completion(&result).await;
                return Ok(result);
            }

            if let Some(max_rt) = max_runtime {
                if start.elapsed() >= max_rt {
                    let result = AgentLoopResult {
                        termination: LoopTermination::MaxRuntimeExceeded,
                        observations,
                        steps_executed: ctx.step,
                        elapsed: start.elapsed(),
                    };
                    self.emit_completion(&result).await;
                    return Ok(result);
                }
            }

            if let Some((spent, limit)) = self.over_budget() {
                let result = AgentLoopResult {
                    termination: LoopTermination::MaxCostExceeded {
                        spent_usd: spent,
                        limit_usd: limit,
                    },
                    observations,
                    steps_executed: ctx.step,
                    elapsed: start.elapsed(),
                };
                self.emit_completion(&result).await;
                return Ok(result);
            }

            if let quarry_core::contracts::AgentAction::Navigate { url } = &request.action {
                if let Err(e) = self.check_domain(url) {
                    let result = AgentLoopResult {
                        termination: LoopTermination::DomainViolation {
                            url: url.clone(),
                        },
                        observations,
                        steps_executed: ctx.step,
                        elapsed: start.elapsed(),
                    };
                    self.emit_event(
                        EventType::ActionFailed,
                        json!({
                            "step": ctx.step,
                            "error": e.to_string(),
                            "url": url,
                        }),
                        format!("{}:action:{}:domain_denied", self.run_id, ctx.step),
                    )
                    .await;
                    self.emit_completion(&result).await;
                    return Ok(result);
                }
            }

            match self.runner.execute(request, session, &mut ctx).await {
                Ok(obs) => observations.push(obs),
                Err(e) => {
                    self.emit_event(
                        EventType::ActionFailed,
                        json!({
                            "step": ctx.step,
                            "error": e.to_string(),
                        }),
                        format!("{}:action:{}:failed", self.run_id, ctx.step),
                    )
                    .await;
                    return Err(e);
                }
            }
        }

        let result = AgentLoopResult {
            termination: LoopTermination::Completed,
            observations,
            steps_executed: ctx.step,
            elapsed: start.elapsed(),
        };
        self.emit_completion(&result).await;
        Ok(result)
    }

    async fn emit_completion(&self, result: &AgentLoopResult) {
        let event_type = match &result.termination {
            LoopTermination::Completed => EventType::AgentCompleted,
            _ => EventType::AgentFailed,
        };
        self.emit_event(
            event_type,
            json!({
                "termination": format!("{:?}", result.termination),
                "steps_executed": result.steps_executed,
                "elapsed_ms": result.elapsed.as_millis() as u64,
                "spent_usd": self.spent_usd(),
            }),
            format!("{}:agent:done", self.run_id),
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::contracts::{AgentAction, AgentConstraints};
    use quarry_core::ids::Id;

    fn make_constraints(max_steps: u32) -> AgentConstraints {
        AgentConstraints {
            max_steps,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: None,
        }
    }

    fn make_request(run_id: &RunKind, action: AgentAction) -> AgentActionRequest {
        AgentActionRequest {
            run_id: run_id.clone(),
            lease_id: Id::new(),
            action,
            instruction: None,
            constraints: make_constraints(100),
            zdr: ZdrMode::Off,
        }
    }

    #[test]
    fn domain_check_allows_matching_host() {
        let run_id: RunKind = Id::new();
        let mut constraints = make_constraints(10);
        constraints.allowed_domains = vec!["example.com".into()];

        let agent = AgentLoop::new(
            Arc::new(crate::tests::MockBrowserDriver),
            constraints,
            ZdrMode::Off,
            run_id,
        );

        assert!(agent.check_domain("https://example.com/page").is_ok());
        assert!(agent.check_domain("https://sub.example.com/page").is_ok());
        assert!(agent.check_domain("https://evil.com/page").is_err());
    }

    #[test]
    fn domain_check_allows_all_when_empty() {
        let run_id: RunKind = Id::new();
        let constraints = make_constraints(10);

        let agent = AgentLoop::new(
            Arc::new(crate::tests::MockBrowserDriver),
            constraints,
            ZdrMode::Off,
            run_id,
        );

        assert!(agent.check_domain("https://anything.com").is_ok());
    }

    #[tokio::test]
    async fn record_cost_accumulates_and_triggers_termination() {
        use quarry_core::lease::{BrowserLease, ProxyAffinity};
        use quarry_core::ids::kinds;

        let run_id: RunKind = Id::new();
        let browser = Arc::new(crate::tests::MockBrowserDriver);
        let mut constraints = make_constraints(10);
        constraints.max_cost_usd = Some(0.50);

        let agent = AgentLoop::new(browser.clone(), constraints, ZdrMode::Off, run_id.clone());
        agent.record_cost(0.30);
        agent.record_cost(0.25);
        // 0.30 + 0.25 = 0.55 ≥ 0.50 → over budget

        assert!((agent.spent_usd() - 0.55).abs() < 1e-6);

        let lease = BrowserLease {
            lease_id: kinds::LeaseKind::new(),
            profile_id: kinds::ProfileKind::new(),
            session_affinity_key: "t".into(),
            proxy_affinity: ProxyAffinity { pool: "default".into(), sticky_key: None },
            ttl_s: 60,
            capabilities: vec![],
            artifact_bucket: "test".into(),
            org_id: "test_org".into(),
        };
        let session = browser.acquire(&lease).await.unwrap();

        let request = make_request(&run_id, AgentAction::Screenshot { full_page: false });
        let result = agent.execute(&[request], &session).await.unwrap();
        match result.termination {
            LoopTermination::MaxCostExceeded { spent_usd, limit_usd } => {
                assert!((spent_usd - 0.55).abs() < 1e-6);
                assert!((limit_usd - 0.50).abs() < 1e-9);
            }
            other => panic!("expected MaxCostExceeded, got {other:?}"),
        }
    }

    #[test]
    fn record_cost_ignores_negative_and_nan() {
        let run_id: RunKind = Id::new();
        let agent = AgentLoop::new(
            Arc::new(crate::tests::MockBrowserDriver),
            make_constraints(10),
            ZdrMode::Off,
            run_id,
        );
        agent.record_cost(-1.0);
        agent.record_cost(f64::NAN);
        agent.record_cost(0.10);
        assert!((agent.spent_usd() - 0.10).abs() < 1e-6);
    }

    #[test]
    fn domain_check_rejects_invalid_url() {
        let run_id: RunKind = Id::new();
        let mut constraints = make_constraints(10);
        constraints.allowed_domains = vec!["example.com".into()];

        let agent = AgentLoop::new(
            Arc::new(crate::tests::MockBrowserDriver),
            constraints,
            ZdrMode::Off,
            run_id,
        );

        assert!(agent.check_domain("not a url").is_err());
    }

    #[tokio::test]
    async fn agent_lifecycle_events_flow_through_event_bus() {
        use crate::event_bus::{EventBus, EventReceiver};
        use async_trait::async_trait;
        use quarry_browser::BrowserDriver;
        use quarry_core::lease::{BrowserLease, ProxyAffinity};
        use quarry_core::ids::kinds;
        use std::sync::Mutex;

        #[derive(Default)]
        struct CapturingBus {
            published: Arc<Mutex<Vec<(EventType, String)>>>,
        }

        #[async_trait]
        impl EventBus for CapturingBus {
            async fn publish(
                &self,
                _run_id: RunKind,
                event_type: EventType,
                _payload: serde_json::Value,
                idempotency_key: String,
            ) -> QuarryResult<()> {
                self.published
                    .lock()
                    .unwrap()
                    .push((event_type, idempotency_key));
                Ok(())
            }

            async fn subscribe(
                &self,
                _run_id: &RunKind,
            ) -> QuarryResult<Box<dyn EventReceiver>> {
                unimplemented!("subscribe not used in this test")
            }

            async fn unsubscribe(&self, _run_id: &RunKind) -> QuarryResult<()> {
                Ok(())
            }
        }

        let run_id: RunKind = Id::new();
        let browser = Arc::new(crate::tests::MockBrowserDriver);
        let bus = Arc::new(CapturingBus::default());
        let captured = bus.published.clone();

        let agent = AgentLoop::new(
            browser.clone(),
            make_constraints(10),
            ZdrMode::Off,
            run_id.clone(),
        )
        .with_event_bus(bus.clone());

        let lease = BrowserLease {
            lease_id: kinds::LeaseKind::new(),
            profile_id: kinds::ProfileKind::new(),
            session_affinity_key: "t".into(),
            proxy_affinity: ProxyAffinity {
                pool: "default".into(),
                sticky_key: None,
            },
            ttl_s: 60,
            capabilities: vec![],
            artifact_bucket: "test".into(),
            org_id: "test_org".into(),
        };
        let session = browser.acquire(&lease).await.unwrap();

        let request = make_request(&run_id, AgentAction::Screenshot { full_page: false });
        let _ = agent.execute(&[request], &session).await.unwrap();

        let events = captured.lock().unwrap().clone();
        let types: Vec<EventType> = events.iter().map(|(t, _)| *t).collect();

        // Must include AgentStarted at the front and AgentCompleted at the end.
        assert!(types.contains(&EventType::AgentStarted));
        assert!(types.contains(&EventType::AgentCompleted));
        // Idempotency keys must be unique per event.
        let mut keys: Vec<&str> = events.iter().map(|(_, k)| k.as_str()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), events.len(), "duplicate idempotency keys leaked");
    }

    #[tokio::test]
    async fn agent_event_bus_failure_does_not_abort_run() {
        use crate::event_bus::{EventBus, EventReceiver};
        use async_trait::async_trait;
        use quarry_browser::BrowserDriver;
        use quarry_core::lease::{BrowserLease, ProxyAffinity};
        use quarry_core::ids::kinds;

        struct BrokenBus;

        #[async_trait]
        impl EventBus for BrokenBus {
            async fn publish(
                &self,
                _run_id: RunKind,
                _event_type: EventType,
                _payload: serde_json::Value,
                _idempotency_key: String,
            ) -> QuarryResult<()> {
                Err(QuarryError::new(
                    ErrorCode::DriverFailed,
                    "bus down",
                ))
            }

            async fn subscribe(
                &self,
                _run_id: &RunKind,
            ) -> QuarryResult<Box<dyn EventReceiver>> {
                unimplemented!()
            }

            async fn unsubscribe(&self, _run_id: &RunKind) -> QuarryResult<()> {
                Ok(())
            }
        }

        let run_id: RunKind = Id::new();
        let browser = Arc::new(crate::tests::MockBrowserDriver);
        let agent = AgentLoop::new(
            browser.clone(),
            make_constraints(10),
            ZdrMode::Off,
            run_id.clone(),
        )
        .with_event_bus(Arc::new(BrokenBus));

        let lease = BrowserLease {
            lease_id: kinds::LeaseKind::new(),
            profile_id: kinds::ProfileKind::new(),
            session_affinity_key: "t".into(),
            proxy_affinity: ProxyAffinity {
                pool: "default".into(),
                sticky_key: None,
            },
            ttl_s: 60,
            capabilities: vec![],
            artifact_bucket: "test".into(),
            org_id: "test_org".into(),
        };
        let session = browser.acquire(&lease).await.unwrap();

        let request = make_request(&run_id, AgentAction::Screenshot { full_page: false });
        let result = agent.execute(&[request], &session).await.unwrap();
        assert_eq!(result.termination, LoopTermination::Completed);
    }

    #[tokio::test]
    async fn mock_planner_drives_agent_loop_e2e() {
        use crate::planner::{MockPlanner, Planner, PlannerDecision};
        use quarry_core::ids::kinds;
        use quarry_core::lease::{BrowserLease, ProxyAffinity};
        use quarry_browser::BrowserDriver;

        let run_id: RunKind = Id::new();
        let browser = Arc::new(crate::tests::MockBrowserDriver);
        let constraints = make_constraints(10);

        let planner = MockPlanner::new(vec![
            vec![make_request(&run_id, AgentAction::Navigate {
                url: "https://example.com".into(),
            })],
            vec![make_request(&run_id, AgentAction::Screenshot {
                full_page: false,
            })],
        ]);

        let agent = AgentLoop::new(
            browser.clone(),
            constraints,
            ZdrMode::Off,
            run_id.clone(),
        );

        let lease = BrowserLease {
            lease_id: kinds::LeaseKind::new(),
            profile_id: kinds::ProfileKind::new(),
            session_affinity_key: "test".into(),
            proxy_affinity: ProxyAffinity { pool: "default".into(), sticky_key: None },
            ttl_s: 60,
            capabilities: vec![],
            artifact_bucket: "test".into(),
            org_id: "test_org".into(),
        };
        let session = browser.acquire(&lease).await.unwrap();

        let initial_obs = quarry_core::contracts::BrowserObservation {
            run_id: run_id.clone(),
            step: 0,
            url: String::new(),
            title: None,
            dom_summary: None,
            screenshot_artifact_id: None,
            console_summary: vec![],
            network_summary: vec![],
            policy_denials: vec![],
            observed_at: chrono::Utc::now(),
        };

        let mut all_observations = Vec::new();
        let mut current_obs = initial_obs;

        loop {
            let decision = planner.next_actions(&current_obs).await.unwrap();
            match decision {
                PlannerDecision::Done => break,
                PlannerDecision::Continue(actions) => {
                    let result = agent.execute(&actions, &session).await.unwrap();
                    assert_eq!(result.termination, LoopTermination::Completed);
                    if let Some(obs) = result.observations.last() {
                        current_obs = obs.clone();
                    }
                    all_observations.extend(result.observations);
                }
            }
        }

        assert_eq!(all_observations.len(), 2);
    }
}
