use std::collections::{BTreeMap, BTreeSet};

use inference_client_rs::{
    Failure, InferenceError, MintError, KIND_TRAILER, PENDING_RETRY_AFTER, RETRY_AFTER_MS_TRAILER,
    SKIP_RECHECK_AFTER,
};
use serde_json::Value;
use tonic::metadata::{MetadataKey, MetadataMap, MetadataValue};

use super::*;
use crate::context::testing::{
    migration_sql, quoted_set_after, read_pinned, require_contract, STATUS_GOLDEN,
};

const MS: fn(u64) -> Duration = Duration::from_millis;

/// A status as it arrives from inference-core: the code, and the trailers it sets.
/// The message says something that points the wrong way on every case on purpose: it
/// must not matter.
fn received(code_number: i64, trailers: &serde_json::Map<String, Value>) -> tonic::Status {
    let mut metadata = MetadataMap::new();
    for (key, value) in trailers {
        metadata.insert(
            MetadataKey::from_bytes(key.as_bytes()).expect("a trailer key"),
            MetadataValue::try_from(value.as_str().expect("a trailer value")).expect("ascii"),
        );
    }
    tonic::Status::with_metadata(
        tonic::Code::from_i32(i32::try_from(code_number).expect("a code")),
        "service unavailable, please retry; this is not a zero data retention refusal",
        metadata,
    )
}

fn call_failure(
    code: tonic::Code,
    kind: Option<&'static str>,
    retry_ms: Option<&'static str>,
) -> InferenceError {
    let mut metadata = MetadataMap::new();
    if let Some(kind) = kind {
        metadata.insert(KIND_TRAILER, MetadataValue::from_static(kind));
    }
    if let Some(retry_ms) = retry_ms {
        metadata.insert(RETRY_AFTER_MS_TRAILER, MetadataValue::from_static(retry_ms));
    }
    InferenceError::from_status(&tonic::Status::with_metadata(code, "", metadata))
}

fn transient(class: ErrorClass, retry_after: Option<Duration>) -> ContextOutcome {
    ContextOutcome::transient(class, retry_after)
}

/// The decision for every way a call to inference-core can fail, keyed by the id of
/// the case in the pinned contract inference-core generates. A case the contract
/// gains and nobody decides about fails the completeness check below, so a new way to
/// fail cannot arrive as an unconsidered default.
fn decisions() -> BTreeMap<&'static str, ContextOutcome> {
    use ErrorClass::*;
    let wait = Some(MAX_RETRY_AFTER);
    BTreeMap::from([
        ("provider_http", transient(ProviderUnavailable, None)),
        ("provider_unavailable", transient(ProviderUnavailable, None)),
        (
            "provider_subscription_failed",
            transient(ProviderUnavailable, None),
        ),
        (
            "provider_all_exhausted",
            transient(ProviderUnavailable, None),
        ),
        // The provider answered something that is not an answer; or the request is
        // one this client cannot make succeed. Not worth the document again.
        (
            "provider_invalid_response",
            ContextOutcome::permanent(Internal),
        ),
        (
            "provider_unsupported_model",
            ContextOutcome::permanent(Internal),
        ),
        // `too_long` rides UNAVAILABLE so a fallback chain can try a larger model;
        // by code alone it would be retried until the budget ran out.
        ("provider_too_long", ContextOutcome::permanent(TooLong)),
        (
            "provider_rate_limited",
            transient(RateLimited, Some(MS(1_500))),
        ),
        // The refusals are the answer. No provider was called.
        (
            "provider_zdr_unavailable",
            ContextOutcome::skipped_zdr(ZdrReason::NoAttestedRoute),
        ),
        (
            "provider_residency_violation",
            ContextOutcome::ResidencyUnavailable,
        ),
        (
            "modality_refused_under_zdr",
            ContextOutcome::skipped_zdr(ZdrReason::NoAttestedRoute),
        ),
        (
            "embedding_zdr_unavailable",
            ContextOutcome::skipped_zdr(ZdrReason::NoAttestedRoute),
        ),
        (
            "embedding_residency_unavailable",
            ContextOutcome::ResidencyUnavailable,
        ),
        // This client never asks for a tier: a server that refuses on one changed
        // its contract. Wait, look again, and say it is not the unit's fault.
        (
            "provider_tier_unavailable",
            transient(Internal, Some(SKIP_RECHECK_AFTER)),
        ),
        (
            "embedding_tier_unavailable",
            transient(Internal, Some(SKIP_RECHECK_AFTER)),
        ),
        ("embedding_provider_http", transient(Internal, None)),
        // The subscription route is never selected by the Data Plane. A usage limit
        // is still a throttle (and its wait is capped), the other kinds are not
        // learned and read as what their code says.
        (
            "subscription_usage_limit_with_timing",
            transient(RateLimited, wait),
        ),
        (
            "subscription_usage_limit_without_timing",
            transient(RateLimited, None),
        ),
        (
            "subscription_reauth_required_with_timing",
            transient(ProviderUnavailable, wait),
        ),
        (
            "subscription_reauth_required_without_timing",
            transient(ProviderUnavailable, None),
        ),
        (
            "subscription_upstream_unavailable_with_timing",
            transient(ProviderUnavailable, wait),
        ),
        (
            "subscription_upstream_unavailable_without_timing",
            transient(ProviderUnavailable, None),
        ),
        (
            "subscription_bad_request_with_timing",
            transient(ProviderUnavailable, wait),
        ),
        (
            "subscription_bad_request_without_timing",
            transient(ProviderUnavailable, None),
        ),
    ])
}

