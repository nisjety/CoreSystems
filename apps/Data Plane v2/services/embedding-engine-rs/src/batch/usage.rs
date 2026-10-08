//! The usage ledger half of a batch (design task CR-T10): what the batch paid for, as events.
//!
//! A batch pays a provider in two places, and each is recorded where the money is spent:
//!
//! * **Contextualization** (`feature = contextual-retrieval`): one chat completion per chunk,
//!   made inside [`super::contexts::ensure`]. Its [`MeteredCall`]s carry exactly what the
//!   provider reported, so the event holds exact tokens. The calls are recorded inside `ensure`,
//!   as soon as they are made and before its own database writes, so nothing that fails after
//!   them (the write-ahead, the rest of the batch) can lose them: a redelivery that finds the
//!   contexts stored calls nothing, so each call is recorded once, however many times the batch
//!   is retried.
//! * **Embedding** (`feature = embedding`): one request per (organization, zero-retention)
//!   group. An embedding request returns vectors and no usage, so its tokens are this service's
//!   estimate (the length of the text it sent divided by four) and the event says so
//!   (`estimated`). Recorded right after the request returns, for the same reason: if a later
//!   step fails and the batch is redelivered, the unit is embedded, and paid for, again, and that
//!   second payment is a second event (it has its own call id).
//!
//! Neither event names a user, a document or a chunk. Spend is attributed to the organization:
//! background spend booked to the person whose upload caused it would count against that
//! person's chat budget. A zero-retention organization's embedding is recorded too, with
//! `zdr = true` and the same counters, which is what makes its spend visible at all.
//!
//! Recording never fails a batch: a ledger that cannot be reached is logged loudly (an error per
//! event, with its feature and counts and nothing else) and ingest carries on. The alternative,
//! refusing to index a document because its bill could not be written down, is worse for the
//! person who uploaded it and no better for the books.
//!
//! The ledger is `Pipeline::usage`. It is `None` unless `USAGE_EMIT_ENABLED=1`, and then this
//! service refuses to start when it cannot create or verify the durable stream. While it is
//! `None` the version 1 event (`v1_cost_events`) is published as before.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

use usage_ledger_rs::{CallUsage, Feature, LedgerSink, UsageBuilder, UsageEvent};

use super::contexts::MeteredDocument;
use super::BatchItem;
use crate::context::MeteredCall;

/// What the usage ledger needs from the service's event security: the key that signs the events.
///
/// `Ok(None)` when the ledger is off. When it is on and the service has no signing key (signed
/// events are not enabled) it is an error, so the service stops at startup: the consumer
/// verifies every usage event against the producer's key and would refuse an unsigned one, so
/// metering that cannot be signed would be metering that is silently thrown away.
pub(crate) fn usage_signer(
    enabled: bool,
    signer: Option<std::sync::Arc<event_envelope_rs::EventSigner>>,
) -> anyhow::Result<Option<std::sync::Arc<event_envelope_rs::EventSigner>>> {
    match (enabled, signer) {
        (false, _) => Ok(None),
        (true, Some(signer)) => Ok(Some(signer)),
        (true, None) => anyhow::bail!(
            "USAGE_EMIT_ENABLED=1 needs signed events (ENABLE_SIGNED_EVENT_CONSUMERS=1 and \
             EMBEDDING_EVENT_PRIVATE_KEY_PATH): the consumer refuses an unsigned usage event"
        ),
    }
}

/// One embedding request a batch made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EmbedCall {
    pub org_id: String,
    pub zdr: bool,
    /// Made for this one request: an embedding request has no provider request id. It is what
    /// makes the event idempotent under a publish retry and distinct from a later re-embed of
    /// the same text.
    pub call_id: String,
    pub texts: usize,
    /// `bytes / 4` of the text that was sent, as the estimate has always been made.
    pub estimated_tokens: u64,
}

/// One contextualization call as the ledger counts it. The counts are the provider's, copied
/// exactly: inference-core folds the cache reads into the prompt tokens already, so nothing is
/// added to them here.
fn call_of(call: &MeteredCall) -> CallUsage {
    CallUsage::from_counts(
        &call.request_id,
        &call.model_used,
        &call.provider_used,
        u64::from(call.usage.input_tokens),
        u64::from(call.usage.output_tokens),
        u64::from(call.usage.cache_read_input_tokens),
        call.usage.usage_reported,
        call.usage.response_cache_hit,
    )
}

