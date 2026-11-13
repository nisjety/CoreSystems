//! Unit tests for the failure classes and the bench board (R2-OD-40-a).
//!
//! The clock is a plain `tokio::time::Instant` passed in by the test, so no test
//! sleeps or reads the wall clock beyond one `Instant::now()` origin.

use std::time::Duration;

use tokio::time::Instant;

use super::*;
use crate::provider::{ProviderError, SubscriptionRefusal};

/// One example of every `ProviderError` variant, with the class it must get.
///
/// The match below has no wildcard arm: a new variant fails to compile here until
/// the test names the class it expects, as `classify` itself does.
fn every_variant() -> Vec<(ProviderError, Option<FailureClass>)> {
    let examples = vec![
        ProviderError::Http("503 Service Unavailable: overloaded".to_owned()),
        ProviderError::RateLimited { retry_after_ms: 0 },
        ProviderError::SubscriptionRefused {
            reason: SubscriptionRefusal::UsageLimit,
            retry_after_ms: None,
            resets_at: None,
        },
        ProviderError::InvalidResponse("not json".to_owned()),
        ProviderError::Unavailable("ANTHROPIC_API_KEY is empty".to_owned()),
        ProviderError::SubscriptionFailed("broker dropped".to_owned()),
        ProviderError::TooLong {
            detail: "context_length_exceeded".to_owned(),
        },
        ProviderError::RequestRejected {
            detail: "invalid tool schema".to_owned(),
        },
        ProviderError::AllExhausted { attempts: 3 },
        ProviderError::UnsupportedModel("x".to_owned()),
        ProviderError::ResidencyViolation("eu".to_owned()),
        ProviderError::ZdrUnavailable("none".to_owned()),
        ProviderError::TierUnavailable("eu_resident".to_owned()),
        ProviderError::VisionUnsupported("no images".to_owned()),
        ProviderError::ModelNotEntitled {
            model: "gpt-4o".to_owned(),
            reason: mp_contracts::model_entitlement::Reason::NotInAllowList,
        },
        ProviderError::EntitlementUnavailable,
        ProviderError::ModelHidden {
            model: "gpt-4o".to_owned(),
        },
        ProviderError::ImageModelUnavailable {
            model: "dall-e".to_owned(),
        },
        ProviderError::ProvidersBenched {
            retry_after_ms: 1_500,
        },
    ];
    examples
        .into_iter()
        .map(|error| {
            let expected = match &error {
                ProviderError::Http(_) => Some(FailureClass::Server),
                ProviderError::RateLimited { .. } => Some(FailureClass::RateLimit),
                ProviderError::SubscriptionRefused { .. } => Some(FailureClass::Quota),
                ProviderError::InvalidResponse(_) => Some(FailureClass::Server),
                // A configuration refusal with no status says nothing about health.
                ProviderError::Unavailable(_) => None,
                ProviderError::SubscriptionFailed(_) => Some(FailureClass::Server),
                ProviderError::TooLong { .. } => Some(FailureClass::ContextTooLong),
                ProviderError::RequestRejected { .. } => Some(FailureClass::MalformedRequest),
                ProviderError::AllExhausted { .. }
                | ProviderError::UnsupportedModel(_)
                | ProviderError::ResidencyViolation(_)
                | ProviderError::ZdrUnavailable(_)
                | ProviderError::TierUnavailable(_)
                | ProviderError::VisionUnsupported(_)
                | ProviderError::ModelNotEntitled { .. }
                | ProviderError::EntitlementUnavailable
                | ProviderError::ModelHidden { .. }
                | ProviderError::ImageModelUnavailable { .. }
                | ProviderError::ProvidersBenched { .. } => None,
            };
            (error, expected)
        })
        .collect()
}

#[test]
fn classify_covers_one_example_of_every_variant() {
    let examples = every_variant();
    assert_eq!(examples.len(), 19, "one example per ProviderError variant");
    for (error, expected) in examples {
        assert_eq!(classify(&error), expected, "{error:?}");
    }
}

#[test]
fn classify_reads_subscription_reasons() {
    let refused = |reason| ProviderError::SubscriptionRefused {
        reason,
        retry_after_ms: Some(10),
        resets_at: None,
    };
    assert_eq!(
        classify(&refused(SubscriptionRefusal::UsageLimit)),
        Some(FailureClass::Quota)
    );
    assert_eq!(
        classify(&refused(SubscriptionRefusal::ReauthRequired)),
        Some(FailureClass::Auth)
    );
    assert_eq!(
        classify(&refused(SubscriptionRefusal::UpstreamUnavailable)),
        Some(FailureClass::Refusal)
    );
    assert_eq!(
        classify(&refused(SubscriptionRefusal::BadRequest)),
        Some(FailureClass::Refusal)
    );
}

