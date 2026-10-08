//! The metered call path under the real rules.
//!
//! Only the outside world is replaced, by local servers: the JWKS endpoint and
//! the Azure `OpenAI` resource. Everything between is the code that runs in the
//! container. A real [`JwtVerifier`] verifies real RS256 tokens, the real
//! [`InferenceService`] authorizes and routes them, [`FallbackChain::from_config`]
//! builds the real Azure provider from a real [`InferenceConfig`] (attested for
//! ZDR through the real [`ZdrAttestation`] validation, or not attested at all),
//! the metering decorator counts, and the assertions read the rendered
//! Prometheus text, not a recorder double.
//!
//! The proof that a refusal happens before any provider is called is the Azure
//! server's own request log: it stays empty.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use metrics_exporter_prometheus::PrometheusBuilder;
use rand::thread_rng;
use rsa::pkcs8::{EncodePrivateKey, LineEnding};
use rsa::traits::PublicKeyParts;
use serde_json::json;
use tokio_stream::StreamExt;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::auth::AuthConfig;
use crate::config::InferenceConfig;
use crate::metered::MeteredCore;
use crate::provider::zdr::{canonical_digest, RawAttestation, ZdrAttestation};

// ---------------------------------------------------------------------------
// Real credentials
// ---------------------------------------------------------------------------

fn rsa_keypair() -> &'static (String, String, String) {
    static KEYS: OnceLock<(String, String, String)> = OnceLock::new();
    KEYS.get_or_init(|| {
        let private = rsa::RsaPrivateKey::new(&mut thread_rng(), 2048).expect("RSA key");
        let public = rsa::RsaPublicKey::from(&private);
        let private_pem = private
            .to_pkcs8_pem(LineEnding::LF)
            .expect("private PEM")
            .to_string();
        let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.n().to_bytes_be());
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.e().to_bytes_be());
        (private_pem, n, e)
    })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

const KID: &str = "metering-contract-key";

/// A service-principal token shaped exactly as Auth Core mints one: `sub` and
/// `service_id` both `service:<id>`, no user, a reason, the audience of this
/// service. `zdr` is the issuer's ZDR claim.
fn service_token(service: &str, org: &str, scopes: &[&str], zdr: bool) -> String {
    let id = format!("service:{service}");
    let claims = json!({
        "sub": id, "iss": "auth-core", "aud": "inference-core",
        "iat": now(), "nbf": now() - 5, "exp": now() + 3600,
        "org_id": org, "principal_type": "service", "service_id": id,
        "reason": "metering contract test", "scopes": scopes, "zdr": zdr,
    });
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.to_owned());
    encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(rsa_keypair().0.as_bytes()).expect("encoding key"),
    )
    .expect("JWT")
}

fn bearer<T>(token: &str, message: T) -> Request<T> {
    let mut request = Request::new(message);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {token}").parse().expect("metadata"),
    );
    request
}

async fn verifier() -> (JwtVerifier, MockServer) {
    let server = MockServer::start().await;
    let (_, n, e) = rsa_keypair();
    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keys": [{ "kty": "RSA", "use": "sig", "alg": "RS256", "kid": KID, "n": n, "e": e }]
        })))
        .mount(&server)
        .await;
    let verifier = JwtVerifier::from_config(AuthConfig {
        jwks_url: format!("{}/jwks", server.uri()),
        issuer: "auth-core".to_owned(),
        audience: "inference-core".to_owned(),
        jwks_ttl: Duration::from_mins(5),
        leeway_secs: 0,
    })
    .await
    .expect("verifier");
    (verifier, server)
}

// ---------------------------------------------------------------------------
// The real provider configuration
// ---------------------------------------------------------------------------

/// The operator attestation, validated by the real `ZdrAttestation::validate`
/// with the shared canonical digest.
fn attestation() -> ZdrAttestation {
    let mut raw = RawAttestation {
        resource_id: "/subscriptions/abc/rg/eu/foundry-openai".to_owned(),
        approval_ref: "MAM-2026-0042".to_owned(),
        effective_date: "2026-06-01".to_owned(),
        review_by: "2027-06-01".to_owned(),
        reviewer: "ima@aquatiq.com".to_owned(),
        digest: String::new(),
    };
    raw.digest = canonical_digest(&raw);
    ZdrAttestation::validate(
        "AZURE_OPENAI",
        &raw,
        chrono::NaiveDate::from_ymd_opt(2026, 8, 17).expect("static date"),
    )
    .expect("the fixture attestation validates")
}

