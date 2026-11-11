//! The model entitlement enforcement point (WS10-S3): the gate that pulls an
//! organisation's policy from org-core (a wiremock stands in), and the provider
//! chain that applies it.
//!
//! Every test here runs against the real gate and the real `FallbackChain`; only
//! the providers and org-core are doubles.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use inference_core::config::EntitlementConfig;
use inference_core::entitlement::{EntitlementGate, RequestAccess, Resolved};
use inference_core::provider::fallback::{FallbackChain, ProviderRouterDyn};
use inference_core::provider::role::{RoleCall, ROLE_MODEL_ID};
use inference_core::provider::routing_policy::{RoleEntry, RolePurpose, RoutingPolicy};
use inference_core::provider::{
    Bearer, ChatMessage, EmbedRequest, EmbedResponse, InferChunk, InferRequest, InferResponse,
    ModelFamily, ModelInfo, ProviderCapabilities, ProviderError,
};
use mp_contracts::model_entitlement::{Mode, Reason, ResolvedAccess};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ORG: &str = "org-1";
const USER: &str = "user-1";

// ---------------------------------------------------------------------------
// Doubles
// ---------------------------------------------------------------------------

/// A provider that records the models it was asked for.
struct Recording {
    family: ModelFamily,
    zdr: bool,
    seen: Arc<Mutex<Vec<String>>>,
    models: Vec<(&'static str, &'static str)>,
}

impl Recording {
    fn openai(seen: &Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            family: ModelFamily::OpenAiCompatible,
            zdr: false,
            seen: Arc::clone(seen),
            models: vec![("gpt-5", "chat"), ("gpt-5-mini", "chat"), ("text-embed", "embedding")],
        }
    }

    fn anthropic(seen: &Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            family: ModelFamily::Anthropic,
            zdr: false,
            seen: Arc::clone(seen),
            models: vec![("claude-sonnet-4-5", "chat")],
        }
    }
}

#[async_trait::async_trait]
impl ProviderRouterDyn for Recording {
    async fn infer_dyn(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        self.seen.lock().unwrap().push(req.model.clone());
        Ok(InferResponse {
            request_id: req.request_id.clone(),
            content: format!("answer from {}", req.model),
            model_used: req.model.clone(),
            stop_reason: "end_turn".to_owned(),
            input_tokens: 3,
            output_tokens: 2,
            ..Default::default()
        })
    }

    async fn infer_stream_dyn(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        self.seen.lock().unwrap().push(req.model.clone());
        let (tx, rx) = mpsc::channel(1);
        let chunk = InferChunk {
            compaction_summary: String::new(),
            reasoning_delta: String::new(),
            request_id: req.request_id.clone(),
            delta: "streamed".to_owned(),
            done: true,
            model_used: req.model.clone(),
            input_tokens: 0,
            output_tokens: 0,
            stop_reason: "end_turn".to_owned(),
            provider_used: String::new(),
            residency: String::new(),
            token_confidence: None,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            provider_timing: None,
            service_tier_used: String::new(),
        };
        tokio::spawn(async move {
            let _ = tx.send(chunk).await;
        });
        Ok(rx)
    }

    async fn create_embedding_dyn(&self, _: &EmbedRequest) -> Result<EmbedResponse, ProviderError> {
        Err(ProviderError::UnsupportedModel("embedding".to_owned()))
    }

    fn list_models_dyn(&self) -> Vec<ModelInfo> {
        self.models
            .iter()
            .map(|(id, modality)| ModelInfo {
                id: (*id).to_owned(),
                provider: String::new(),
                modality: (*modality).to_owned(),
                streaming: true,
                ..Default::default()
            })
            .collect()
    }

    fn capabilities_dyn(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            model_family: self.family,
            supports_zdr: self.zdr,
            ..ProviderCapabilities::default()
        }
    }
}

fn chain(seen: &Arc<Mutex<Vec<String>>>) -> FallbackChain {
    FallbackChain::new_with_providers(
        vec![
            ("anthropic".to_owned(), Arc::new(Recording::anthropic(seen))),
            ("openai".to_owned(), Arc::new(Recording::openai(seen))),
        ],
        1,
    )
}

