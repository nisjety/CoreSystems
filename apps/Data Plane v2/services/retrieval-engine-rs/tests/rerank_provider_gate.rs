//! QB-T05b: the provider gate shared by tenant retrieval and `POST /v1/rerank`.
//!
//! What is under test is the real gate and the real `RerankClient` over HTTP. The only double is
//! the provider itself: a localhost stand-in for the Azure Cohere route that counts the calls it
//! receives, so every "no provider call" claim is a count, not an assumption. The gate's timing
//! rules (the breaker's 30 s windows, the budget's refill, the tenant wait) run on tokio's paused
//! clock, which makes them exact; the HTTP tests use real time with wide margins.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::sync::Semaphore;

use retrieval_engine::config::{parse_bounded, Config};
use retrieval_engine::pipeline::types::ScoredCandidate;
use retrieval_engine::rerank_api::CircuitGate;
use retrieval_engine::search::rerank::{
    BreakerPolicy, ProviderGate, ProviderLimits, Refusal, RerankClient, RerankError,
    TENANT_RESERVED_PERMITS,
};

fn limits(max_inflight: usize, requests_per_second: u32) -> ProviderLimits {
    ProviderLimits {
        max_inflight,
        requests_per_second,
        tenant_wait: Duration::from_millis(1_000),
        breaker: BreakerPolicy::default(),
    }
}

/// A gate whose breaker has just opened: five failed web calls inside the window.
fn open_breaker() -> Arc<ProviderGate> {
    let gate = ProviderGate::new(limits(8, 1_000));
    for _ in 0..5 {
        gate.admit_web().expect("admitted").settle(false);
    }
    assert!(gate.is_open(), "five failures open the breaker");
    gate
}

// ─────────────────────────── the gate, on the paused clock ───────────────────────────

#[tokio::test(start_paused = true)]
async fn the_default_limits_leave_tenant_retrieval_unbounded_and_give_web_no_capacity() {
    let gate = ProviderGate::new(ProviderLimits::default());
    for _ in 0..50 {
        gate.admit_tenant()
            .await
            .expect("tenant is never refused by default")
            .settle(true);
    }
    assert_eq!(gate.admit_web().err(), Some(Refusal::Saturated));
    assert!(!gate.is_open());
}

#[tokio::test(start_paused = true)]
async fn web_saturation_leaves_the_reserved_permits_for_tenant_retrieval() {
    // Four permits in all: web may hold four minus the two reserved for tenant retrieval.
    let gate = ProviderGate::new(limits(4, 1_000));
    let web_a = gate.admit_web().expect("web call 1");
    let web_b = gate.admit_web().expect("web call 2");
    assert_eq!(
        gate.admit_web().err(),
        Some(Refusal::Saturated),
        "web never takes a reserved permit"
    );

    // The reserved permits are free for tenant calls at once.
    let tenant_a = gate.admit_tenant().await.expect("tenant call 1");
    let tenant_b = gate.admit_tenant().await.expect("tenant call 2");

    // The pool is now full. A further tenant call waits for tenant_wait, then gives up.
    let started = tokio::time::Instant::now();
    assert_eq!(
        gate.admit_tenant().await.err(),
        Some(Refusal::TenantWaitExpired)
    );
    assert!(started.elapsed() >= Duration::from_millis(1_000));

    drop((web_a, web_b, tenant_a, tenant_b));
}