#[derive(Clone, Copy)]
struct Posture {
    /// The Azure resource declares an EU region, as the running stack does.
    eu_region: bool,
    /// An evidence-bound ZDR attestation is configured for it. The running stack
    /// has none.
    attested: bool,
}

fn config(azure_endpoint: &str, posture: Posture) -> InferenceConfig {
    InferenceConfig {
        provider_order: vec!["azure".to_owned()],
        anthropic_api_key: None,
        openai_api_base: None,
        openai_api_key: None,
        codex_subscription_integration_core_url: None,
        codex_subscription_internal_api_key: None,
        codex_subscription_models: vec![],
        azure_openai_endpoint: Some(azure_endpoint.to_owned()),
        azure_openai_api_key: Some("test-key".to_owned()),
        azure_openai_api_version: "2025-01-01-preview".to_owned(),
        openai_chat_models: vec![],
        openai_embedding_models: vec![],
        azure_openai_chat_deployments: vec!["gpt-4o-mini".to_owned()],
        azure_openai_embedding_deployments: vec!["azure-embed".to_owned()],
        azure_anthropic_endpoint: None,
        azure_anthropic_api_key: None,
        azure_anthropic_deployments: vec![],
        azure_cohere_endpoint: None,
        azure_cohere_api_key: None,
        azure_cohere_api_version: "2024-05-01-preview".to_owned(),
        azure_cohere_deployment: "cohere-command-a-plus".to_owned(),
        azure_cohere_region: None,
        max_retries_per_provider: 1,
        cache_ttl_secs: 60,
        verevon_intent_enabled: false,
        cost_core_url: None,
        verevon_intent_budget_usd: 50.0,
        session_core_url: None,
        router_policy_refresh_secs: 60,
        azure_openai_region: posture.eu_region.then(|| "swedencentral".to_owned()),
        azure_anthropic_region: None,
        azure_openai_zdr: posture.attested.then(attestation),
        azure_anthropic_zdr: None,
        azure_openai_deployment_type: posture.eu_region.then(|| "DataZoneStandard".to_owned()),
        allow_global_deployment: false,
        allow_global_residency_providers: true,
        allow_non_eu_embedding: false,
    }
}

// ---------------------------------------------------------------------------
// The stack
// ---------------------------------------------------------------------------

/// A speech provider that only counts calls: under ZDR any call at all means
/// audio left for a provider that retains it.
#[derive(Default)]
struct CountingSpeech {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl crate::provider::speech::SpeechProvider for CountingSpeech {
    fn provider_name(&self) -> &'static str {
        "counting"
    }

    async fn synthesize(
        &self,
        _req: &SpeechSynthesisRequest,
    ) -> Result<crate::provider::speech::AudioResult, provider::ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(provider::ProviderError::Unavailable("not used".to_owned()))
    }

    async fn transcribe(
        &self,
        _req: &SpeechTranscriptionRequest,
    ) -> Result<crate::provider::speech::TranscriptResult, provider::ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(provider::ProviderError::Unavailable("not used".to_owned()))
    }
}

struct Stack {
    service: MeteredCore<InferenceService>,
    azure: MockServer,
    speech: Arc<CountingSpeech>,
    _jwks: MockServer,
}

impl Stack {
    async fn new(posture: Posture) -> Self {
        let azure = MockServer::start().await;
        let (auth, jwks) = verifier().await;
        let speech = Arc::new(CountingSpeech::default());
        let service = InferenceService {
            auth,
            chain: FallbackChain::from_config(&config(&azure.uri(), posture)),
            speech: SpeechChain::new_with_providers(vec![(
                "counting".to_owned(),
                speech.clone() as Arc<dyn crate::provider::speech::SpeechProvider>,
            )]),
            translation: TranslationChain::new_with_providers(vec![]),
            vision: VisionChain::new_with_providers(vec![]),
            doc_intel: DocIntelChain::from_env(),
            language: LanguageAnalyticsChain::from_env(),
            realtime: RealtimeChain::new_with_providers(vec![]),
            video: VideoChain::new_with_providers(vec![]),
        };
        Self {
            service: MeteredCore::new(service),
            azure,
            speech,
            _jwks: jwks,
        }
    }