#[test]
fn classify_reads_http_status_and_transport_text() {
    let http = |text: &str| classify(&ProviderError::Http(text.to_owned()));
    assert_eq!(
        http("503 Service Unavailable: x"),
        Some(FailureClass::Server)
    );
    assert_eq!(http("500: boom"), Some(FailureClass::Server));
    assert_eq!(http("401: bad key"), Some(FailureClass::Auth));
    assert_eq!(http("403 Forbidden"), Some(FailureClass::Auth));
    assert_eq!(http("402: pay"), Some(FailureClass::Quota));
    assert_eq!(http("429: insufficient_quota"), Some(FailureClass::Quota));
    assert_eq!(http("429: billing hard limit"), Some(FailureClass::Quota));
    assert_eq!(
        http("429: slow down"),
        None,
        "a bare 429 is typed RateLimited upstream"
    );
    assert_eq!(http("408 Request Timeout"), Some(FailureClass::Timeout));
    assert_eq!(http("504: gateway"), Some(FailureClass::Timeout));
    assert_eq!(
        http("404: no deployment"),
        None,
        "other 4xx says nothing about health"
    );
    assert_eq!(http("400: bad"), None);
    // No status: a transport error.
    assert_eq!(
        http("error sending request: operation timed out"),
        Some(FailureClass::Timeout)
    );
    assert_eq!(http("connect TIMEOUT"), Some(FailureClass::Timeout));
    assert_eq!(http("connection refused"), Some(FailureClass::Server));
    // A four-digit leading number is not a status.
    assert_eq!(http("5030 things"), Some(FailureClass::Server));
}

#[test]
fn classify_reads_unavailable_only_with_a_status() {
    let unavailable = |text: &str| classify(&ProviderError::Unavailable(text.to_owned()));
    assert_eq!(unavailable("ANTHROPIC_API_KEY is empty"), None);
    assert_eq!(unavailable("no subscription connection"), None);
    assert_eq!(
        unavailable("request timed out"),
        None,
        "no status: never a health signal"
    );
    assert_eq!(unavailable("503: down"), Some(FailureClass::Server));
    assert_eq!(unavailable("401: key"), Some(FailureClass::Auth));
}

#[test]
fn classify_empty_response_is_empty_class() {
    assert_eq!(
        classify(&ProviderError::InvalidResponse(String::new())),
        Some(FailureClass::Empty)
    );
    assert_eq!(
        classify(&ProviderError::InvalidResponse(" \n\t".to_owned())),
        Some(FailureClass::Empty)
    );
    assert_eq!(
        classify(&ProviderError::InvalidResponse("garbled".to_owned())),
        Some(FailureClass::Server)
    );
}

#[test]
fn class_labels_are_the_closed_vocabulary() {
    let labels: Vec<&str> = FailureClass::ALL
        .iter()
        .map(|class| class.as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "auth",
            "quota",
            "rate_limit",
            "context_too_long",
            "malformed_request",
            "refusal",
            "server",
            "timeout",
            "empty"
        ]
    );
}

#[test]
fn only_provider_classes_bench() {
    for class in FailureClass::ALL {
        let expected = !matches!(
            class,
            FailureClass::ContextTooLong | FailureClass::MalformedRequest | FailureClass::Refusal
        );
        assert_eq!(benches(class), expected, "{class:?}");
    }
}

#[test]
fn three_server_failures_from_two_orgs_bench() {
    let board = BenchBoard::new();
    let now = Instant::now();
    assert!(board
        .record_failure("P", "M", "org-a", FailureClass::Server, None, now)
        .is_none());
    assert!(board
        .record_failure("p", "m", "org-b", FailureClass::Server, None, now)
        .is_none());
    let until = board
        .record_failure("p", "m", "org-a", FailureClass::Server, None, now)
        .expect("third failure from two orgs benches");
    assert_eq!(until, now + BENCH_BASE);
    assert_eq!(board.benched_until("p", "m", now), Some(until));
    // Keys are case-insensitive.
    assert_eq!(board.benched_until("P", "M", now), Some(until));
    // Another model on the same provider is not benched.
    assert_eq!(board.benched_until("p", "other", now), None);
}

#[test]
fn three_server_failures_from_one_org_do_not_bench() {
    let board = BenchBoard::new();
    let now = Instant::now();
    for _ in 0..5 {
        assert!(board
            .record_failure("p", "m", "org-a", FailureClass::Server, None, now)
            .is_none());
    }
    assert_eq!(board.benched_until("p", "m", now), None);
}

#[test]
fn two_server_failures_from_two_orgs_do_not_bench() {
    let board = BenchBoard::new();
    let now = Instant::now();
    assert!(board
        .record_failure("p", "m", "org-a", FailureClass::Timeout, None, now)
        .is_none());
    assert!(board
        .record_failure("p", "m", "org-b", FailureClass::Empty, None, now)
        .is_none());
    assert_eq!(board.benched_until("p", "m", now), None);
}

#[test]
fn auth_and_quota_bench_at_once() {
    let board = BenchBoard::new();
    let now = Instant::now();
    assert_eq!(
        board.record_failure("p", "m", "org-a", FailureClass::Auth, None, now),
        Some(now + BENCH_BASE)
    );
    assert_eq!(
        board.record_failure("q", "m", "org-a", FailureClass::Quota, None, now),
        Some(now + BENCH_BASE)
    );
}

