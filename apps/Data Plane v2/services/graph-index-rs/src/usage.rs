//! graph-index's place in the usage ledger (design task CR-T10): its own signing key and the
//! durable stream, and (design task CR-T12) what it records with them: one event per paid call,
//! feature `graph-extraction` for a chunk and `graph-community-summary` for a community, with the
//! exact tokens `InferResponse` reported ([`record_call`]). The first half had to exist first,
//! because the consumer must trust a producer before that producer's first event arrives.
//!
//! Recording never fails the work: a ledger that cannot be reached is logged loudly (an error
//! per event, with its feature and counts and nothing else) and extraction carries on. The
//! alternative, refusing to extract a chunk because its bill could not be written down, is worse
//! for the person who uploaded the document and no better for the books. Nothing here names a
//! document, a chunk or a user: spend is the organization's.
//!
//! The key is graph-index's alone: issuer `service:graph-index-rs`, key id `graph-events-v1`,
//! scope `events:graph:publish`, and that scope may sign the cost subject and nothing else
//! (`event-envelope-rs`). The public half is mounted into data-orchestrator, which verifies every
//! usage event against the key of the producer it claims to be; embedding-engine and
//! retrieval-engine have keys of their own, so none of the three can stand in for another.
//! Production keys come from the secret manager at an absolute path, never from
//! `.secrets/event-keys` (the cross-plane preflight rejects that path).
//!
//! Off by default (`USAGE_EMIT_ENABLED`). Turned on, graph-index refuses to start unless signed
//! events are enabled, the key is readable and the durable stream can be created or verified: a
//! producer that cannot reach a durable ledger must not run with metering on, because every event
//! it published would be lost without a sound.

use std::sync::Arc;

use anyhow::Context;
use event_envelope_rs::EventSigner;
use usage_ledger_rs::{
    CallUsage, Feature, JetStreamLedger, LedgerSink, UsageBuilder, WireTiming, WireUsage,
};

use crate::config::Config;
use crate::extractor::model_plane::v1::InferResponse;

pub const ISSUER: &str = "service:graph-index-rs";
pub const KEY_ID: &str = "graph-events-v1";
pub const SCOPE: &str = "events:graph:publish";

/// What one answered call cost, as the response reports it. A response served from
/// inference-core's completed-response cache called no provider and records nothing; a provider
/// that reported no usage is recorded as a call with unknown counts (`estimated`), never as free.
pub(crate) fn call_of(response: &InferResponse) -> CallUsage {
    CallUsage::from_wire(WireUsage {
        request_id: &response.request_id,
        model_used: &response.model_used,
        provider_used: &response.provider_used,
        input_tokens: response.input_tokens,
        output_tokens: response.output_tokens,
        cache_read_input_tokens: response.cache_read_input_tokens,
        timing: response.provider_timing.as_ref().map(|timing| WireTiming {
            usage_reported: timing.usage_reported,
            response_cache_hit: timing.response_cache_hit,
        }),
    })
}

/// Record one answered call. `zdr` is the organization's posture as the bearer said it: it only
/// flags the event, never decides whether it is recorded (a zero-retention organization's spend
/// is recorded too; the event is counters only).
///
/// Returns how many events were not recorded, for a caller that wants to count them.
pub(crate) async fn record_call(
    ledger: &dyn LedgerSink,
    feature: Feature,
    org_id: &str,
    zdr: bool,
    response: &InferResponse,
) -> usize {
    record(ledger, feature, org_id, zdr, call_of(response)).await
}

async fn record(
    ledger: &dyn LedgerSink,
    feature: Feature,
    org_id: &str,
    zdr: bool,
    call: CallUsage,
) -> usize {
    let mut builder = match UsageBuilder::new(feature, org_id, zdr) {
        Ok(builder) => builder,
        Err(error) => {
            tracing::error!(
                feature = feature.as_str(),
                %error,
                "SPEND NOT RECORDED: a usage event could not be built"
            );
            return 1;
        }
    };
    builder.push(call);
    let mut lost = 0;
    for event in builder.build() {
        if let Err(error) = ledger.publish(&event).await {
            lost += 1;
            tracing::error!(
                feature = event.feature,
                count = event.count,
                input_tokens = event.input_tokens,
                output_tokens = event.output_tokens,
                %error,
                "SPEND NOT RECORDED: a usage event was not acknowledged by the ledger stream"
            );
        }
    }
    lost
}

/// What the configuration asks of the ledger.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    Off,
    On,
}