    /// How many requests the Azure resource has received: the only honest
    /// measure of "a provider was called".
    async fn azure_calls(&self) -> usize {
        self.azure
            .received_requests()
            .await
            .expect("request recording is on")
            .len()
    }

    /// Azure answers a chat completion the way the live resource does: a
    /// versioned snapshot id, and a prompt count that already includes the
    /// cached legs.
    async fn serve_chat(&self) {
        Mock::given(method("POST"))
            .and(path_regex(
                r"^/openai/deployments/gpt-4o-mini/chat/completions$",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-1",
                "model": "gpt-4o-mini-2024-07-18",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 2000,
                    "completion_tokens": 5,
                    "prompt_tokens_details": {"cached_tokens": 1024, "cache_write_tokens": 512}
                }
            })))
            .mount(&self.azure)
            .await;
    }

    async fn serve_chat_stream(&self) {
        let events = [
            r#"{"id":"c1","model":"gpt-4o-mini-2024-07-18","choices":[{"index":0,"delta":{"content":"Hei"},"finish_reason":null}]}"#,
            r#"{"id":"c1","model":"gpt-4o-mini-2024-07-18","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
            r#"{"id":"c1","model":"gpt-4o-mini-2024-07-18","choices":[],"usage":{"prompt_tokens":1566,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":1408}}}"#,
        ];
        let mut body = String::new();
        for event in events {
            body.push_str("data: ");
            body.push_str(event);
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        Mock::given(method("POST"))
            .and(path_regex(
                r"^/openai/deployments/gpt-4o-mini/chat/completions$",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .mount(&self.azure)
            .await;
    }

    async fn serve_embedding(&self) {
        Mock::given(method("POST"))
            .and(path("/openai/deployments/azure-embed/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "text-embedding-3-large",
                "data": [{"index": 0, "embedding": [0.25, 0.5, 0.75]}]
            })))
            .mount(&self.azure)
            .await;
    }
}

fn infer_request(zdr: bool, text: &str) -> pb::InferRequest {
    pb::InferRequest {
        request_id: format!("req-{}", text.len()),
        org_id: "org-1".to_owned(),
        model: "gpt-4o-mini".to_owned(),
        messages: vec![pb::ChatMessage {
            role: "user".to_owned(),
            content: text.to_owned(),
            ..pb::ChatMessage::default()
        }],
        zdr,
        ..pb::InferRequest::default()
    }
}

fn embedding_request(zdr: bool, region: &str) -> pb::CreateEmbeddingRequest {
    pb::CreateEmbeddingRequest {
        request_id: "emb-1".to_owned(),
        org_id: "org-1".to_owned(),
        text: "embed me".to_owned(),
        model: "azure-embed".to_owned(),
        zdr,
        region: region.to_owned(),
        ..pb::CreateEmbeddingRequest::default()
    }
}

/// Run `body` against a fresh Prometheus recorder, on a runtime that keeps every
/// counter on the thread the recorder is scoped to, and return what it rendered.
fn scrape<F, Fut>(body: F) -> String
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    let recorder = PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    ::metrics::with_local_recorder(&recorder, || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(body());
    });
    handle.render()
}

/// The value of the series of `name` that carries every `key="value"` pair.
fn series(text: &str, name: &str, labels: &[(&str, &str)]) -> Option<u64> {
    text.lines()
        .find(|line| {
            line.starts_with(&format!("{name}{{"))
                && labels
                    .iter()
                    .all(|(key, value)| line.contains(&format!("{key}=\"{value}\"")))
        })
        .and_then(|line| line.rsplit(' ').next())
        .and_then(|value| value.parse().ok())
}

fn kind(status: &Status) -> Option<&str> {
    status
        .metadata()
        .get(PROVIDER_ERROR_KIND_TRAILER)
        .and_then(|value| value.to_str().ok())
}

const INVOKE: &[&str] = &["inference:invoke"];

