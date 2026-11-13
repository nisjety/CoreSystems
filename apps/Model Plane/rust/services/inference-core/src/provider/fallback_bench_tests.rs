//! Chain benching (R2-OD-40-b3) under `INFERENCE_PROVIDER_BENCHING`.
//!
//! The stubs count their calls, so "no network call" is the stub's own counter
//! staying where it was. Every request gets its own user id, so the response cache
//! never answers one call with another's result.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::provider::SubscriptionRefusal;

/// How a stub answers every call.
#[derive(Clone)]
enum Answer {
    Serve,
    Fail(fn() -> ProviderError),
}

struct BenchStub {
    provider_id: &'static str,
    residency: Residency,
    supports_zdr: bool,
    answer: Answer,
    calls: Arc<AtomicUsize>,
}

fn stub(
    provider_id: &'static str,
    residency: Residency,
    supports_zdr: bool,
    answer: Answer,
) -> (BoxedProvider, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let provider: BoxedProvider = Arc::new(BenchStub {
        provider_id,
        residency,
        supports_zdr,
        answer,
        calls: calls.clone(),
    });
    (provider, calls)
}

fn server_error() -> ProviderError {
    ProviderError::Http("500 Internal Server Error: upstream".to_owned())
}

fn auth_error() -> ProviderError {
    ProviderError::Http("401: invalid api key".to_owned())
}

fn usage_limit() -> ProviderError {
    ProviderError::SubscriptionRefused {
        reason: SubscriptionRefusal::UsageLimit,
        retry_after_ms: None,
        resets_at: None,
    }
}

#[async_trait::async_trait]
impl ProviderRouter for BenchStub {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_id: self.provider_id.to_owned(),
            model_family: ModelFamily::OpenAiCompatible,
            residency: self.residency,
            supports_zdr: self.supports_zdr,
            ..ProviderCapabilities::default()
        }
    }

    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.answer {
            Answer::Serve => Ok(InferResponse {
                request_id: req.request_id.clone(),
                content: "ok".to_owned(),
                model_used: req.model.clone(),
                stop_reason: "stop".to_owned(),
                ..InferResponse::default()
            }),
            Answer::Fail(error) => Err(error()),
        }
    }

    async fn infer_stream(
        &self,
        _req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.answer {
            Answer::Serve => {
                let (_tx, rx) = mpsc::channel(1);
                Ok(rx)
            }
            Answer::Fail(error) => Err(error()),
        }
    }
}

static NEXT_USER: AtomicUsize = AtomicUsize::new(0);

fn request(org: &str) -> InferRequest {
    let user = NEXT_USER.fetch_add(1, Ordering::SeqCst);
    InferRequest {
        request_id: format!("bench-{user}"),
        org_id: org.to_owned(),
        user_id: format!("user-{user}"),
        model: "gpt-4o-mini".to_owned(),
        ..Default::default()
    }
}

fn calls(counter: &Arc<AtomicUsize>) -> usize {
    counter.load(Ordering::SeqCst)
}

/// Switch off: a provider that fails every call is asked on every call, from any number of orgs.
#[tokio::test]
async fn switch_off_never_benches() {
    let (failing, failing_calls) = stub(
        "azure-openai",
        Residency::Eu,
        false,
        Answer::Fail(server_error),
    );
    let chain = FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), failing)], 2)
        .with_benching(false);
    for org in ["org-a", "org-b", "org-a", "org-b", "org-c"] {
        let error = chain.infer(&request(org)).await.unwrap_err();
        assert!(
            !matches!(error, ProviderError::ProvidersBenched { .. }),
            "{error:?}"
        );
    }
    assert_eq!(
        calls(&failing_calls),
        10,
        "two attempts on every call, never benched"
    );
    assert!(chain.bench.is_none());
}

/// Three server failures from two orgs bench the provider; the next call skips it to a same-tier
/// provider without calling it.
#[tokio::test]
async fn three_server_failures_from_two_orgs_bench_and_the_next_call_skips() {
    let (failing, failing_calls) = stub(
        "azure-openai",
        Residency::Eu,
        false,
        Answer::Fail(server_error),
    );
    let (serving, serving_calls) = stub("mistral-eu", Residency::Eu, false, Answer::Serve);
    let chain = FallbackChain::new_with_providers(
        vec![
            ("azure-openai".to_owned(), failing),
            ("mistral-eu".to_owned(), serving),
        ],
        1,
    )
    .with_benching(true);
    for org in ["org-a", "org-b", "org-a"] {
        chain
            .infer(&request(org))
            .await
            .expect("the second provider serves");
    }
    assert_eq!(calls(&failing_calls), 3);
    chain.infer(&request("org-c")).await.expect("served");
    assert_eq!(
        calls(&failing_calls),
        3,
        "the benched provider is not called"
    );
    assert_eq!(calls(&serving_calls), 4);
}