/// The decision table, driven from the pinned contract: what inference-core really
/// puts on the wire, read through the real decoder.
#[test]
fn every_status_inference_core_can_emit_has_a_decided_outcome() {
    let golden = read_pinned(STATUS_GOLDEN);
    let cases = golden["cases"].as_array().expect("cases");
    let decisions = decisions();

    let in_golden: BTreeSet<&str> = cases
        .iter()
        .map(|case| case["id"].as_str().expect("id"))
        .collect();
    let decided: BTreeSet<&str> = decisions.keys().copied().collect();
    assert_eq!(
        in_golden,
        decided,
        "the contract and this table must name the same cases: a new way for a call to fail \
         needs a decision here (in the golden only: {:?}; decided only: {:?})",
        in_golden.difference(&decided).collect::<Vec<_>>(),
        decided.difference(&in_golden).collect::<Vec<_>>(),
    );

    for case in cases {
        let id = case["id"].as_str().expect("id");
        let status = received(
            case["grpc_code_number"].as_i64().expect("code number"),
            case["trailers"].as_object().expect("trailers"),
        );
        let error = InferenceError::from_status(&status);
        assert_eq!(outcome_for_failure(&error), decisions[id], "{id}");
    }
}

/// What every outcome the table can produce says to the database: the CHECK
/// constraints require a class on every failed row and every zero-retention skip.
#[test]
fn every_decided_outcome_satisfies_the_checks_of_its_status() {
    let mut outcomes: Vec<ContextOutcome> = decisions().into_values().collect();
    outcomes.extend([
        ContextOutcome::skipped_zdr(ZdrReason::Org),
        ContextOutcome::skipped_zdr(ZdrReason::Document),
        ContextOutcome::pending(PendingReason::DeadlineExceeded, Duration::ZERO),
        ContextOutcome::pending(PendingReason::PostureUnavailable, PENDING_RETRY_AFTER),
        ContextOutcome::pending(PendingReason::BudgetThrottled, Duration::ZERO),
        ContextOutcome::SkippedPolicy,
        ContextOutcome::SkippedBudget,
    ]);
    for outcome in &outcomes {
        match outcome.status() {
            Status::FailedTransient | Status::FailedPermanent | Status::SkippedZdr => {
                assert!(outcome.error_class().is_some(), "{outcome:?} needs a class");
            }
            Status::Pending | Status::Ready | Status::SkippedPolicy | Status::SkippedBudget => {}
        }
        // A class is only ever a member of the closed set.
        if let Some(class) = outcome.error_class() {
            assert!(ErrorClass::ALL.contains(&class));
        }
        // What is waiting for an attempt says when; what is final does not.
        match outcome {
            ContextOutcome::Permanent { .. } => assert_eq!(outcome.retry_after(), None),
            ContextOutcome::Pending { retry_after, .. } => {
                assert_eq!(outcome.retry_after(), Some(*retry_after));
            }
            _ => {}
        }
        // Nothing is ever retried later than the cap.
        assert!(outcome
            .retry_after()
            .is_none_or(|after| after <= SKIP_RECHECK_AFTER));
    }
}