// ---------------------------------------------------------------------------
// The contract
// ---------------------------------------------------------------------------

/// The live-proof case: a valid service bearer asks for ZDR and no provider is
/// attested. The refusal is typed, no provider is called, and the counter that
/// moves is the refusal's own.
#[test]
fn a_zdr_infer_is_refused_before_any_provider_call_and_counted_under_its_kind() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: false,
        })
        .await;
        stack.serve_chat().await;

        // ZDR from the request's own flag, and from the issuer's claim.
        for (issuer_zdr, request_zdr) in [(false, true), (true, false)] {
            let token = service_token("data-orchestrator", "org-1", INVOKE, issuer_zdr);
            let status = stack
                .service
                .infer(bearer(&token, infer_request(request_zdr, "zdr please")))
                .await
                .expect_err("ZDR is refused when no provider is attested");

            assert_eq!(status.code(), tonic::Code::FailedPrecondition);
            assert_eq!(
                kind(&status),
                Some("zdr_unavailable"),
                "a client reads the kind"
            );
            assert_eq!(
                status.message(),
                "no matching provider deployment has verified ZDR support",
                "the message is the chain's own, content-free sentence"
            );
        }
        assert_eq!(stack.azure_calls().await, 0, "no provider was called");
    });

    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[
                ("rpc", "infer"),
                ("service", "service:data-orchestrator"),
                ("model", "unknown"),
                ("provider", "unknown"),
                ("outcome", "zdr_unavailable"),
            ]
        ),
        Some(2),
        "{text}"
    );
    assert_eq!(
        series(&text, "inference_requests_total", &[("outcome", "ok")]),
        None,
        "the answered-request series never moved: {text}"
    );
    assert!(
        !text.contains("inference_tokens_total"),
        "no provider, no tokens: {text}"
    );
}

/// The positive control: with an attestation the real configuration path
/// validates, the same ZDR request IS served, so the refusal above is the
/// attestation rule at work and not a broken harness. The tokens counted are
/// the provider's `prompt_tokens`, which already include the cached legs.
#[test]
fn an_attested_zdr_route_serves_the_request_and_its_tokens_are_counted_once() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: true,
        })
        .await;
        stack.serve_chat().await;
        let token = service_token("data-orchestrator", "org-1", INVOKE, true);

        let response = stack
            .service
            .infer(bearer(&token, infer_request(true, "an attested zdr call")))
            .await
            .expect("an attested route serves ZDR")
            .into_inner();

        assert_eq!(response.provider_used, "azure-openai");
        assert_eq!(response.input_tokens, 2000);
        assert_eq!(stack.azure_calls().await, 1);
    });

    let served = [
        ("rpc", "infer"),
        ("service", "service:data-orchestrator"),
        ("model", "gpt-4o-mini"),
        ("provider", "azure-openai"),
    ];
    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[
                served[0],
                served[1],
                served[2],
                served[3],
                ("outcome", "ok")
            ]
        ),
        Some(1),
        "{text}"
    );
    assert_eq!(
        series(
            &text,
            "inference_tokens_total",
            &[served[1], served[2], served[3], ("direction", "input")]
        ),
        Some(2000),
        "input_tokens already folds the 1024 read and 512 write cache tokens in: {text}"
    );
    assert_eq!(
        series(
            &text,
            "inference_tokens_total",
            &[served[1], served[2], served[3], ("direction", "output")]
        ),
        Some(5),
        "{text}"
    );
}

#[test]
fn a_streamed_answer_is_counted_once_when_it_ends_with_the_providers_usage() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: false,
        })
        .await;
        stack.serve_chat_stream().await;
        let token = service_token("embedding-engine", "org-1", INVOKE, false);

        let mut stream = stack
            .service
            .infer_stream(bearer(&token, infer_request(false, "stream me")))
            .await
            .expect("the stream opens")
            .into_inner();
        let mut chunks = 0;
        while let Some(chunk) = stream.next().await {
            chunk.expect("a chunk");
            chunks += 1;
        }
        assert!(chunks >= 2, "a delta and a final chunk");
    });

    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[
                ("rpc", "infer_stream"),
                ("service", "service:embedding-engine"),
                ("model", "gpt-4o-mini"),
                ("provider", "azure-openai"),
                ("outcome", "ok"),
            ]
        ),
        Some(1),
        "{text}"
    );
    assert_eq!(
        series(&text, "inference_tokens_total", &[("direction", "input")]),
        Some(1566),
        "{text}"
    );
    assert_eq!(
        series(&text, "inference_tokens_total", &[("direction", "output")]),
        Some(2),
        "{text}"
    );
}

