//! Concrete [`BrowserEventSink`] that publishes browser-agent progress to
//! session-core's orchestration broadcast (B4).
//!
//! The browser loop (`browser_agent::run_browser_agent_loop`) is detached from
//! session-core's `OrchestrationEvent` broadcast — there is no shared channel.
//! This sink bridges that gap: it translates each dispatched action and
//! received observation into a proto `OrchestrationEvent` and publishes it via
//! the additive `RecordOrchestrationEvent` RPC. session-core then fans it out
//! on the per-run `StreamRunEvents` stream that the gateway proxies to
//! `/v1/runs/:id/events`.
//!
//! Everything here is best-effort: a failure to publish never surfaces to the
//! loop (the trait methods return `()` and errors are logged, not propagated).

use mp_contracts::model_plane::v1::{
    self as pb, orchestration_core_service_client::OrchestrationCoreServiceClient,
    orchestration_event,
};
use tonic::transport::Channel;
use tracing::debug;

use crate::browser_agent::{
    approval_poll_interval, approval_timeout_seconds, ApprovalOutcome, BrowserAction,
    BrowserEventSink, BrowserObservation, PlanConfig, RiskyActionDetail,
};

/// Publishes browser-agent events to session-core's orchestration broadcast.
///
/// Holds the shared session-core channel; a fresh client is cheaply cloned per
/// call (tonic channels are reference-counted and cheap to clone).
pub struct OrchestrationEventSink {
    channel: Channel,
}

impl OrchestrationEventSink {
    /// Build a sink over the session-core gRPC channel.
    #[must_use]
    pub fn new(channel: Channel) -> Self {
        Self { channel }
    }

    fn client(&self) -> OrchestrationCoreServiceClient<Channel> {
        OrchestrationCoreServiceClient::new(self.channel.clone())
    }

    /// Publish one event, swallowing (and logging) any transport error.
    async fn publish(&self, run_id: &str, event: orchestration_event::Event) {
        let request = pb::RecordOrchestrationEventRequest {
            event: Some(pb::OrchestrationEvent {
                // session-core assigns event_id + at.
                event_id: String::new(),
                at: None,
                event: Some(event),
            }),
        };
        if let Err(error) = self.client().record_orchestration_event(request).await {
            // Best-effort: a missing/unavailable session-core (e.g. running the
            // browser agent in isolation) must not disturb the loop.
            debug!(
                run_id = %run_id,
                error = %error,
                "failed to publish browser orchestration event (best-effort)"
            );
        }
    }

    /// Publish the Phase 5 `BrowserActionDecided` companion event and map the
    /// decision string onto an `ApprovalOutcome`. Shared by every exit path
    /// of `require_approval` so the timeline always sees a decision event,
    /// even on a transport failure (`decision = "denied"`, fail closed).
    async fn decided(
        &self,
        config: &PlanConfig,
        detail: &RiskyActionDetail,
        approval_id: &str,
        decision: &str,
        reason: &str,
    ) -> ApprovalOutcome {
        self.publish(
            &config.run_id,
            orchestration_event::Event::BrowserActionDecided(
                orchestration_event::BrowserActionDecided {
                    run_id: config.run_id.clone(),
                    plan_id: config.plan_id.clone(),
                    action_id: detail.action_id.clone(),
                    approval_id: approval_id.to_owned(),
                    decision: decision.to_owned(),
                    // The decider's identity lives on the durable Approval
                    // record itself (`decided_by`, set by whoever called
                    // `DecideApproval`); this sink has no independent
                    // knowledge of it beyond what `GetApproval` last returned,
                    // which callers can fetch separately if needed.
                    decided_by: String::new(),
                },
            ),
        )
        .await;
        match decision {
            "granted" => ApprovalOutcome::Granted,
            "timed_out" => ApprovalOutcome::TimedOut,
            _ => ApprovalOutcome::Denied(reason.to_owned()),
        }
    }
}