#[test]
fn a_provider_wait_is_kept_and_capped() {
    // A usage limit that resets in fourteen hours is not scheduled that far out: a
    // unit sitting for a day is outside every backlog query.
    let capped = transient(ErrorClass::RateLimited, Some(Duration::from_secs(52_260)));
    assert_eq!(capped.retry_after(), Some(MAX_RETRY_AFTER));
    let exact = transient(ErrorClass::RateLimited, Some(MS(1_500)));
    assert_eq!(exact.retry_after(), Some(MS(1_500)));
    assert_eq!(transient(ErrorClass::Internal, None).retry_after(), None);
}

#[test]
fn a_local_timeout_is_told_from_a_provider_outage_by_its_code() {
    for code in [tonic::Code::DeadlineExceeded, tonic::Code::Cancelled] {
        assert_eq!(
            outcome_for_failure(&call_failure(code, None, None)),
            transient(ErrorClass::Timeout, None),
            "{code:?}"
        );
    }
    for code in [tonic::Code::Unavailable, tonic::Code::Aborted] {
        assert_eq!(
            outcome_for_failure(&call_failure(code, None, None)),
            transient(ErrorClass::ProviderUnavailable, None),
            "{code:?}"
        );
    }
}

/// The mint is Auth Core's answer, not the provider's. Whatever it is, nothing was
/// sent, and none of it is the unit's fault.
#[test]
fn a_refused_or_unreachable_mint_never_sends_never_skips_and_never_counts_against_the_unit() {
    let waits = [
        MintError::PostureUnavailable,
        MintError::DirectoryUnavailable,
        MintError::Unavailable("connection refused".into()),
        MintError::ServerError { status: 502 },
        MintError::ServerError { status: 429 },
        MintError::Malformed("not json"),
    ];
    for error in waits {
        let outcome = outcome_for_failure(&InferenceError::from(error.clone()));
        assert_eq!(
            outcome,
            ContextOutcome::pending(PendingReason::PostureUnavailable, PENDING_RETRY_AFTER),
            "{error:?}"
        );
        assert_eq!(outcome.status(), Status::Pending);
        assert_eq!(outcome.error_class(), Some(ErrorClass::PostureUnavailable));
        assert_eq!(outcome.retry_after(), Some(Duration::from_secs(300)));
        assert!(!outcome.counts_as_attempt());
        assert!(outcome.is_systemic());
    }
    let deployment_problems = [
        MintError::Refused { status: 403 },
        MintError::Refused { status: 401 },
        MintError::Rejected { status: 400 },
        MintError::InvalidOrg,
        MintError::CredentialUnavailable,
        MintError::Contract("token audience is not inference-core"),
        MintError::PostureMismatch {
            observed: "zdr",
            expected: "persistent",
        },
    ];
    for error in deployment_problems {
        let outcome = outcome_for_failure(&InferenceError::from(error.clone()));
        assert_eq!(
            outcome,
            transient(ErrorClass::Auth, Some(PENDING_RETRY_AFTER)),
            "{error:?}"
        );
        assert!(!outcome.counts_as_attempt(), "{error:?}");
        assert!(outcome.is_systemic(), "{error:?}");
    }
}