/// The events for every provider call a batch's context phase made, one per organization and
/// model. Empty when it made none (everything reused, restricted or deferred), and a call that
/// inference-core answered from its response cache adds nothing.
///
/// The posture is `zdr = false`: a call is made only for a document that is not restricted and
/// under a bearer whose organization is not zero-retention (the contextualizer withholds both
/// before any call). If a route attested for zero retention ever serves contextualization, the
/// flag has to travel with the call: [`MeteredCall`] would carry it, and this function would
/// group by it.
pub(super) fn context_events(metered: &[MeteredDocument]) -> Vec<UsageEvent> {
    let mut builders: BTreeMap<&str, UsageBuilder> = BTreeMap::new();
    for document in metered {
        let Ok(builder) = builder_for(
            &mut builders,
            Feature::ContextualRetrieval,
            &document.org_id,
            false,
        ) else {
            continue;
        };
        for call in &document.calls {
            builder.push(call_of(call));
        }
    }
    builders
        .into_values()
        .flat_map(UsageBuilder::build)
        .collect()
}

/// The events for the embedding requests a batch made, one per organization and posture.
pub(super) fn embed_events(calls: &[EmbedCall], model: &str, provider: &str) -> Vec<UsageEvent> {
    let mut builders: BTreeMap<(&str, bool), UsageBuilder> = BTreeMap::new();
    for call in calls {
        let builder = match builders.entry((call.org_id.as_str(), call.zdr)) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                match UsageBuilder::new(Feature::Embedding, &call.org_id, call.zdr) {
                    Ok(builder) => entry.insert(builder),
                    Err(error) => {
                        tracing::error!(
                            %error,
                            "an embedding request could not be recorded in the usage ledger"
                        );
                        continue;
                    }
                }
            }
        };
        builder.push(CallUsage::estimated(
            &call.call_id,
            model,
            provider,
            call.texts as u64,
            call.estimated_tokens,
        ));
    }
    builders
        .into_values()
        .flat_map(UsageBuilder::build)
        .collect()
}

fn builder_for<'a, 'm>(
    builders: &'m mut BTreeMap<&'a str, UsageBuilder>,
    feature: Feature,
    org_id: &'a str,
    zdr: bool,
) -> Result<&'m mut UsageBuilder, usage_ledger_rs::LedgerError> {
    if !builders.contains_key(org_id) {
        builders.insert(org_id, UsageBuilder::new(feature, org_id, zdr)?);
    }
    Ok(builders.get_mut(org_id).expect("inserted above"))
}

