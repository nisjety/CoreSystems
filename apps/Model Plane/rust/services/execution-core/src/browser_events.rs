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

use mp_contracts::approval_cause::{self, CauseCode};
use mp_contracts::model_plane::v1::{
    self as pb, orchestration_core_service_client::OrchestrationCoreServiceClient,
    orchestration_event,
};
use tonic::transport::Channel;
use tonic::{Request, Status};
use tracing::{debug, warn};

use crate::browser_agent::{
    approval_poll_interval, approval_timeout_seconds, ApprovalOutcome, BrowserAction,
    BrowserEventSink, BrowserObservation, PlanConfig, RiskyActionDetail,
};
use crate::state::{RunStatus, StateStore};

/// The tool whose actions the in-loop gate holds, named on the approval's cause.
const BROWSER_AGENT_TOOL: &str = "browser_agent";

/// E3.2-a: longest decider name published on the decision event.
const MAX_DECIDED_BY_CHARS: usize = 128;
/// E3.2-a: the decider label when the gate failed closed before any person could
/// decide (no durable approval could be made).
const DECIDED_BY_FAIL_CLOSED: &str = "system:fail-closed";
/// The labels the cancel and timeout paths already send to `DecideApproval`.
const DECIDED_BY_USER_CANCELLED: &str = "system:user-cancelled";
const DECIDED_BY_TIMEOUT: &str = "system:approval-timeout";

/// E3.2-c: the internal decision word for a person's rejection that carries
/// instructions. Never published: the timeline sees `denied`.
const DECISION_REDIRECTED: &str = "redirected";

/// E3.2-c: the redirect text of a denied approval record, if the loop should
/// treat the rejection as a redirect: the flag is on, a person (not a
/// `system:*` decider such as the cancel path, and not nobody) decided it, and
/// some text is left after bounding. The text is the decision reason, bounded.
fn redirect_text(enabled: bool, decided_by: &str, reason: &str) -> Option<String> {
    let decider = decided_by.trim();
    if !enabled || decider.is_empty() || decider.starts_with("system:") {
        return None;
    }
    crate::browser_agent::bound_redirect_text(reason)
}

/// `BrowserRunPaused.reason` when a person took the agent tab (WS8.G).
pub const PAUSED_REASON_USER_TOOK_OVER: &str = "user_took_over";
/// `BrowserRunResumed.reason` when the person gave the tab back (WS8.G).
pub const RESUMED_REASON_CONTROL_RETURNED: &str = "control_returned";

/// Publishes browser-agent events to session-core's orchestration broadcast.
///
/// Holds the shared session-core channel; a fresh client is cheaply cloned per
/// call (tonic channels are reference-counted and cheap to clone).
pub struct OrchestrationEventSink {
    channel: Channel,
    /// Shared run-state store (Phase 6 fast-follow). Lets the in-loop
    /// `require_approval` poll notice a user-initiated stop/cancel while
    /// waiting on a human decision, mirroring `wait_out_pause_or_cancel`'s
    /// between-actions check. `None` when the caller has no `StateStore` to
    /// share (e.g. a unit test) — the poll then behaves exactly as before
    /// (grant/deny/timeout only).
    state: Option<StateStore>,
    bearer: Option<std::sync::Arc<str>>,
    receipt_persistence_confirmed: bool,
}

impl OrchestrationEventSink {
    /// Build a sink over the session-core gRPC channel, optionally sharing a
    /// `StateStore` so the in-loop approval gate can observe a user-initiated
    /// stop while it is polling (Phase 6).
    #[must_use]
    pub fn new(channel: Channel, state: Option<StateStore>) -> Self {
        Self {
            channel,
            state,
            bearer: None,
            receipt_persistence_confirmed: false,
        }
    }

    #[must_use]
    pub fn with_verified_bearer(mut self, bearer: &str) -> Self {
        self.bearer = Some(std::sync::Arc::from(bearer));
        self
    }

    /// Only an authenticated ingress can confirm durable receipt eligibility.
    #[must_use]
    pub fn with_receipt_persistence_confirmed(mut self, confirmed: bool) -> Self {
        self.receipt_persistence_confirmed = confirmed;
        self
    }

    #[allow(clippy::result_large_err)]
    fn request<T>(&self, value: T) -> Result<Request<T>, Status> {
        let bearer = self
            .bearer
            .as_deref()
            .ok_or_else(|| Status::unauthenticated("verified session credential required"))?;
        let mut request = Request::new(value);
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {bearer}")
                .parse()
                .map_err(|_| Status::internal("verified session credential is not forwardable"))?,
        );
        Ok(request)
    }

    /// `true` when a user stopped `run_id` (via the normal pause/stop
    /// control) while this sink's caller is blocked waiting on something
    /// else — e.g. a pending HITL approval. No-op (`false`) when this sink
    /// was built without a `StateStore` or `run_id` is empty.
    fn was_cancelled(&self, run_id: &str) -> bool {
        if run_id.is_empty() {
            return false;
        }
        self.state
            .as_ref()
            .is_some_and(|s| s.get_or_create(run_id).status == RunStatus::Cancelled)
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
        let result = match self.request(request) {
            Ok(request) => self.client().record_orchestration_event(request).await,
            Err(error) => Err(error),
        };
        if let Err(error) = result {
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
        decided_by: &str,
    ) -> ApprovalOutcome {
        // E3.2-c: a redirect is a rejection on the timeline ("denied"); only the
        // outcome the loop gets differs.
        let published = if decision == DECISION_REDIRECTED {
            "denied"
        } else {
            decision
        };
        self.publish(
            &config.run_id,
            orchestration_event::Event::BrowserActionDecided(
                orchestration_event::BrowserActionDecided {
                    run_id: config.run_id.clone(),
                    plan_id: config.plan_id.clone(),
                    action_id: detail.action_id.clone(),
                    approval_id: approval_id.to_owned(),
                    decision: published.to_owned(),
                    // E3.2-a: the decider as the durable Approval record names it
                    // (`GetApproval.decided_by`, set by whoever called
                    // `DecideApproval`), or the `system:*` label of the path that
                    // ended the wait here. A ZDR run names nobody on the event.
                    decided_by: if config.zdr {
                        String::new()
                    } else {
                        decided_by.chars().take(MAX_DECIDED_BY_CHARS).collect()
                    },
                },
            ),
        )
        .await;
        match decision {
            "granted" => ApprovalOutcome::Granted,
            "timed_out" => ApprovalOutcome::TimedOut,
            DECISION_REDIRECTED => ApprovalOutcome::Redirect(reason.to_owned()),
            _ => ApprovalOutcome::Denied(reason.to_owned()),
        }
    }
}

