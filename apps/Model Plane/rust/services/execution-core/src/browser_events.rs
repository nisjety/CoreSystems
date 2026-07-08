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

use crate::browser_agent::{BrowserAction, BrowserEventSink, BrowserObservation, PlanConfig};

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
}