fn request(model: &str) -> InferRequest {
    InferRequest {
        request_id: "req-1".to_owned(),
        model: model.to_owned(),
        org_id: ORG.to_owned(),
        user_id: USER.to_owned(),
        messages: vec![ChatMessage {
            compaction_summary: String::new(),
            role: "user".to_owned(),
            content: "hello".to_owned(),
            name: String::new(),
        }],
        ..Default::default()
    }
}

fn policy_body(state: &str, policy: &Value) -> Value {
    json!({
        "state": state,
        "revision": 4,
        "role": "member",
        "can_manage": false,
        "policy": policy,
        "sources": [{"layer": "org_default"}],
    })
}

fn listed(allow: &[&str], deny: &[&str]) -> Value {
    json!({"allow_mode": "listed", "allow": allow, "deny": deny})
}

fn open(deny: &[&str]) -> Value {
    json!({"allow_mode": "all", "allow": [], "deny": deny})
}

fn access_of(mode: Mode, policy: &Value) -> Arc<RequestAccess> {
    let resolved = ResolvedAccess::from_body(&policy_body("org", policy)).expect("a readable body");
    Arc::new(RequestAccess::new(Arc::new(resolved), mode, ORG))
}

fn with_access(mut request: InferRequest, access: &Arc<RequestAccess>) -> InferRequest {
    request.access = Some(Arc::clone(access));
    request
}

fn not_entitled(error: ProviderError) -> (String, Reason) {
    match error {
        ProviderError::ModelNotEntitled { model, reason } => (model, reason),
        other => panic!("expected ModelNotEntitled, got {other:?}"),
    }
}

fn seen(seen: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    seen.lock().unwrap().clone()
}

// ---------------------------------------------------------------------------
// The chain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_pinned_model_the_policy_denies_is_refused_before_any_provider_is_called() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let chain = chain(&calls);
    let access = access_of(Mode::Enforce, &open(&["gpt-5"]));

    let error = chain
        .infer(&with_access(request("gpt-5"), &access))
        .await
        .expect_err("a denied model is refused");
    assert_eq!(not_entitled(error), ("gpt-5".to_owned(), Reason::ExplicitlyDenied));

    let error = chain
        .infer_stream(&with_access(request("gpt-5"), &access))
        .await
        .expect_err("a denied model is refused on the stream path too");
    assert_eq!(not_entitled(error), ("gpt-5".to_owned(), Reason::ExplicitlyDenied));

    assert!(seen(&calls).is_empty(), "no provider was called");
}

#[tokio::test]
async fn a_denied_model_is_refused_even_when_its_answer_is_cached() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let chain = chain(&calls);

    // Filled while the model was allowed.
    let first = chain.infer(&request("gpt-5")).await.expect("served");
    assert_eq!(seen(&calls), ["gpt-5"]);

    // The policy now denies it: the same prompt is refused, not answered from
    // the cache that the earlier call filled.
    let access = access_of(Mode::Enforce, &open(&["gpt-5"]));
    let error = chain
        .infer(&with_access(request("gpt-5"), &access))
        .await
        .expect_err("refused before the cache");
    assert_eq!(not_entitled(error).1, Reason::ExplicitlyDenied);
    assert_eq!(seen(&calls), ["gpt-5"], "and no second provider call");

    // An allowed caller still gets the cached answer, so the cache is not
    // simply off.
    let allowed = access_of(Mode::Enforce, &open(&[]));
    let again = chain
        .infer(&with_access(request("gpt-5"), &allowed))
        .await
        .expect("served from cache");
    assert_eq!(again.content, first.content);
    assert_eq!(seen(&calls), ["gpt-5"]);
}

#[tokio::test]
async fn a_cached_answer_of_a_model_that_was_revoked_is_not_served_to_an_automatic_request() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let chain = chain(&calls);

    // An unpinned request: served by the first provider's default.
    chain.infer(&request("")).await.expect("served");
    let served_by = seen(&calls)[0].clone();

    // The policy now denies that default. The cache key does not know the
    // policy, so the stored answer must be checked against it.
    let access = access_of(Mode::Enforce, &open(&[served_by.as_str()]));
    chain
        .infer(&with_access(request(""), &access))
        .await
        .expect("served by the next provider");
    let calls = seen(&calls);
    assert_eq!(calls.len(), 2, "the revoked model's cached answer was not served");
    assert_ne!(calls[1], served_by);
}