#[test]
fn a_catalogue_read_is_counted_under_the_calling_service_and_serves_no_model() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: false,
        })
        .await;
        let token = service_token("model-gateway", "org-1", &["inference:read"], false);

        let models = stack
            .service
            .list_models(bearer(&token, pb::ListModelsRequest::default()))
            .await
            .expect("a read credential lists the catalogue")
            .into_inner()
            .models;

        assert!(models.iter().any(|model| model.id == "gpt-4o-mini"));
        assert_eq!(
            stack.azure_calls().await,
            0,
            "a catalogue read spends nothing"
        );
    });

    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[
                ("rpc", "list_models"),
                ("service", "service:model-gateway"),
                ("model", "none"),
                ("provider", "none"),
                ("outcome", "ok"),
            ]
        ),
        Some(1),
        "{text}"
    );
}

/// A call is counted under the caller that verified, even when the checks after
/// verification refuse it, and a call that never verified is `unauthenticated`.
#[test]
fn a_refused_call_is_counted_under_the_caller_that_verified_and_never_under_an_org_or_user() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: false,
        })
        .await;

        let forged = stack
            .service
            .infer(bearer(
                "attacker-controlled",
                infer_request(false, "forged"),
            ))
            .await
            .expect_err("a forged bearer");
        assert_eq!(forged.code(), tonic::Code::Unauthenticated);

        let no_scope = service_token("autoprompt", "org-1", &["inference:read"], false);
        let denied = stack
            .service
            .infer(bearer(&no_scope, infer_request(false, "no invoke scope")))
            .await
            .expect_err("no invoke scope");
        assert_eq!(denied.code(), tonic::Code::PermissionDenied);

        let other_org = service_token("autoprompt", "org-1", INVOKE, false);
        let mut cross_tenant = infer_request(false, "another tenant");
        cross_tenant.org_id = "org-2".to_owned();
        let denied = stack
            .service
            .infer(bearer(&other_org, cross_tenant))
            .await
            .expect_err("a request for another tenant");
        assert_eq!(denied.code(), tonic::Code::PermissionDenied);

        assert_eq!(stack.azure_calls().await, 0);
    });

    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[
                ("service", "unauthenticated"),
                ("outcome", "unauthenticated")
            ]
        ),
        Some(1),
        "{text}"
    );
    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[
                ("service", "service:autoprompt"),
                ("outcome", "permission_denied")
            ]
        ),
        Some(2),
        "the scope refusal and the tenant refusal, both under the verified service: {text}"
    );
    assert!(
        !text.contains("org-1") && !text.contains("org-2"),
        "no org label: {text}"
    );
}

#[test]
fn an_embedding_refusal_is_typed_and_reaches_no_provider() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: false,
        })
        .await;
        stack.serve_embedding().await;
        let token = service_token("embedding-engine", "org-1", INVOKE, false);

        let zdr = stack
            .service
            .create_embedding(bearer(&token, embedding_request(true, "")))
            .await
            .expect_err("no attested embedding deployment");
        assert_eq!(zdr.code(), tonic::Code::FailedPrecondition);
        assert_eq!(kind(&zdr), Some("zdr_unavailable"));

        let residency = stack
            .service
            .create_embedding(bearer(&token, embedding_request(false, "eastus")))
            .await
            .expect_err("a non-EU region");
        assert_eq!(residency.code(), tonic::Code::FailedPrecondition);
        assert_eq!(kind(&residency), Some("residency_unavailable"));

        assert_eq!(
            stack.azure_calls().await,
            0,
            "both refused before the network"
        );

        // An ordinary embedding is still served, and counted under the model the
        // provider named.
        let served = stack
            .service
            .create_embedding(bearer(&token, embedding_request(false, "")))
            .await
            .expect("an ordinary embedding")
            .into_inner();
        assert_eq!(served.vector.len(), 3);
        assert_eq!(stack.azure_calls().await, 1);
    });

    for (outcome, model, provider) in [
        ("zdr_unavailable", "unknown", "unknown"),
        ("residency_unavailable", "unknown", "unknown"),
        ("ok", "text-embedding-3-large", "azure-openai"),
    ] {
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &[
                    ("rpc", "create_embedding"),
                    ("service", "service:embedding-engine"),
                    ("model", model),
                    ("provider", provider),
                    ("outcome", outcome),
                ]
            ),
            Some(1),
            "{outcome}: {text}"
        );
    }
}