#[test]
fn what_counts_against_a_unit_and_what_does_not() {
    let counted = [
        transient(ErrorClass::RateLimited, None),
        transient(ErrorClass::ProviderUnavailable, None),
        transient(ErrorClass::Timeout, None),
        transient(ErrorClass::InvalidOutput, None),
        transient(ErrorClass::Internal, None),
        ContextOutcome::permanent(ErrorClass::TooLong),
        ContextOutcome::permanent(ErrorClass::ContentFilter),
    ];
    for outcome in counted {
        assert!(outcome.counts_as_attempt(), "{outcome:?}");
    }
    let not_counted = [
        transient(ErrorClass::Auth, Some(PENDING_RETRY_AFTER)),
        ContextOutcome::ResidencyUnavailable,
        ContextOutcome::skipped_zdr(ZdrReason::Org),
        ContextOutcome::skipped_zdr(ZdrReason::Document),
        ContextOutcome::skipped_zdr(ZdrReason::NoAttestedRoute),
        ContextOutcome::pending(PendingReason::DeadlineExceeded, Duration::ZERO),
        ContextOutcome::pending(PendingReason::PostureUnavailable, PENDING_RETRY_AFTER),
        ContextOutcome::pending(PendingReason::BudgetThrottled, Duration::ZERO),
        ContextOutcome::SkippedPolicy,
        ContextOutcome::SkippedBudget,
    ];
    for outcome in not_counted {
        assert!(!outcome.counts_as_attempt(), "{outcome:?}");
    }
}

/// A restricted document and a zero-retention organization are different reasons and
/// stay distinguishable in the coverage view; only the organization's is rechecked.
#[test]
fn the_three_zero_retention_skips_are_distinct_and_only_the_documents_is_final() {
    let org = ContextOutcome::skipped_zdr(ZdrReason::Org);
    let document = ContextOutcome::skipped_zdr(ZdrReason::Document);
    let route = ContextOutcome::skipped_zdr(ZdrReason::NoAttestedRoute);
    assert_eq!(org.error_class(), Some(ErrorClass::ZdrOrg));
    assert_eq!(document.error_class(), Some(ErrorClass::ZdrDocument));
    assert_eq!(route.error_class(), Some(ErrorClass::ZdrUnavailable));
    for outcome in [&org, &document, &route] {
        assert_eq!(outcome.status(), Status::SkippedZdr);
    }
    assert_eq!(org.retry_after(), Some(SKIP_RECHECK_AFTER));
    assert_eq!(route.retry_after(), Some(SKIP_RECHECK_AFTER));
    assert_eq!(document.retry_after(), None);
    // What is true of the organization is true of every chunk; what is true of one
    // document is not a statement about the deployment.
    assert!(org.is_systemic() && route.is_systemic());
    assert!(!document.is_systemic());
}

#[test]
fn only_what_is_the_same_answer_for_every_chunk_is_systemic() {
    let systemic = [
        ContextOutcome::skipped_zdr(ZdrReason::Org),
        ContextOutcome::skipped_zdr(ZdrReason::NoAttestedRoute),
        ContextOutcome::ResidencyUnavailable,
        ContextOutcome::pending(PendingReason::PostureUnavailable, PENDING_RETRY_AFTER),
        transient(ErrorClass::Auth, Some(PENDING_RETRY_AFTER)),
    ];
    for outcome in systemic {
        assert!(outcome.is_systemic(), "{outcome:?}");
    }
    let per_chunk = [
        transient(ErrorClass::RateLimited, None),
        transient(ErrorClass::InvalidOutput, None),
        transient(ErrorClass::Timeout, None),
        ContextOutcome::permanent(ErrorClass::ContentFilter),
        ContextOutcome::permanent(ErrorClass::TooLong),
        ContextOutcome::pending(PendingReason::DeadlineExceeded, Duration::ZERO),
        ContextOutcome::pending(PendingReason::BudgetThrottled, Duration::ZERO),
        ContextOutcome::skipped_zdr(ZdrReason::Document),
    ];
    for outcome in per_chunk {
        assert!(!outcome.is_systemic(), "{outcome:?}");
    }
}

/// The two outcomes the budget gate (CR-T9b) records. A throttled chunk is a deferral: it is
/// `pending`, due at once, carries no error class (nothing went wrong, and the class set is the
/// database's own closed vocabulary) and never counts against the unit. A stopped chunk is
/// `skipped_budget`: final until the gate lets the organization through again, with no class and
/// no time of its own, exactly as `skipped_policy` is. Neither is the provider's fault, so
/// neither is systemic.
#[test]
fn the_budget_gate_defers_a_throttled_chunk_and_marks_a_stopped_one() {
    let throttled = ContextOutcome::pending(PendingReason::BudgetThrottled, Duration::ZERO);
    assert_eq!(throttled.status(), Status::Pending);
    assert_eq!(throttled.error_class(), None);
    assert_eq!(throttled.retry_after(), Some(Duration::ZERO), "due at once");
    assert!(!throttled.counts_as_attempt());
    assert!(!throttled.is_systemic());

    let stopped = ContextOutcome::SkippedBudget;
    assert_eq!(stopped.status(), Status::SkippedBudget);
    assert_eq!(stopped.status().as_str(), "skipped_budget");
    assert_eq!(stopped.error_class(), None);
    assert_eq!(
        stopped.retry_after(),
        None,
        "the gate, not a clock, lifts it"
    );
    assert!(!stopped.counts_as_attempt());
    assert!(!stopped.is_systemic());
}

