//! The outcome table of a chunk's extraction: the vocabulary, the ladder, the poison cap and what
//! becomes of every failure inference-core can send.

use std::collections::BTreeSet;
use std::time::Duration;

use inference_client_rs::{InferenceError, MintError, PENDING_RETRY_AFTER, SKIP_RECHECK_AFTER};
use serde_json::Value;
use tonic::metadata::{MetadataMap, MetadataValue};
use tonic::{Code, Status as GrpcStatus};

use super::*;

fn stamp(entities: i32, relationships: i32, claims: i32) -> Stamp {
    Stamp {
        prompt_version: PROMPT_VERSION.to_owned(),
        model: "gpt-4o-mini-2024-07-18".to_owned(),
        provider: "azure-openai".to_owned(),
        entities,
        relationships,
        claims,
    }
}

fn transient(class: ErrorClass) -> Outcome {
    Outcome::Transient {
        class,
        retry_after: None,
    }
}

// ── the vocabulary ───────────────────────────────────────────────────────────

#[test]
fn the_vocabularies_the_migration_pins_are_the_enums() {
    let statuses: Vec<&str> = Status::ALL.iter().map(|status| status.as_str()).collect();
    assert_eq!(
        statuses,
        STATUS_NAMES.to_vec(),
        "STATUS_NAMES is what tests/contract/postgres compares with the CHECK"
    );
    let classes: Vec<&str> = ErrorClass::ALL.iter().map(|class| class.as_str()).collect();
    assert_eq!(classes, ERROR_CLASS_NAMES.to_vec());
    for status in Status::ALL {
        assert_eq!(Status::parse(status.as_str()), Some(status));
    }
    assert_eq!(Status::parse("done"), None);
    assert_eq!(Status::parse(""), None);
    // No duplicates: a duplicated name would make one status unreachable from the database.
    assert_eq!(
        statuses.iter().collect::<BTreeSet<_>>().len(),
        statuses.len()
    );
    assert_eq!(classes.iter().collect::<BTreeSet<_>>().len(), classes.len());
}

#[test]
fn a_status_and_an_error_class_are_spelled_the_way_the_check_accepts_them() {
    for name in STATUS_NAMES.iter().chain(ERROR_CLASS_NAMES.iter()) {
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{name}"
        );
    }
}

#[test]
fn only_ready_and_failed_permanent_are_final_on_their_own() {
    let finals: Vec<Status> = Status::ALL
        .into_iter()
        .filter(|status| status.is_final())
        .collect();
    assert_eq!(finals, [Status::Ready, Status::FailedPermanent]);
}

// ── the ladder ───────────────────────────────────────────────────────────────

#[test]
fn the_ladder_backs_off_from_five_minutes_to_six_hours_and_stays_there() {
    let steps: Vec<u64> = (1..=8).map(|attempt| backoff(attempt).as_secs()).collect();
    assert_eq!(
        steps,
        [300, 900, 2_700, 8_100, 21_600, 21_600, 21_600, 21_600]
    );
    assert_eq!(
        backoff(1),
        Duration::from_secs(5 * 60),
        "the design's floor"
    );
    assert_eq!(
        backoff(5),
        Duration::from_secs(6 * 60 * 60),
        "the design's ceiling"
    );
    assert_eq!(
        backoff(0),
        backoff(1),
        "attempt zero is not before the first"
    );
    assert_eq!(backoff(-3), backoff(1), "a negative count is no history");
    assert_eq!(backoff(i32::MAX), MAX_RETRY_WAIT);
    assert!(
        LADDER.windows(2).all(|pair| pair[0] < pair[1]),
        "the ladder only goes up"
    );
    assert_eq!(*LADDER.last().expect("a ladder"), MAX_RETRY_WAIT);
}

#[test]
fn the_cap_is_six_attempts_and_the_ceiling_is_a_ceiling() {
    assert_eq!(DEFAULT_MAX_ATTEMPTS, 6);
    const { assert!(DEFAULT_MAX_ATTEMPTS <= MAX_ATTEMPTS_CEILING) };
}

// ── the poison cap ───────────────────────────────────────────────────────────