#[test]
fn a_residency_floor_and_a_privacy_tier_nobody_meets_are_typed_refusals_before_any_provider() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: false,
        })
        .await;
        stack.serve_chat().await;
        let token = service_token("data-orchestrator", "org-1", INVOKE, false);

        let mut floor = infer_request(false, "norway only");
        floor.min_residency = "norway".to_owned();
        let status = stack
            .service
            .infer(bearer(&token, floor))
            .await
            .expect_err("no Norwegian deployment");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(kind(&status), Some("residency_unavailable"));

        let mut tier = infer_request(false, "sovereign only");
        tier.min_privacy_tier = provider::PrivacyTier::Sovereign.as_wire_i32();
        let status = stack
            .service
            .infer(bearer(&token, tier))
            .await
            .expect_err("no sovereign deployment");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(kind(&status), Some("tier_unavailable"));

        assert_eq!(stack.azure_calls().await, 0);
    });

    for outcome in ["residency_unavailable", "tier_unavailable"] {
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &[("rpc", "infer"), ("outcome", outcome)]
            ),
            Some(1),
            "{outcome}: {text}"
        );
    }
}

/// The non-chat modalities carry the same typed refusal through the same
/// helper, and count under their own rpc.
#[test]
fn a_non_chat_modality_refused_under_zdr_is_typed_and_reaches_no_provider() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: false,
        })
        .await;
        let token = service_token("voice-gateway", "org-1", INVOKE, false);

        let status = stack
            .service
            .transcribe_speech(bearer(
                &token,
                pb::TranscribeSpeechRequest {
                    request_id: "stt-1".to_owned(),
                    org_id: "org-1".to_owned(),
                    audio: b"webm-bytes".to_vec(),
                    format: "webm".to_owned(),
                    zdr: true,
                    ..pb::TranscribeSpeechRequest::default()
                },
            ))
            .await
            .expect_err("no speech deployment is attested");

        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(kind(&status), Some("zdr_unavailable"));
        assert_eq!(
            stack.speech.calls.load(Ordering::SeqCst),
            0,
            "no audio left"
        );
    });

    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[
                ("rpc", "transcribe_speech"),
                ("service", "service:voice-gateway"),
                ("outcome", "zdr_unavailable"),
            ]
        ),
        Some(1),
        "{text}"
    );
}

/// A second identical call is answered from the completed-response cache. The
/// stored copy still carries the token counts of the call that filled it, but no
/// provider was reached, so those tokens must not be counted again: the call is
/// counted (it was answered), the tokens are counted once.
#[test]
fn a_response_cache_hit_is_counted_as_a_call_and_adds_no_provider_tokens() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: false,
        })
        .await;
        stack.serve_chat().await;
        let token = service_token("data-orchestrator", "org-1", INVOKE, false);

        for call in 1..=2 {
            let response = stack
                .service
                .infer(bearer(&token, infer_request(false, "the same question")))
                .await
                .expect("served")
                .into_inner();
            assert_eq!(
                response.input_tokens, 2000,
                "call {call}: the cached copy carries the original counts"
            );
        }
        assert_eq!(
            stack.azure_calls().await,
            1,
            "the second call never reached the provider"
        );
    });

    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[("rpc", "infer"), ("outcome", "ok")]
        ),
        Some(2),
        "both calls were answered: {text}"
    );
    assert_eq!(
        series(&text, "inference_tokens_total", &[("direction", "input")]),
        Some(2000),
        "the provider reported 2000 once; the cache hit adds none: {text}"
    );
    assert_eq!(
        series(&text, "inference_tokens_total", &[("direction", "output")]),
        Some(5),
        "{text}"
    );
}