/// A provider's own wish is waited for exactly: the retry-after test the design asks
/// for. A ladder step is only the fallback for a provider that named none.
#[test]
fn a_retry_waits_exactly_what_the_provider_said() {
    let base = MS(2_000);
    for attempt in 0..4 {
        for chunk in 0..8 {
            let throttled = call_failure(
                tonic::Code::ResourceExhausted,
                Some("rate_limited"),
                Some("1500"),
            );
            assert_eq!(
                inline_retry_wait(&throttled, base, attempt, chunk),
                Some(MS(1_500)),
                "attempt {attempt}, chunk {chunk}: the wish, not the ladder"
            );
        }
    }
    // A wish past the cap is waited for at most the cap.
    let long = call_failure(
        tonic::Code::ResourceExhausted,
        Some("rate_limited"),
        Some("52260000"),
    );
    assert_eq!(inline_retry_wait(&long, base, 0, 0), Some(MAX_RETRY_AFTER));
    // An unavailable service that names a wait is waited for too.
    let unavailable = call_failure(tonic::Code::Unavailable, None, Some("2500"));
    assert_eq!(inline_retry_wait(&unavailable, base, 3, 3), Some(MS(2_500)));
}

#[test]
fn without_a_provider_wish_the_ladder_applies() {
    let base = MS(2_000);
    let throttled = call_failure(tonic::Code::ResourceExhausted, Some("rate_limited"), None);
    assert_eq!(
        inline_retry_wait(&throttled, base, 0, 0),
        Some(ladder_backoff(base, 0, 0))
    );
    assert_eq!(
        inline_retry_wait(&throttled, base, 2, 3),
        Some(ladder_backoff(base, 2, 3))
    );
    let internal = call_failure(tonic::Code::Internal, None, None);
    assert_eq!(
        inline_retry_wait(&internal, base, 1, 1),
        Some(ladder_backoff(base, 1, 1))
    );
}

#[test]
fn a_refusal_a_bad_request_and_a_mint_are_never_retried_inline() {
    let base = MS(2_000);
    for error in [
        call_failure(
            tonic::Code::FailedPrecondition,
            Some("zdr_unavailable"),
            None,
        ),
        call_failure(
            tonic::Code::FailedPrecondition,
            Some("residency_unavailable"),
            None,
        ),
        call_failure(
            tonic::Code::FailedPrecondition,
            Some("tier_unavailable"),
            None,
        ),
        call_failure(tonic::Code::Unavailable, Some("too_long"), None),
        call_failure(tonic::Code::InvalidArgument, None, None),
        call_failure(tonic::Code::Unauthenticated, None, None),
        call_failure(tonic::Code::PermissionDenied, None, None),
        call_failure(tonic::Code::NotFound, None, None),
        call_failure(tonic::Code::FailedPrecondition, None, None),
        InferenceError::from(MintError::PostureUnavailable),
        InferenceError::from(MintError::Unavailable("down".into())),
        InferenceError::from(MintError::Refused { status: 403 }),
    ] {
        assert_eq!(inline_retry_wait(&error, base, 0, 0), None, "{error}");
    }
}

#[test]
fn the_ladder_doubles_and_spans_the_providers_retry_window() {
    let base = MS(2_000);
    // Attempt n waits 2^n * base, so 4 attempts span 2+4+8+16 = 30s: the 30-second
    // Retry-After this Azure deployment has been seen to return.
    let total: Duration = (0..4).map(|attempt| ladder_backoff(base, attempt, 0)).sum();
    assert_eq!(total, MS(30_000));
    assert_eq!(ladder_backoff(base, 0, 0), MS(2_000));
    assert_eq!(ladder_backoff(base, 3, 0), MS(16_000));
}

