//! The schema_version 2 usage event and the builder that makes it.
//!
//! One event covers the calls one organization's work made to one model of one provider
//! for one feature, in one batch. The fields are exactly these and no others:
//!
//! | field | meaning |
//! | --- | --- |
//! | `schema_version` | `2` |
//! | `event_type` | `embedding`, `llm` or `rerank` ([`crate::EventType`]) |
//! | `feature` | what caused the spend ([`crate::Feature`]) |
//! | `model_used`, `provider_used` | what actually served the calls (`InferResponse`), never what was asked for |
//! | `count` | the items the calls covered: provider calls for `llm`, texts for `embedding`, candidates for `rerank` |
//! | `input_tokens`, `output_tokens`, `cache_read_tokens` | exact sums of what the provider reported; `cache_read_tokens` is detail of `input_tokens` |
//! | `units` | billed units that are not tokens (a reranker's search units), absent otherwise |
//! | `cost_basis`, `estimated` | `metered` / `false`, or `estimated` / `true` when any call's counts are not provider-reported |
//! | `request_digest` | sha256 of the sorted request ids of the calls |
//! | `org_id`, `zdr` | the organization and whether it was zero-retention (pinned by the signed claims) |
//! | `idempotency_key` | `usage:{feature}:{request_digest}` |
//!
//! There is no document id, no chunk id, no user id and no text: the event is content-free
//! by construction, which is what lets a zero-retention organization's usage be recorded at
//! all. Spend is attributed to the organization, never to the user whose upload caused it:
//! background spend booked to a person would count against that person's chat budget.
//!
//! The key is the hash of the request ids and nothing else, so it is the same whichever
//! order the calls were collected in and however many times the event is published, and it
//! differs for a re-embed of the same text, which is a second payment.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

use crate::call::{CallBasis, CallUsage};
use crate::feature::Feature;
use crate::{LedgerError, SCHEMA_VERSION, SUBJECT_COST_LEDGER};

/// The most any one token or unit counter of an event may hold. The verifier refuses more,
/// and the builder saturates here rather than overflow: a sum this large is a bug upstream,
/// and a capped event is visible where a wrapped one would not be.
pub const MAX_COUNTER: u64 = 1 << 40;

/// The most the item count of an event may hold: it is stored in a 32-bit column.
pub const MAX_COUNT: u64 = i32::MAX as u64;

/// The longest model or provider name an event carries (the verifier's bound).
const MAX_NAME_BYTES: usize = 256;

/// The name used when a response named no model or provider: valid, and visibly wrong.
const UNKNOWN: &str = "unknown";

/// The v2 payload. See the module documentation for every field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UsageEvent {
    pub schema_version: u32,
    pub event_type: &'static str,
    pub feature: &'static str,
    pub model_used: String,
    pub provider_used: String,
    pub count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<u64>,
    pub cost_basis: &'static str,
    pub estimated: bool,
    pub request_digest: String,
    pub org_id: String,
    pub zdr: bool,
    pub idempotency_key: String,
}

impl UsageEvent {
    /// The payload bytes that get signed.
    pub fn payload(&self) -> Result<Vec<u8>, LedgerError> {
        Ok(serde_json::to_vec(self)?)
    }

    /// The signed envelope for the cost subject. No user is named: the claim and the
    /// payload both carry the organization only.
    pub fn sign(&self, signer: &event_envelope_rs::EventSigner) -> Result<Vec<u8>, LedgerError> {
        Ok(signer.sign(
            SUBJECT_COST_LEDGER,
            &self.org_id,
            None,
            self.zdr,
            &self.payload()?,
        )?)
    }

    /// The id the broker de-duplicates a publish by (`Nats-Msg-Id`). Includes the
    /// organization so two organizations can never collide on it.
    pub fn message_id(&self) -> String {
        format!("{}:{}", self.org_id, self.idempotency_key)
    }
}

/// Collects the calls one organization's work made for one feature and aggregates them.
#[derive(Debug)]
pub struct UsageBuilder {
    feature: Feature,
    org_id: String,
    zdr: bool,
    calls: Vec<CallUsage>,
}