/// The live repro, as a table: a chunk whose provider keeps failing is retried on the ladder,
/// and the sixth failed attempt is the last. Nothing retries it afterwards.
#[test]
fn a_chunk_that_keeps_failing_is_given_up_on_at_the_sixth_attempt() {
    let outcome = transient(ErrorClass::InvalidOutput);
    let mut waits = Vec::new();
    for attempt in 1..=DEFAULT_MAX_ATTEMPTS {
        let settled = outcome.settle(attempt, DEFAULT_MAX_ATTEMPTS);
        if attempt < DEFAULT_MAX_ATTEMPTS {
            assert_eq!(settled.status, Status::FailedTransient, "attempt {attempt}");
            assert!(!settled.refund_attempt, "a failure of the work counts");
            waits.push(settled.retry_in.expect("a wait").as_secs());
        } else {
            assert_eq!(settled.status, Status::FailedPermanent);
            assert_eq!(settled.error_class, Some(ErrorClass::InvalidOutput));
            assert_eq!(settled.retry_in, None, "given up: nothing is scheduled");
        }
    }
    assert_eq!(waits, [300, 900, 2_700, 8_100, 21_600]);
}

#[test]
fn a_failure_that_cannot_be_fixed_by_trying_again_is_permanent_at_once() {
    let settled = Outcome::Permanent {
        class: ErrorClass::TooLong,
    }
    .settle(1, DEFAULT_MAX_ATTEMPTS);
    assert_eq!(settled.status, Status::FailedPermanent);
    assert_eq!(settled.error_class, Some(ErrorClass::TooLong));
    assert_eq!(settled.retry_in, None);
    assert!(!settled.refund_attempt);
}

#[test]
fn a_configured_cap_moves_the_cap_and_nothing_else() {
    let outcome = transient(ErrorClass::Timeout);
    assert_eq!(outcome.settle(2, 3).status, Status::FailedTransient);
    assert_eq!(outcome.settle(3, 3).status, Status::FailedPermanent);
    assert_eq!(outcome.settle(1, 1).status, Status::FailedPermanent);
}

#[test]
fn what_the_provider_asked_for_wins_when_it_is_longer_than_the_ladder_and_is_capped() {
    let named = |seconds| Outcome::Transient {
        class: ErrorClass::RateLimited,
        retry_after: Some(Duration::from_secs(seconds)),
    };
    // Shorter than the floor: the ladder's floor holds (the provider's wish is subsumed).
    assert_eq!(
        named(2).settle(1, DEFAULT_MAX_ATTEMPTS).retry_in,
        Some(backoff(1))
    );
    // Longer than the ladder step: the provider is obeyed.
    assert_eq!(
        named(3 * 3_600).settle(1, DEFAULT_MAX_ATTEMPTS).retry_in,
        Some(Duration::from_secs(3 * 3_600))
    );
    // A usage limit that resets in fourteen hours is waited for six, not fourteen.
    assert_eq!(
        named(14 * 3_600).settle(1, DEFAULT_MAX_ATTEMPTS).retry_in,
        Some(MAX_RETRY_WAIT)
    );
}

// ── what every outcome becomes ───────────────────────────────────────────────