#[tokio::test(start_paused = true)]
async fn a_tenant_call_that_waits_is_admitted_when_a_permit_comes_back() {
    let gate = ProviderGate::new(limits(3, 1_000));
    let a = gate.admit_tenant().await.expect("a");
    let b = gate.admit_tenant().await.expect("b");
    let c = gate.admit_tenant().await.expect("c");

    let waiter = {
        let gate = gate.clone();
        tokio::spawn(async move { gate.admit_tenant().await.is_ok() })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(a);
    assert!(
        waiter.await.expect("join"),
        "the waiting call got the permit"
    );
    drop((b, c));
}

#[tokio::test(start_paused = true)]
async fn the_request_budget_refuses_web_past_its_rate_and_refills() {
    // Two requests a second, so a burst of two and one token back every half second.
    let gate = ProviderGate::new(limits(8, 2));
    let first = gate.admit_web().expect("first");
    let second = gate.admit_web().expect("second");
    assert_eq!(gate.admit_web().err(), Some(Refusal::Throttled));

    tokio::time::advance(Duration::from_millis(400)).await;
    assert_eq!(
        gate.admit_web().err(),
        Some(Refusal::Throttled),
        "0.8 of a token is not a call"
    );
    tokio::time::advance(Duration::from_millis(200)).await;
    let third = gate.admit_web().expect("one token has come back by 600 ms");
    assert_eq!(gate.admit_web().err(), Some(Refusal::Throttled));
    drop((first, second, third));
}

#[tokio::test(start_paused = true)]
async fn tenant_calls_are_never_refused_by_the_budget_but_they_spend_it() {
    let gate = ProviderGate::new(limits(8, 2));
    for _ in 0..5 {
        gate.admit_tenant()
            .await
            .expect("tenant is never refused by the budget")
            .settle(true);
    }
    // The budget is in debt, floored at one burst (-2). Web needs 1.5 s of refill to get a token.
    assert_eq!(gate.admit_web().err(), Some(Refusal::Throttled));
    tokio::time::advance(Duration::from_millis(1_400)).await;
    assert_eq!(gate.admit_web().err(), Some(Refusal::Throttled));
    tokio::time::advance(Duration::from_millis(200)).await;
    assert!(gate.admit_web().is_ok(), "the debt is repaid by 1.6 s");
}

#[tokio::test(start_paused = true)]
async fn the_breaker_opens_on_the_fifth_failure_inside_thirty_seconds() {
    let gate = ProviderGate::new(limits(8, 1_000));
    for _ in 0..4 {
        gate.admit_web().expect("admitted").settle(false);
    }
    assert!(!gate.is_open(), "four failures do not open it");
    gate.admit_web().expect("admitted").settle(false);
    assert!(gate.is_open(), "the fifth failure opens it");
    assert_eq!(gate.admit_web().err(), Some(Refusal::CircuitOpen));
}

#[tokio::test(start_paused = true)]
async fn failures_older_than_the_window_do_not_count_toward_opening() {
    let gate = ProviderGate::new(limits(8, 1_000));
    for _ in 0..4 {
        gate.admit_web().expect("admitted").settle(false);
    }
    tokio::time::advance(Duration::from_secs(31)).await;
    gate.admit_web().expect("admitted").settle(false);
    assert!(
        !gate.is_open(),
        "the first four have aged out of the window"
    );
}

#[tokio::test(start_paused = true)]
async fn the_open_breaker_admits_one_probe_after_thirty_seconds_and_closes_on_success() {
    let gate = open_breaker();
    tokio::time::advance(Duration::from_secs(29)).await;
    assert_eq!(gate.admit_web().err(), Some(Refusal::CircuitOpen));

    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(!gate.is_open(), "half-open: the next caller is the probe");
    let probe = gate.admit_web().expect("the probe is admitted");
    assert!(gate.is_open(), "while the probe is out, nobody else is");
    assert_eq!(gate.admit_web().err(), Some(Refusal::CircuitOpen));

    probe.settle(true);
    assert!(!gate.is_open(), "a good answer closes the breaker");
    gate.admit_web().expect("closed again").settle(true);
}

#[tokio::test(start_paused = true)]
async fn a_failed_probe_reopens_the_breaker_for_a_full_open_period() {
    let gate = open_breaker();
    tokio::time::advance(Duration::from_secs(31)).await;
    gate.admit_web().expect("probe").settle(false);
    assert!(gate.is_open());

    tokio::time::advance(Duration::from_secs(29)).await;
    assert!(gate.is_open(), "a fresh thirty seconds");
    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(!gate.is_open());
    gate.admit_web().expect("the next probe").settle(true);
}

#[tokio::test(start_paused = true)]
async fn a_probe_dropped_without_an_answer_is_released_for_the_next_caller() {
    let gate = open_breaker();
    tokio::time::advance(Duration::from_secs(31)).await;
    drop(gate.admit_web().expect("probe"));
    assert!(!gate.is_open(), "not stuck: the next caller may probe");
    gate.admit_web().expect("probe again").settle(true);
    assert!(!gate.is_open());
}

#[tokio::test(start_paused = true)]
async fn tenant_failures_open_the_breaker_for_web_but_never_refuse_tenant_calls() {
    let gate = ProviderGate::new(limits(8, 1_000));
    for _ in 0..5 {
        gate.admit_tenant().await.expect("tenant").settle(false);
    }
    assert!(gate.is_open(), "tenant failures count toward the breaker");
    gate.admit_tenant()
        .await
        .expect("tenant calls still reach the provider")
        .settle(true);
    assert_eq!(gate.admit_web().err(), Some(Refusal::CircuitOpen));
}

// ─────────────────────────── configuration and capacity rules ───────────────────────────

#[test]
fn provider_settings_read_bounded_whole_numbers_and_refuse_anything_else() {
    assert_eq!(
        parse_bounded("X", "", 7, 0, 10).expect("empty is the default"),
        7
    );
    assert_eq!(parse_bounded("X", " 4 ", 0, 0, 10).expect("trimmed"), 4);
    assert_eq!(
        parse_bounded("X", "10", 0, 0, 10).expect("the bound is in"),
        10
    );
    assert!(parse_bounded("X", "11", 0, 0, 10).is_err());
    assert!(parse_bounded("X", "-1", 0, 0, 10).is_err());
    assert!(parse_bounded("X", "four", 0, 0, 10).is_err());
}

#[test]
fn web_has_capacity_only_above_the_tenant_reserve_and_with_a_budget() {
    assert!(!limits(0, 0).web_capacity());
    assert!(
        !limits(TENANT_RESERVED_PERMITS, 5).web_capacity(),
        "the reserve is never web's"
    );
    assert!(
        !limits(TENANT_RESERVED_PERMITS + 1, 0).web_capacity(),
        "no budget, no web"
    );
    assert!(limits(TENANT_RESERVED_PERMITS + 1, 1).web_capacity());
}

// ─────────────────────────── the gate, through the real client over HTTP ───────────────────────────

/// A provider stand-in on localhost. It counts calls. A query beginning `hold` waits for a permit
/// the test returns; one beginning `fail` answers 500, one beginning `throttle` answers 429 with a
/// one-second `Retry-After`; any other is scored, `top_n` respected.
#[derive(Clone)]
struct Provider {
    calls: Arc<AtomicUsize>,
    release: Arc<Semaphore>,
}

async fn answer(State(provider): State<Provider>, Json(body): Json<Value>) -> Response {
    provider.calls.fetch_add(1, Ordering::SeqCst);
    let query = body["query"].as_str().unwrap_or_default().to_owned();
    if query.starts_with("hold") {
        provider
            .release
            .acquire()
            .await
            .expect("the release semaphore stays open")
            .forget();
    }
    if query.starts_with("fail") {
        return (StatusCode::INTERNAL_SERVER_ERROR, "provider internal").into_response();
    }
    if query.starts_with("throttle") {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", "1")],
            "slow down",
        )
            .into_response();
    }
    let documents = body["documents"].as_array().map_or(0, Vec::len);
    let top_n = body["top_n"].as_u64().map_or(documents, |n| n as usize);
    let results: Vec<Value> = (0..documents.min(top_n))
        .map(|index| json!({ "index": index, "relevance_score": 1.0 - index as f64 * 0.01 }))
        .collect();
    Json(json!({ "results": results, "meta": { "billed_units": { "search_units": 1 } } }))
        .into_response()
}

