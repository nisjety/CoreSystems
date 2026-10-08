//! ChatGPT-subscription refusals, as inference-core types them, for the
//! governed agent loop.
//!
//! A run pinned to the user's `ChatGPT` subscription (`agent_route`) can fail
//! because the plan's Codex usage limit is spent, the sign-in must be renewed,
//! or `ChatGPT` is briefly unavailable. inference-core is the only layer that
//! reads the subscription broker's reply, so it classifies a refusal once and
//! declares it on the failed `Infer` status's trailers: the reason on
//! `x-mp-provider-error`, and `x-mp-retry-after-ms` and `x-mp-resets-at` where
//! they are known. This module reads those and nothing else. The status
//! message is never parsed and never logged for a pinned run.
//!
//! It is a copy of model-gateway's `subscription_failure` reader, not an
//! import: the two services are deployed separately and neither depends on
//! the other, so each spells the trailer keys and reason codes as its own
//! literals. `tests/subscription_refusal_contract.rs` pins this copy to
//! inference-core's constants, and model-gateway's
//! `subscription_failure_contract` pins it to the gateway's. A silent
//! disagreement would fail nothing: the run would simply stop being typed and
//! the user would again be told "I wasn't able to complete that request".
//!
//! # What the loop does with a refusal
//!
//! * Any failed round of a pinned run is a typed [`Refusal`]: the reason
//!   inference-core declared, or [`SUBSCRIPTION_ROUTE_UNAVAILABLE`] when it
//!   declared none (an untyped 403 scope mismatch, a broker that did not
//!   answer, an older inference-core). A pinned run is never answered by
//!   another model, so there is no other outcome to report.
//! * A usage limit or an expired sign-in ([`Refusal::latches`]) is true of the
//!   whole run, not of one round: the next round of any loop of the run would
//!   spend a Codex process spawn to be refused again. The first one is kept in
//!   a [`RefusalLatch`] shared by the run's loops, so siblings of a wide
//!   research fan-out and the parent stop calling the route and the root run
//!   finalizes at once.
//! * The terminal receipt carries a code from Session Core's fixed vocabulary
//!   ([`Refusal::terminal_failure_code`]) and the response carries the typed
//!   code, so the gateway can word the reason for the user. execution-core
//!   appends no assistant message for it: the gateway persists the one it
//!   words, so the thread carries exactly one.
//!
//! Everything here is content-free: a code, a number and a timestamp.

use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Utc};

/// The connected plan's Codex usage limit is exhausted.
pub(crate) const SUBSCRIPTION_USAGE_LIMIT: &str = "subscription_usage_limit";
/// The `ChatGPT` sign-in must be renewed.
pub(crate) const SUBSCRIPTION_REAUTH_REQUIRED: &str = "subscription_reauth_required";
/// `ChatGPT` or Codex failed transiently, after Codex's own retries.
pub(crate) const SUBSCRIPTION_UPSTREAM_UNAVAILABLE: &str = "subscription_upstream_unavailable";
/// `ChatGPT` rejected the request itself.
pub(crate) const SUBSCRIPTION_BAD_REQUEST: &str = "subscription_bad_request";
/// A short-term throttle. Honoured here because a pinned run is on the
/// subscription route by definition.
pub(crate) const RATE_LIMITED: &str = "rate_limited";
/// The code for a subscription failure with no typed reason, and for a round
/// that was served by another provider or model than the run is pinned to.
pub(crate) const SUBSCRIPTION_ROUTE_UNAVAILABLE: &str = "subscription_route_unavailable";
/// The code for a prompt still too long after compaction.
pub(crate) const PROMPT_TOO_LONG: &str = "prompt_too_long";

/// Trailer carrying inference-core's classification of a provider failure.
pub(crate) const PROVIDER_ERROR_KIND_TRAILER: &str = "x-mp-provider-error";
/// [`PROVIDER_ERROR_KIND_TRAILER`] value for a prompt over the input limit.
pub(crate) const PROVIDER_ERROR_TOO_LONG: &str = "too_long";
/// Trailer with the wait before a retry could succeed, in whole milliseconds.
pub(crate) const RETRY_AFTER_MS_TRAILER: &str = "x-mp-retry-after-ms";
/// Trailer with a usage limit's reset time, RFC 3339 UTC to whole seconds.
pub(crate) const RESETS_AT_TRAILER: &str = "x-mp-resets-at";