/// Three failures from one org do not bench: one tenant cannot bench a platform provider.
#[tokio::test]
async fn three_server_failures_from_one_org_do_not_bench() {
    let (failing, failing_calls) = stub(
        "azure-openai",
        Residency::Eu,
        false,
        Answer::Fail(server_error),
    );
    let (serving, _) = stub("mistral-eu", Residency::Eu, false, Answer::Serve);
    let chain = FallbackChain::new_with_providers(
        vec![
            ("azure-openai".to_owned(), failing),
            ("mistral-eu".to_owned(), serving),
        ],
        1,
    )
    .with_benching(true);
    for _ in 0..4 {
        chain.infer(&request("org-a")).await.expect("served");
    }
    assert_eq!(calls(&failing_calls), 4, "still asked on every call");
}

/// Every eligible provider benched: `ProvidersBenched` with a retry-after, and no network call.
#[tokio::test]
async fn every_eligible_provider_benched_answers_providers_benched_without_a_call() {
    let (failing, failing_calls) = stub(
        "azure-openai",
        Residency::Eu,
        false,
        Answer::Fail(auth_error),
    );
    let chain = FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), failing)], 3)
        .with_benching(true);
    let first = chain.infer(&request("org-a")).await.unwrap_err();
    assert!(
        !matches!(first, ProviderError::ProvidersBenched { .. }),
        "an attempt was made: {first:?}"
    );
    assert_eq!(
        calls(&failing_calls),
        1,
        "an auth failure benches at once: no more retries"
    );

    let error = chain.infer(&request("org-b")).await.unwrap_err();
    let ProviderError::ProvidersBenched { retry_after_ms } = error else {
        panic!("expected providers_benched, got {error:?}");
    };
    assert!((1..=30_000).contains(&retry_after_ms), "{retry_after_ms}");
    assert_eq!(calls(&failing_calls), 1, "no network call");
}

/// A ZDR request whose only attested provider is benched answers `ProvidersBenched`: never
/// `zdr_unavailable`, and never a non-attested provider.
#[tokio::test]
async fn a_zdr_request_with_its_attested_provider_benched_is_providers_benched() {
    let (attested, attested_calls) = stub(
        "azure-openai",
        Residency::Eu,
        true,
        Answer::Fail(auth_error),
    );
    let (plain, plain_calls) = stub("openai", Residency::Global, false, Answer::Serve);
    let chain = FallbackChain::new_with_providers(
        vec![
            ("azure-openai".to_owned(), attested),
            ("openai".to_owned(), plain),
        ],
        1,
    )
    .with_benching(true);
    let mut zdr = request("org-a");
    zdr.zdr = true;
    chain.infer(&zdr).await.unwrap_err();
    assert_eq!(calls(&attested_calls), 1);

    let mut zdr = request("org-b");
    zdr.zdr = true;
    let error = chain.infer(&zdr).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::ProvidersBenched { .. }),
        "{error:?}"
    );
    assert_eq!(calls(&attested_calls), 1);
    assert_eq!(calls(&plain_calls), 0, "never a non-attested provider");
}

/// A tier-floored and a residency-floored request whose only eligible provider is benched answer
/// `ProvidersBenched`.
#[tokio::test]
async fn tier_and_residency_floored_requests_with_their_provider_benched_are_providers_benched() {
    let (eu, eu_calls) = stub(
        "azure-openai",
        Residency::Eu,
        false,
        Answer::Fail(auth_error),
    );
    let (global, global_calls) = stub("openai", Residency::Global, false, Answer::Serve);
    let chain = FallbackChain::new_with_providers(
        vec![
            ("azure-openai".to_owned(), eu),
            ("openai".to_owned(), global),
        ],
        1,
    )
    .with_benching(true);
    let mut floored = request("org-a");
    floored.min_privacy_tier = PrivacyTier::EuResident;
    chain.infer(&floored).await.unwrap_err();
    assert_eq!(calls(&eu_calls), 1);

    let mut tier = request("org-b");
    tier.min_privacy_tier = PrivacyTier::EuResident;
    let error = chain.infer(&tier).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::ProvidersBenched { .. }),
        "tier: {error:?}"
    );

    let mut residency = request("org-c");
    residency.min_residency = "eu".to_owned();
    let error = chain.infer(&residency).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::ProvidersBenched { .. }),
        "residency: {error:?}"
    );

    assert_eq!(calls(&eu_calls), 1, "no network call while benched");
    assert_eq!(calls(&global_calls), 0, "the floor never drops to GLOBAL");
}

/// A request with no eligible provider at all still answers the posture refusals, as today.
#[tokio::test]
async fn no_eligible_provider_still_answers_the_posture_refusal() {
    let (global, global_calls) = stub("openai", Residency::Global, false, Answer::Serve);
    let chain = FallbackChain::new_with_providers(vec![("openai".to_owned(), global)], 1)
        .with_benching(true);
    let mut zdr = request("org-a");
    zdr.zdr = true;
    let error = chain.infer(&zdr).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::ZdrUnavailable(_)),
        "{error:?}"
    );
    let mut tier = request("org-a");
    tier.min_privacy_tier = PrivacyTier::EuResident;
    let error = chain.infer(&tier).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::TierUnavailable(_)),
        "{error:?}"
    );
    let mut residency = request("org-a");
    residency.min_residency = "eu".to_owned();
    let error = chain.infer(&residency).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::ResidencyViolation(_)),
        "{error:?}"
    );
    assert_eq!(calls(&global_calls), 0);
}