#[test]
fn concurrent_chunks_do_not_retry_in_lockstep() {
    let base = MS(2_000);
    let delays: Vec<Duration> = (0..4).map(|chunk| ladder_backoff(base, 0, chunk)).collect();
    let distinct: BTreeSet<u128> = delays.iter().map(Duration::as_millis).collect();
    assert_eq!(distinct.len(), 4, "offsets collided: {delays:?}");
    for delay in &delays {
        assert!(
            *delay >= base && *delay < base + base / 4,
            "{delay:?} escaped its step"
        );
    }
}

#[test]
fn the_ladder_saturates_rather_than_overflowing() {
    for attempt in [6u32, 7, 32, u32::MAX] {
        assert!(ladder_backoff(Duration::from_secs(1), attempt, 1) >= Duration::from_secs(1));
    }
}

/// The two closed sets are the database's: the CHECK constraints of the migration.
/// A class or a status added to one and not the other is caught here, before it is
/// a constraint violation on a write.
#[test]
fn the_vocabulary_is_the_migrations() {
    let Some(sql) = migration_sql() else {
        assert!(
            !require_contract(),
            "the migration 20261003130000_context_provenance.sql is not reachable from this checkout"
        );
        eprintln!("skipped: the migration is not in this tree (the Docker build context holds services/ only)");
        return;
    };
    let statuses = quoted_set_after(
        &sql,
        "ADD CONSTRAINT knowledge_units_context_status_chk CHECK",
    );
    let mut ours: BTreeSet<String> = Status::ALL.iter().map(|s| s.as_str().to_owned()).collect();
    // `legacy` is a state the migration gives a row, never the result of generating.
    ours.insert("legacy".to_owned());
    assert_eq!(ours, statuses, "context_status");

    let classes = quoted_set_after(
        &sql,
        "ADD CONSTRAINT knowledge_units_context_error_class_chk CHECK",
    );
    let ours: BTreeSet<String> = ErrorClass::ALL
        .iter()
        .map(|c| c.as_str().to_owned())
        .collect();
    assert_eq!(ours, classes, "context_error_class");
}

#[test]
fn the_vocabulary_has_no_duplicates_and_round_trips_its_names() {
    let names: BTreeSet<&str> = ErrorClass::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(names.len(), ErrorClass::ALL.len());
    let statuses: BTreeSet<&str> = Status::ALL.iter().map(|s| s.as_str()).collect();
    assert_eq!(statuses.len(), Status::ALL.len());
    // The labels are lowercase words and underscores: never free text.
    for name in names.iter().chain(statuses.iter()) {
        assert!(
            name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "{name}"
        );
    }
}

#[test]
fn only_a_provider_call_that_cost_something_is_billable() {
    let fresh = Usage {
        input_tokens: 612,
        output_tokens: 48,
        cache_read_input_tokens: 512,
        cache_creation_input_tokens: 0,
        usage_reported: true,
        response_cache_hit: false,
    };
    assert!(fresh.is_billable());
    // Answered from inference-core's own cache: no provider was called.
    assert!(!Usage {
        response_cache_hit: true,
        ..fresh
    }
    .is_billable());
    // The provider reported nothing: the counts mean unknown, not free, and are not
    // metered as if they were exact.
    assert!(!Usage {
        usage_reported: false,
        ..fresh
    }
    .is_billable());
    assert!(!Usage::default().is_billable());
}

#[test]
fn a_failure_is_never_rendered_into_a_class_through_its_message() {
    // The decoder decides from the code and the kind only; this pins that the
    // mapping inherits it. The same message on a typed and an untyped status does
    // not change what each is.
    let message = "no deployment has a verified zero data retention contract";
    let untyped = InferenceError::from_status(&tonic::Status::failed_precondition(message));
    assert_eq!(untyped.failure(), &Failure::Other);
    assert_eq!(
        outcome_for_failure(&untyped),
        ContextOutcome::permanent(ErrorClass::Internal)
    );
}