impl UsageBuilder {
    /// `zdr` is the organization's posture as the work knew it (the bearer's claim, or the
    /// document's classification): it only decides how the event is flagged, never
    /// whether it is recorded.
    pub fn new(feature: Feature, org_id: &str, zdr: bool) -> Result<Self, LedgerError> {
        if org_id.trim().is_empty() || org_id != org_id.trim() {
            return Err(LedgerError::InvalidOrganization);
        }
        Ok(Self {
            feature,
            org_id: org_id.to_owned(),
            zdr,
            calls: Vec::new(),
        })
    }

    pub fn push(&mut self, call: CallUsage) -> &mut Self {
        self.calls.push(call);
        self
    }

    /// The events: one per (model, provider) that served any of the calls, in a stable
    /// order. Empty when nothing was spent (no calls, or only calls answered from
    /// inference-core's cache).
    pub fn build(self) -> Vec<UsageEvent> {
        let Self {
            feature,
            org_id,
            zdr,
            calls,
        } = self;
        // A request id seen twice is one call that was pushed twice. A call with no id
        // cannot be told from another, so it gets one of its own: counting it twice would
        // be better than dropping the second.
        let mut seen = BTreeSet::new();
        let mut groups: BTreeMap<(String, String), Vec<CallUsage>> = BTreeMap::new();
        for mut call in calls.into_iter().filter(CallUsage::is_metered) {
            if call.request_id.is_empty() {
                call.request_id = uuid::Uuid::new_v4().to_string();
            }
            if !seen.insert(call.request_id.clone()) {
                continue;
            }
            let key = (
                name_or_unknown(&call.model_used),
                name_or_unknown(&call.provider_used),
            );
            groups.entry(key).or_default().push(call);
        }

        groups
            .into_iter()
            .map(|((model_used, provider_used), calls)| {
                aggregate(feature, &org_id, zdr, model_used, provider_used, &calls)
            })
            .collect()
    }
}

/// One event for the calls of one (model, provider).
fn aggregate(
    feature: Feature,
    org_id: &str,
    zdr: bool,
    model_used: String,
    provider_used: String,
    calls: &[CallUsage],
) -> UsageEvent {
    let sum = |pick: fn(&CallUsage) -> u64, ceiling: u64| -> u64 {
        calls
            .iter()
            .fold(0u64, |total, call| total.saturating_add(pick(call)))
            .min(ceiling)
    };
    let units = calls
        .iter()
        .filter_map(|call| call.units)
        .reduce(u64::saturating_add)
        .map(|units| units.min(MAX_COUNTER));
    let estimated = calls
        .iter()
        .any(|call| matches!(call.basis, CallBasis::Estimated | CallBasis::Unreported));
    let mut ids: Vec<&str> = calls.iter().map(|call| call.request_id.as_str()).collect();
    ids.sort_unstable();
    let request_digest = digest(&ids);
    UsageEvent {
        schema_version: SCHEMA_VERSION,
        event_type: feature.event_type().as_str(),
        feature: feature.as_str(),
        model_used,
        provider_used,
        count: sum(|call| call.items, MAX_COUNT),
        input_tokens: sum(|call| call.input_tokens, MAX_COUNTER),
        output_tokens: sum(|call| call.output_tokens, MAX_COUNTER),
        cache_read_tokens: sum(|call| call.cache_read_tokens, MAX_COUNTER),
        units,
        cost_basis: if estimated { "estimated" } else { "metered" },
        estimated,
        idempotency_key: format!("usage:{}:{request_digest}", feature.as_str()),
        request_digest,
        org_id: org_id.to_owned(),
        zdr,
    }
}

fn name_or_unknown(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return UNKNOWN.to_owned();
    }
    // Cut at a character boundary: a name is text, and a cut in the middle of a character
    // is not valid UTF-8.
    let mut end = name.len().min(MAX_NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name[..end].to_owned()
}

