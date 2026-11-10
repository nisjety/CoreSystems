//! The inference boundary under a connection-bound waiver of zero retention (Z-16).
//!
//! Real credentials and real configuration: a real [`JwtVerifier`] verifies real RS256 user
//! tokens that carry the `org_zdr` and `zdr_waiver` claims Auth Core mints, and a real
//! [`InferenceService`] routes them through the chain [`FallbackChain::from_config`] builds from
//! the subscription and Azure settings. Only the outside world is replaced, by local servers: the
//! JWKS endpoint, the Azure resource and Integration Core's subscription broker. The broker double
//! enforces the owner and organization scope of `validateCodexSubscriptionInference`, so a call it
//! receives for another member's connection is refused there as well. Every assertion reads the
//! request log of the server that would have served the call, so "a provider was called" is
//! measured, not assumed.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use mp_contracts::zdr_refusal::Refusal;
use rand::thread_rng;
use rsa::pkcs8::{EncodePrivateKey, LineEnding};
use rsa::traits::PublicKeyParts;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request as WireRequest, Respond, ResponseTemplate};

use super::*;
use crate::auth::AuthConfig;
use crate::config::InferenceConfig;
use crate::provider::speech::{
    AudioResult, SpeechProvider, SpeechSynthesisRequest, SpeechTranscriptionRequest,
    TranscriptResult,
};

const KID: &str = "waiver-route-key";
const ORG: &str = "org-1";
const BROKER_KEY: &str = "broker-test-key";
const SUBSCRIPTION: &str = "openai-codex-subscription";
const MODEL: &str = "gpt-5.6-terra";
const AZURE_EMBEDDING: &str = "azure-embed";

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

fn sign(claims: &Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.to_owned());
    encode(
        &header,
        claims,
        &EncodingKey::from_rsa_pem(rsa_keypair().0.as_bytes()).expect("encoding key"),
    )
    .expect("JWT")
}

/// A user token shaped as Auth Core mints one. `extra` adds the posture claims a test sets
/// (`org_zdr`, `zdr_waiver`): Auth Core stamps those only on the tokens they describe.
fn user_token(user: &str, zdr: bool, extra: &Value) -> String {
    let mut claims = json!({
        "sub": user, "iss": "auth-core", "aud": "inference-core",
        "iat": now(), "nbf": now() - 5, "exp": now() + 3600,
        "org_id": ORG, "user_id": user, "principal_type": "user",
        "scopes": ["inference:invoke"], "zdr": zdr,
    });
    for (key, value) in extra.as_object().expect("posture claims are an object") {
        claims[key] = value.clone();
    }
    sign(&claims)
}

/// The `zdr_waiver` claim Auth Core stamps for one acknowledged connection.
fn waiver(connection: &str) -> Value {
    json!({
        "version": 1, "provider": SUBSCRIPTION, "connection": connection,
        "ack_id": "ack-1", "digest": "digest-1", "decision_jti": "jti-1",
    })
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
        jwks_ttl: Duration::from_secs(300),
        leeway_secs: 0,
    })
    .await
    .expect("verifier");
    (verifier, server)
}

// ---------------------------------------------------------------------------
// The broker double
// ---------------------------------------------------------------------------