#[tonic::async_trait]
impl BrowserEventSink for OrchestrationEventSink {
    async fn action_dispatched(&self, config: &PlanConfig, action: &BrowserAction) {
        // Only run-scoped events are deliverable on the per-run stream; skip
        // when the loop wasn't given a run id (avoids a guaranteed RPC reject).
        if config.run_id.is_empty() {
            return;
        }
        let event = orchestration_event::Event::BrowserActionDispatched(
            orchestration_event::BrowserActionDispatched {
                run_id: config.run_id.clone(),
                plan_id: config.plan_id.clone(),
                action_id: action.action_id.clone(),
                action_type: action.action_type.as_str().to_owned(),
                url: action.url.clone(),
                reason: action.reason.clone(),
            },
        );
        self.publish(&config.run_id, event).await;
    }

    async fn observation_received(&self, config: &PlanConfig, observation: &BrowserObservation) {
        if config.run_id.is_empty() {
            return;
        }
        let event = orchestration_event::Event::BrowserObservationReceived(
            orchestration_event::BrowserObservationReceived {
                run_id: config.run_id.clone(),
                plan_id: config.plan_id.clone(),
                action_id: observation.action_id.clone(),
                status: observation.status.as_str().to_owned(),
                page_url: observation.page_url.clone(),
                page_title: observation.page_title.clone(),
                // Reference ids only — never inlined bytes, and `extracted_text`
                // is deliberately excluded from the wire event (ZDR intent).
                screenshot_ref: observation.screenshot_ref.clone(),
                dom_snapshot_ref: observation.dom_snapshot_ref.clone(),
            },
        );
        self.publish(&config.run_id, event).await;
    }

    async fn run_paused(&self, config: &PlanConfig) {
        if config.run_id.is_empty() {
            return;
        }
        let event =
            orchestration_event::Event::BrowserRunPaused(orchestration_event::BrowserRunPaused {
                run_id: config.run_id.clone(),
                plan_id: config.plan_id.clone(),
            });
        self.publish(&config.run_id, event).await;
    }

    async fn run_resumed(&self, config: &PlanConfig) {
        if config.run_id.is_empty() {
            return;
        }
        let event =
            orchestration_event::Event::BrowserRunResumed(orchestration_event::BrowserRunResumed {
                run_id: config.run_id.clone(),
                plan_id: config.plan_id.clone(),
            });
        self.publish(&config.run_id, event).await;
    }