async fn start_provider() -> (String, Provider) {
    let provider = Provider {
        calls: Arc::new(AtomicUsize::new(0)),
        release: Arc::new(Semaphore::new(0)),
    };
    let app = Router::new()
        .route("/v2/rerank", post(answer))
        .with_state(provider.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the provider stand-in");
    let addr: SocketAddr = listener.local_addr().expect("local address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/v2/rerank"), provider)
}

async fn wait_for_calls(provider: &Provider, count: usize) {
    for _ in 0..500 {
        if provider.calls.load(Ordering::SeqCst) >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the provider never received {count} calls");
}

fn texts(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("candidate {index}"))
        .collect()
}

fn tenant_candidates(count: usize) -> Vec<ScoredCandidate> {
    (0..count)
        .map(|index| ScoredCandidate {
            knowledge_id: format!("k{index}"),
            document_id: format!("d{index}"),
            text: format!("tenant text {index}"),
            dense_score: 0.5,
            sparse_score: 0.5,
            rerank_score: 0.0,
            final_score: 0.5,
            chunk_index: 0,
            metadata: Default::default(),
        })
        .collect()
}

fn web_client(url: &str, gate: &Arc<ProviderGate>) -> RerankClient {
    RerankClient::for_route(reqwest::Client::builder(), "web-key", "model", url, false)
        .expect("web client")
        .with_gate(gate.clone())
}