/// Integration Core's subscription broker, as far as scope goes. It answers only for a
/// connection that the request's organization and user own, which is what
/// `validateCodexSubscriptionInference` checks, and refuses any other request with a 403.
struct Broker {
    /// Connection id, owning organization, owning user.
    connections: Vec<(&'static str, &'static str, &'static str)>,
}

impl Respond for Broker {
    fn respond(&self, request: &WireRequest) -> ResponseTemplate {
        let key = request
            .headers
            .get("x-internal-api-key")
            .and_then(|value| value.to_str().ok());
        if key != Some(BROKER_KEY) {
            return ResponseTemplate::new(401)
                .set_body_json(json!({"success": false, "error": {"code": "unauthorized"}}));
        }
        let body: Value = serde_json::from_slice(&request.body).expect("the broker receives JSON");
        let connection = body["connectionId"].as_str().unwrap_or_default();
        let Some(owner) = self.connections.iter().find(|entry| entry.0 == connection) else {
            return ResponseTemplate::new(409).set_body_json(json!({
                "success": false, "error": {"code": "subscription_connection_inactive"}
            }));
        };
        if owner.1 != body["organizationId"].as_str().unwrap_or_default()
            || owner.2 != body["userId"].as_str().unwrap_or_default()
        {
            return ResponseTemplate::new(403).set_body_json(json!({
                "success": false, "error": {"code": "subscription_connection_scope_mismatch"}
            }));
        }
        ResponseTemplate::new(200).set_body_json(json!({"success": true, "data": {"response": {
            "requestId": body["requestId"],
            "content": "ok",
            "modelUsed": body["model"],
        }}}))
    }
}

/// A speech provider that only counts calls: under a waiver any call at all would send audio to a
/// provider the turn was never allowed to reach.
#[derive(Default)]
struct CountingSpeech {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl SpeechProvider for CountingSpeech {
    fn provider_name(&self) -> &'static str {
        "counting"
    }

    async fn synthesize(
        &self,
        _req: &SpeechSynthesisRequest,
    ) -> Result<AudioResult, crate::provider::ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(crate::provider::ProviderError::Unavailable(
            "not used".to_owned(),
        ))
    }

    async fn transcribe(
        &self,
        _req: &SpeechTranscriptionRequest,
    ) -> Result<TranscriptResult, crate::provider::ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(crate::provider::ProviderError::Unavailable(
            "not used".to_owned(),
        ))
    }
}

// ---------------------------------------------------------------------------
// The real provider configuration and the stack
// ---------------------------------------------------------------------------

/// The subscription route and the Azure resource, both registered the way the running stack
/// registers them. Azure is not attested for ZDR, as in the running stack.
fn config(azure_endpoint: &str, broker_url: &str) -> InferenceConfig {
    InferenceConfig {
        provider_order: vec!["azure".to_owned(), SUBSCRIPTION.to_owned()],
        anthropic_api_key: None,
        openai_api_base: None,
        openai_api_key: None,
        codex_subscription_integration_core_url: Some(broker_url.to_owned()),
        codex_subscription_internal_api_key: Some(BROKER_KEY.to_owned()),
        codex_subscription_models: vec![MODEL.to_owned()],
        byo_gateway_enabled: false,
        customer_gateway_internal_api_key: None,
        azure_openai_endpoint: Some(azure_endpoint.to_owned()),
        azure_openai_api_key: Some("test-key".to_owned()),
        azure_openai_api_version: "2025-01-01-preview".to_owned(),
        openai_chat_models: vec![],
        openai_embedding_models: vec![],
        azure_openai_chat_deployments: vec!["gpt-4o-mini".to_owned()],
        azure_openai_embedding_deployments: vec![AZURE_EMBEDDING.to_owned()],
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
        azure_openai_region: None,
        azure_anthropic_region: None,
        azure_openai_zdr: None,
        azure_anthropic_zdr: None,
        azure_openai_deployment_type: None,
        allow_global_deployment: false,
        allow_global_residency_providers: true,
        allow_non_eu_embedding: false,
    }
}

struct Stack {
    service: InferenceService,
    broker: MockServer,
    azure: MockServer,
    speech: Arc<CountingSpeech>,
    _jwks: MockServer,
}