#[test]
fn every_outcome_becomes_the_status_the_design_names() {
    struct Case {
        outcome: Outcome,
        status: Status,
        class: Option<ErrorClass>,
        retry_in: Option<Duration>,
        refund: bool,
        systemic: bool,
        failure: bool,
    }
    let hour = SKIP_RECHECK_AFTER;
    let five = PENDING_RETRY_AFTER;
    let cases = [
        Case {
            outcome: Outcome::Extracted(stamp(3, 1, 0)),
            status: Status::Ready,
            class: None,
            retry_in: None,
            refund: false,
            systemic: false,
            failure: false,
        },
        Case {
            outcome: Outcome::Extracted(stamp(0, 0, 0)),
            status: Status::Empty,
            class: None,
            retry_in: None,
            refund: false,
            systemic: false,
            failure: false,
        },
        Case {
            outcome: Outcome::Extracted(stamp(0, 0, 2)),
            status: Status::Ready,
            class: None,
            retry_in: None,
            refund: false,
            systemic: false,
            failure: false,
        },
        Case {
            outcome: Outcome::SkippedZdr(ZdrReason::Org),
            status: Status::SkippedZdr,
            class: Some(ErrorClass::ZdrOrg),
            retry_in: None,
            refund: true,
            systemic: true,
            failure: false,
        },
        Case {
            outcome: Outcome::SkippedZdr(ZdrReason::Document),
            status: Status::SkippedZdr,
            class: Some(ErrorClass::ZdrDocument),
            retry_in: None,
            refund: true,
            systemic: false,
            failure: false,
        },
        Case {
            outcome: Outcome::SkippedZdr(ZdrReason::NoAttestedRoute),
            status: Status::SkippedZdr,
            class: Some(ErrorClass::ZdrUnavailable),
            retry_in: Some(hour),
            refund: true,
            systemic: true,
            failure: false,
        },
        Case {
            outcome: Outcome::SkippedPolicy,
            status: Status::SkippedPolicy,
            class: None,
            retry_in: None,
            refund: true,
            systemic: true,
            failure: false,
        },
        Case {
            outcome: Outcome::SkippedVisibility,
            status: Status::SkippedVisibility,
            class: None,
            retry_in: None,
            refund: true,
            systemic: false,
            failure: false,
        },
        Case {
            outcome: Outcome::SkippedBudget,
            status: Status::SkippedBudget,
            class: None,
            retry_in: None,
            refund: true,
            systemic: true,
            failure: false,
        },
        Case {
            outcome: Outcome::PostureUnavailable { retry_after: five },
            status: Status::Pending,
            class: Some(ErrorClass::PostureUnavailable),
            retry_in: Some(five),
            refund: true,
            systemic: true,
            failure: false,
        },
        Case {
            outcome: Outcome::ResidencyUnavailable,
            status: Status::FailedTransient,
            class: Some(ErrorClass::ResidencyUnavailable),
            retry_in: Some(hour),
            refund: true,
            systemic: true,
            failure: false,
        },
        Case {
            outcome: transient(ErrorClass::Auth),
            status: Status::FailedTransient,
            class: Some(ErrorClass::Auth),
            retry_in: Some(five),
            refund: true,
            systemic: true,
            failure: true,
        },
        Case {
            outcome: transient(ErrorClass::RateLimited),
            status: Status::FailedTransient,
            class: Some(ErrorClass::RateLimited),
            retry_in: Some(backoff(1)),
            refund: false,
            systemic: true,
            failure: true,
        },
        Case {
            outcome: transient(ErrorClass::ProviderUnavailable),
            status: Status::FailedTransient,
            class: Some(ErrorClass::ProviderUnavailable),
            retry_in: Some(backoff(1)),
            refund: false,
            systemic: true,
            failure: true,
        },
        Case {
            outcome: transient(ErrorClass::Timeout),
            status: Status::FailedTransient,
            class: Some(ErrorClass::Timeout),
            retry_in: Some(backoff(1)),
            refund: false,
            systemic: false,
            failure: true,
        },
        Case {
            outcome: transient(ErrorClass::InvalidOutput),
            status: Status::FailedTransient,
            class: Some(ErrorClass::InvalidOutput),
            retry_in: Some(backoff(1)),
            refund: false,
            systemic: false,
            failure: true,
        },
        Case {
            outcome: transient(ErrorClass::Internal),
            status: Status::FailedTransient,
            class: Some(ErrorClass::Internal),
            retry_in: Some(backoff(1)),
            refund: false,
            systemic: false,
            failure: true,
        },
        Case {
            outcome: Outcome::Permanent {
                class: ErrorClass::TooLong,
            },
            status: Status::FailedPermanent,
            class: Some(ErrorClass::TooLong),
            retry_in: None,
            refund: false,
            systemic: false,
            failure: true,
        },
    ];
    for case in &cases {
        let settled = case.outcome.settle(1, DEFAULT_MAX_ATTEMPTS);
        let label = format!("{:?}", case.outcome);
        assert_eq!(settled.status, case.status, "{label}");
        assert_eq!(settled.error_class, case.class, "{label}");
        assert_eq!(settled.retry_in, case.retry_in, "{label}");
        assert_eq!(settled.refund_attempt, case.refund, "{label}");
        assert_eq!(
            settled.refund_attempt,
            !case.outcome.counts_as_attempt(),
            "{label}"
        );
        assert_eq!(case.outcome.is_systemic(), case.systemic, "{label}");
        assert_eq!(case.outcome.is_failure(), case.failure, "{label}");
        assert_eq!(
            settled.stamp.is_some(),
            matches!(case.outcome, Outcome::Extracted(_)),
            "only an extraction carries a stamp: {label}"
        );
    }
}