/// A subscription refusal benches nothing: neither on the subscription route nor when a platform
/// provider answers with one.
#[tokio::test]
async fn a_subscription_refusal_benches_nothing() {
    let id = mp_contracts::agentic_subscription::PROVIDER_ID;
    let (subscription, subscription_calls) =
        stub(id, Residency::Eu, false, Answer::Fail(usage_limit));
    let chain = FallbackChain::new_with_providers(vec![(id.to_owned(), subscription)], 1)
        .with_benching(true);
    for org in ["org-a", "org-b", "org-c"] {
        let mut pinned = request(org);
        pinned.provider_hint = id.to_owned();
        pinned.subscription_connection_id = "connection".to_owned();
        let error = chain.infer(&pinned).await.unwrap_err();
        assert!(
            matches!(error, ProviderError::SubscriptionRefused { .. }),
            "{error:?}"
        );
    }
    assert_eq!(
        calls(&subscription_calls),
        3,
        "asked every time: nothing benched"
    );

    let (platform, platform_calls) = stub(
        "azure-openai",
        Residency::Eu,
        false,
        Answer::Fail(usage_limit),
    );
    let chain = FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), platform)], 1)
        .with_benching(true);
    for org in ["org-a", "org-b", "org-c"] {
        chain.infer(&request(org)).await.unwrap_err();
    }
    assert_eq!(calls(&platform_calls), 3);
    let board = chain.bench.as_ref().expect("benching on");
    assert_eq!(
        board.benched_until("azure-openai", "gpt-4o-mini", tokio::time::Instant::now()),
        None
    );
}

/// The stream walk gives the same answers: it benches, skips a benched provider without a call, and
/// answers `ProvidersBenched` when every eligible provider is benched.
#[tokio::test]
async fn the_stream_walk_benches_the_same_way() {
    let (failing, failing_calls) = stub(
        "azure-openai",
        Residency::Eu,
        true,
        Answer::Fail(auth_error),
    );
    let (plain, plain_calls) = stub("openai", Residency::Global, false, Answer::Serve);
    let chain = FallbackChain::new_with_providers(
        vec![
            ("azure-openai".to_owned(), failing),
            ("openai".to_owned(), plain),
        ],
        2,
    )
    .with_benching(true);
    let mut zdr = request("org-a");
    zdr.zdr = true;
    chain.infer_stream(&zdr).await.unwrap_err();
    assert_eq!(calls(&failing_calls), 1, "an auth failure benches at once");

    let mut zdr = request("org-b");
    zdr.zdr = true;
    let error = chain.infer_stream(&zdr).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::ProvidersBenched { .. }),
        "{error:?}"
    );
    assert_eq!(calls(&failing_calls), 1, "no network call");
    assert_eq!(calls(&plain_calls), 0, "never a non-attested provider");

    // An unfloored stream skips the benched provider to the next one.
    chain
        .infer_stream(&request("org-c"))
        .await
        .expect("the next provider streams");
    assert_eq!(calls(&failing_calls), 1);
    assert_eq!(calls(&plain_calls), 1);
}

/// A success clears the run: after a success, the old failures no longer count towards a bench.
#[tokio::test]
async fn a_success_through_the_chain_clears_the_run() {
    use std::sync::atomic::AtomicBool;
    struct Flaky {
        fail: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl ProviderRouter for Flaky {
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                provider_id: "azure-openai".to_owned(),
                model_family: ModelFamily::OpenAiCompatible,
                residency: Residency::Eu,
                ..ProviderCapabilities::default()
            }
        }
        async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                return Err(server_error());
            }
            Ok(InferResponse {
                request_id: req.request_id.clone(),
                content: "ok".to_owned(),
                model_used: req.model.clone(),
                ..InferResponse::default()
            })
        }
        async fn infer_stream(
            &self,
            _req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            Err(server_error())
        }
    }
    let fail = Arc::new(AtomicBool::new(true));
    let calls_counter = Arc::new(AtomicUsize::new(0));
    let flaky: BoxedProvider = Arc::new(Flaky {
        fail: fail.clone(),
        calls: calls_counter.clone(),
    });
    let chain = FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), flaky)], 1)
        .with_benching(true);
    chain.infer(&request("org-a")).await.unwrap_err();
    chain.infer(&request("org-b")).await.unwrap_err();
    fail.store(false, Ordering::SeqCst);
    chain.infer(&request("org-a")).await.expect("served");
    fail.store(true, Ordering::SeqCst);
    chain.infer(&request("org-b")).await.unwrap_err();
    // Two failures since the success: not benched, so the next call is still tried.
    chain.infer(&request("org-a")).await.unwrap_err();
    assert_eq!(calls(&calls_counter), 5);
}