impl Stack {
    async fn new() -> Self {
        let broker = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/model-subscriptions/openai-codex/infer"))
            .respond_with(Broker {
                connections: vec![("conn-a", ORG, "user-a"), ("conn-b", ORG, "user-b")],
            })
            .mount(&broker)
            .await;
        // No Azure route is mocked: every call that reaches the resource is recorded, and the
        // tests assert that none did.
        let azure = MockServer::start().await;
        let (auth, jwks) = verifier().await;
        let speech = Arc::new(CountingSpeech::default());
        let service = InferenceService {
            auth,
            chain: FallbackChain::from_config(&config(&azure.uri(), &broker.uri())),
            speech: SpeechChain::new_with_providers(vec![(
                "counting".to_owned(),
                speech.clone() as Arc<dyn SpeechProvider>,
            )]),
            translation: TranslationChain::new_with_providers(vec![]),
            vision: VisionChain::new_with_providers(vec![]),
            doc_intel: DocIntelChain::from_env(),
            language: LanguageAnalyticsChain::from_env(),
            realtime: RealtimeChain::new_with_providers(vec![]),
            video: VideoChain::new_with_providers(vec![]),
            customer_gateway: None,
            capture: None,
        };
        Self {
            service,
            broker,
            azure,
            speech,
            _jwks: jwks,
        }
    }

    async fn broker_calls(&self) -> usize {
        self.broker
            .received_requests()
            .await
            .expect("request recording is on")
            .len()
    }

    async fn azure_calls(&self) -> usize {
        self.azure
            .received_requests()
            .await
            .expect("request recording is on")
            .len()
    }
}

/// A chat turn. `text` is the whole prompt, so no two turns share a cache entry.
fn chat(connection: &str, provider: &str, zdr: bool, text: &str) -> pb::InferRequest {
    pb::InferRequest {
        request_id: text.to_owned(),
        org_id: ORG.to_owned(),
        model: MODEL.to_owned(),
        provider_hint: provider.to_owned(),
        subscription_connection_id: connection.to_owned(),
        zdr,
        messages: vec![pb::ChatMessage {
            role: "user".to_owned(),
            content: text.to_owned(),
            ..pb::ChatMessage::default()
        }],
        ..pb::InferRequest::default()
    }
}