/// The boot-time check of the switch, with no I/O: the ledger needs signed events (the consumer
/// refuses an unsigned usage event) and a private key path.
pub(crate) fn plan(enabled: bool, signed_events: bool, key_path: &str) -> anyhow::Result<Plan> {
    if !enabled {
        return Ok(Plan::Off);
    }
    if !signed_events {
        anyhow::bail!(
            "USAGE_EMIT_ENABLED=1 needs signed events (ENABLE_SIGNED_EVENT_CONSUMERS=1): the \
             consumer refuses an unsigned usage event"
        );
    }
    if key_path.trim().is_empty() {
        anyhow::bail!(
            "USAGE_EMIT_ENABLED=1 needs GRAPH_EVENT_PRIVATE_KEY_PATH: the private key that signs \
             graph-index's usage events"
        );
    }
    Ok(Plan::On)
}

/// The signer for graph-index's usage events.
pub(crate) fn signer_from_pem(pem: &[u8], audience: &str) -> anyhow::Result<EventSigner> {
    EventSigner::from_rsa_pem(pem, ISSUER, KEY_ID, audience, SCOPE)
        .map_err(|error| anyhow::anyhow!("graph-index usage signing key: {error}"))
}

/// The ledger, or `None` when the switch is off. `nats` is the connection the signed-event
/// runtime already holds.
pub(crate) async fn start(
    cfg: &Config,
    signed_events: bool,
    nats: Option<&async_nats::Client>,
) -> anyhow::Result<Option<Arc<JetStreamLedger>>> {
    match plan(
        cfg.usage_emit_enabled,
        signed_events,
        &cfg.graph_event_private_key_path,
    )? {
        Plan::Off => {
            tracing::info!("usage ledger disabled (USAGE_EMIT_ENABLED)");
            Ok(None)
        }
        Plan::On => {
            let nats = nats.context(
                "USAGE_EMIT_ENABLED=1 needs the NATS connection of the signed event runtime",
            )?;
            let pem = std::fs::read(&cfg.graph_event_private_key_path)
                .context("read GRAPH_EVENT_PRIVATE_KEY_PATH")?;
            let signer = Arc::new(signer_from_pem(&pem, &cfg.event_auth_audience)?);
            let ledger = JetStreamLedger::new(nats.clone(), signer);
            usage_ledger_rs::ensure_stream(ledger.context()).await?;
            tracing::info!(
                stream = usage_ledger_rs::STREAM_NAME,
                "usage ledger ENABLED: the signer and the durable stream are ready"
            );
            Ok(Some(Arc::new(ledger)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use event_envelope_rs::EventVerifier;
    use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey};
    use rsa::{RsaPrivateKey, RsaPublicKey};
    use usage_ledger_rs::{CallUsage, Feature, UsageBuilder};

    #[test]
    fn the_ledger_is_off_by_default_and_asks_for_what_it_needs_when_on() {
        assert_eq!(plan(false, false, "").unwrap(), Plan::Off);
        assert_eq!(
            plan(false, true, "/run/event-keys/graph-events.pem").unwrap(),
            Plan::Off
        );
        for (signed, path) in [(false, "/k.pem"), (true, ""), (true, "   "), (false, "")] {
            assert!(
                plan(true, signed, path).is_err(),
                "signed={signed} path={path:?}"
            );
        }
        assert_eq!(
            plan(true, true, "/run/event-keys/graph-events.pem").unwrap(),
            Plan::On
        );
    }

    fn keys() -> (String, String) {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("key");
        let public = RsaPublicKey::from(&private);
        (
            private
                .to_pkcs1_pem(Default::default())
                .expect("pem")
                .to_string(),
            public
                .to_pkcs1_pem(Default::default())
                .expect("pem")
                .to_string(),
        )
    }

    fn verifier(public: &str, issuer: &str, key_id: &str, scope: &str) -> EventVerifier {
        EventVerifier::from_rsa_pem(
            public.as_bytes(),
            issuer,
            key_id,
            "dataplane-events",
            scope,
            10,
        )
        .expect("verifier")
    }

    /// The signer is the one the consumer pins: a usage event built by the real builder, signed
    /// with it, is verified by a verifier with graph-index's issuer, key id and scope; the same
    /// envelope is nobody else's, and a zero-retention organization's event signs like any other.
    #[test]
    fn the_signer_is_the_one_the_consumer_pins_and_nobody_elses() {
        let (private, public) = keys();
        let signer = signer_from_pem(private.as_bytes(), "dataplane-events").expect("signer");

        for zdr in [false, true] {
            let mut builder = UsageBuilder::new(Feature::GraphExtraction, "org-a", zdr).unwrap();
            builder.push(CallUsage::from_counts(
                "r1",
                "gpt-4o",
                "azure-openai",
                2100,
                480,
                0,
                true,
                false,
            ));
            let event = builder.build().remove(0);
            let envelope = event.sign(&signer).expect("a graph usage event signs");

            let verified = verifier(&public, ISSUER, KEY_ID, SCOPE)
                .verify("dataplane.cost.ledger", &envelope)
                .expect("the consumer's pins verify it");
            assert_eq!(verified.claims.org_id, "org-a");
            assert_eq!(verified.claims.zdr, zdr);
            assert_eq!(verified.claims.user_id, None);

            for (issuer, key_id, scope) in [
                (
                    "service:embedding-engine-rs",
                    "embedding-events-v1",
                    "events:embedding:publish",
                ),
                (
                    "service:retrieval-engine-rs",
                    "retrieval-events-v1",
                    "events:retrieval:publish",
                ),
            ] {
                assert!(
                    verifier(&public, issuer, key_id, scope)
                        .verify("dataplane.cost.ledger", &envelope)
                        .is_err(),
                    "graph-index's envelope was accepted as {issuer}'s"
                );
            }
        }
    }

    #[test]
    fn the_graph_key_can_sign_the_cost_subject_and_nothing_else() {
        let (private, _) = keys();
        let signer = signer_from_pem(private.as_bytes(), "dataplane-events").expect("signer");
        for subject in [
            "dataplane.documents.indexed",
            "dataplane.documents.created",
            "dataplane.knowledge.units.created",
            "dataplane.dlq.graph-index",
        ] {
            assert!(
                signer
                    .sign(subject, "org-a", None, false, br#"{"org_id":"org-a"}"#)
                    .is_err(),
                "the graph key signed {subject}"
            );
        }
    }

    #[test]
    fn a_key_that_is_not_a_key_is_refused_at_startup() {
        assert!(signer_from_pem(b"not a key", "dataplane-events").is_err());
        let (private, _) = keys();
        assert!(
            signer_from_pem(private.as_bytes(), "  ").is_err(),
            "a blank audience"
        );
    }

    // ── what graph-index records (CR-T12) ────────────────────────────────────

    use crate::extractor::model_plane::v1::ProviderTiming;
    use usage_ledger_rs::MemoryLedger;

    /// An `InferResponse` as inference-core returns it: 612 prompt tokens of which 512 were
    /// served from the provider's prompt cache, 48 completion tokens.
    fn response(request_id: &str, timing: Option<ProviderTiming>) -> InferResponse {
        InferResponse {
            request_id: request_id.to_owned(),
            content: "{}".to_owned(),
            model_used: "gpt-4o-mini-2024-07-18".to_owned(),
            provider_used: "azure-openai".to_owned(),
            input_tokens: 612,
            output_tokens: 48,
            cache_read_input_tokens: 512,
            provider_timing: timing,
            ..Default::default()
        }
    }

    fn reported() -> Option<ProviderTiming> {
        Some(ProviderTiming {
            usage_reported: true,
            response_cache_hit: false,
            attempts: 1,
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn a_graph_call_is_recorded_with_the_exact_tokens_and_the_feature_of_its_kind() {
        let ledger = MemoryLedger::new();
        let lost = record_call(
            &ledger,
            Feature::GraphExtraction,
            "org-a",
            false,
            &response("req-1", reported()),
        )
        .await;
        assert_eq!(lost, 0);
        let events = ledger.events();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(
            (event.feature, event.event_type),
            ("graph-extraction", "llm")
        );
        assert_eq!(
            (
                event.input_tokens,
                event.output_tokens,
                event.cache_read_tokens,
                event.count
            ),
            (612, 48, 512, 1),
            "exactly what the provider reported; the cache reads are detail of the prompt tokens, never added to them"
        );
        assert_eq!(
            (event.model_used.as_str(), event.provider_used.as_str()),
            ("gpt-4o-mini-2024-07-18", "azure-openai"),
            "what served the call"
        );
        assert_eq!(event.org_id, "org-a");
        assert!(!event.zdr && !event.estimated);
        // Counters only: no document, no chunk, no user, no text.
        let wire = serde_json::to_string(event).expect("an event is JSON");
        for forbidden in ["document", "knowledge", "user", "chunk", "text", "content"] {
            assert!(
                !wire.contains(forbidden),
                "the event names `{forbidden}`: {wire}"
            );
        }
    }

    #[tokio::test]
    async fn a_community_summary_is_its_own_feature_and_a_replay_is_the_same_event() {
        let ledger = MemoryLedger::new();
        let call = response("req-summary", reported());
        record_call(
            &ledger,
            Feature::GraphCommunitySummary,
            "org-a",
            false,
            &call,
        )
        .await;
        record_call(
            &ledger,
            Feature::GraphCommunitySummary,
            "org-a",
            false,
            &call,
        )
        .await;
        record_call(
            &ledger,
            Feature::GraphCommunitySummary,
            "org-a",
            false,
            &response("req-other", reported()),
        )
        .await;
        let events = ledger.events();
        assert_eq!(events.len(), 3);
        assert!(events
            .iter()
            .all(|event| event.feature == "graph-community-summary"));
        assert_eq!(
            events[0].idempotency_key, events[1].idempotency_key,
            "the same call published twice is one payment: the broker and the consumer both drop the second"
        );
        assert_ne!(
            events[0].idempotency_key, events[2].idempotency_key,
            "another call is another payment"
        );
    }

    #[tokio::test]
    async fn a_response_served_from_the_response_cache_called_no_provider_and_records_nothing() {
        let ledger = MemoryLedger::new();
        let cached = Some(ProviderTiming {
            usage_reported: true,
            response_cache_hit: true,
            ..Default::default()
        });
        record_call(
            &ledger,
            Feature::GraphExtraction,
            "org-a",
            false,
            &response("req-cached", cached),
        )
        .await;
        assert!(ledger.events().is_empty());
    }

    #[tokio::test]
    async fn a_provider_that_reported_no_usage_is_a_call_with_unknown_counts_and_never_free() {
        let ledger = MemoryLedger::new();
        let unreported = Some(ProviderTiming {
            usage_reported: false,
            response_cache_hit: false,
            ..Default::default()
        });
        record_call(
            &ledger,
            Feature::GraphExtraction,
            "org-a",
            false,
            &response("req-unreported", unreported),
        )
        .await;
        let events = ledger.events();
        assert_eq!(
            events.len(),
            1,
            "the call happened: it is counted, not dropped"
        );
        assert!(events[0].estimated, "and it says its counts are not exact");
        assert_eq!(events[0].cost_basis, "estimated");
    }

    #[tokio::test]
    async fn a_zero_retention_organizations_spend_is_recorded_and_flagged() {
        let ledger = MemoryLedger::new();
        record_call(
            &ledger,
            Feature::GraphExtraction,
            "org-zdr",
            true,
            &response("req-zdr", reported()),
        )
        .await;
        let events = ledger.events();
        assert_eq!(events.len(), 1);
        assert!(events[0].zdr, "the posture rides on the event");
        assert_eq!(events[0].org_id, "org-zdr");
    }

    /// Recording never fails the work, and a ledger that does not answer is loud, with the
    /// counts that were lost and nothing else (no organization, no document).
    #[tokio::test]
    async fn a_ledger_that_refuses_is_logged_loudly_and_never_fails_the_work() {
        let (logs, _guard) = crate::testing::capture_logs();
        let ledger = MemoryLedger::new();
        ledger.refuse_next(1);
        let lost = record_call(
            &ledger,
            Feature::GraphExtraction,
            "org-a",
            false,
            &response("req-refused", reported()),
        )
        .await;
        assert_eq!(lost, 1);
        assert!(ledger.events().is_empty());
        let logged = logs.events_saying(
            "SPEND NOT RECORDED: a usage event was not acknowledged by the ledger stream",
        );
        assert_eq!(logged.len(), 1, "{:?}", logs.events());
        assert_eq!(logged[0]["level"], "ERROR");
        assert_eq!(logged[0]["feature"], "graph-extraction");
        assert_eq!(logged[0]["input_tokens"], 612);
        assert!(
            !logs.text().contains("org-a"),
            "the loss names the spend, not the organization"
        );
    }

    #[tokio::test]
    async fn a_blank_organization_cannot_be_recorded_and_says_so() {
        let (logs, _guard) = crate::testing::capture_logs();
        let ledger = MemoryLedger::new();
        let lost = record_call(
            &ledger,
            Feature::GraphExtraction,
            "  ",
            false,
            &response("req-blank", reported()),
        )
        .await;
        assert_eq!(lost, 1);
        assert_eq!(
            logs.events_saying("SPEND NOT RECORDED: a usage event could not be built")
                .len(),
            1
        );
    }
}