#[tokio::test]
async fn tenant_retrieval_keeps_its_reserved_permits_while_web_is_saturated() {
    let (url, provider) = start_provider().await;
    let gate = ProviderGate::new(ProviderLimits {
        max_inflight: 4,
        requests_per_second: 1_000,
        tenant_wait: Duration::from_secs(1),
        breaker: BreakerPolicy::default(),
    });
    let web = web_client(&url, &gate);
    let tenant =
        RerankClient::with_endpoint("tenant-key", "model", &url, false).with_gate(gate.clone());

    // Web saturates its share: two calls held open by the provider.
    let held: Vec<_> = (0..2)
        .map(|index| {
            let web = web.clone();
            tokio::spawn(async move {
                web.rerank_texts(
                    &format!("hold-{index}"),
                    &texts(3),
                    2,
                    Duration::from_secs(20),
                )
                .await
            })
        })
        .collect();
    wait_for_calls(&provider, 2).await;

    // A third web call is refused at once, and the provider never sees it.
    let refused = web
        .rerank_texts("web-three", &texts(3), 2, Duration::from_secs(20))
        .await;
    assert_eq!(refused.err(), Some(RerankError::RateLimited));
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        2,
        "the refused web call made no provider call"
    );

    // Tenant retrieval still gets a permit at once, and its call reaches the provider.
    let started = std::time::Instant::now();
    let answered = tenant
        .rerank("tenant query", &tenant_candidates(3), 2)
        .await
        .expect("tenant retrieval is admitted");
    assert!(
        started.elapsed() < Duration::from_millis(900),
        "tenant retrieval did not wait for a permit"
    );
    assert_eq!(answered.len(), 2);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 3);

    provider.release.add_permits(2);
    for handle in held {
        assert!(
            handle.await.expect("join").is_ok(),
            "the held web calls complete"
        );
    }
}

#[tokio::test]
async fn a_streak_of_provider_failures_opens_the_breaker_and_web_is_refused_without_a_call() {
    let (url, provider) = start_provider().await;
    let gate = ProviderGate::new(limits(8, 1_000));
    let web = web_client(&url, &gate);

    for _ in 0..5 {
        assert_eq!(
            web.rerank_texts("fail-now", &texts(2), 2, Duration::from_secs(20))
                .await
                .err(),
            Some(RerankError::Provider(500))
        );
    }
    assert!(
        gate.is_open(),
        "five failed provider calls open the breaker"
    );

    let before = provider.calls.load(Ordering::SeqCst);
    assert_eq!(
        web.rerank_texts("web-after", &texts(2), 2, Duration::from_secs(20))
            .await
            .err(),
        Some(RerankError::CircuitOpen)
    );
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        before,
        "no provider call while the breaker is open"
    );
}