/// The reason codes inference-core may declare for a subscription refusal.
const TRAILER_CODES: [&str; 5] = [
    SUBSCRIPTION_USAGE_LIMIT,
    SUBSCRIPTION_REAUTH_REQUIRED,
    SUBSCRIPTION_UPSTREAM_UNAVAILABLE,
    SUBSCRIPTION_BAD_REQUEST,
    RATE_LIMITED,
];

/// Session Core's failure vocabulary entry for a provider that refused or could
/// not answer. The code itself reaches the user through the response, because
/// Session Core's durable contract accepts only its own fixed list.
const TERMINAL_PROVIDER_UNAVAILABLE: &str = "provider_unavailable";
/// Session Core's entry for a request the provider rejected as it stands: a
/// retry of the same request cannot succeed.
const TERMINAL_INFERENCE_FAILED: &str = "inference_failed";

/// A typed refusal: its code and the content-free timing that came with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(crate) code: &'static str,
    pub(crate) retry_after_ms: Option<u64>,
    pub(crate) resets_at: Option<DateTime<Utc>>,
}

impl Refusal {
    /// A refusal known only by its code.
    pub(crate) const fn bare(code: &'static str) -> Self {
        Self {
            code,
            retry_after_ms: None,
            resets_at: None,
        }
    }

    /// The reset time as the wire carries it: RFC 3339 UTC, whole seconds.
    pub(crate) fn resets_at_label(&self) -> Option<String> {
        self.resets_at
            .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    }

    /// Whether the refusal is true of the whole run rather than of one round.
    ///
    /// A usage limit stays spent until its reset and a sign-in stays expired
    /// until the user renews it, so no later round of any loop of the run can
    /// succeed. A throttle, a transient upstream failure, a rejected request
    /// and an overlong prompt are about the round that met them.
    pub(crate) fn latches(&self) -> bool {
        matches!(
            self.code,
            SUBSCRIPTION_USAGE_LIMIT | SUBSCRIPTION_REAUTH_REQUIRED
        )
    }

    /// The failure code the run's terminal receipt records, from Session
    /// Core's fixed vocabulary. The same mapping model-gateway applies to a
    /// direct subscription turn: a provider that refused or could not answer
    /// is `provider_unavailable`; a request `ChatGPT` rejected, or a prompt too
    /// long for it, is the request's own failure.
    pub(crate) fn terminal_failure_code(&self) -> &'static str {
        match self.code {
            SUBSCRIPTION_BAD_REQUEST | PROMPT_TOO_LONG => TERMINAL_INFERENCE_FAILED,
            _ => TERMINAL_PROVIDER_UNAVAILABLE,
        }
    }
}

/// The typed refusal a failed inference call declares, if any.
///
/// Reads the reason trailer against the exact five codes, and `too_long`
/// (the overflow inference-core classifies from the raw provider body) as
/// [`PROMPT_TOO_LONG`]; an unknown value is no typed refusal. A timing
/// trailer that does not parse is dropped on its own, so a bad number or date
/// never costs the reason. The status message is never read.
pub(crate) fn refusal_from_status(status: &tonic::Status) -> Option<Refusal> {
    let metadata = status.metadata();
    let text = |key: &str| {
        metadata
            .get(key)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
    };
    let kind = text(PROVIDER_ERROR_KIND_TRAILER)?;
    if kind.eq_ignore_ascii_case(PROVIDER_ERROR_TOO_LONG) {
        return Some(Refusal::bare(PROMPT_TOO_LONG));
    }
    let code = TRAILER_CODES.into_iter().find(|code| *code == kind)?;
    let retry_after_ms = text(RETRY_AFTER_MS_TRAILER).and_then(|value| value.parse::<u64>().ok());
    let resets_at = text(RESETS_AT_TRAILER)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|at| at.with_timezone(&Utc));
    Some(Refusal {
        code,
        retry_after_ms,
        resets_at,
    })
}