/// The migration's CHECKs, applied to every settlement this module can produce: a status the
/// table would refuse is a bug found here and not in production.
#[test]
fn no_settlement_violates_a_check_of_the_table() {
    let outcomes = [
        Outcome::Extracted(stamp(1, 0, 0)),
        Outcome::Extracted(stamp(0, 0, 0)),
        Outcome::SkippedZdr(ZdrReason::Org),
        Outcome::SkippedZdr(ZdrReason::Document),
        Outcome::SkippedZdr(ZdrReason::NoAttestedRoute),
        Outcome::SkippedPolicy,
        Outcome::SkippedVisibility,
        Outcome::SkippedBudget,
        Outcome::PostureUnavailable {
            retry_after: PENDING_RETRY_AFTER,
        },
        Outcome::ResidencyUnavailable,
        transient(ErrorClass::Auth),
        transient(ErrorClass::Timeout),
        Outcome::Permanent {
            class: ErrorClass::Internal,
        },
    ];
    for outcome in outcomes {
        for attempts in [1, 5, 6, 7] {
            let settled = outcome.settle(attempts, DEFAULT_MAX_ATTEMPTS);
            let label = format!("{outcome:?} at attempt {attempts}");
            // failure_class_chk: a failure and a zero-retention skip always say why.
            if matches!(
                settled.status,
                Status::FailedTransient | Status::FailedPermanent | Status::SkippedZdr
            ) {
                assert!(settled.error_class.is_some(), "{label}");
            }
            // stamp_chk and yield_chk: ready stored something, empty stored nothing, and both
            // are stamped.
            match settled.status {
                Status::Ready | Status::Empty => {
                    let stamp = settled
                        .stamp
                        .as_ref()
                        .expect("a completed extraction is stamped");
                    assert!(
                        !stamp.model.is_empty() && !stamp.prompt_version.is_empty(),
                        "{label}"
                    );
                    assert_eq!(
                        stamp.total() > 0,
                        settled.status == Status::Ready,
                        "{label}"
                    );
                }
                _ => assert!(settled.stamp.is_none(), "{label}"),
            }
            // A final status never schedules anything.
            if settled.status.is_final() {
                assert_eq!(settled.retry_in, None, "{label}");
            }
        }
    }
}

// ── the failures inference-core can send ─────────────────────────────────────

fn golden() -> Value {
    crate::testing::status_golden()
}

fn as_received(case: &Value) -> GrpcStatus {
    crate::testing::status_as_received(case)
}

fn after(milliseconds: u64) -> Option<Duration> {
    Some(Duration::from_millis(milliseconds))
}