#[tokio::test]
async fn an_automatic_request_skips_a_denied_provider_default_and_uses_the_next() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let chain = chain(&calls);
    // `anthropic` is first in the chain; its default is a claude model.
    let access = access_of(Mode::Enforce, &open(&["provider:anthropic"]));

    let response = chain
        .infer(&with_access(request(""), &access))
        .await
        .expect("the next provider serves it");
    assert_eq!(response.provider_used, "openai");
    assert_eq!(seen(&calls).len(), 1);
    assert!(!seen(&calls)[0].starts_with("claude"));
}

#[tokio::test]
async fn an_automatic_request_with_no_entitled_route_is_refused_as_no_entitled_route() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let chain = chain(&calls);
    let access = access_of(Mode::Enforce, &listed(&["some-other-model"], &[]));

    let error = chain
        .infer(&with_access(request(""), &access))
        .await
        .expect_err("nothing is entitled");
    assert_eq!(not_entitled(error), (String::new(), Reason::NoEntitledRoute));
    let error = chain
        .infer_stream(&with_access(request(""), &access))
        .await
        .expect_err("nothing is entitled on the stream path");
    assert_eq!(not_entitled(error), (String::new(), Reason::NoEntitledRoute));
    assert!(seen(&calls).is_empty());
}

#[tokio::test]
async fn a_deny_wins_over_an_allow_and_a_provider_selector_admits_its_models() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let chain = chain(&calls);

    let both = access_of(Mode::Enforce, &listed(&["gpt-5"], &["gpt-5"]));
    let error = chain
        .infer(&with_access(request("gpt-5"), &both))
        .await
        .expect_err("deny wins");
    assert_eq!(not_entitled(error).1, Reason::ExplicitlyDenied);

    let by_provider = access_of(Mode::Enforce, &listed(&["provider:openai"], &[]));
    chain
        .infer(&with_access(request("gpt-5-mini"), &by_provider))
        .await
        .expect("a provider selector admits every model of that provider");
    let error = chain
        .infer(&with_access(request("claude-sonnet-4-5"), &by_provider))
        .await
        .expect_err("and none of another provider's");
    assert_eq!(not_entitled(error), ("claude-sonnet-4-5".to_owned(), Reason::NotInAllowList));
    assert_eq!(seen(&calls), ["gpt-5-mini"]);
}

#[tokio::test]
async fn role_candidate_not_entitled_is_skipped() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let chain = chain(&calls);
    let mut policy = RoutingPolicy::default();
    policy.roles.insert(
        RolePurpose::Title,
        RoleEntry {
            candidates: vec!["gpt-5".to_owned(), "claude-sonnet-4-5".to_owned()],
            effort: None,
        },
    );
    chain.policy_handle().store(Arc::new(policy));
    let access = access_of(Mode::Enforce, &open(&["gpt-5"]));

    let (response, _) = chain
        .infer_role_noted(
            &with_access(request(ROLE_MODEL_ID), &access),
            &RoleCall {
                purpose: Some(RolePurpose::Title),
                fallback_model: String::new(),
            },
            false,
        )
        .await
        .expect("the entitled candidate serves the title");
    assert_eq!(response.model_used, "claude-sonnet-4-5");
    assert_eq!(seen(&calls), vec!["claude-sonnet-4-5".to_owned()]);
}

#[tokio::test]
async fn the_tool_ladder_never_reaches_a_denied_rung() {
    // verevon-budget on a trivial prompt resolves to gpt-5-nano; the ladder
    // behind it is four claude rungs, then gpt-5-mini, then gpt-4o-mini. The
    // policy denies the resolved model, gpt-5-mini and the whole anthropic
    // provider, so the one rung left is gpt-4o-mini: denied rungs do not use up
    // the ladder's four steps.
    let calls = Arc::new(Mutex::new(Vec::new()));
    let chain = chain(&calls).with_intent_enabled(true);
    let access = access_of(
        Mode::Enforce,
        &open(&["gpt-5-nano", "gpt-5-mini", "provider:anthropic"]),
    );

    let response = chain
        .infer(&with_access(request("verevon-budget"), &access))
        .await
        .expect("the one rung left serves it");
    assert_eq!(response.model_used, "gpt-4o-mini");
    assert_eq!(seen(&calls), ["gpt-4o-mini"]);
}