#[tonic::async_trait]
impl BrowserEventSink for OrchestrationEventSink {
    async fn record_final_answer(&self, config: &PlanConfig, answer: &str) {
        // A zero-retention result must not cross a durable content boundary.
        if config.zdr || config.run_id.trim().is_empty() || answer.trim().is_empty() {
            return;
        }
        let request = match self.request(pb::RecordRunOutputRequest {
            run_id: config.run_id.clone(),
            output: answer.to_owned(),
        }) {
            Ok(request) => request,
            Err(_) => {
                warn!(run_id = %config.run_id, reason = "credential_unavailable", "browser final answer was not stored");
                return;
            }
        };
        let mut client = pb::managed_run_lifecycle_client::ManagedRunLifecycleClient::new(self.channel.clone());
        match tokio::time::timeout(std::time::Duration::from_secs(3), client.record_run_output(request)).await {
            Ok(Ok(_)) => {}
            // Never log the answer or a transport message that may contain it.
            Ok(Err(status)) => warn!(run_id = %config.run_id, grpc_code = ?status.code(), "browser final answer was not stored"),
            Err(_) => warn!(run_id = %config.run_id, reason = "timeout", "browser final answer was not stored"),
        }
    }

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
                status: if observation.challenge.as_ref().is_some_and(|challenge| challenge.requires_user()) {
                    "site_blocked"
                } else {
                    observation.status.as_str()
                }.to_owned(),
                // WS12.S4: the event leaves the loop (session-core fans it out
                // to the UI and the log), so the URL loses a query token and
                // the title a credential shape here, at the boundary. The
                // observation itself is untouched.
                page_url: crate::scrub::scrub_url(&observation.page_url),
                page_title: crate::scrub::scrub_live(&observation.page_title),
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
                // A user-initiated pause or resume carries no reason.
                reason: String::new(),
                control_epoch: 0,
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
                // A user-initiated pause or resume carries no reason.
                reason: String::new(),
                control_epoch: 0,
            });
        self.publish(&config.run_id, event).await;
    }

    /// WS8.G: the person took the agent tab of an attached run. The same
    /// `BrowserRunPaused` as a user pause, told apart by its reason. The tab id
    /// is not on the event (the proto carries the run's ids only).
    async fn paused_by_user(&self, config: &PlanConfig, _tab_id: &str) {
        if config.run_id.is_empty() {
            return;
        }
        let event =
            orchestration_event::Event::BrowserRunPaused(orchestration_event::BrowserRunPaused {
                run_id: config.run_id.clone(),
                plan_id: config.plan_id.clone(),
                reason: PAUSED_REASON_USER_TOOK_OVER.to_owned(),
                control_epoch: 0,
            });
        self.publish(&config.run_id, event).await;
    }

    /// WS8.G: the person gave the agent tab back; the run goes on.
    async fn resumed_after_takeover(&self, config: &PlanConfig, _tab_id: &str) {
        if config.run_id.is_empty() {
            return;
        }
        let event =
            orchestration_event::Event::BrowserRunResumed(orchestration_event::BrowserRunResumed {
                run_id: config.run_id.clone(),
                plan_id: config.plan_id.clone(),
                reason: RESUMED_REASON_CONTROL_RETURNED.to_owned(),
                control_epoch: 0,
            });
        self.publish(&config.run_id, event).await;
    }

    /// WS8.G (D4): the one-time audit that an attached run shares the person's
    /// cookie jar. Ids only: never a URL or a page title.
    async fn attach_shared_jar(
        &self,
        config: &PlanConfig,
        session_run_id: &str,
        attach_id: &str,
        tab_id: &str,
    ) {
        if !self.receipt_persistence_confirmed || config.zdr || config.run_id.is_empty() {
            return;
        }
        let event = orchestration_event::Event::BrowserAttachSharedJar(
            orchestration_event::BrowserAttachSharedJar {
                run_id: config.run_id.clone(),
                plan_id: config.plan_id.clone(),
                attach_id: attach_id.to_owned(),
                session_run_id: session_run_id.to_owned(),
                tab_id: tab_id.to_owned(),
            },
        );
        let request = pb::RecordOrchestrationEventRequest {
            event: Some(pb::OrchestrationEvent { event_id: String::new(), at: None, event: Some(event) }),
        };
        let result = match self.request(request) {
            Ok(request) => self.client().record_orchestration_event(request).await,
            Err(error) => Err(error),
        };
        // The typed event alone is owner-writable, so it is not proof of origin.
        // Always publish a signed receipt on success or when an older session
        // rejects that unknown oneof; never bypass ZDR or authorization errors.
        let can_publish_signed = result.as_ref().is_ok_and(|_| true)
            || result.as_ref().is_err_and(|error| error.code() == tonic::Code::InvalidArgument);
        if can_publish_signed {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs());
                let key = std::env::var("BROWSER_ATTACH_COMPAT_HMAC_KEY").ok();
                if let (Some(now), Some(key)) = (now, key) {
                    let receipt = crate::browser_attach_compat::Receipt {
                        run_id: config.run_id.clone(), plan_id: config.plan_id.clone(),
                        session_run_id: session_run_id.to_owned(), attach_id: attach_id.to_owned(),
                        tab_id: tab_id.to_owned(), issued_at: now,
                    };
                    if let Some(envelope) = crate::browser_attach_compat::sign(key.as_bytes(), &receipt) {
                        let legacy = orchestration_event::Event::BrowserObservationReceived(
                            orchestration_event::BrowserObservationReceived {
                                run_id: config.run_id.clone(), plan_id: config.plan_id.clone(),
                                action_id: "verevon-attach-compat-v1".to_owned(), status: envelope,
                                page_url: String::new(), page_title: String::new(),
                                screenshot_ref: String::new(), dom_snapshot_ref: String::new(),
                            },
                        );
                        self.publish(&config.run_id, legacy).await;
                        return;
                    }
                }
        }
        if let Err(error) = result {
            debug!(run_id = %config.run_id, error = %error, "failed to publish browser attachment (best-effort)");
        }
    }

    /// The run-level gate's decision, on the same event the in-loop gates use.
    /// `start_run` is the synthetic action slug for a run-level browser gate, and
    /// `approval_id` stays empty because no durable Approval row exists: the
    /// user's run grant was the approval.
    async fn run_start_authorized(&self, run_id: &str, plan_id: &str, decided_by: &str) {
        if run_id.is_empty() {
            return;
        }
        let event = orchestration_event::Event::BrowserActionDecided(
            orchestration_event::BrowserActionDecided {
                run_id: run_id.to_owned(),
                plan_id: plan_id.to_owned(),
                action_id: "start_run".to_owned(),
                approval_id: String::new(),
                decision: "granted".to_owned(),
                decided_by: decided_by.to_owned(),
            },
        );
        self.publish(run_id, event).await;
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
        let created = match self.request(pb::CreateApprovalRequest {
            continuation_mode: String::new(),
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
            continuation_descriptor_json: String::new(),
            // A live approval: the running browser loop polls it, no worker resumes it, and
            // the card must say so (build plan WS10 S1).
            tool_key: String::new(),
            action_digest: String::new(),
            turn_origin: String::new(),
            grant_use_token: String::new(),
            context_json: crate::space_gate::card_context_json(
                BROWSER_AGENT_TOOL,
                "",
                crate::space_gate::RESUME_LIVE_POLL,
            ),
            // The in-loop gate has no tool call to key on, only the category the
            // risk classifier assigned the Model's proposed action.
            cause: Some(pb::ApprovalCause {
                tool: BROWSER_AGENT_TOOL.to_owned(),
                action_key: BROWSER_AGENT_TOOL.to_owned(),
                risk_category: detail.risk_category.as_str().to_owned(),
                // E3.2-a: a closed class word for the host, never the host. A ZDR
                // run states none (session-core stores none for it either).
                host_class: if config.zdr {
                    String::new()
                } else {
                    crate::browser_agent::host_class(
                        detail.risk_category,
                        &detail.url,
                    )
                    .to_owned()
                },
                ..approval_cause::for_code(CauseCode::BrowserActionRisk)
            }),
            ..Default::default()
        }) {
            Ok(request) => orchestration.create_approval(request).await,
            Err(error) => Err(error),
        };

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
                            DECIDED_BY_FAIL_CLOSED,
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
                        DECIDED_BY_FAIL_CLOSED,
                    )
                    .await;
            }
        };

        // The first event explains the pending action even if creation fails;
        // this second one binds that same action to the actual durable record.
        // A client must never offer an approval control for the empty-id event.
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
                    approval_id: approval_id.clone(),
                },
            ),
        ).await;

        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(u64::from(expires_in));
        loop {
            // Phase 6 fast-follow: a user-initiated stop must be able to
            // interrupt a run sitting at a pending approval, not just wait
            // out the (up to 30-minute) timeout. Mirrors
            // `wait_out_pause_or_cancel`'s between-actions cancel check, but
            // here it runs on every iteration of THIS gate's own wait loop.
            // Checked before polling `GetApproval` so a cancelled run never
            // issues another needless RPC.
            if self.was_cancelled(&config.run_id) {
                if let Ok(request) = self.request(pb::DecideApprovalRequest {
                    approval_id: approval_id.clone(),
                    decision: pb::ApprovalState::Denied as i32,
                    decided_by: "system:user-cancelled".to_owned(),
                    decision_reason: "run cancelled by user while awaiting approval".to_owned(),
                    org_id: config.org_id.clone(),
                    decision_source: pb::ApprovalDecisionSource::System as i32,
                    ..Default::default()
                }) {
                    let _ = orchestration.decide_approval(request).await;
                }
                return self
                    .decided(
                        config,
                        detail,
                        &approval_id,
                        "denied",
                        "run cancelled by user while awaiting approval",
                        DECIDED_BY_USER_CANCELLED,
                    )
                    .await;
            }

            let approval_request = self.request(pb::GetApprovalRequest {
                approval_id: approval_id.clone(),
                // Cross-org IDOR fix (Phase 6): this approval was just
                // created with this same org_id above, so asserting it
                // here is a real ownership check, not a no-op.
                org_id: config.org_id.clone(),
                ..Default::default()
            });
            let approval_result = match approval_request {
                Ok(request) => orchestration.get_approval(request).await,
                Err(error) => Err(error),
            };
            match approval_result {
                Ok(response) => {
                    if let Some(approval) = response.into_inner().approval {
                        match pb::ApprovalState::try_from(approval.state) {
                            Ok(pb::ApprovalState::Granted) => {
                                return self
                                    .decided(
                                        config,
                                        detail,
                                        &approval_id,
                                        "granted",
                                        "",
                                        &approval.decided_by,
                                    )
                                    .await;
                            }
                            Ok(pb::ApprovalState::Denied) => {
                                if let Some(text) = redirect_text(
                                    config.approval_redirect,
                                    &approval.decided_by,
                                    &approval.decision_reason,
                                ) {
                                    return self
                                        .decided(
                                            config,
                                            detail,
                                            &approval_id,
                                            DECISION_REDIRECTED,
                                            &text,
                                            &approval.decided_by,
                                        )
                                        .await;
                                }
                                return self
                                    .decided(
                                        config,
                                        detail,
                                        &approval_id,
                                        "denied",
                                        &approval.decision_reason,
                                        &approval.decided_by,
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
                                        &approval.decided_by,
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
                if let Ok(request) = self.request(pb::DecideApprovalRequest {
                    approval_id: approval_id.clone(),
                    decision: pb::ApprovalState::TimedOut as i32,
                    decided_by: "system:approval-timeout".to_owned(),
                    decision_reason: "browser action approval wait budget exceeded".to_owned(),
                    org_id: config.org_id.clone(),
                    decision_source: pb::ApprovalDecisionSource::Timeout as i32,
                    ..Default::default()
                }) {
                    let _ = orchestration.decide_approval(request).await;
                }
                return self
                    .decided(
                        config,
                        detail,
                        &approval_id,
                        "timed_out",
                        "approval wait budget exceeded",
                        DECIDED_BY_TIMEOUT,
                    )
                    .await;
            }

            tokio::time::sleep(approval_poll_interval()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    //! Exercises `OrchestrationEventSink::require_approval` against a real
    //! (in-process) `OrchestrationCoreService` implementation over a real
    //! tonic channel — the sink's poll loop is not otherwise unit-testable
    //! without a gRPC peer. Mirrors the mock-server pattern already used by
    //! `model-gateway/tests/orchestration_http_test.rs`. Both
    //! `approval_poll_interval`/`approval_timeout_seconds` are shortened
    //! under `cfg!(test)` (5ms / 1s), so these tests run in well under a
    //! second of real wall-clock time.

    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use mp_contracts::model_plane::v1::{
        orchestration_core_service_server::{
            OrchestrationCoreService, OrchestrationCoreServiceServer,
        },
        AcknowledgeApprovalDeliveryRequest, AcknowledgeApprovalDeliveryResponse,
        AttachSubagentRequest, AttachSubagentResponse, ClaimApprovalDeliveriesRequest,
        ClaimApprovalDeliveriesResponse, CreateApprovalRequest, CreateApprovalResponse,
        ConsumeApprovalRequest, ConsumeApprovalResponse,
        DecideApprovalRequest, DecideApprovalResponse, GetApprovalContinuationRequest,
        GetApprovalContinuationResponse, GetApprovalRequest, GetApprovalResponse, GetPlanRequest,
        GetPlanResponse, GetSubagentLineageRequest, GetSubagentLineageResponse, GetTodoRequest,
        GetTodoResponse, ListApprovalsRequest, ListApprovalsResponse, ListPlansRequest,
        ListPlansResponse, ListTodosRequest, ListTodosResponse, OrgPendingApprovalsRequest,
        OrgPendingApprovalsResponse, RecordOrchestrationEventRequest,
        RecordOrchestrationEventResponse, StreamRunEventsRequest, TransitionPlanRequest,
        TransitionPlanResponse, TransitionTodoRequest, TransitionTodoResponse,
    };
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{
        transport::{Endpoint, Server},
        Request as TonicRequest, Response, Status,
    };

    use super::*;
    use crate::browser_agent::RiskCategory;

    type MockEventStream =
        Pin<Box<dyn futures::Stream<Item = Result<pb::OrchestrationEvent, Status>> + Send>>;

    /// Captured `(decision, decided_by, reason)` from the last `decide_approval` call.
    type DecideCapture = Arc<Mutex<Option<(i32, String, String)>>>;

    /// A minimal `OrchestrationCoreService` that never resolves an approval
    /// on its own (`get_approval` always reports `Requested`) — so
    /// `require_approval`'s poll loop only exits via this test's own
    /// cancel/timeout path, never a race with a mock-granted decision.
    #[derive(Clone, Default)]
    struct NeverResolvingOrchestration {
        decide_calls: DecideCapture,
        /// Every `CreateApproval` the gate sent, in order.
        created: Arc<Mutex<Vec<CreateApprovalRequest>>>,
        /// Every `RecordOrchestrationEvent` the sink sent, in order.
        recorded: Arc<Mutex<Vec<RecordOrchestrationEventRequest>>>,
        reject_attach: bool,
    }

    #[tonic::async_trait]
    impl OrchestrationCoreService for NeverResolvingOrchestration {
        // Main's RPCs (merged 2026-11-02); this double does not serve them.
        async fn reroute_approval(
            &self,
            _: tonic::Request<mp_contracts::model_plane::v1::RerouteApprovalRequest>,
        ) -> Result<tonic::Response<mp_contracts::model_plane::v1::RerouteApprovalResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("reroute_approval is not served by this double"))
        }

        async fn claim_once_approval(
            &self,
            _: tonic::Request<mp_contracts::model_plane::v1::ClaimOnceApprovalRequest>,
        ) -> Result<tonic::Response<mp_contracts::model_plane::v1::ClaimOnceApprovalResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("claim_once_approval is not served by this double"))
        }

        async fn hold_quarantined_output(
            &self,
            _: tonic::Request<mp_contracts::model_plane::v1::HoldQuarantinedOutputRequest>,
        ) -> Result<tonic::Response<mp_contracts::model_plane::v1::HoldQuarantinedOutputResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("hold_quarantined_output is not served by this double"))
        }

        async fn claim_quarantine_release(
            &self,
            _: tonic::Request<mp_contracts::model_plane::v1::ClaimQuarantineReleaseRequest>,
        ) -> Result<tonic::Response<mp_contracts::model_plane::v1::ClaimQuarantineReleaseResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("claim_quarantine_release is not served by this double"))
        }

        async fn get_quarantine_preview(
            &self,
            _: tonic::Request<mp_contracts::model_plane::v1::GetQuarantinePreviewRequest>,
        ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetQuarantinePreviewResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_quarantine_preview is not served by this double"))
        }

        async fn get_approval_preview(
            &self,
            _: tonic::Request<mp_contracts::model_plane::v1::GetApprovalPreviewRequest>,
        ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetApprovalPreviewResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_approval_preview is not served by this double"))
        }

        async fn get_run_proof_bundle(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::GetRunProofBundleRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::GetRunProofBundleResponse>, Status>
        {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn get_verification_metrics(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::GetVerificationMetricsRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::GetVerificationMetricsResponse>, Status>
        {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn list_plans(
            &self,
            _: TonicRequest<ListPlansRequest>,
        ) -> Result<Response<ListPlansResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn get_plan(
            &self,
            _: TonicRequest<GetPlanRequest>,
        ) -> Result<Response<GetPlanResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn transition_plan(
            &self,
            _: TonicRequest<TransitionPlanRequest>,
        ) -> Result<Response<TransitionPlanResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn propose_plan(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::ProposePlanRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ProposePlanResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn list_todos(
            &self,
            _: TonicRequest<ListTodosRequest>,
        ) -> Result<Response<ListTodosResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn get_todo(
            &self,
            _: TonicRequest<GetTodoRequest>,
        ) -> Result<Response<GetTodoResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn transition_todo(
            &self,
            _: TonicRequest<TransitionTodoRequest>,
        ) -> Result<Response<TransitionTodoResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn create_approval(
            &self,
            request: TonicRequest<CreateApprovalRequest>,
        ) -> Result<Response<CreateApprovalResponse>, Status> {
            let request = request.into_inner();
            self.created.lock().unwrap().push(request.clone());
            Ok(Response::new(CreateApprovalResponse {
                approval: Some(pb::Approval {
                    consumed_at: None,
                    tool_key: String::new(),
                    action_label: String::new(),
                    continuation: None,
                    continuation_mode: String::new(),
                    id: "appr-test-1".into(),
                    run_id: request.run_id,
                    step_id: request.step_id,
                    kind: request.kind,
                    state: pb::ApprovalState::Requested as i32,
                    requested_of: request.requested_of,
                    decided_by: String::new(),
                    decision_reason: String::new(),
                    context: None,
                    requested_at: None,
                    decided_at: None,
                    expires_at: None,
                    org_id: request.org_id,
                    cause: request.cause,
                    ..Default::default()
                }),
            }))
        }
        async fn list_approvals(
            &self,
            _: TonicRequest<ListApprovalsRequest>,
        ) -> Result<Response<ListApprovalsResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn list_pending_approvals(
            &self,
            _: TonicRequest<OrgPendingApprovalsRequest>,
        ) -> Result<Response<OrgPendingApprovalsResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn get_approval(
            &self,
            request: TonicRequest<GetApprovalRequest>,
        ) -> Result<Response<GetApprovalResponse>, Status> {
            let approval_id = request.into_inner().approval_id;
            // Always "Requested" — a decision never arrives on its own, so
            // the only way `require_approval` returns is this test's own
            // cancel check or the (short, cfg!(test)) timeout.
            Ok(Response::new(GetApprovalResponse {
                approval: Some(pb::Approval {
                    consumed_at: None,
                    tool_key: String::new(),
                    action_label: String::new(),
                    continuation: None,
                    continuation_mode: String::new(),
                    id: approval_id,
                    run_id: "run_001".into(),
                    step_id: "act_1".into(),
                    kind: pb::ApprovalKind::Destructive as i32,
                    state: pb::ApprovalState::Requested as i32,
                    requested_of: "org_test".into(),
                    decided_by: String::new(),
                    decision_reason: String::new(),
                    context: None,
                    requested_at: None,
                    decided_at: None,
                    expires_at: None,
                    org_id: "org_test".into(),
                    cause: None,
                    ..Default::default()
                }),
            }))
        }
        async fn consume_approval(
            &self,
            _: TonicRequest<ConsumeApprovalRequest>,
        ) -> Result<Response<ConsumeApprovalResponse>, Status> {
            Err(Status::unimplemented("consume_approval not needed in test"))
        }

        async fn create_run_question(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::CreateRunQuestionRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::CreateRunQuestionResponse>, Status>
        {
            Err(Status::unimplemented(
                "create_run_question not needed in test",
            ))
        }

        async fn list_run_questions(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::ListRunQuestionsRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ListRunQuestionsResponse>, Status>
        {
            Err(Status::unimplemented("list_run_questions not needed in test"))
        }

        async fn answer_run_question(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::AnswerRunQuestionRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::AnswerRunQuestionResponse>, Status>
        {
            Err(Status::unimplemented(
                "answer_run_question not needed in test",
            ))
        }

        async fn decide_approval(
            &self,
            request: TonicRequest<DecideApprovalRequest>,
        ) -> Result<Response<DecideApprovalResponse>, Status> {
            let request = request.into_inner();
            *self.decide_calls.lock().unwrap() = Some((
                request.decision,
                request.decided_by.clone(),
                request.decision_reason.clone(),
            ));
            Ok(Response::new(DecideApprovalResponse {
                approval: Some(pb::Approval {
                    consumed_at: None,
                    tool_key: String::new(),
                    action_label: String::new(),
                    continuation: None,
                    continuation_mode: String::new(),
                    id: request.approval_id,
                    run_id: "run_001".into(),
                    step_id: "act_1".into(),
                    kind: pb::ApprovalKind::Destructive as i32,
                    state: request.decision,
                    requested_of: "org_test".into(),
                    decided_by: request.decided_by,
                    decision_reason: request.decision_reason,
                    context: None,
                    requested_at: None,
                    decided_at: None,
                    expires_at: None,
                    org_id: "org_test".into(),
                    cause: None,
                    ..Default::default()
                }),
            }))
        }
        async fn claim_approval_deliveries(
            &self,
            _: TonicRequest<ClaimApprovalDeliveriesRequest>,
        ) -> Result<Response<ClaimApprovalDeliveriesResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn get_approval_continuation(
            &self,
            _: TonicRequest<GetApprovalContinuationRequest>,
        ) -> Result<Response<GetApprovalContinuationResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn record_approval_continuation_started(
            &self,
            _: TonicRequest<pb::RecordApprovalContinuationStartedRequest>,
        ) -> Result<Response<pb::RecordApprovalContinuationStartedResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn record_approval_continuation_outcome(
            &self,
            _: TonicRequest<pb::RecordApprovalContinuationOutcomeRequest>,
        ) -> Result<Response<pb::RecordApprovalContinuationOutcomeResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn acknowledge_approval_delivery(
            &self,
            _: TonicRequest<AcknowledgeApprovalDeliveryRequest>,
        ) -> Result<Response<AcknowledgeApprovalDeliveryResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn get_subagent_lineage(
            &self,
            _: TonicRequest<GetSubagentLineageRequest>,
        ) -> Result<Response<GetSubagentLineageResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn attach_subagent(
            &self,
            _: TonicRequest<AttachSubagentRequest>,
        ) -> Result<Response<AttachSubagentResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        type StreamRunEventsStream = MockEventStream;
        async fn stream_run_events(
            &self,
            _: TonicRequest<StreamRunEventsRequest>,
        ) -> Result<Response<Self::StreamRunEventsStream>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn record_orchestration_event(
            &self,
            request: TonicRequest<RecordOrchestrationEventRequest>,
        ) -> Result<Response<RecordOrchestrationEventResponse>, Status> {
            let request = request.into_inner();
            let reject = self.reject_attach && matches!(
                request.event.as_ref().and_then(|event| event.event.as_ref()),
                Some(orchestration_event::Event::BrowserAttachSharedJar(_))
            );
            self.recorded.lock().unwrap().push(request);
            if reject { return Err(Status::invalid_argument("event payload is required")); }
            Ok(Response::new(RecordOrchestrationEventResponse {
                event_id: "evt-test".into(),
            }))
        }
    }

    async fn spawn_mock(svc: NeverResolvingOrchestration) -> Channel {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            Server::builder()
                .add_service(OrchestrationCoreServiceServer::new(svc))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn legacy_attach_fallback_is_signed_and_absent_for_zdr_or_missing_key() {
        let key = "test-only-compat-key-32-characters-long";
        std::env::set_var("BROWSER_ATTACH_COMPAT_HMAC_KEY", key);
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let channel = spawn_mock(NeverResolvingOrchestration {
            recorded: Arc::clone(&recorded), reject_attach: true, ..Default::default()
        }).await;
        let unconfirmed = OrchestrationEventSink::new(channel.clone(), None).with_verified_bearer("test-bearer");
        unconfirmed.attach_shared_jar(&test_config(), "session_1", "attach_1", "tab_1").await;
        assert!(recorded.lock().unwrap().is_empty(), "unknown retention must not publish");
        let sink = OrchestrationEventSink::new(channel, None).with_verified_bearer("test-bearer").with_receipt_persistence_confirmed(true);
        let config = test_config();
        sink.attach_shared_jar(&config, "session_1", "attach_1", "tab_1").await;
        {
            let events = recorded.lock().unwrap();
            assert_eq!(events.len(), 2);
            let Some(orchestration_event::Event::BrowserObservationReceived(payload)) =
                events[1].event.as_ref().and_then(|event| event.event.as_ref()) else { panic!("missing legacy envelope") };
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
            let receipt = crate::browser_attach_compat::verify(key.as_bytes(), &payload.status, &config.run_id, &config.plan_id, now).unwrap();
            assert_eq!((&*receipt.session_run_id, &*receipt.attach_id, &*receipt.tab_id), ("session_1", "attach_1", "tab_1"));
            assert!(payload.page_url.is_empty() && payload.page_title.is_empty());
        }
        let mut zdr = test_config(); zdr.zdr = true;
        sink.attach_shared_jar(&zdr, "session_1", "attach_1", "tab_1").await;
        assert_eq!(recorded.lock().unwrap().len(), 2);
        let supported = Arc::new(Mutex::new(Vec::new()));
        let channel = spawn_mock(NeverResolvingOrchestration {
            recorded: Arc::clone(&supported), ..Default::default()
        }).await;
        let modern_sink = OrchestrationEventSink::new(channel, None).with_verified_bearer("test-bearer").with_receipt_persistence_confirmed(true);
        modern_sink.attach_shared_jar(&config, "session_1", "attach_1", "tab_1").await;
        assert_eq!(supported.lock().unwrap().len(), 2, "a modern server must also get signed proof");
        assert!(matches!(supported.lock().unwrap()[1].event.as_ref().and_then(|event| event.event.as_ref()), Some(orchestration_event::Event::BrowserObservationReceived(_))));
        std::env::remove_var("BROWSER_ATTACH_COMPAT_HMAC_KEY");
        sink.attach_shared_jar(&config, "session_1", "attach_1", "tab_1").await;
        assert_eq!(recorded.lock().unwrap().len(), 3, "no unsigned fallback");
    }

    fn test_config() -> PlanConfig {
        PlanConfig {
            plan_id: "plan_001".to_owned(),
            grant_id: "grant_001".to_owned(),
            run_id: "run_001".to_owned(),
            org_id: "org_test".to_owned(),
            system_prompt: String::new(),
            max_steps: 10,
            max_runtime_s: 60,
            allowed_domains: vec!["example.com".to_owned()],
            stop_criteria: String::new(),
            require_approval: false,
            max_cost_usd: None,
            zdr: false,
            profile_id: None,
            start_url: None,
            postcondition: String::new(),
            ..Default::default()
        }
    }

    fn test_detail() -> RiskyActionDetail {
        RiskyActionDetail {
            action_id: "act_1".to_owned(),
            action_type: "goto".to_owned(),
            url: "https://example.com/login".to_owned(),
            selector: String::new(),
            risk_category: RiskCategory::Login,
            reason: "test".to_owned(),
        }
    }

    #[derive(Clone, Default)]
    struct OutputRecorder(Arc<Mutex<Vec<(String, pb::RecordRunOutputRequest)>>>);

    #[tonic::async_trait]
    impl pb::managed_run_lifecycle_server::ManagedRunLifecycle for OutputRecorder {
        async fn record_run_output(&self, request: Request<pb::RecordRunOutputRequest>) -> Result<Response<pb::RecordRunOutputResponse>, Status> {
            let bearer = request.metadata().get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
            if bearer != "Bearer delegated-owner" { return Err(Status::permission_denied("not owner")); }
            let body = request.into_inner();
            if body.run_id != "run_001" { return Err(Status::not_found("not owner")); }
            let response = pb::RecordRunOutputResponse { run_id: body.run_id.clone(), stored_chars: body.output.chars().count() as u32 };
            self.0.lock().unwrap().push((bearer, body));
            Ok(Response::new(response))
        }
        async fn start_managed_run(&self, _: Request<pb::StartManagedRunRequest>) -> Result<Response<pb::StartManagedRunResponse>, Status> { Err(Status::unimplemented("unused")) }
        async fn record_terminal_outcome(&self, _: Request<pb::RecordTerminalOutcomeRequest>) -> Result<Response<pb::RecordTerminalOutcomeResponse>, Status> { Err(Status::unimplemented("unused")) }
        async fn heartbeat_managed_run(&self, _: Request<pb::HeartbeatManagedRunRequest>) -> Result<Response<pb::HeartbeatManagedRunResponse>, Status> { Err(Status::unimplemented("unused")) }
        async fn complete_direct_run_with_message(&self, _: Request<pb::CompleteDirectRunWithMessageRequest>) -> Result<Response<pb::CompleteDirectRunWithMessageResponse>, Status> { Err(Status::unimplemented("unused")) }
    }

    #[tokio::test]
    async fn final_answer_uses_owner_credential_and_never_records_zdr() {
        let recorder = OutputRecorder::default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let service = pb::managed_run_lifecycle_server::ManagedRunLifecycleServer::new(recorder.clone());
        let server = tokio::spawn(async move {
            Server::builder().add_service(service).serve_with_incoming(TcpListenerStream::new(listener)).await.unwrap();
        });
        let channel = Endpoint::from_shared(format!("http://{addr}")).unwrap().connect().await.unwrap();
        let sink = OrchestrationEventSink::new(channel.clone(), None).with_verified_bearer("delegated-owner");
        sink.record_final_answer(&test_config(), "Actual planner answer").await;
        let mut zdr = test_config();
        zdr.zdr = true;
        sink.record_final_answer(&zdr, "must not persist").await;
        sink.record_final_answer(&test_config(), " ").await;
        let wrong = OrchestrationEventSink::new(channel, None).with_verified_bearer("other-user");
        wrong.record_final_answer(&test_config(), "not authorized").await;
        let sent = recorder.0.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "Bearer delegated-owner");
        assert_eq!(sent[0].1.run_id, "run_001");
        assert_eq!(sent[0].1.output, "Actual planner answer");
        server.abort();
    }

    #[tokio::test]
    async fn require_approval_exits_promptly_when_run_is_cancelled_mid_poll() {
        let decide_calls: DecideCapture = Arc::default();
        let channel = spawn_mock(NeverResolvingOrchestration {
            decide_calls: decide_calls.clone(),
            ..Default::default()
        })
        .await;

        let state = StateStore::new();
        // Cancelled BEFORE the gate starts polling — mirrors a user clicking
        // Stop while a run is already sitting at a pending approval.
        assert!(state.cancel("run_001", Some("user_stop".to_owned())));

        let sink =
            OrchestrationEventSink::new(channel, Some(state)).with_verified_bearer("test-bearer");
        let config = test_config();
        let detail = test_detail();

        let started = std::time::Instant::now();
        let outcome = sink.require_approval(&config, &detail).await;
        let elapsed = started.elapsed();

        assert_eq!(
            outcome,
            ApprovalOutcome::Denied("run cancelled by user while awaiting approval".to_owned())
        );
        // The test-mode timeout is 1s; a prompt cancel must return in a small
        // fraction of that, proving the run did NOT wait out the full budget.
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "expected a prompt cancel exit, took {elapsed:?}"
        );

        let (decision, decided_by, reason) = decide_calls
            .lock()
            .unwrap()
            .clone()
            .expect("decide_approval was called");
        assert_eq!(decision, pb::ApprovalState::Denied as i32);
        assert_eq!(decided_by, "system:user-cancelled");
        assert_eq!(reason, "run cancelled by user while awaiting approval");
    }

    #[tokio::test]
    async fn risk_event_republishes_the_durable_approval_id_for_the_same_action() {
        let recorded = Arc::default();
        let channel = spawn_mock(NeverResolvingOrchestration {
            recorded: Arc::clone(&recorded), ..Default::default()
        }).await;
        let state = StateStore::new();
        assert!(state.cancel("run_001", Some("stop".to_owned())));
        let sink = OrchestrationEventSink::new(channel, Some(state)).with_verified_bearer("test-bearer");
        let _ = sink.require_approval(&test_config(), &test_detail()).await;
        let sent = recorded.lock().unwrap();
        let approvals: Vec<_> = sent.iter().filter_map(|r| match r.event.as_ref()?.event.as_ref()? {
            orchestration_event::Event::BrowserActionApprovalRequired(e) => Some(e),
            _ => None,
        }).collect();
        assert_eq!(approvals.len(), 2);
        assert_eq!(approvals[0].approval_id, "");
        assert_eq!(approvals[1].approval_id, "appr-test-1");
        assert_eq!(approvals[1].run_id, approvals[0].run_id);
        assert_eq!(approvals[1].action_id, approvals[0].action_id);
    }

    #[tokio::test]
    async fn require_approval_times_out_normally_without_a_state_store() {
        // No StateStore wired at all (the pre-Phase-6 shape) — behavior must
        // be unchanged: fail closed via the existing timeout path, never a
        // cancel-shaped denial.
        let decide_calls: DecideCapture = Arc::default();
        let channel = spawn_mock(NeverResolvingOrchestration {
            decide_calls: decide_calls.clone(),
            ..Default::default()
        })
        .await;

        let sink = OrchestrationEventSink::new(channel, None).with_verified_bearer("test-bearer");
        let config = test_config();
        let detail = test_detail();

        let outcome = sink.require_approval(&config, &detail).await;

        assert_eq!(outcome, ApprovalOutcome::TimedOut);
        let (decision, decided_by, _reason) = decide_calls
            .lock()
            .unwrap()
            .clone()
            .expect("decide_approval was called");
        assert_eq!(decision, pb::ApprovalState::TimedOut as i32);
        assert_eq!(decided_by, "system:approval-timeout");
    }

    #[tokio::test]
    async fn require_approval_ignores_cancellation_of_a_different_run() {
        // A StateStore IS wired, but the cancelled run_id doesn't match this
        // gate's run — must not spuriously deny; falls through to the normal
        // timeout path exactly as if no cancellation had occurred anywhere.
        let decide_calls: DecideCapture = Arc::default();
        let channel = spawn_mock(NeverResolvingOrchestration {
            decide_calls: decide_calls.clone(),
            ..Default::default()
        })
        .await;

        let state = StateStore::new();
        assert!(state.cancel("some_other_run", Some("user_stop".to_owned())));

        let sink =
            OrchestrationEventSink::new(channel, Some(state)).with_verified_bearer("test-bearer");
        let config = test_config();
        let detail = test_detail();

        let outcome = sink.require_approval(&config, &detail).await;

        assert_eq!(outcome, ApprovalOutcome::TimedOut);
        let (decision, decided_by, _reason) = decide_calls
            .lock()
            .unwrap()
            .clone()
            .expect("decide_approval was called");
        assert_eq!(decision, pb::ApprovalState::TimedOut as i32);
        assert_eq!(decided_by, "system:approval-timeout");
    }

    #[tokio::test]
    async fn a_run_start_authorized_by_its_grant_is_recorded_as_a_granted_start_run() {
        let recorded = Arc::default();
        let channel = spawn_mock(NeverResolvingOrchestration {
            recorded: Arc::clone(&recorded),
            ..Default::default()
        })
        .await;
        let sink = OrchestrationEventSink::new(channel, None).with_verified_bearer("test-bearer");

        sink.run_start_authorized("run_001", "plan_001", "user_001")
            .await;
        // No run, no run-scoped event.
        sink.run_start_authorized("", "plan_001", "user_001").await;

        let sent = recorded.lock().unwrap();
        assert_eq!(sent.len(), 1);
        let event = sent[0]
            .event
            .as_ref()
            .and_then(|event| event.event.as_ref())
            .expect("an event body");
        let orchestration_event::Event::BrowserActionDecided(decided) = event else {
            panic!("expected BrowserActionDecided, got {event:?}");
        };
        assert_eq!(
            decided,
            &orchestration_event::BrowserActionDecided {
                run_id: "run_001".to_owned(),
                plan_id: "plan_001".to_owned(),
                action_id: "start_run".to_owned(),
                approval_id: String::new(),
                decision: "granted".to_owned(),
                decided_by: "user_001".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn a_browser_gate_carries_the_typed_risk_category() {
        let created = Arc::default();
        let channel = spawn_mock(NeverResolvingOrchestration {
            created: Arc::clone(&created),
            ..Default::default()
        })
        .await;
        let sink = OrchestrationEventSink::new(channel, None).with_verified_bearer("test-bearer");

        let outcome = sink.require_approval(&test_config(), &test_detail()).await;

        assert_eq!(outcome, ApprovalOutcome::TimedOut);
        let sent = created.lock().unwrap();
        assert_eq!(sent.len(), 1, "one durable approval per gated action");
        let cause = sent[0].cause.as_ref().expect("the gate states its cause");
        assert_eq!(cause.code, "browser_action_risk");
        assert_eq!(cause.risk_category, "login");
        assert_eq!(cause.tool, "browser_agent");
        assert_eq!(cause.scope_options, vec!["once".to_owned()]);
        assert!(
            cause.call_digest.is_empty() && !cause.action_key.contains("example.com"),
            "no URL or selector reaches the cause: {cause:?}"
        );
    }

    #[tokio::test]
    async fn a_browser_gate_states_a_closed_host_class_and_none_under_zdr() {
        let created = Arc::default();
        let channel = spawn_mock(NeverResolvingOrchestration {
            created: Arc::clone(&created),
            ..Default::default()
        })
        .await;
        let sink = OrchestrationEventSink::new(channel, None).with_verified_bearer("test-bearer");

        let _ = sink.require_approval(&test_config(), &test_detail()).await;
        let mut zdr = test_config();
        zdr.zdr = true;
        zdr.run_id = "run_zdr".to_owned();
        let _ = sink.require_approval(&zdr, &test_detail()).await;

        let sent = created.lock().unwrap();
        assert_eq!(sent.len(), 2);
        let class = |index: usize| sent[index].cause.as_ref().unwrap().host_class.clone();
        // A sign-in step on a public host; the word, never the host.
        assert_eq!(class(0), "sensitive");
        assert!(!format!("{:?}", sent[0].cause).contains("example.com"));
        assert_eq!(class(1), "", "a ZDR run states no class");
    }

    #[tokio::test]
    async fn the_decision_event_names_the_decider_the_record_names_and_nobody_under_zdr() {
        let recorded = Arc::default();
        let channel = spawn_mock(NeverResolvingOrchestration {
            recorded: Arc::clone(&recorded),
            ..Default::default()
        })
        .await;
        let sink = OrchestrationEventSink::new(channel, None).with_verified_bearer("test-bearer");
        let detail = test_detail();
        let mut zdr = test_config();
        zdr.zdr = true;

        let _ = sink
            .decided(&test_config(), &detail, "appr_1", "granted", "", "user_42")
            .await;
        let _ = sink
            .decided(&test_config(), &detail, "appr_2", "timed_out", "x", "system:approval-timeout")
            .await;
        let _ = sink.decided(&zdr, &detail, "appr_3", "granted", "", "user_42").await;

        let sent = recorded.lock().unwrap();
        let deciders: Vec<String> = sent
            .iter()
            .filter_map(|request| request.event.as_ref()?.event.clone())
            .filter_map(|event| match event {
                orchestration_event::Event::BrowserActionDecided(decided) => Some(decided.decided_by),
                _ => None,
            })
            .collect();
        assert_eq!(
            deciders,
            vec!["user_42".to_owned(), "system:approval-timeout".to_owned(), String::new()]
        );
    }

    #[tokio::test]
    async fn takeover_pause_resume_and_shared_jar_carry_reasons_and_ids_only() {
        let recorded = Arc::default();
        let channel = spawn_mock(NeverResolvingOrchestration {
            recorded: Arc::clone(&recorded),
            ..Default::default()
        })
        .await;
        let sink = OrchestrationEventSink::new(channel, None).with_verified_bearer("test-bearer");
        let config = test_config();

        sink.run_paused(&config).await;
        sink.paused_by_user(&config, "tab_9").await;
        sink.resumed_after_takeover(&config, "tab_9").await;
        sink.attach_shared_jar(&config, "run_user", "att_1", "tab_9").await;
        // No run, no run-scoped event.
        let no_run = PlanConfig {
            run_id: String::new(),
            ..test_config()
        };
        sink.attach_shared_jar(&no_run, "run_user", "att_1", "tab_9").await;

        let sent = recorded.lock().unwrap();
        let events: Vec<_> = sent
            .iter()
            .filter_map(|request| request.event.as_ref()?.event.clone())
            .collect();
        assert_eq!(events.len(), 4);
        let orchestration_event::Event::BrowserRunPaused(user_pause) = &events[0] else {
            panic!("{:?}", events[0]);
        };
        assert_eq!(user_pause.reason, "", "a user pause carries no reason");
        let orchestration_event::Event::BrowserRunPaused(took_over) = &events[1] else {
            panic!("{:?}", events[1]);
        };
        assert_eq!(took_over.reason, "user_took_over");
        assert_eq!(took_over.run_id, config.run_id);
        let orchestration_event::Event::BrowserRunResumed(given_back) = &events[2] else {
            panic!("{:?}", events[2]);
        };
        assert_eq!(given_back.reason, "control_returned");
        let orchestration_event::Event::BrowserAttachSharedJar(jar) = &events[3] else {
            panic!("{:?}", events[3]);
        };
        assert_eq!(
            jar,
            &orchestration_event::BrowserAttachSharedJar {
                run_id: config.run_id.clone(),
                plan_id: config.plan_id.clone(),
                attach_id: "att_1".to_owned(),
                session_run_id: "run_user".to_owned(),
                tab_id: "tab_9".to_owned(),
            }
        );
    }

    fn observation_at(url: &str, title: &str) -> BrowserObservation {
        BrowserObservation {
            observation_id: "obs_1".to_owned(),
            action_id: "act_1".to_owned(),
            grant_id: "grant_001".to_owned(),
            status: crate::browser_agent::ObservationStatus::Success,
            page_url: url.to_owned(),
            page_title: title.to_owned(),
            extracted_text: "never on the wire".to_owned(),
            screenshot_ref: String::new(),
            dom_snapshot_ref: "snap-1".to_owned(),
            snapshot_generation: None,
            rendered_controls: Vec::new(),
            snapshot_targets: Vec::new(),
            error_message: String::new(),
            challenge: None,
            redaction_rule_version: None,
            quality: Default::default(),
        }
    }

    #[test]
    fn redirect_text_needs_the_flag_a_person_and_some_text() {
        assert_eq!(redirect_text(true, "user_1", "use the guest option").as_deref(), Some("use the guest option"));
        // Off, a system decider, nobody, or no text: a plain denial.
        assert_eq!(redirect_text(false, "user_1", "use the guest option"), None);
        assert_eq!(redirect_text(true, "system:user-cancelled", "run cancelled by user while awaiting approval"), None);
        assert_eq!(redirect_text(true, "system:timeout", "expired"), None);
        assert_eq!(redirect_text(true, "", "text"), None);
        assert_eq!(redirect_text(true, "user_1", "   "), None);
        // Bounded.
        let long = "y".repeat(5_000);
        assert_eq!(redirect_text(true, "user_1", &long).map(|t| t.chars().count()), Some(1000));
    }

    #[tokio::test]
    async fn a_redirected_decision_is_denied_on_the_timeline_and_a_redirect_for_the_loop() {
        let recorded = Arc::default();
        let channel = spawn_mock(NeverResolvingOrchestration {
            recorded: Arc::clone(&recorded),
            ..Default::default()
        })
        .await;
        let sink = OrchestrationEventSink::new(channel, None).with_verified_bearer("test-bearer");
        let config = test_config();
        let detail = RiskyActionDetail {
            action_id: "act_1".to_owned(),
            action_type: "click".to_owned(),
            url: "https://example.com/checkout".to_owned(),
            selector: String::new(),
            risk_category: crate::browser_agent::RiskCategory::Checkout,
            reason: "checkout".to_owned(),
        };
        let outcome = sink
            .decided(&config, &detail, "appr_1", DECISION_REDIRECTED, "use guest", "user_1")
            .await;
        assert_eq!(outcome, ApprovalOutcome::Redirect("use guest".to_owned()));
        let sent = recorded.lock().unwrap();
        let decisions: Vec<_> = sent
            .iter()
            .filter_map(|request| request.event.as_ref()?.event.clone())
            .filter_map(|event| match event {
                orchestration_event::Event::BrowserActionDecided(decided) => Some(decided.decision),
                _ => None,
            })
            .collect();
        assert_eq!(decisions, vec!["denied".to_owned()]);
        drop(sent);
        assert_eq!(
            sink.decided(&config, &detail, "appr_1", "denied", "no", "user_1").await,
            ApprovalOutcome::Denied("no".to_owned())
        );
    }

    #[tokio::test]
    async fn observation_received_event_has_no_url_token() {
        let recorded = Arc::default();
        let channel = spawn_mock(NeverResolvingOrchestration {
            recorded: Arc::clone(&recorded),
            ..Default::default()
        })
        .await;
        let sink = OrchestrationEventSink::new(channel, None).with_verified_bearer("test-bearer");
        let config = test_config();
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdEFGH1234";

        // Assembled at run time so no credential-shaped literal sits in the source.
        let secret = ["tok", "abc123def456ghi789"].join("_");
        sink.observation_received(
            &config,
            &observation_at(
                &format!("https://example.com/cb?lang=en&access_token={secret}"),
                &format!("Welcome {jwt}"),
            ),
        )
        .await;
        // A clean page is byte-identical.
        sink.observation_received(
            &config,
            &observation_at("https://example.com/a?b=1", "Example Domain"),
        )
        .await;
        let mut challenge = observation_at("https://example.com/", "Site access");
        challenge.challenge = Some(crate::browser_agent::BrowserChallenge {
            kind: crate::browser_agent::ChallengeKind::Waf,
            strength: crate::browser_agent::ChallengeStrength::Strong,
            block_reason: Some(crate::browser_agent::BlockReason::WafInterstitial),
            recovery: crate::browser_agent::ChallengeRecovery::Exhausted,
            confidence_per_mille: 1000,
        });
        sink.observation_received(&config, &challenge).await;

        let sent = recorded.lock().unwrap();
        let events: Vec<_> = sent
            .iter()
            .filter_map(|request| request.event.as_ref()?.event.clone())
            .collect();
        assert_eq!(events.len(), 3);
        let orchestration_event::Event::BrowserObservationReceived(masked) = &events[0] else {
            panic!("{:?}", events[0]);
        };
        assert!(!masked.page_url.contains(&secret), "{}", masked.page_url);
        assert!(masked.page_url.starts_with("https://example.com/cb?lang=en"), "{}", masked.page_url);
        assert!(!masked.page_title.contains("eyJhbGciOiJIUzI1NiJ9"), "{}", masked.page_title);
        assert!(masked.page_title.starts_with("Welcome "), "{}", masked.page_title);
        let orchestration_event::Event::BrowserObservationReceived(clean) = &events[1] else {
            panic!("{:?}", events[1]);
        };
        assert_eq!(clean.page_url, "https://example.com/a?b=1");
        assert_eq!(clean.page_title, "Example Domain");
        assert_eq!(clean.status, "success");
        let orchestration_event::Event::BrowserObservationReceived(blocked) = &events[2] else {
            panic!("{:?}", events[2]);
        };
        assert_eq!(blocked.status, "site_blocked");
    }
}