/// What this service makes of each case in the golden. A case the golden gains that is not in
/// this table fails `the_table_covers_every_case_in_the_golden`.
fn expected() -> Vec<(&'static str, Outcome)> {
    let provider = Outcome::Transient {
        class: ErrorClass::ProviderUnavailable,
        retry_after: None,
    };
    let provider_for = |milliseconds| Outcome::Transient {
        class: ErrorClass::ProviderUnavailable,
        retry_after: after(milliseconds),
    };
    let limited = |retry_after| Outcome::Transient {
        class: ErrorClass::RateLimited,
        retry_after,
    };
    let internal_hourly = Outcome::Transient {
        class: ErrorClass::Internal,
        retry_after: Some(SKIP_RECHECK_AFTER),
    };
    vec![
        ("provider_http", provider.clone()),
        ("provider_unavailable", provider.clone()),
        ("provider_subscription_failed", provider.clone()),
        (
            "provider_invalid_response",
            Outcome::Permanent {
                class: ErrorClass::Internal,
            },
        ),
        (
            "provider_unsupported_model",
            Outcome::Permanent {
                class: ErrorClass::Internal,
            },
        ),
        ("provider_all_exhausted", provider.clone()),
        (
            "provider_too_long",
            Outcome::Permanent {
                class: ErrorClass::TooLong,
            },
        ),
        ("provider_rate_limited", limited(after(1_500))),
        (
            "provider_zdr_unavailable",
            Outcome::SkippedZdr(ZdrReason::NoAttestedRoute),
        ),
        (
            "provider_residency_violation",
            Outcome::ResidencyUnavailable,
        ),
        ("provider_tier_unavailable", internal_hourly.clone()),
        (
            "subscription_usage_limit_with_timing",
            limited(after(52_260_000)),
        ),
        ("subscription_usage_limit_without_timing", limited(None)),
        (
            "subscription_reauth_required_with_timing",
            provider_for(52_260_000),
        ),
        (
            "subscription_reauth_required_without_timing",
            provider.clone(),
        ),
        (
            "subscription_upstream_unavailable_with_timing",
            provider_for(52_260_000),
        ),
        (
            "subscription_upstream_unavailable_without_timing",
            provider.clone(),
        ),
        (
            "subscription_bad_request_with_timing",
            provider_for(52_260_000),
        ),
        ("subscription_bad_request_without_timing", provider.clone()),
        (
            "modality_refused_under_zdr",
            Outcome::SkippedZdr(ZdrReason::NoAttestedRoute),
        ),
        (
            "embedding_zdr_unavailable",
            Outcome::SkippedZdr(ZdrReason::NoAttestedRoute),
        ),
        (
            "embedding_residency_unavailable",
            Outcome::ResidencyUnavailable,
        ),
        ("embedding_tier_unavailable", internal_hourly),
        (
            "embedding_provider_http",
            Outcome::Transient {
                class: ErrorClass::Internal,
                retry_after: None,
            },
        ),
    ]
}