#[tokio::test]
async fn shadow_never_refuses_and_a_request_with_no_policy_is_byte_identical() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let chain = chain(&calls);

    let baseline = chain.infer(&request("gpt-5")).await.expect("served");

    // Shadow: the policy denies the model and the call is served the same.
    let shadow = access_of(Mode::Shadow, &open(&["gpt-5"]));
    let served = chain
        .infer(&with_access(request("gpt-5"), &shadow))
        .await
        .expect("shadow serves it");
    assert_eq!(served.content, baseline.content);
    assert_eq!(served.model_used, baseline.model_used);
    assert_eq!(served.provider_used, baseline.provider_used);

    // Enforce with no policy at all (state none): nothing changes either.
    let none = ResolvedAccess::from_body(&policy_body("none", &Value::Null)).expect("readable");
    let access = Arc::new(RequestAccess::new(Arc::new(none), Mode::Enforce, ORG));
    let served = chain
        .infer(&with_access(request("gpt-5"), &access))
        .await
        .expect("no policy serves it");
    assert_eq!(served.content, baseline.content);
    assert_eq!(served.provider_used, baseline.provider_used);

    // Shadow does not skip an automatic request's denied default either.
    let denies_all = access_of(Mode::Shadow, &listed(&[], &[]));
    chain
        .infer(&with_access(request("gpt-5-mini"), &denies_all))
        .await
        .expect("shadow never refuses");
}

#[tokio::test]
async fn an_entitled_request_names_its_privacy_floor_through_the_existing_gate() {
    let policy = json!({
        "allow_mode": "all", "allow": [], "deny": [],
        "min_privacy_tier": "sovereign",
    });
    assert_eq!(
        access_of(Mode::Enforce, &policy).min_privacy_tier(),
        inference_core::provider::PrivacyTier::Sovereign
    );
    assert_eq!(
        access_of(Mode::Shadow, &policy).min_privacy_tier(),
        inference_core::provider::PrivacyTier::Unspecified,
        "shadow applies nothing"
    );
    let unknown = json!({"allow_mode": "all", "allow": [], "deny": [], "min_privacy_tier": "from_the_future"});
    assert_eq!(
        access_of(Mode::Enforce, &unknown).min_privacy_tier(),
        inference_core::provider::PrivacyTier::Sovereign,
        "a tier this build does not know reads as the strongest"
    );
}

// ---------------------------------------------------------------------------
// The gate and org-core
// ---------------------------------------------------------------------------

/// A clock a test moves by hand.
#[derive(Clone)]
struct TestClock {
    base: Instant,
    offset: Arc<Mutex<Duration>>,
}

impl TestClock {
    fn new() -> Self {
        Self {
            base: Instant::now(),
            offset: Arc::new(Mutex::new(Duration::ZERO)),
        }
    }

    fn advance(&self, by: Duration) {
        *self.offset.lock().unwrap() += by;
    }

    fn as_clock(&self) -> inference_core::entitlement::Clock {
        let this = self.clone();
        Arc::new(move || this.base + *this.offset.lock().unwrap())
    }
}

fn gate(mode: Mode, server: &MockServer, clock: &TestClock) -> EntitlementGate {
    EntitlementGate::with_clock(
        &EntitlementConfig {
            mode,
            org_core_url: Some(server.uri()),
            service_token: Bearer::new("test-token"),
        },
        clock.as_clock(),
    )
}