/// The refusal a failed inference round of a PINNED run ends with: the typed
/// one, or the generic [`SUBSCRIPTION_ROUTE_UNAVAILABLE`] when inference-core
/// declared none. Only called for a run pinned to the subscription route.
pub(crate) fn refusal_for_failed_round(status: &tonic::Status) -> Refusal {
    refusal_from_status(status).unwrap_or(Refusal::bare(SUBSCRIPTION_ROUTE_UNAVAILABLE))
}

/// The first run-wide refusal any loop of a run met, shared by all of them.
///
/// Cloning shares the slot, like `agent_route::ModelRoute`'s stop flag: the
/// root loop and every delegated child hold the same one, so the first usage
/// limit or expired sign-in stops them all. First one wins, so the code the
/// run reports is the one that actually stopped it.
#[derive(Clone, Debug, Default)]
pub(crate) struct RefusalLatch {
    slot: Arc<Mutex<Option<Refusal>>>,
}

impl RefusalLatch {
    /// Latch `refusal` unless one is already latched, and return the one that
    /// is latched afterwards.
    pub(crate) fn latch(&self, refusal: Refusal) -> Refusal {
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert(refusal)
            .clone()
    }

    /// The latched refusal, if any loop of the run met one.
    pub(crate) fn latched(&self) -> Option<Refusal> {
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(trailers: &[(&'static str, &str)]) -> tonic::Status {
        let mut metadata = tonic::metadata::MetadataMap::new();
        for (key, value) in trailers {
            metadata.insert(*key, value.parse().unwrap());
        }
        tonic::Status::with_metadata(tonic::Code::Unavailable, "subscription refused", metadata)
    }

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn reads_each_of_the_five_codes_and_nothing_else() {
        for code in TRAILER_CODES {
            let refusal = refusal_from_status(&status(&[("x-mp-provider-error", code)]))
                .unwrap_or_else(|| panic!("{code} is typed"));
            assert_eq!(refusal, Refusal::bare(code));
        }
        for other in [
            "subscription_broker_failed",
            "SUBSCRIPTION_USAGE_LIMIT",
            "subscription_route_unavailable",
            "prompt_too_long",
            "",
        ] {
            assert_eq!(
                refusal_from_status(&status(&[("x-mp-provider-error", other)])),
                None,
                "{other:?}"
            );
        }
        assert_eq!(
            refusal_from_status(&tonic::Status::unavailable("subscription_usage_limit")),
            None,
            "the message is never read"
        );
    }

    #[test]
    fn too_long_is_prompt_too_long() {
        for spelling in ["too_long", "TOO_LONG", " too_long "] {
            assert_eq!(
                refusal_from_status(&status(&[("x-mp-provider-error", spelling)])),
                Some(Refusal::bare(PROMPT_TOO_LONG)),
                "{spelling:?}"
            );
        }
    }

    #[test]
    fn reads_the_timing_and_drops_only_a_bad_field() {
        let refusal = refusal_from_status(&status(&[
            ("x-mp-provider-error", "subscription_usage_limit"),
            ("x-mp-retry-after-ms", "52260000"),
            ("x-mp-resets-at", "2026-09-26T01:21:00Z"),
        ]))
        .unwrap();
        assert_eq!(refusal.retry_after_ms, Some(52_260_000));
        assert_eq!(refusal.resets_at, Some(at("2026-09-26T01:21:00Z")));
        assert_eq!(
            refusal.resets_at_label().as_deref(),
            Some("2026-09-26T01:21:00Z")
        );

        let refusal = refusal_from_status(&status(&[
            ("x-mp-provider-error", "subscription_usage_limit"),
            ("x-mp-retry-after-ms", "-5"),
            ("x-mp-resets-at", "tomorrow"),
        ]))
        .unwrap();
        assert_eq!(refusal, Refusal::bare(SUBSCRIPTION_USAGE_LIMIT));
    }

    #[test]
    fn a_reset_with_an_offset_is_normalised_to_utc_whole_seconds() {
        let refusal = refusal_from_status(&status(&[
            ("x-mp-provider-error", "subscription_usage_limit"),
            ("x-mp-resets-at", "2026-09-26T03:21:00.700+02:00"),
        ]))
        .unwrap();
        assert_eq!(
            refusal.resets_at_label().as_deref(),
            Some("2026-09-26T01:21:00Z")
        );
    }

    #[test]
    fn a_failed_round_with_no_typed_reason_is_the_generic_route_failure() {
        for failed in [
            tonic::Status::unavailable("all providers exhausted"),
            tonic::Status::permission_denied("scope mismatch"),
            tonic::Status::unauthenticated("expired"),
            status(&[("x-mp-provider-error", "something_new")]),
        ] {
            assert_eq!(
                refusal_for_failed_round(&failed),
                Refusal::bare(SUBSCRIPTION_ROUTE_UNAVAILABLE)
            );
        }
        let usage = status(&[("x-mp-provider-error", "subscription_usage_limit")]);
        assert_eq!(refusal_for_failed_round(&usage).code, SUBSCRIPTION_USAGE_LIMIT);
    }

    #[test]
    fn only_a_usage_limit_and_an_expired_sign_in_are_true_of_the_whole_run() {
        for (code, latches) in [
            (SUBSCRIPTION_USAGE_LIMIT, true),
            (SUBSCRIPTION_REAUTH_REQUIRED, true),
            (SUBSCRIPTION_UPSTREAM_UNAVAILABLE, false),
            (SUBSCRIPTION_BAD_REQUEST, false),
            (RATE_LIMITED, false),
            (SUBSCRIPTION_ROUTE_UNAVAILABLE, false),
            (PROMPT_TOO_LONG, false),
        ] {
            assert_eq!(Refusal::bare(code).latches(), latches, "{code}");
        }
    }

    #[test]
    fn the_terminal_code_is_in_session_cores_vocabulary_and_matches_the_gateways() {
        for (code, terminal) in [
            (SUBSCRIPTION_USAGE_LIMIT, "provider_unavailable"),
            (SUBSCRIPTION_REAUTH_REQUIRED, "provider_unavailable"),
            (SUBSCRIPTION_UPSTREAM_UNAVAILABLE, "provider_unavailable"),
            (RATE_LIMITED, "provider_unavailable"),
            (SUBSCRIPTION_ROUTE_UNAVAILABLE, "provider_unavailable"),
            (SUBSCRIPTION_BAD_REQUEST, "inference_failed"),
            (PROMPT_TOO_LONG, "inference_failed"),
        ] {
            assert_eq!(Refusal::bare(code).terminal_failure_code(), terminal, "{code}");
        }
    }

    #[test]
    fn the_route_failure_code_is_the_one_the_three_services_share() {
        assert_eq!(
            SUBSCRIPTION_ROUTE_UNAVAILABLE,
            mp_contracts::agentic_subscription::ROUTE_UNAVAILABLE_CODE
        );
    }

    #[test]
    fn the_latch_keeps_the_first_refusal_and_is_shared_by_every_clone() {
        let latch = RefusalLatch::default();
        let child = latch.clone();
        assert_eq!(latch.latched(), None);

        let first = Refusal {
            resets_at: Some(at("2026-10-03T17:45:00Z")),
            ..Refusal::bare(SUBSCRIPTION_USAGE_LIMIT)
        };
        assert_eq!(child.latch(first.clone()), first);
        assert_eq!(latch.latched(), Some(first.clone()), "a child's latch reaches the parent");

        // A second one does not replace it, and the caller learns which won.
        let later = Refusal::bare(SUBSCRIPTION_REAUTH_REQUIRED);
        assert_eq!(latch.latch(later), first);
        assert_eq!(child.latched(), Some(first));

        // A different run's latch is independent.
        assert_eq!(RefusalLatch::default().latched(), None);
    }
}