#[test]
fn the_table_covers_every_case_in_the_golden() {
    let golden = golden();
    let in_golden: BTreeSet<String> = golden["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|case| case["id"].as_str().expect("id").to_owned())
        .collect();
    let in_table: BTreeSet<String> = expected()
        .into_iter()
        .map(|(id, _)| id.to_owned())
        .collect();
    assert_eq!(
        in_golden, in_table,
        "inference-core gained or lost a way to fail: decide what a chunk becomes, then update `expected`"
    );
}

#[test]
fn every_failure_inference_core_can_send_becomes_the_outcome_decided_for_it() {
    let golden = golden();
    let table = expected();
    for case in golden["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let want = &table
            .iter()
            .find(|(name, _)| *name == id)
            .unwrap_or_else(|| panic!("no decision for {id}"))
            .1;
        let decoded = InferenceError::from_status(&as_received(case));
        assert_eq!(
            &outcome_for_failure(&decoded),
            want,
            "{id}: decoded as {decoded}"
        );
        // And it survives being wrapped in an anyhow error, which is how it reaches the pipeline.
        assert_eq!(
            &outcome_for_error(&anyhow::Error::new(decoded)),
            want,
            "{id} through anyhow"
        );
    }
}

/// A usage limit that resets in fourteen hours must not park a chunk for fourteen hours, and a
/// refusal to send content must never be retried into a send.
#[test]
fn a_zero_retention_refusal_is_a_skip_and_never_a_failure_that_counts() {
    let golden = golden();
    for case in golden["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let outcome = outcome_for_failure(&InferenceError::from_status(&as_received(case)));
        if let Outcome::SkippedZdr(_) = outcome {
            assert!(!outcome.counts_as_attempt(), "{id}");
            let settled = outcome.settle(DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_ATTEMPTS);
            assert_eq!(
                settled.status,
                Status::SkippedZdr,
                "{id}: a refusal never reaches failed_permanent, whatever the attempts"
            );
        }
    }
}

#[test]
fn a_bearer_that_could_not_be_had_is_a_wait_or_a_deployment_problem_and_never_a_chunk_failure() {
    let wait = |error: MintError| outcome_for_failure(&InferenceError::from(error));
    for error in [
        MintError::PostureUnavailable,
        MintError::DirectoryUnavailable,
        MintError::Unavailable("connection refused".to_owned()),
        MintError::ServerError { status: 502 },
        MintError::Malformed("the response is not a token bundle"),
    ] {
        let outcome = wait(error.clone());
        assert_eq!(
            outcome,
            Outcome::PostureUnavailable {
                retry_after: PENDING_RETRY_AFTER
            },
            "{error}"
        );
        let settled = outcome.settle(DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_ATTEMPTS);
        assert_eq!(
            (settled.status, settled.error_class),
            (Status::Pending, Some(ErrorClass::PostureUnavailable)),
            "a posture Auth Core could not resolve is pending at any attempt count: {error}"
        );
        assert!(settled.refund_attempt, "{error}");
    }
    for error in [
        MintError::Refused { status: 403 },
        MintError::CredentialUnavailable,
        MintError::Contract("claims exceeded requested authority"),
        MintError::PostureMismatch {
            observed: "zdr",
            expected: "persistent",
        },
        MintError::InvalidOrg,
        MintError::Rejected { status: 400 },
    ] {
        let outcome = wait(error.clone());
        assert_eq!(
            outcome,
            Outcome::Transient {
                class: ErrorClass::Auth,
                retry_after: Some(PENDING_RETRY_AFTER)
            },
            "{error}"
        );
        let settled = outcome.settle(DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_ATTEMPTS);
        assert_eq!(
            settled.status,
            Status::FailedTransient,
            "a credential problem never becomes failed_permanent: {error}"
        );
        assert!(settled.refund_attempt, "{error}");
    }
}

#[test]
fn the_errors_the_extractor_types_itself_are_decoded_by_type() {
    use anyhow::Error;
    assert_eq!(
        outcome_for_error(&Error::new(ZdrOrganization)),
        Outcome::SkippedZdr(ZdrReason::Org)
    );
    assert_eq!(
        outcome_for_error(&Error::new(ExtractionTimeout {
            after: Duration::from_secs(60)
        })),
        transient(ErrorClass::Timeout)
    );
    assert_eq!(
        outcome_for_error(&Error::new(InvalidModelOutput)),
        transient(ErrorClass::InvalidOutput)
    );
    assert_eq!(
        outcome_for_error(&Error::new(EmptyModelOutput)),
        transient(ErrorClass::InvalidOutput)
    );
    // Anything else is an unknown failure: it counts toward the cap, so it cannot loop forever,
    // and it is never turned into a send or into a skip for good.
    assert_eq!(
        outcome_for_error(&anyhow::anyhow!(
            "pool timed out while waiting for an open connection"
        )),
        transient(ErrorClass::Internal)
    );
    // Context wrapped around a typed failure does not hide it from the decoder.
    let wrapped = Error::new(InvalidModelOutput).context("graph extraction");
    assert_eq!(
        outcome_for_error(&wrapped),
        transient(ErrorClass::InvalidOutput)
    );
}

#[test]
fn a_log_line_carries_the_code_and_the_kind_as_fields() {
    let status = GrpcStatus::with_metadata(Code::Unavailable, "upstream down", {
        let mut metadata = MetadataMap::new();
        metadata.insert(
            "x-mp-provider-error",
            MetadataValue::try_from("rate_limited").expect("ascii"),
        );
        metadata
    });
    let error =
        anyhow::Error::new(InferenceError::from_status(&status).during("model-plane inference"));
    assert_eq!(
        failure_fields(&error),
        FailureFields {
            kind: "rate_limited",
            stage: Some("call"),
            grpc_code: Some("Unavailable".to_owned()),
            provider_error: Some("rate_limited".to_owned()),
        }
    );
    let mint = anyhow::Error::new(InferenceError::from(MintError::PostureUnavailable));
    assert_eq!(
        failure_fields(&mint),
        FailureFields {
            kind: "posture_unavailable",
            stage: Some("mint"),
            grpc_code: None,
            provider_error: Some("retention_posture_unavailable".to_owned()),
        }
    );
    assert_eq!(
        failure_fields(&anyhow::Error::new(ZdrOrganization)).kind,
        "zdr_organization"
    );
    assert_eq!(
        failure_fields(&anyhow::anyhow!("boom")).kind,
        "unclassified"
    );
}