/// Publish the events. Never fails: see the module documentation. Returns how many were not
/// recorded, for the batch's log line.
pub(super) async fn publish(ledger: &dyn LedgerSink, events: &[UsageEvent]) -> usize {
    let mut lost = 0;
    for event in events {
        if let Err(error) = ledger.publish(event).await {
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

/// The version 1 cost events a batch publishes while the usage ledger is off: one per
/// (organization, user, zero-retention) group, each with its own idempotency key.
///
/// The key used to be one value shared by every group of a batch (a hash of all the batch's
/// knowledge ids and the model). The consumer stores the key behind the organization only, so a
/// second user of the same organization in a batch, or the zero-retention group of the same
/// organization, collided with the first group's key and was silently dropped as a duplicate.
/// Now the key is a function of what the group is, so a group is never mistaken for another and a
/// redelivery of the same group is still the same key.
pub(super) fn v1_cost_events(
    items: &[BatchItem],
    model: &str,
    provider: &str,
    embedded_len: impl Fn(&BatchItem) -> usize,
) -> Vec<V1CostEvent> {
    type Group<'a> = (usize, usize, Vec<&'a str>);
    let mut groups: BTreeMap<(&str, Option<&str>, bool), Group<'_>> = BTreeMap::new();
    for item in items {
        let group = groups
            .entry((item.org_id.as_str(), item.user_id.as_deref(), item.zdr))
            .or_default();
        group.0 += 1;
        // Bill the text that was actually EMBEDDED, not the raw chunk: contextual retrieval
        // makes the embedded string longer, and measuring the chunk would under-report it.
        group.1 += embedded_len(item) / 4;
        group.2.push(item.knowledge_id.as_str());
    }
    groups
        .into_iter()
        .map(
            |((org_id, user_id, zdr), (count, estimated_tokens, mut ids))| {
                ids.sort_unstable();
                let identity =
                    format!("{org_id}|{}|{zdr}|{}", user_id.unwrap_or(""), ids.join(","));
                let idempotency_key = super::make_idempotency_key("embed.cost", &identity, model);
                V1CostEvent {
                    org_id: org_id.to_owned(),
                    user_id: user_id.map(str::to_owned),
                    zdr,
                    payload: serde_json::json!({
                        "event_type": "embedding", "model": model,
                        "provider": provider, "count": count,
                        "estimated_tokens": estimated_tokens, "org_id": org_id,
                        "user_id": user_id, "zdr": zdr,
                        "idempotency_key": idempotency_key,
                    }),
                    idempotency_key,
                }
            },
        )
        .collect()
}

/// A version 1 cost event, ready to be signed and published.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct V1CostEvent {
    pub org_id: String,
    pub user_id: Option<String>,
    pub zdr: bool,
    pub idempotency_key: String,
    pub payload: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::outcome::Usage;
    use crate::context::MeteredCall;
    use usage_ledger_rs::MemoryLedger;

    fn usage(input: u32, output: u32, cache: u32) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            cache_read_input_tokens: cache,
            cache_creation_input_tokens: 0,
            usage_reported: true,
            response_cache_hit: false,
        }
    }

    fn call(id: &str, model: &str, usage: Usage) -> MeteredCall {
        MeteredCall {
            request_id: id.to_owned(),
            model_used: model.to_owned(),
            provider_used: "azure-openai".to_owned(),
            usage,
        }
    }

    fn document(org: &str, doc: &str, calls: Vec<MeteredCall>) -> MeteredDocument {
        MeteredDocument {
            org_id: org.to_owned(),
            document_id: doc.to_owned(),
            calls,
        }
    }

    // ── contextualization ───────────────────────────────────────────────────

    /// The numbers of a real gpt-4o-mini answer, as the contextualizer captures them: 612
    /// prompt tokens (of which 512 came from the provider's prompt cache) and 48 completion
    /// tokens. They arrive in the event exactly, with the cache reads as detail and not added.
    #[test]
    fn the_provider_reported_tokens_are_copied_exactly_into_the_event() {
        let events = context_events(&[document(
            "org-a",
            "doc-1",
            vec![call("req-1", "gpt-4o-mini-2024-07-18", usage(612, 48, 512))],
        )]);
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.feature, "contextual-retrieval");
        assert_eq!(event.event_type, "llm");
        assert_eq!(event.model_used, "gpt-4o-mini-2024-07-18");
        assert_eq!(event.provider_used, "azure-openai");
        assert_eq!(
            (
                event.input_tokens,
                event.output_tokens,
                event.cache_read_tokens
            ),
            (612, 48, 512)
        );
        assert_eq!(event.count, 1);
        assert_eq!(
            (event.cost_basis, event.estimated, event.zdr),
            ("metered", false, false)
        );
        assert_eq!(event.org_id, "org-a");
    }

    #[test]
    fn a_response_served_from_inference_cores_cache_emits_zero() {
        let mut cached = usage(612, 48, 0);
        cached.response_cache_hit = true;
        assert!(
            context_events(&[document(
                "org-a",
                "doc-1",
                vec![call("req-1", "gpt-4o-mini", cached)]
            )])
            .is_empty(),
            "a cached answer called no provider and spent nothing"
        );
    }

    #[test]
    fn calls_are_summed_per_organization_and_never_across_organizations() {
        let events = context_events(&[
            document(
                "org-a",
                "doc-1",
                vec![
                    call("a1", "gpt-4o-mini", usage(100, 10, 0)),
                    call("a2", "gpt-4o-mini", usage(200, 20, 0)),
                ],
            ),
            document(
                "org-b",
                "doc-2",
                vec![call("b1", "gpt-4o-mini", usage(50, 5, 0))],
            ),
            document(
                "org-a",
                "doc-3",
                vec![call("a3", "gpt-4o-mini", usage(300, 30, 0))],
            ),
        ]);
        assert_eq!(events.len(), 2);
        let by_org = |org: &str| events.iter().find(|event| event.org_id == org).unwrap();
        assert_eq!(
            (by_org("org-a").count, by_org("org-a").input_tokens),
            (3, 600)
        );
        assert_eq!(
            (by_org("org-b").count, by_org("org-b").input_tokens),
            (1, 50)
        );
    }

    #[test]
    fn a_call_whose_provider_reported_no_usage_is_still_recorded_and_marks_the_event_estimated() {
        let mut unreported = usage(0, 0, 0);
        unreported.usage_reported = false;
        let events = context_events(&[document(
            "org-a",
            "doc-1",
            vec![call("u1", "gpt-4o-mini", unreported)],
        )]);
        assert_eq!(events.len(), 1, "the call happened");
        assert_eq!((events[0].count, events[0].estimated), (1, true));
    }

    #[test]
    fn nothing_called_nothing_recorded() {
        assert!(context_events(&[]).is_empty());
        assert!(context_events(&[document("org-a", "doc-1", vec![])]).is_empty());
    }

    #[test]
    fn the_same_calls_make_the_same_key_and_other_calls_another() {
        let calls = |ids: &[&str]| {
            context_events(&[document(
                "org-a",
                "doc-1",
                ids.iter()
                    .map(|id| call(id, "gpt-4o-mini", usage(1, 1, 0)))
                    .collect(),
            )])
            .remove(0)
            .idempotency_key
        };
        assert_eq!(
            calls(&["a", "b"]),
            calls(&["b", "a"]),
            "a redelivered batch is the same event"
        );
        assert_ne!(calls(&["a", "b"]), calls(&["a", "c"]));
    }

    #[test]
    fn the_context_events_carry_no_document_id_and_no_user() {
        let events = context_events(&[document(
            "org-a",
            "doc-secret-name",
            vec![call("a1", "gpt-4o-mini", usage(1, 1, 0))],
        )]);
        let payload = String::from_utf8(events[0].payload().unwrap()).unwrap();
        assert!(!payload.contains("doc-secret-name"));
        assert!(!payload.contains("user_id"));
        assert!(!payload.contains("document_id"));
    }

    // ── embedding ───────────────────────────────────────────────────────────

    fn embed(org: &str, zdr: bool, id: &str, texts: usize, tokens: u64) -> EmbedCall {
        EmbedCall {
            org_id: org.to_owned(),
            zdr,
            call_id: id.to_owned(),
            texts,
            estimated_tokens: tokens,
        }
    }

    #[test]
    fn an_embedding_event_is_estimated_counts_texts_and_names_the_model_that_embedded() {
        let events = embed_events(
            &[embed("org-a", false, "call-1", 24, 9100)],
            "embed-v-4-0",
            "azure-cohere",
        );
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(
            (event.feature, event.event_type),
            ("embedding", "embedding")
        );
        assert_eq!(
            (event.model_used.as_str(), event.provider_used.as_str()),
            ("embed-v-4-0", "azure-cohere")
        );
        assert_eq!(
            (event.count, event.input_tokens, event.output_tokens),
            (24, 9100, 0)
        );
        assert_eq!((event.cost_basis, event.estimated), ("estimated", true));
    }

    /// A zero-retention organization's embedding is recorded as counters with `zdr = true`,
    /// separately from its other embedding: the posture is part of the event.
    #[test]
    fn zero_retention_embedding_is_recorded_numeric_only_and_apart_from_the_rest() {
        let events = embed_events(
            &[
                embed("org-a", false, "call-1", 10, 400),
                embed("org-a", true, "call-2", 3, 120),
            ],
            "embed-v-4-0",
            "azure-cohere",
        );
        assert_eq!(events.len(), 2);
        let restricted = events.iter().find(|event| event.zdr).expect("a zdr event");
        let open = events
            .iter()
            .find(|event| !event.zdr)
            .expect("an open event");
        assert_eq!((restricted.count, restricted.input_tokens), (3, 120));
        assert_eq!((open.count, open.input_tokens), (10, 400));
        assert_ne!(restricted.idempotency_key, open.idempotency_key);
        let payload = String::from_utf8(restricted.payload().unwrap()).unwrap();
        assert!(
            !payload.contains("user_id") && !payload.contains("document"),
            "{payload}"
        );
    }

    #[test]
    fn a_re_embed_is_a_second_payment_and_a_second_event() {
        let first = embed_events(&[embed("org-a", false, "call-1", 5, 200)], "m", "p");
        let again = embed_events(&[embed("org-a", false, "call-2", 5, 200)], "m", "p");
        assert_ne!(
            first[0].idempotency_key, again[0].idempotency_key,
            "the same text embedded twice was paid for twice; the second must not be dropped as a duplicate"
        );
        let retried = embed_events(&[embed("org-a", false, "call-1", 5, 200)], "m", "p");
        assert_eq!(
            first[0].idempotency_key, retried[0].idempotency_key,
            "a publish retry is the same event"
        );
    }

    // ── the version 1 event while the ledger is off ─────────────────────────

    fn item(kid: &str, org: &str, user: Option<&str>, zdr: bool, text: &str) -> BatchItem {
        BatchItem {
            knowledge_id: kid.to_owned(),
            document_id: format!("doc-of-{kid}"),
            org_id: org.to_owned(),
            chunk_index: 0,
            text: text.to_owned(),
            zdr,
            user_id: user.map(str::to_owned),
            document_date: None,
            chunk_context: None,
        }
    }

    /// The key the producer used before this change: one value for every group of the batch.
    fn shared_key_before(items: &[BatchItem], model: &str) -> String {
        let mut ids: Vec<String> = items.iter().map(|i| i.knowledge_id.clone()).collect();
        ids.sort();
        super::super::make_idempotency_key("embed.cost", &ids.join(","), model)
    }

    /// The defect: one idempotency key for all the (organization, user, zdr) groups of a batch,
    /// while the consumer stores the key behind the organization only. A second user of the same
    /// organization in the same batch was therefore silently dropped as a duplicate.
    #[test]
    fn two_users_of_one_organization_in_a_batch_no_longer_share_a_key() {
        let items = vec![
            item("k1", "org-a", Some("user-1"), false, "alpha text"),
            item("k2", "org-a", Some("user-2"), false, "beta text"),
        ];
        // What the old code did, on exactly this batch: every group got the same key.
        let old = shared_key_before(&items, "embed-v-4-0");
        let events = v1_cost_events(&items, "embed-v-4-0", "azure-cohere", |i| i.text.len());
        assert_eq!(events.len(), 2, "one event per user");
        assert_ne!(
            events[0].idempotency_key, events[1].idempotency_key,
            "two users of one organization share a key, so the consumer drops the second ({old})"
        );
        for event in &events {
            assert_ne!(event.idempotency_key, old);
            assert_eq!(
                event.payload["idempotency_key"],
                event.idempotency_key.as_str()
            );
        }
    }

    #[test]
    fn the_zero_retention_group_of_the_same_user_is_not_mistaken_for_the_open_one() {
        let items = vec![
            item("k1", "org-a", Some("user-1"), false, "alpha"),
            item("k2", "org-a", Some("user-1"), true, "beta"),
            item("k3", "org-b", Some("user-1"), false, "gamma"),
        ];
        let events = v1_cost_events(&items, "m", "p", |i| i.text.len());
        let keys: std::collections::BTreeSet<&str> = events
            .iter()
            .map(|event| event.idempotency_key.as_str())
            .collect();
        assert_eq!(events.len(), 3);
        assert_eq!(keys.len(), 3, "every group has a key of its own");
    }

    #[test]
    fn a_redelivered_group_gets_the_same_key_whatever_order_its_units_arrived_in() {
        let forward = vec![
            item("k1", "org-a", Some("user-1"), false, "a"),
            item("k2", "org-a", Some("user-1"), false, "b"),
        ];
        let mut backward = forward
            .iter()
            .map(|i| item(&i.knowledge_id, "org-a", Some("user-1"), false, &i.text))
            .collect::<Vec<_>>();
        backward.reverse();
        let key = |items: &[BatchItem]| {
            v1_cost_events(items, "m", "p", |i| i.text.len())
                .remove(0)
                .idempotency_key
        };
        assert_eq!(key(&forward), key(&backward));
    }

    #[test]
    fn the_version_1_estimate_counts_what_was_embedded_not_the_bare_chunk() {
        let items = vec![item("k1", "org-a", None, false, "abcd")];
        let events = v1_cost_events(&items, "m", "p", |_| 4000);
        assert_eq!(events[0].payload["estimated_tokens"], 1000);
        assert_eq!(events[0].payload["count"], 1);
        assert!(events[0].payload["user_id"].is_null());
    }

    // ── the gate ────────────────────────────────────────────────────────────

    #[test]
    fn the_ledger_is_off_by_default_and_refuses_to_run_unsigned() {
        assert!(usage_signer(false, None).unwrap().is_none());
        let Err(error) = usage_signer(true, None) else {
            panic!("metering that cannot be signed was allowed to start");
        };
        assert!(error.to_string().contains("USAGE_EMIT_ENABLED"), "{error}");
    }

    // ── publishing ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn a_ledger_that_refuses_loses_the_event_loudly_and_never_the_batch() {
        let ledger = MemoryLedger::new();
        ledger.refuse_next(1);
        let events = embed_events(
            &[
                embed("org-a", false, "c1", 1, 1),
                embed("org-b", false, "c2", 1, 1),
            ],
            "m",
            "p",
        );
        assert_eq!(events.len(), 2);
        let lost = publish(&ledger, &events).await;
        assert_eq!(lost, 1, "one event was refused");
        assert_eq!(ledger.events().len(), 1, "the other was still recorded");
    }

    /// When a real `InferResponse` captured from the live deployment exists
    /// (`tests/fixtures/contracts/infer-response-azure-live.json`, written by
    /// `tests/scripts/contextual-retrieval-live-proof.sh`), every token field of every case
    /// arrives in the event exactly. Without it this prints `pending`, and FAILS when
    /// `REQUIRE_CONTRACT` is set, as the validation tests of the same capture do.
    #[test]
    fn a_captured_live_response_arrives_in_the_event_token_for_token() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/contracts/infer-response-azure-live.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            assert!(
                std::env::var_os("REQUIRE_CONTRACT").is_none(),
                "REQUIRE_CONTRACT is set and {} has not been captured yet",
                path.display()
            );
            eprintln!("pending: the live InferResponse capture does not exist yet");
            return;
        };
        let capture: serde_json::Value = serde_json::from_str(&text).expect("the capture is JSON");
        let cases = capture["cases"].as_array().expect("cases");
        assert!(!cases.is_empty());
        for (index, case) in cases.iter().enumerate() {
            let token = |key: &str| case[key].as_u64().unwrap_or(0) as u32;
            let metered = MeteredCall {
                request_id: format!("live-{index}"),
                model_used: case["model_used"].as_str().unwrap_or("unknown").to_owned(),
                provider_used: case["provider_used"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned(),
                usage: Usage {
                    input_tokens: token("input_tokens"),
                    output_tokens: token("output_tokens"),
                    cache_read_input_tokens: token("cache_read_input_tokens"),
                    cache_creation_input_tokens: token("cache_creation_input_tokens"),
                    usage_reported: case["usage_reported"].as_bool().unwrap_or(true),
                    response_cache_hit: case["response_cache_hit"].as_bool().unwrap_or(false),
                },
            };
            let events = context_events(&[document("org-live", "doc", vec![metered.clone()])]);
            if metered.usage.response_cache_hit {
                assert!(events.is_empty());
                continue;
            }
            assert_eq!(events.len(), 1, "case {index}");
            assert_eq!(
                events[0].input_tokens,
                u64::from(metered.usage.input_tokens),
                "case {index}"
            );
            assert_eq!(
                events[0].output_tokens,
                u64::from(metered.usage.output_tokens),
                "case {index}"
            );
            assert_eq!(
                events[0].cache_read_tokens,
                u64::from(metered.usage.cache_read_input_tokens),
                "case {index}"
            );
        }
    }
}