/// A client that gives up while the provider is still working drops the call's
/// future. The provider may already be spending, so the call is still counted,
/// under the service that verified and the outcome `cancelled`; nothing is
/// counted twice when the call does finish.
#[test]
fn a_unary_call_abandoned_while_the_provider_works_is_counted_as_cancelled() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: false,
        })
        .await;
        Mock::given(method("POST"))
            .and(path_regex(
                r"^/openai/deployments/gpt-4o-mini/chat/completions$",
            ))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
            .mount(&stack.azure)
            .await;
        let token = service_token("data-orchestrator", "org-1", INVOKE, false);

        // Abandon the call only once the provider has really been reached, so the
        // test does not depend on how fast this machine is.
        let mut call = Box::pin(
            stack
                .service
                .infer(bearer(&token, infer_request(false, "slow question"))),
        );
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                tokio::select! {
                    _ = &mut call => panic!("the provider is slow: the call cannot have answered"),
                    () = tokio::time::sleep(Duration::from_millis(20)) => {
                        if stack.azure_calls().await >= 1 {
                            break;
                        }
                    }
                }
            }
        })
        .await
        .expect("the provider was reached");
        drop(call);
        assert_eq!(
            stack.azure_calls().await,
            1,
            "the provider was already called"
        );
    });

    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[
                ("rpc", "infer"),
                ("service", "service:data-orchestrator"),
                ("outcome", "cancelled"),
            ]
        ),
        Some(1),
        "{text}"
    );
    assert_eq!(
        series(&text, "inference_requests_total", &[("outcome", "ok")]),
        None,
        "{text}"
    );
}

/// ZDR can arrive on the issuer's token claim alone. An embedding is the call
/// whose whole payload is tenant text, so the claim must refuse it exactly as the
/// request's own flag does, before any provider sees the text.
#[test]
fn an_embedding_under_the_issuers_zdr_claim_is_refused_before_any_provider_call() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: false,
        })
        .await;
        stack.serve_embedding().await;
        let token = service_token("embedding-engine", "org-1", INVOKE, true);

        let status = stack
            .service
            .create_embedding(bearer(&token, embedding_request(false, "")))
            .await
            .expect_err("the token's ZDR claim alone requires zero retention");

        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(kind(&status), Some("zdr_unavailable"));
        assert_eq!(stack.azure_calls().await, 0, "no text left for a provider");
    });

    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[
                ("rpc", "create_embedding"),
                ("service", "service:embedding-engine"),
                ("outcome", "zdr_unavailable"),
            ]
        ),
        Some(1),
        "{text}"
    );
}

/// A ZDR chat turn is a stream: that is the path chat uses. It is refused before
/// any provider is called with the same typed kind as the unary call, and it is
/// counted under its own rpc and that kind, not as an abandoned call.
#[test]
fn a_zdr_stream_is_refused_before_any_provider_call_and_counted_under_its_kind() {
    let text = scrape(|| async {
        let stack = Stack::new(Posture {
            eu_region: true,
            attested: false,
        })
        .await;
        stack.serve_chat_stream().await;
        // ZDR from the issuer's claim this time, not the request's flag.
        let token = service_token("model-gateway", "org-1", INVOKE, true);

        let status = stack
            .service
            .infer_stream(bearer(&token, infer_request(false, "a zdr stream")))
            .await
            .err()
            .expect("no attested deployment can open a ZDR stream");

        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(kind(&status), Some("zdr_unavailable"));
        assert_eq!(stack.azure_calls().await, 0, "no provider was called");
    });

    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[
                ("rpc", "infer_stream"),
                ("service", "service:model-gateway"),
                ("model", "unknown"),
                ("provider", "unknown"),
                ("outcome", "zdr_unavailable"),
            ]
        ),
        Some(1),
        "{text}"
    );
    assert_eq!(
        series(
            &text,
            "inference_requests_total",
            &[("outcome", "cancelled")]
        ),
        None,
        "a refused stream is not an abandoned one: {text}"
    );
    assert_eq!(
        series(&text, "inference_requests_total", &[("outcome", "ok")]),
        None,
        "{text}"
    );
}