/// org-core answering the contract's request with `body`.
async fn org_core_says(server: &MockServer, body: Value) {
    server.reset().await;
    Mock::given(method("GET"))
        .and(path(format!("/internal/orgs/{ORG}/model-entitlement")))
        .and(query_param("user_id", USER))
        .and(header("x-service-id", "inference-core"))
        .and(header("x-service-token", "test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn org_core_is_down(server: &MockServer) {
    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(server)
        .await;
}

async fn requests_seen(server: &MockServer) -> usize {
    server.received_requests().await.expect("recording is on").len()
}

async fn answer(gate: &EntitlementGate) -> Result<Resolved, ProviderError> {
    gate.resolve_for(ORG, Some(USER)).await
}

fn access(resolved: Resolved) -> Arc<RequestAccess> {
    match resolved {
        Resolved::Access(access) => access,
        other => panic!("expected an answer to apply, got {other:?}"),
    }
}

#[tokio::test]
async fn off_makes_no_http_call_and_resolves_nothing() {
    let server = MockServer::start().await;
    org_core_says(&server, policy_body("org", &open(&["gpt-5"]))).await;
    let gate = gate(Mode::Off, &server, &TestClock::new());

    assert!(matches!(answer(&gate).await, Ok(Resolved::Off)));
    assert!(matches!(
        gate.resolve_for(ORG, None).await,
        Ok(Resolved::Off)
    ));
    assert_eq!(requests_seen(&server).await, 0);
}

#[tokio::test]
async fn the_pull_names_the_caller_and_carries_the_service_credential() {
    let server = MockServer::start().await;
    org_core_says(&server, policy_body("org", &open(&["gpt-5"]))).await;
    let gate = gate(Mode::Enforce, &server, &TestClock::new());

    let applied = access(answer(&gate).await.expect("resolved"));
    assert_eq!(applied.resolved().revision, 4);
    assert_eq!(
        applied.check("gpt-5", "openai"),
        Err(Reason::ExplicitlyDenied)
    );
    assert!(applied.enforced());
    // The mock only answers a request with the id, the token and the user.
    assert_eq!(requests_seen(&server).await, 1);
}

#[tokio::test]
async fn a_service_principal_is_exempt_and_asks_org_core_nothing() {
    let server = MockServer::start().await;
    org_core_says(&server, policy_body("org", &listed(&[], &[]))).await;
    for mode in [Mode::Shadow, Mode::Enforce] {
        let gate = gate(mode, &server, &TestClock::new());
        assert!(matches!(
            gate.resolve_for(ORG, None).await,
            Ok(Resolved::Exempt)
        ));
    }
    assert_eq!(requests_seen(&server).await, 0);
}

#[tokio::test]
async fn an_answer_is_held_for_the_ttl_then_refreshed() {
    let server = MockServer::start().await;
    let clock = TestClock::new();
    org_core_says(&server, policy_body("org", &open(&["gpt-5"]))).await;
    let gate = gate(Mode::Enforce, &server, &clock);

    let first = access(answer(&gate).await.unwrap());
    assert!(first.check("gpt-5", "openai").is_err());

    // The organisation changes its policy; inside the TTL nobody notices.
    org_core_says(&server, policy_body("org", &open(&[]))).await;
    clock.advance(Duration::from_secs(29));
    let cached = access(answer(&gate).await.unwrap());
    assert!(cached.check("gpt-5", "openai").is_err());
    assert_eq!(requests_seen(&server).await, 0, "served from the cache");

    // Past the TTL the next call reads the new answer.
    clock.advance(Duration::from_secs(2));
    let fresh = access(answer(&gate).await.unwrap());
    assert!(fresh.check("gpt-5", "openai").is_ok());
    assert_eq!(requests_seen(&server).await, 1);
}

#[tokio::test]
async fn concurrent_calls_for_one_caller_share_one_fetch() {
    let server = MockServer::start().await;
    org_core_says(&server, policy_body("org", &open(&[]))).await;
    let gate = Arc::new(gate(Mode::Enforce, &server, &TestClock::new()));

    let calls: Vec<_> = (0..16)
        .map(|_| {
            let gate = Arc::clone(&gate);
            tokio::spawn(async move { gate.resolve_for(ORG, Some(USER)).await })
        })
        .collect();
    for call in calls {
        assert!(matches!(call.await.unwrap(), Ok(Resolved::Access(_))));
    }
    assert_eq!(requests_seen(&server).await, 1);
}

#[tokio::test]
async fn a_restrictive_answer_is_served_stale_for_900_seconds_when_org_core_is_down() {
    let server = MockServer::start().await;
    let clock = TestClock::new();
    org_core_says(&server, policy_body("org", &open(&["gpt-5"]))).await;
    let gate = gate(Mode::Enforce, &server, &clock);
    assert!(access(answer(&gate).await.unwrap())
        .check("gpt-5", "openai")
        .is_err());

    org_core_is_down(&server).await;
    clock.advance(Duration::from_secs(899));
    let stale = access(answer(&gate).await.expect("inside the bound"));
    assert!(stale.check("gpt-5", "openai").is_err(), "the revocation holds");

    // Past the bound: refused, never served unrestricted.
    clock.advance(Duration::from_secs(3));
    assert!(matches!(
        answer(&gate).await,
        Err(ProviderError::EntitlementUnavailable)
    ));
}

#[tokio::test]
async fn an_unrestricted_answer_is_served_stale_for_only_120_seconds() {
    let server = MockServer::start().await;
    let clock = TestClock::new();
    org_core_says(&server, policy_body("none", &Value::Null)).await;
    let gate = gate(Mode::Enforce, &server, &clock);
    assert!(!access(answer(&gate).await.unwrap()).resolved().restricted());

    org_core_is_down(&server).await;
    clock.advance(Duration::from_secs(119));
    assert!(answer(&gate).await.is_ok(), "inside the bound");
    clock.advance(Duration::from_secs(3));
    assert!(matches!(
        answer(&gate).await,
        Err(ProviderError::EntitlementUnavailable)
    ));
}

#[tokio::test]
async fn an_answer_never_seen_refuses_under_enforce_and_is_counted_under_shadow() {
    let server = MockServer::start().await;
    org_core_is_down(&server).await;

    let enforce = gate(Mode::Enforce, &server, &TestClock::new());
    assert!(matches!(
        answer(&enforce).await,
        Err(ProviderError::EntitlementUnavailable)
    ));

    // Shadow never refuses: the call goes on unrestricted.
    let shadow = gate(Mode::Shadow, &server, &TestClock::new());
    assert!(matches!(answer(&shadow).await, Ok(Resolved::Unreadable)));
}

#[tokio::test]
async fn a_body_the_matcher_cannot_read_is_unavailable_not_unrestricted() {
    let server = MockServer::start().await;
    org_core_says(&server, json!({"state": "role", "policy": null})).await;
    let gate = gate(Mode::Enforce, &server, &TestClock::new());
    assert!(matches!(
        answer(&gate).await,
        Err(ProviderError::EntitlementUnavailable)
    ));
}

#[tokio::test]
async fn a_gate_with_no_org_core_to_ask_refuses_under_enforce() {
    let gate = EntitlementGate::new(&EntitlementConfig {
        mode: Mode::Enforce,
        org_core_url: None,
        service_token: Bearer::default(),
    });
    assert!(matches!(
        answer(&gate).await,
        Err(ProviderError::EntitlementUnavailable)
    ));
}

#[test]
fn the_decisions_are_counted_by_decision_and_mode() {
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    metrics::with_local_recorder(&recorder, || {
        runtime.block_on(async {
            let server = MockServer::start().await;
            org_core_says(&server, policy_body("org", &open(&["gpt-5"]))).await;
            let gate = gate(Mode::Shadow, &server, &TestClock::new());
            let applied = access(answer(&gate).await.unwrap());
            // Two checks of a denied model in one call count once.
            assert_eq!(applied.refuses("gpt-5", "openai"), None);
            assert_eq!(applied.refuses("gpt-5", "openai"), None);
            let _ = gate.resolve_for(ORG, None).await;

            let enforce = gate_for_refusal(&server).await;
            let refused = enforce.refusal("gpt-5", Reason::ExplicitlyDenied);
            assert!(matches!(refused, ProviderError::ModelNotEntitled { .. }));
        });
    });
    let scrape = handle.render();
    for series in [
        r#"model_entitlement_total{decision="resolved",mode="shadow"} 1"#,
        r#"model_entitlement_total{decision="would_refuse",mode="shadow"} 1"#,
        r#"model_entitlement_total{decision="service_exempt",mode="shadow"} 1"#,
        r#"model_entitlement_total{decision="refuse",mode="enforce"} 1"#,
    ] {
        assert!(scrape.contains(series), "{series} is missing from:\n{scrape}");
    }
    assert!(
        !scrape.contains(ORG) && !scrape.contains(USER),
        "no organisation or user label"
    );
}

async fn gate_for_refusal(server: &MockServer) -> Arc<RequestAccess> {
    org_core_says(server, policy_body("org", &open(&["gpt-5"]))).await;
    let gate = gate(Mode::Enforce, server, &TestClock::new());
    access(answer(&gate).await.unwrap())
}