#[test]
fn request_classes_never_touch_the_board() {
    let board = BenchBoard::new();
    let now = Instant::now();
    for org in ["a", "b", "c", "d"] {
        for class in [
            FailureClass::ContextTooLong,
            FailureClass::MalformedRequest,
            FailureClass::Refusal,
        ] {
            assert!(board
                .record_failure("p", "m", org, class, None, now)
                .is_none());
        }
    }
    assert_eq!(board.benched_until("p", "m", now), None);
}

#[test]
fn rate_limit_benches_at_once_only_with_a_declared_retry_after() {
    let board = BenchBoard::new();
    let now = Instant::now();
    assert!(board
        .record_failure(
            "p",
            "m",
            "org-a",
            FailureClass::RateLimit,
            Some(Duration::ZERO),
            now
        )
        .is_none());
    assert!(board
        .record_failure("p", "m", "org-a", FailureClass::RateLimit, None, now)
        .is_none());
    assert_eq!(
        board.record_failure(
            "q",
            "m",
            "org-a",
            FailureClass::RateLimit,
            Some(Duration::from_secs(2)),
            now
        ),
        Some(now + Duration::from_secs(2))
    );
}

#[test]
fn a_declared_retry_after_is_clamped_to_the_max() {
    let board = BenchBoard::new();
    let now = Instant::now();
    assert_eq!(
        board.record_failure(
            "p",
            "m",
            "org-a",
            FailureClass::RateLimit,
            Some(Duration::from_secs(900)),
            now
        ),
        Some(now + BENCH_MAX)
    );
    assert_eq!(BENCH_MAX, Duration::from_secs(300));
}

#[test]
fn a_success_clears_the_run_and_the_bench() {
    let board = BenchBoard::new();
    let now = Instant::now();
    board
        .record_failure("p", "m", "org-a", FailureClass::Auth, None, now)
        .expect("benched");
    board.record_success("P", "M");
    assert_eq!(board.benched_until("p", "m", now), None);
    // A partial guarded run is cleared too: two more failures after the success do not bench.
    let board = BenchBoard::new();
    board.record_failure("p", "m", "org-a", FailureClass::Server, None, now);
    board.record_failure("p", "m", "org-b", FailureClass::Server, None, now);
    board.record_success("p", "m");
    assert!(board
        .record_failure("p", "m", "org-a", FailureClass::Server, None, now)
        .is_none());
    assert!(board
        .record_failure("p", "m", "org-b", FailureClass::Server, None, now)
        .is_none());
}

#[test]
fn a_failure_during_a_running_bench_changes_nothing() {
    let board = BenchBoard::new();
    let now = Instant::now();
    let until = board
        .record_failure("p", "m", "a", FailureClass::Auth, None, now)
        .unwrap();
    let later = now + Duration::from_secs(1);
    assert!(board
        .record_failure("p", "m", "a", FailureClass::Auth, None, later)
        .is_none());
    assert_eq!(board.benched_until("p", "m", later), Some(until));
}

#[test]
fn an_ended_bench_is_half_open_and_a_repeat_doubles_up_to_the_cap() {
    let board = BenchBoard::new();
    let mut now = Instant::now();
    let mut expected = BENCH_BASE;
    for _ in 0..7 {
        let until = board
            .record_failure("p", "m", "org-a", FailureClass::Auth, None, now)
            .expect("a half-open entry benches again on one auth failure");
        assert_eq!(until - now, expected);
        // At the end of the bench the entry is half-open: not benched.
        now = until;
        assert_eq!(board.benched_until("p", "m", now), None);
        expected = (expected * 2).min(BENCH_MAX);
    }
    assert_eq!(expected, BENCH_MAX);
}

#[test]
fn the_map_cap_holds() {
    let board = BenchBoard::new();
    let now = Instant::now();
    for index in 0..(MAX_ENTRIES + 50) {
        board.record_failure(
            &format!("p{index}"),
            "m",
            "org-a",
            FailureClass::Server,
            None,
            now,
        );
    }
    let len = board.lock().len();
    assert!(len <= MAX_ENTRIES, "{len} entries exceed the cap");
    // Entries benched later are kept over the oldest unbenched ones: a fresh bench is visible.
    let later = now + Duration::from_secs(1);
    let until = board
        .record_failure("fresh", "m", "org-a", FailureClass::Auth, None, later)
        .expect("benched");
    assert_eq!(board.benched_until("fresh", "m", later), Some(until));
    assert!(board.lock().len() <= MAX_ENTRIES);
}

#[test]
fn the_board_debug_output_holds_no_entries() {
    let board = BenchBoard::new();
    board.record_failure(
        "p",
        "m",
        "secret-org",
        FailureClass::Auth,
        None,
        Instant::now(),
    );
    let debug = format!("{board:?}");
    assert!(!debug.contains("secret-org"));
    assert!(debug.starts_with("BenchBoard"));
}