/// A refusal is the catalogue's own: its kind names it, and the gRPC code is the one the boundary
/// documents for it.
fn assert_refused(status: &Status, refusal: Refusal, grpc: tonic::Code) {
    assert_eq!(
        status.code(),
        grpc,
        "gRPC code for {}: {}",
        refusal.code(),
        status.message()
    );
    assert_eq!(
        Refusal::from_status(status),
        Some(refusal),
        "refusal kind for {}: {}",
        refusal.code(),
        status.message()
    );
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

/// The named backstop test: removing the route binding at the boundary must fail it. Without the
/// binding, another member's connection reaches the broker (which refuses it with a plain
/// unavailable, untyped) and the Azure route reaches the resource.
#[tokio::test]
async fn a_waived_turn_reaches_only_the_connection_it_binds() {
    let stack = Stack::new().await;
    let token = user_token(
        "user-a",
        false,
        &json!({"org_zdr": true, "zdr_waiver": waiver("conn-a")}),
    );

    let answer = stack
        .service
        .infer(bearer(
            &token,
            chat("conn-a", SUBSCRIPTION, false, "bound route"),
        ))
        .await
        .expect("the connection the waiver binds answers");
    assert_eq!(answer.get_ref().content, "ok");
    assert_eq!(stack.broker_calls().await, 1);

    // Another member's connection is not the one this waiver binds. The boundary refuses it, so the
    // broker, which would refuse it on owner scope too, is never asked.
    let status = stack
        .service
        .infer(bearer(
            &token,
            chat("conn-b", SUBSCRIPTION, false, "other member"),
        ))
        .await
        .expect_err("another member's connection is refused");
    assert_refused(
        &status,
        Refusal::WAIVER_ROUTE_MISMATCH,
        tonic::Code::PermissionDenied,
    );
    assert_eq!(stack.broker_calls().await, 1);

    // The Azure resource is never the route of a waived turn, named or by default.
    for (index, provider) in ["azure-openai", ""].into_iter().enumerate() {
        let status = stack
            .service
            .infer(bearer(
                &token,
                chat("conn-a", provider, false, &format!("azure route {index}")),
            ))
            .await
            .expect_err("a waived turn names no other provider");
        assert_refused(
            &status,
            Refusal::WAIVER_ROUTE_MISMATCH,
            tonic::Code::PermissionDenied,
        );
    }
    assert_eq!(stack.azure_calls().await, 0);
    assert_eq!(stack.broker_calls().await, 1);

    // The subscription route without its connection, and an explicit ZDR request on the bound
    // route, are not the route the waiver binds either.
    for (connection, zdr, text) in [
        ("", false, "no connection"),
        ("conn-a", true, "explicit zdr"),
    ] {
        let status = stack
            .service
            .infer(bearer(&token, chat(connection, SUBSCRIPTION, zdr, text)))
            .await
            .expect_err("a waiver covers the bound route and nothing else");
        assert_refused(
            &status,
            Refusal::WAIVER_ROUTE_MISMATCH,
            tonic::Code::PermissionDenied,
        );
    }
    assert_eq!(stack.broker_calls().await, 1);

    // The alias spelling names the same route, so the same connection answers it.
    stack
        .service
        .infer(bearer(
            &token,
            chat("conn-a", "chatgpt-codex", false, "alias route"),
        ))
        .await
        .expect("the alias of the bound route answers");
    assert_eq!(stack.broker_calls().await, 2);
    assert_eq!(stack.azure_calls().await, 0);
}

/// An image never rides a waived turn: the bound route is the subscription, which reads no
/// images, so the turn is refused with the vision kind before the broker is asked, and never
/// answered from its text alone. Unary and streaming alike.
#[tokio::test]
async fn a_waived_turn_with_image_parts_is_refused_before_the_broker() {
    let stack = Stack::new().await;
    let token = user_token(
        "user-a",
        false,
        &json!({"org_zdr": true, "zdr_waiver": waiver("conn-a")}),
    );
    let with_image = |text: &str| {
        let mut request = chat("conn-a", SUBSCRIPTION, false, text);
        request.image_parts = vec![pb::InferImagePart {
            message_index: 0,
            data: vec![1, 2, 3].into(),
            mime_type: "image/png".to_owned(),
        }];
        request
    };
    let kind = |status: &Status| {
        status
            .metadata()
            .get("x-mp-provider-error")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };

    let status = stack
        .service
        .infer(bearer(&token, with_image("image unary")))
        .await
        .expect_err("a waived turn with an image is refused");
    assert_eq!(status.code(), tonic::Code::FailedPrecondition, "{}", status.message());
    assert_eq!(kind(&status).as_deref(), Some("vision_unsupported"));

    let status = stack
        .service
        .infer_stream(bearer(&token, with_image("image stream")))
        .await
        .expect_err("a waived streaming turn with an image is refused");
    assert_eq!(kind(&status).as_deref(), Some("vision_unsupported"));

    assert_eq!(stack.broker_calls().await, 0);
    assert_eq!(stack.azure_calls().await, 0);
}

/// A waived caller reaches no modality: each is refused before a provider is named, so the speech
/// provider, the Azure resource and the broker see nothing.
#[tokio::test]
async fn a_waived_turn_reaches_no_non_chat_modality() {
    let stack = Stack::new().await;
    let token = user_token(
        "user-a",
        false,
        &json!({"org_zdr": true, "zdr_waiver": waiver("conn-a")}),
    );

    let embedding = pb::CreateEmbeddingRequest {
        request_id: "embedding".to_owned(),
        org_id: ORG.to_owned(),
        text: "hei".to_owned(),
        model: AZURE_EMBEDDING.to_owned(),
        provider_hint: SUBSCRIPTION.to_owned(),
        ..pb::CreateEmbeddingRequest::default()
    };
    let status = stack
        .service
        .create_embedding(bearer(&token, embedding))
        .await
        .expect_err("a waived turn gets no embedding");
    assert_refused(
        &status,
        Refusal::WAIVER_ROUTE_MISMATCH,
        tonic::Code::PermissionDenied,
    );

    // The same embedding with no route named is the default chain, which is the Azure resource. The
    // refusal holds there too, before the resource is called (the final count checks it).
    let default_route = pb::CreateEmbeddingRequest {
        request_id: "embedding default route".to_owned(),
        org_id: ORG.to_owned(),
        text: "hei".to_owned(),
        model: AZURE_EMBEDDING.to_owned(),
        ..pb::CreateEmbeddingRequest::default()
    };
    let status = stack
        .service
        .create_embedding(bearer(&token, default_route))
        .await
        .expect_err("a waived turn gets no embedding on the default route either");
    assert_refused(
        &status,
        Refusal::WAIVER_ROUTE_MISMATCH,
        tonic::Code::PermissionDenied,
    );

    let speech = pb::SynthesizeSpeechRequest {
        request_id: "speech".to_owned(),
        org_id: ORG.to_owned(),
        text: "hei".to_owned(),
        provider_hint: SUBSCRIPTION.to_owned(),
        ..pb::SynthesizeSpeechRequest::default()
    };
    let status = stack
        .service
        .synthesize_speech(bearer(&token, speech))
        .await
        .expect_err("a waived turn gets no speech");
    assert_refused(
        &status,
        Refusal::WAIVER_ROUTE_MISMATCH,
        tonic::Code::PermissionDenied,
    );

    let translation = pb::TranslateTextRequest {
        request_id: "translation".to_owned(),
        org_id: ORG.to_owned(),
        text: "hei".to_owned(),
        target_language: "en".to_owned(),
        ..pb::TranslateTextRequest::default()
    };
    let status = stack
        .service
        .translate_text(bearer(&token, translation))
        .await
        .expect_err("a waived turn gets no translation");
    assert_refused(
        &status,
        Refusal::WAIVER_ROUTE_MISMATCH,
        tonic::Code::PermissionDenied,
    );

    assert_eq!(stack.speech.calls.load(Ordering::SeqCst), 0);
    assert_eq!(stack.azure_calls().await, 0);
    assert_eq!(stack.broker_calls().await, 0);
}

/// The outcome of a call that must be refused. A call that is served fails the test: nothing a
/// waived turn asks for may be answered.
fn refusal_of<T>(result: Result<T, Status>, operation: &str) -> Status {
    match result {
        Ok(_) => panic!("{operation} answered a waived turn"),
        Err(status) => status,
    }
}

/// One modality entry called with a default request that carries only the verified organization,
/// and the status it returns.
macro_rules! modality_refusal {
    ($service:expr, $token:expr, $method:ident, $request:ty) => {{
        let mut request = <$request>::default();
        request.org_id = ORG.to_owned();
        (
            stringify!($method),
            refusal_of(
                $service.$method(bearer($token, request)).await,
                stringify!($method),
            ),
        )
    }};
}

/// The fourteen modality entries that `reject_unattested_zdr_modality` guards each refuse a waived
/// caller, by name, before they validate the request or name a provider.
#[tokio::test]
async fn a_waived_turn_reaches_none_of_the_fourteen_modality_entries() {
    let stack = Stack::new().await;
    let token = user_token(
        "user-a",
        false,
        &json!({"org_zdr": true, "zdr_waiver": waiver("conn-a")}),
    );
    let service = &stack.service;
    let refusals = [
        modality_refusal!(
            service,
            &token,
            synthesize_speech,
            pb::SynthesizeSpeechRequest
        ),
        modality_refusal!(
            service,
            &token,
            transcribe_speech,
            pb::TranscribeSpeechRequest
        ),
        modality_refusal!(service, &token, translate_text, pb::TranslateTextRequest),
        modality_refusal!(
            service,
            &token,
            batch_translate_text,
            pb::BatchTranslateTextRequest
        ),
        modality_refusal!(
            service,
            &token,
            detect_text_language,
            pb::DetectTextLanguageRequest
        ),
        modality_refusal!(service, &token, generate_image, pb::GenerateImageRequest),
        modality_refusal!(service, &token, analyze_image, pb::AnalyzeImageRequest),
        modality_refusal!(
            service,
            &token,
            extract_image_text,
            pb::ExtractImageTextRequest
        ),
        modality_refusal!(
            service,
            &token,
            analyze_document,
            pb::AnalyzeDocumentRequest
        ),
        modality_refusal!(
            service,
            &token,
            analyze_language,
            pb::AnalyzeLanguageRequest
        ),
        modality_refusal!(
            service,
            &token,
            create_realtime_session,
            pb::CreateRealtimeSessionRequest
        ),
        modality_refusal!(
            service,
            &token,
            create_video_generation_job,
            pb::CreateVideoGenerationJobRequest
        ),
        modality_refusal!(
            service,
            &token,
            get_video_generation_job,
            pb::GetVideoGenerationJobRequest
        ),
        modality_refusal!(
            service,
            &token,
            stream_video_generation_content,
            pb::StreamVideoGenerationContentRequest
        ),
    ];
    for (operation, status) in &refusals {
        assert_eq!(status.code(), tonic::Code::PermissionDenied, "{operation}");
        assert_eq!(
            Refusal::from_status(status),
            Some(Refusal::WAIVER_ROUTE_MISMATCH),
            "{operation}"
        );
    }
    assert_eq!(stack.speech.calls.load(Ordering::SeqCst), 0);
    assert_eq!(stack.azure_calls().await, 0);
    assert_eq!(stack.broker_calls().await, 0);
}

/// A persistent token of a ZDR org that carries no waiver is never served, on any route.
#[tokio::test]
async fn a_persistent_token_of_a_zdr_org_without_a_waiver_is_refused() {
    let stack = Stack::new().await;
    let token = user_token("user-a", false, &json!({"org_zdr": true}));

    for (connection, provider, text) in [
        ("conn-a", SUBSCRIPTION, "connection route"),
        ("", "azure-openai", "default route"),
    ] {
        let status = stack
            .service
            .infer(bearer(&token, chat(connection, provider, false, text)))
            .await
            .expect_err("a persistent token of a ZDR org is refused");
        assert_refused(
            &status,
            Refusal::POSTURE_MISMATCH,
            tonic::Code::PermissionDenied,
        );
    }
    assert_eq!(stack.broker_calls().await, 0);
    assert_eq!(stack.azure_calls().await, 0);
}

/// A ZDR token of a ZDR org never reaches a connection route without an acknowledgement. This is
/// the matrix row `e_zdr_token_never_reaches_subscription`.
#[tokio::test]
async fn a_zdr_org_token_without_a_waiver_never_reaches_a_connection() {
    let stack = Stack::new().await;
    let token = user_token("user-a", true, &json!({"org_zdr": true}));

    let status = stack
        .service
        .infer(bearer(
            &token,
            chat("conn-a", SUBSCRIPTION, false, "zdr connection"),
        ))
        .await
        .expect_err("a ZDR turn never reaches a connection without an acknowledgement");
    assert_refused(
        &status,
        Refusal::CONNECTION_NOT_ACKNOWLEDGED,
        tonic::Code::FailedPrecondition,
    );
    assert_eq!(stack.broker_calls().await, 0);
}

/// A waiver that overreaches its token, names another version, or is malformed is refused, and a
/// service credential can never carry one.
#[tokio::test]
async fn an_overreaching_or_malformed_waiver_is_refused() {
    let stack = Stack::new().await;

    let zdr_with_waiver = user_token(
        "user-a",
        true,
        &json!({"org_zdr": true, "zdr_waiver": waiver("conn-a")}),
    );
    let status = stack
        .service
        .infer(bearer(
            &zdr_with_waiver,
            chat("conn-a", SUBSCRIPTION, false, "zdr waiver"),
        ))
        .await
        .expect_err("a waiver on a ZDR token is refused");
    assert_refused(
        &status,
        Refusal::POSTURE_MISMATCH,
        tonic::Code::PermissionDenied,
    );

    let mut unknown_version = waiver("conn-a");
    unknown_version["version"] = json!(2);
    let future = user_token(
        "user-a",
        false,
        &json!({"org_zdr": true, "zdr_waiver": unknown_version}),
    );
    let status = stack
        .service
        .infer(bearer(
            &future,
            chat("conn-a", SUBSCRIPTION, false, "future waiver"),
        ))
        .await
        .expect_err("a waiver of another contract version is refused");
    assert_refused(
        &status,
        Refusal::POSTURE_MISMATCH,
        tonic::Code::PermissionDenied,
    );

    let mut unnamed = waiver("conn-a");
    unnamed
        .as_object_mut()
        .expect("object")
        .remove("connection");
    let malformed = user_token(
        "user-a",
        false,
        &json!({"org_zdr": true, "zdr_waiver": unnamed}),
    );
    let status = stack
        .service
        .infer(bearer(
            &malformed,
            chat("conn-a", SUBSCRIPTION, false, "malformed"),
        ))
        .await
        .expect_err("a waiver that names no connection is not a credential");
    assert_eq!(status.code(), tonic::Code::Unauthenticated);

    let service_claims = json!({
        "sub": "service:retrieval", "iss": "auth-core", "aud": "inference-core",
        "iat": now(), "nbf": now() - 5, "exp": now() + 3600,
        "org_id": ORG, "principal_type": "service", "service_id": "service:retrieval",
        "reason": "waiver probe", "scopes": ["inference:invoke"], "zdr": false,
        "org_zdr": true, "zdr_waiver": waiver("conn-a"),
    });
    let service = sign(&service_claims);
    let status = stack
        .service
        .infer(bearer(
            &service,
            chat("conn-a", SUBSCRIPTION, false, "service waiver"),
        ))
        .await
        .expect_err("a service credential never carries a waiver");
    assert_eq!(status.code(), tonic::Code::Unauthenticated);
    assert_eq!(stack.broker_calls().await, 0);
    assert_eq!(stack.azure_calls().await, 0);
}

/// The unchanged path: a persistent user of an org that is not ZDR reaches its own subscription
/// connection exactly as before.
#[tokio::test]
async fn a_persistent_user_of_an_org_without_zdr_reaches_its_connection() {
    let stack = Stack::new().await;
    let token = user_token("user-a", false, &json!({"org_zdr": false}));

    stack
        .service
        .infer(bearer(
            &token,
            chat("conn-a", SUBSCRIPTION, false, "persistent route"),
        ))
        .await
        .expect("a persistent turn answers on its own connection");
    assert_eq!(stack.broker_calls().await, 1);
}

/// A persistent token of an organisation that is not ZDR may ask for ZDR on its own connection. A
/// connection cannot give ZDR on request, so the boundary refuses the turn before the broker is asked.
#[tokio::test]
async fn an_explicit_zdr_request_on_a_connection_is_refused_before_the_broker() {
    let stack = Stack::new().await;
    let token = user_token("user-a", false, &json!({"org_zdr": false}));

    let status = stack
        .service
        .infer(bearer(
            &token,
            chat("conn-a", SUBSCRIPTION, true, "explicit zdr on a connection"),
        ))
        .await
        .expect_err("a connection cannot give ZDR on request");
    assert_refused(
        &status,
        Refusal::EXPLICIT_REQUEST_NOT_SUPPORTED_BY_CONNECTION,
        tonic::Code::FailedPrecondition,
    );
    assert_eq!(stack.broker_calls().await, 0);
    assert_eq!(stack.azure_calls().await, 0);
}