/// Lowercase hex sha256 of the sorted ids, one per line. A sorted list, so the digest does
/// not depend on the order the calls finished in.
fn digest(sorted_ids: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for id in sorted_ids {
        hasher.update(id.as_bytes());
        hasher.update(b"\n");
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::{WireTiming, WireUsage};

    fn reported(id: &str, model: &str, input: i32, output: i32, cache: i32) -> CallUsage {
        CallUsage::from_wire(WireUsage {
            request_id: id,
            model_used: model,
            provider_used: "azure-openai",
            input_tokens: input,
            output_tokens: output,
            cache_read_input_tokens: cache,
            timing: Some(WireTiming {
                usage_reported: true,
                response_cache_hit: false,
            }),
        })
    }

    fn builder() -> UsageBuilder {
        UsageBuilder::new(Feature::ContextualRetrieval, "org-a", false).expect("builder")
    }

    #[test]
    fn exact_token_fields_are_copied_from_the_response_and_summed() {
        let mut builder = builder();
        builder
            .push(reported("r1", "gpt-4o-mini-2024-07-18", 612, 48, 512))
            .push(reported("r2", "gpt-4o-mini-2024-07-18", 700, 52, 0))
            .push(reported("r3", "gpt-4o-mini-2024-07-18", 100, 9, 64));
        let events = builder.build();

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.schema_version, 2);
        assert_eq!(event.event_type, "llm");
        assert_eq!(event.feature, "contextual-retrieval");
        assert_eq!(event.model_used, "gpt-4o-mini-2024-07-18");
        assert_eq!(event.provider_used, "azure-openai");
        assert_eq!(event.count, 3, "three provider calls");
        assert_eq!(event.input_tokens, 612 + 700 + 100);
        assert_eq!(event.output_tokens, 48 + 52 + 9);
        assert_eq!(
            event.cache_read_tokens,
            512 + 64,
            "cache reads are detail of the input"
        );
        assert_eq!(event.units, None);
        assert_eq!((event.cost_basis, event.estimated), ("metered", false));
        assert_eq!((event.org_id.as_str(), event.zdr), ("org-a", false));
    }

    #[test]
    fn a_response_served_from_the_cache_emits_nothing() {
        let cached = CallUsage::from_wire(WireUsage {
            request_id: "cached",
            model_used: "gpt-4o-mini",
            provider_used: "azure-openai",
            input_tokens: 612,
            output_tokens: 48,
            cache_read_input_tokens: 0,
            timing: Some(WireTiming {
                usage_reported: true,
                response_cache_hit: true,
            }),
        });
        let mut only_cached = builder();
        only_cached.push(cached.clone());
        assert!(only_cached.build().is_empty(), "no spend, no event");

        let mut mixed = builder();
        mixed
            .push(cached)
            .push(reported("paid", "gpt-4o-mini", 100, 10, 0));
        let events = mixed.build();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].count, 1,
            "the cached call is not one of the calls"
        );
        assert_eq!(events[0].input_tokens, 100);
    }

    #[test]
    fn no_calls_build_nothing() {
        assert!(builder().build().is_empty());
    }

    #[test]
    fn a_call_with_unreported_usage_is_counted_and_marks_the_event_estimated() {
        let unreported = CallUsage::from_wire(WireUsage {
            request_id: "u1",
            model_used: "gpt-4o-mini",
            provider_used: "azure-openai",
            input_tokens: 0,
            output_tokens: 0,
            cache_read_input_tokens: 0,
            timing: Some(WireTiming {
                usage_reported: false,
                response_cache_hit: false,
            }),
        });
        let mut builder = builder();
        builder
            .push(reported("r1", "gpt-4o-mini", 100, 10, 0))
            .push(unreported);
        let event = &builder.build()[0];
        assert_eq!(event.count, 2, "the call happened");
        assert_eq!(event.input_tokens, 100);
        assert_eq!((event.cost_basis, event.estimated), ("estimated", true));
    }

    #[test]
    fn an_embedding_event_is_estimated_and_counts_texts() {
        let mut builder = UsageBuilder::new(Feature::Embedding, "org-a", false).unwrap();
        builder.push(CallUsage::estimated(
            "call-1",
            "embed-v-4-0",
            "azure-cohere",
            24,
            900,
        ));
        let event = &builder.build()[0];
        assert_eq!(event.event_type, "embedding");
        assert_eq!(event.feature, "embedding");
        assert_eq!(
            (event.count, event.input_tokens, event.output_tokens),
            (24, 900, 0)
        );
        assert_eq!((event.cost_basis, event.estimated), ("estimated", true));
    }

    #[test]
    fn billed_units_are_summed_and_absent_when_no_call_has_any() {
        let mut builder = UsageBuilder::new(Feature::WebRerank, "org-a", false).unwrap();
        builder
            .push(reported("a", "rerank-v4.0-pro", 0, 0, 0).with_units(3))
            .push(reported("b", "rerank-v4.0-pro", 0, 0, 0).with_units(2));
        assert_eq!(builder.build()[0].units, Some(5));

        let mut none = UsageBuilder::new(Feature::Rerank, "org-a", false).unwrap();
        none.push(reported("a", "rerank-v4.0-pro", 0, 0, 0));
        let event = &none.build()[0];
        assert_eq!(event.units, None);
        assert!(
            !String::from_utf8(event.payload().unwrap())
                .unwrap()
                .contains("units"),
            "an absent value is not written"
        );
    }

    #[test]
    fn the_key_does_not_depend_on_the_order_the_calls_finished_in() {
        let calls = [
            reported("c", "gpt-4o-mini", 1, 1, 0),
            reported("a", "gpt-4o-mini", 2, 2, 0),
            reported("b", "gpt-4o-mini", 3, 3, 0),
        ];
        let mut forward = builder();
        for call in calls.iter().cloned() {
            forward.push(call);
        }
        let mut backward = builder();
        for call in calls.iter().rev().cloned() {
            backward.push(call);
        }
        let (forward, backward) = (forward.build(), backward.build());
        assert_eq!(forward[0].idempotency_key, backward[0].idempotency_key);
        assert_eq!(forward[0].request_digest, backward[0].request_digest);
    }

    #[test]
    fn the_key_names_the_feature_and_a_digest_and_none_of_the_ids() {
        let mut builder = builder();
        builder.push(reported(
            "secret-looking-request-id",
            "gpt-4o-mini",
            1,
            1,
            0,
        ));
        let event = &builder.build()[0];
        assert_eq!(
            event.idempotency_key,
            format!("usage:contextual-retrieval:{}", event.request_digest)
        );
        assert_eq!(event.request_digest.len(), 64);
        assert!(event
            .request_digest
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
        assert!(!event.idempotency_key.contains("secret-looking-request-id"));
    }

    #[test]
    fn another_set_of_calls_or_another_feature_is_another_key() {
        let key = |feature: Feature, ids: &[&str]| {
            let mut builder = UsageBuilder::new(feature, "org-a", false).unwrap();
            for id in ids {
                builder.push(reported(id, "gpt-4o-mini", 1, 1, 0));
            }
            builder.build().remove(0).idempotency_key
        };
        let base = key(Feature::ContextualRetrieval, &["a", "b"]);
        assert_ne!(base, key(Feature::ContextualRetrieval, &["a", "c"]));
        assert_ne!(base, key(Feature::ContextualRetrieval, &["a"]));
        assert_ne!(base, key(Feature::GraphExtraction, &["a", "b"]));
        assert_eq!(base, key(Feature::ContextualRetrieval, &["b", "a"]));
    }

    #[test]
    fn a_request_pushed_twice_is_one_call() {
        let mut builder = builder();
        builder
            .push(reported("same", "gpt-4o-mini", 100, 10, 0))
            .push(reported("same", "gpt-4o-mini", 100, 10, 0));
        let event = &builder.build()[0];
        assert_eq!((event.count, event.input_tokens), (1, 100));
    }

    #[test]
    fn calls_without_an_id_are_not_merged_with_each_other() {
        let mut builder = builder();
        builder
            .push(reported("", "gpt-4o-mini", 100, 10, 0))
            .push(reported("  ", "gpt-4o-mini", 200, 20, 0));
        let event = &builder.build()[0];
        assert_eq!((event.count, event.input_tokens), (2, 300));
    }

    #[test]
    fn the_models_that_served_the_calls_get_an_event_each() {
        let mut builder = builder();
        builder
            .push(reported("a", "gpt-4o-mini", 100, 10, 0))
            .push(reported("b", "gpt-4o", 200, 20, 0))
            .push(reported("c", "gpt-4o-mini", 50, 5, 0));
        let events = builder.build();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].model_used, "gpt-4o");
        assert_eq!(events[1].model_used, "gpt-4o-mini");
        assert_eq!(events[1].input_tokens, 150);
        assert_ne!(events[0].idempotency_key, events[1].idempotency_key);
    }

    #[test]
    fn a_blank_model_or_provider_is_named_unknown_and_a_long_one_is_cut_on_a_boundary() {
        let mut builder = builder();
        builder.push(CallUsage::from_counts("a", "  ", "", 1, 1, 0, true, false));
        let event = &builder.build()[0];
        assert_eq!(
            (event.model_used.as_str(), event.provider_used.as_str()),
            ("unknown", "unknown")
        );

        let long = "å".repeat(400);
        let mut builder = self::builder();
        builder.push(CallUsage::from_counts(
            "a", &long, "p", 1, 1, 0, true, false,
        ));
        let event = &builder.build()[0];
        assert!(event.model_used.len() <= MAX_NAME_BYTES);
        assert!(event.model_used.chars().all(|c| c == 'å'));
    }

    #[test]
    fn a_counter_saturates_instead_of_wrapping() {
        let mut builder = builder();
        builder
            .push(CallUsage::from_counts(
                "a",
                "m",
                "p",
                u64::MAX,
                0,
                0,
                true,
                false,
            ))
            .push(CallUsage::from_counts(
                "b",
                "m",
                "p",
                u64::MAX,
                0,
                0,
                true,
                false,
            ));
        assert_eq!(builder.build()[0].input_tokens, MAX_COUNTER);

        // The item count has to fit the 32-bit column it is stored in.
        let mut items = UsageBuilder::new(Feature::Embedding, "org-a", false).unwrap();
        items.push(CallUsage::estimated("a", "m", "p", u64::MAX, 1));
        assert_eq!(items.build()[0].count, MAX_COUNT);
    }

    #[test]
    fn a_blank_or_padded_organization_is_refused() {
        for org in ["", "   ", " org-a", "org-a "] {
            assert!(
                matches!(
                    UsageBuilder::new(Feature::Embedding, org, false),
                    Err(LedgerError::InvalidOrganization)
                ),
                "{org:?}"
            );
        }
    }

    /// The payload has exactly the documented fields: no user, no document, no chunk and no
    /// text can be added to it by accident, because the type has nowhere to put them.
    #[test]
    fn the_payload_has_exactly_the_documented_fields() {
        let mut builder = builder();
        builder.push(reported("r1", "gpt-4o-mini", 612, 48, 512));
        let value: serde_json::Value =
            serde_json::from_slice(&builder.build()[0].payload().unwrap()).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "cache_read_tokens",
                "cost_basis",
                "count",
                "estimated",
                "event_type",
                "feature",
                "idempotency_key",
                "input_tokens",
                "model_used",
                "org_id",
                "output_tokens",
                "provider_used",
                "request_digest",
                "schema_version",
                "zdr",
            ]
        );
        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["input_tokens"], 612);
    }

    #[test]
    fn a_zero_retention_event_is_built_like_any_other_and_flagged() {
        let mut builder = UsageBuilder::new(Feature::GraphExtraction, "org-zdr", true).unwrap();
        builder.push(reported("z1", "gpt-4o-mini", 300, 30, 0));
        let event = &builder.build()[0];
        assert!(event.zdr);
        assert_eq!((event.input_tokens, event.output_tokens), (300, 30));
        assert!(!String::from_utf8(event.payload().unwrap())
            .unwrap()
            .contains("user_id"));
    }

    #[test]
    fn the_message_id_names_the_organization_and_the_key() {
        let mut builder = builder();
        builder.push(reported("r1", "gpt-4o-mini", 1, 1, 0));
        let event = &builder.build()[0];
        assert_eq!(
            event.message_id(),
            format!("org-a:{}", event.idempotency_key)
        );
    }
}