#[tokio::test]
async fn a_successful_web_call_returns_its_permits_and_the_budget_is_spent_per_call() {
    let (url, provider) = start_provider().await;
    let gate = ProviderGate::new(limits(3, 2));
    let web = web_client(&url, &gate);
    // One web share permit (3 - 2 = 1) and a burst of two: two calls in a row succeed.
    let first = web
        .rerank_texts("first", &texts(2), 2, Duration::from_secs(20))
        .await
        .expect("first call");
    assert_eq!(first.scores.len(), 2);
    let second = web
        .rerank_texts("second", &texts(2), 2, Duration::from_secs(20))
        .await
        .expect("second call");
    assert_eq!(second.scores.len(), 2);
    // The third is past the budget: refused with no provider call.
    assert_eq!(
        web.rerank_texts("third", &texts(2), 2, Duration::from_secs(20))
            .await
            .err(),
        Some(RerankError::RateLimited)
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_web_retry_after_a_provider_429_needs_a_token_of_its_own() {
    let (url, provider) = start_provider().await;
    // One request a second, so one token: the first call takes it, and its retry has none.
    let gate = ProviderGate::new(limits(8, 1));
    let web = web_client(&url, &gate);
    assert_eq!(
        web.rerank_texts("throttle-now", &texts(2), 2, Duration::from_secs(20))
            .await
            .err(),
        Some(RerankError::RateLimited)
    );
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        1,
        "the retry that Retry-After asked for is not sent without a token"
    );
}

#[tokio::test]
async fn a_tenant_call_that_finds_the_pool_full_is_never_sent() {
    let (url, provider) = start_provider().await;
    // Two permits and no web share: the tenant pool is the whole of it, and it is full.
    let gate = ProviderGate::new(limits(2, 1_000));
    let held_a = gate.admit_tenant().await.expect("first permit");
    let held_b = gate.admit_tenant().await.expect("second permit");
    let tenant =
        RerankClient::with_endpoint("tenant-key", "model", &url, false).with_gate(gate.clone());

    let error = match tenant
        .rerank("tenant query", &tenant_candidates(3), 2)
        .await
    {
        Ok(_) => panic!("a full pool must refuse the tenant call"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("tenant_wait_expired"), "{error}");
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        0,
        "a refused tenant call makes no provider call"
    );
    drop((held_a, held_b));
}

// QB-T05b: the provider settings are read from their documented environment names, and a
// malformed one is an error whose fallback is the safe side of every limit.
fn config_from(pairs: &[(&str, &str)]) -> Config {
    let mut env: Vec<(String, String)> = vec![
        ("DATABASE_URL".to_owned(), "postgres://unused".to_owned()),
        ("QDRANT_URL".to_owned(), "http://unused".to_owned()),
    ];
    env.extend(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );
    envy::from_iter(env).expect("the engine's configuration deserialises")
}

#[test]
fn the_provider_settings_are_read_from_their_documented_environment_names() {
    let cfg = config_from(&[
        ("RERANK_PROVIDER_MAX_INFLIGHT", "8"),
        ("RERANK_PROVIDER_RPS", "5"),
        ("RERANK_PROVIDER_TENANT_WAIT_MS", "250"),
        ("RERANK_BREAKER_FAILURES", "3"),
        ("RERANK_BREAKER_WINDOW_SECS", "20"),
        ("RERANK_BREAKER_OPEN_SECS", "45"),
    ]);
    let limits = cfg.provider_limits().expect("valid limits");
    assert_eq!(limits.max_inflight, 8);
    assert_eq!(limits.requests_per_second, 5);
    assert_eq!(limits.tenant_wait, Duration::from_millis(250));
    assert_eq!(
        limits.breaker,
        BreakerPolicy {
            failures: 3,
            window: Duration::from_secs(20),
            open_for: Duration::from_secs(45),
        }
    );
    assert!(limits.web_capacity());
}

#[test]
fn unset_provider_settings_give_the_safe_defaults() {
    let cfg = config_from(&[]);
    let limits = cfg.provider_limits().expect("the defaults are valid");
    assert_eq!(limits, ProviderLimits::default());
    assert_eq!(limits.max_inflight, 0, "tenant retrieval unbounded");
    assert!(
        !limits.web_capacity(),
        "no web capacity until both limits are set"
    );
}

#[test]
fn a_malformed_provider_setting_is_an_error_not_a_boot_failure() {
    let cfg = config_from(&[("RERANK_PROVIDER_RPS", "fast")]);
    assert!(cfg.provider_limits().is_err());
    // The caller falls back to the defaults, and the engine still deserialised.
    assert_eq!(ProviderLimits::default().requests_per_second, 0);
}