    /// Phase 5 in-loop HITL gate: create a durable approval via the SAME
    /// general `CreateApproval` RPC the outer whole-tool `ask` gate uses
    /// (`kind = DESTRUCTIVE`, matching `grpc.rs`'s own call), publish the
    /// browser-specific `BrowserActionApprovalRequired` companion, then poll
    /// `GetApproval` until decided or the wait budget expires. Fails CLOSED
    /// on every error path — a classified-risky action is never silently
    /// let through because session-core was unreachable.
    // Cohesive single-gate driver (publish → create → poll → decide); splitting
    // it would obscure the linear request/poll/resolve flow (mirrors the
    // `#[allow(clippy::too_many_lines)]` precedent on `run_browser_agent_loop`).
    #[allow(clippy::too_many_lines)]
    async fn require_approval(
        &self,
        config: &PlanConfig,
        detail: &RiskyActionDetail,
    ) -> ApprovalOutcome {
        if config.run_id.is_empty() {
            // Nothing to attach a durable, run-scoped approval to.
            return ApprovalOutcome::Denied("browser run has no run_id to gate".to_owned());
        }

        // Publish the "why" before creating the durable approval, so the
        // timeline shows the specific action even if CreateApproval itself
        // fails.
        self.publish(
            &config.run_id,
            orchestration_event::Event::BrowserActionApprovalRequired(
                orchestration_event::BrowserActionApprovalRequired {
                    run_id: config.run_id.clone(),
                    plan_id: config.plan_id.clone(),
                    action_id: detail.action_id.clone(),
                    action_type: detail.action_type.clone(),
                    url: detail.url.clone(),
                    selector: detail.selector.clone(),
                    reason: detail.reason.clone(),
                    risk_category: detail.risk_category.as_str().to_owned(),
                    approval_id: String::new(),
                },
            ),
        )
        .await;

        let mut orchestration = self.client();
        let expires_in = approval_timeout_seconds();
        let created = orchestration
            .create_approval(pb::CreateApprovalRequest {
                run_id: config.run_id.clone(),
                step_id: detail.action_id.clone(),
                kind: pb::ApprovalKind::Destructive as i32,
                requested_of: config.org_id.clone(),
                org_id: config.org_id.clone(),
                user_id: String::new(),
                reason: format!(
                    "browser action requires approval ({}): {}",
                    detail.risk_category.as_str(),
                    detail.reason
                ),
                expires_in_seconds: expires_in,
                client_approval_id: String::new(),
                // Stable per-(run, action, category) idempotency key so a
                // retried gate (e.g. the same action re-planned) collapses
                // onto the existing durable approval instead of duplicating
                // it, mirroring `grpc.rs`'s own `{run_id}:{step_id}` pattern.
                idempotency_key: format!(
                    "{}:{}:{}",
                    config.run_id,
                    detail.action_id,
                    detail.risk_category.as_str()
                ),
            })
            .await;

        let approval_id = match created {
            Ok(response) => match response.into_inner().approval {
                Some(a) if !a.id.is_empty() => a.id,
                _ => {
                    debug!(run_id = %config.run_id, "create_approval returned no durable record (fail closed)");
                    return self
                        .decided(
                            config,
                            detail,
                            "",
                            "denied",
                            "approval creation returned no durable record",
                        )
                        .await;
                }
            },
            Err(error) => {
                debug!(run_id = %config.run_id, error = %error, "failed to create browser-action approval (fail closed)");
                return self
                    .decided(
                        config,
                        detail,
                        "",
                        "denied",
                        &format!("approval creation failed: {error}"),
                    )
                    .await;
            }
        };

        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(u64::from(expires_in));
        loop {
            match orchestration
                .get_approval(pb::GetApprovalRequest {
                    approval_id: approval_id.clone(),
                })
                .await
            {
                Ok(response) => {
                    if let Some(approval) = response.into_inner().approval {
                        match pb::ApprovalState::try_from(approval.state) {
                            Ok(pb::ApprovalState::Granted) => {
                                return self
                                    .decided(config, detail, &approval_id, "granted", "")
                                    .await;
                            }
                            Ok(pb::ApprovalState::Denied) => {
                                return self
                                    .decided(
                                        config,
                                        detail,
                                        &approval_id,
                                        "denied",
                                        &approval.decision_reason,
                                    )
                                    .await;
                            }
                            Ok(pb::ApprovalState::TimedOut) => {
                                return self
                                    .decided(
                                        config,
                                        detail,
                                        &approval_id,
                                        "timed_out",
                                        &approval.decision_reason,
                                    )
                                    .await;
                            }
                            // Requested/Unspecified — keep polling.
                            _ => {}
                        }
                    }
                }
                Err(error) => {
                    debug!(
                        run_id = %config.run_id,
                        approval_id = %approval_id,
                        error = %error,
                        "failed to poll browser-action approval"
                    );
                }
            }

            if tokio::time::Instant::now() >= deadline {
                // Fail closed AND make the durable record reflect it, so
                // anyone inspecting the approval later sees the true state
                // rather than a permanently "requested" row.
                let _ = orchestration
                    .decide_approval(pb::DecideApprovalRequest {
                        approval_id: approval_id.clone(),
                        decision: pb::ApprovalState::TimedOut as i32,
                        decided_by: "system:approval-timeout".to_owned(),
                        decision_reason: "browser action approval wait budget exceeded".to_owned(),
                    })
                    .await;
                return self
                    .decided(
                        config,
                        detail,
                        &approval_id,
                        "timed_out",
                        "approval wait budget exceeded",
                    )
                    .await;
            }

            tokio::time::sleep(approval_poll_interval()).await;
        }
    }
}
