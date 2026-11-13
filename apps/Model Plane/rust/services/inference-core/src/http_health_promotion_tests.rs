//! The promotion routes' refusals (W3-EP-14-S10-a): each session-core candidate
//! failure maps to its own HTTP status and typed `{ error: { code, message } }`
//! envelope, and the routes' wiring stays as specified.

use super::*;

async fn envelope(response: Response) -> (StatusCode, serde_json::Value) {
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the refusal body");
    (status, serde_json::from_slice(&body).expect("json"))
}

fn failure(code: tonic::Code, message: &str) -> CandidateError {
    CandidateError {
        code,
        message: message.to_owned(),
    }
}

#[tokio::test]
async fn every_candidate_failure_maps_to_its_own_status_and_code() {
    let cases = [
        (
            tonic::Code::Aborted,
            "superseded",
            StatusCode::CONFLICT,
            "candidate_superseded",
        ),
        (
            tonic::Code::FailedPrecondition,
            "candidate not evaluated",
            StatusCode::UNPROCESSABLE_ENTITY,
            "candidate_not_evaluated",
        ),
        (
            tonic::Code::FailedPrecondition,
            "routing policy candidates are not enabled (SESSION_CORE_ROUTING_CANDIDATES_ENABLED)",
            StatusCode::SERVICE_UNAVAILABLE,
            "candidate_store_disabled",
        ),
        (
            tonic::Code::NotFound,
            "no such candidate",
            StatusCode::NOT_FOUND,
            "candidate_not_found",
        ),
        (
            tonic::Code::InvalidArgument,
            "bad id",
            StatusCode::BAD_REQUEST,
            "candidate_invalid",
        ),
        (
            tonic::Code::Unavailable,
            "down",
            StatusCode::BAD_GATEWAY,
            "policy_store_write_failed",
        ),
        (
            tonic::Code::Internal,
            "parse promoted policy",
            StatusCode::BAD_GATEWAY,
            "policy_store_write_failed",
        ),
    ];
    for (code, message, status, wire) in cases {
        let (got, value) = envelope(candidate_error_response(&failure(code, message))).await;
        assert_eq!(got, status, "{code:?} {message}");
        assert_eq!(value["error"]["code"], wire, "{code:?} {message}");
        assert!(value["error"]["message"].is_string());
        assert!(value.get("data").is_none(), "a refusal carries no data");
    }
}

#[tokio::test]
async fn the_unconfigured_store_and_the_switched_off_promotion_are_typed_503s() {
    let (status, value) = envelope(policy_store_unconfigured()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(value["error"]["code"], "policy_store_unconfigured");

    let (status, value) = envelope(error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "candidate_store_disabled",
        "off",
    ))
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(value["error"]["code"], "candidate_store_disabled");
}

#[test]
fn the_promotion_switch_is_off_unless_the_literal_true() {
    assert!(promotion_required_from(Some("true")));
    for raw in [
        None,
        Some(""),
        Some("1"),
        Some("TRUE"),
        Some("True"),
        Some("on"),
        Some("yes"),
        Some("false"),
    ] {
        assert!(!promotion_required_from(raw), "{raw:?}");
    }
}

/// The route facts count a model as ZDR-routed only from contractual ZDR up, and every chat model as served.
#[test]
fn route_facts_mark_zdr_routes_from_contractual_zdr_up() {
    let model = |id: &str, tier| ModelInfo {
        id: id.to_owned(),
        modality: "chat".to_owned(),
        privacy_tier: tier,
        ..Default::default()
    };
    let facts = route_facts(&[
        model("global", PrivacyTier::Global),
        model("eu", PrivacyTier::EuResident),
        model("zdr", PrivacyTier::ZdrContractual),
        model("sovereign", PrivacyTier::Sovereign),
    ]);
    assert_eq!(
        facts,
        RouteFacts::new(["global", "eu", "zdr", "sovereign"], ["zdr", "sovereign"])
    );
}

/// The promotion routes end to end (W3-EP-14-S10-a): a real RS256 token checked by a real [`JwtVerifier`], the real
/// router on an ephemeral port, and a local session-core `RoutingPolicy` server that records every call. The live
/// policy is the shared `ArcSwap` the chain routes with.
mod end_to_end {
    use std::sync::{Mutex as StdMutex, OnceLock};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use base64::Engine;
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use mp_contracts::model_plane::v1::routing_policy_server::{
        RoutingPolicy as RoutingPolicyRpc, RoutingPolicyServer,
    };
    use mp_contracts::model_plane::v1::{
        GetOrgRoutingOverlayRequest, GetRoutingPolicyRequest, ListCandidatesRequest,
        ListCandidatesResponse, OrgRoutingOverlayMessage, PolicyCandidate, PromoteCandidateRequest,
        PromoteCandidateResponse, ProposeCandidateRequest, ProposeCandidateResponse,
        RecordCandidateEvaluationRequest, RecordCandidateEvaluationResponse, RoutingPolicyMessage,
        SetOrgRoutingOverlayRequest, SetRoutingPolicyRequest,
    };
    use rand::thread_rng;
    use rsa::pkcs8::{EncodePrivateKey, LineEnding};
    use rsa::traits::PublicKeyParts;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::auth::AuthConfig;
    use crate::readiness::GrpcReadiness;

    const KID: &str = "promotion-e2e-key";
    const SWITCH: &str = "ROUTING_POLICY_PROMOTION_REQUIRED";

    /// The switch is a process environment variable read on every write; the tests that set it run one at a time.
    /// A tokio mutex, because the guard is held across the awaits of the whole test.
    async fn env_lock() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        LOCK.lock().await
    }

    fn rsa_keypair() -> &'static (String, String, String) {
        static KEYS: OnceLock<(String, String, String)> = OnceLock::new();
        KEYS.get_or_init(|| {
            let private = rsa::RsaPrivateKey::new(&mut thread_rng(), 2048).expect("RSA key");
            let public = rsa::RsaPublicKey::from(&private);
            let private_pem = private
                .to_pkcs8_pem(LineEnding::LF)
                .expect("private PEM")
                .to_string();
            let n =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.n().to_bytes_be());
            let e =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.e().to_bytes_be());
            (private_pem, n, e)
        })
    }

    fn admin_token(scopes: &[&str]) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        let id = "service:policy-admin";
        let claims = json!({
            "sub": id, "iss": "auth-core", "aud": "inference-core",
            "iat": now, "nbf": now - 5, "exp": now + 3600,
            "org_id": "platform", "principal_type": "service", "service_id": id,
            "reason": "promotion e2e test", "scopes": scopes, "zdr": false,
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

    /// What the local session-core saw.
    #[derive(Default)]
    struct Seen {
        set: Vec<SetRoutingPolicyRequest>,
        proposed: Vec<ProposeCandidateRequest>,
        promoted: Vec<PromoteCandidateRequest>,
    }

    #[derive(Clone)]
    struct FakeSessionCore {
        seen: Arc<StdMutex<Seen>>,
        /// The answer to `PromoteCandidate`: the promoted config, or the refusal.
        promote: Result<String, (tonic::Code, &'static str)>,
    }

    #[tonic::async_trait]
    impl RoutingPolicyRpc for FakeSessionCore {
        async fn get_policy(
            &self,
            _request: tonic::Request<GetRoutingPolicyRequest>,
        ) -> Result<tonic::Response<RoutingPolicyMessage>, tonic::Status> {
            Ok(tonic::Response::new(RoutingPolicyMessage {
                config_json: "{}".to_owned(),
                version: 1,
                ..Default::default()
            }))
        }

        async fn set_policy(
            &self,
            request: tonic::Request<SetRoutingPolicyRequest>,
        ) -> Result<tonic::Response<RoutingPolicyMessage>, tonic::Status> {
            let request = request.into_inner();
            let config_json = request.config_json.clone();
            self.seen.lock().expect("seen").set.push(request);
            Ok(tonic::Response::new(RoutingPolicyMessage {
                config_json,
                version: 2,
                ..Default::default()
            }))
        }

        async fn propose_candidate(
            &self,
            request: tonic::Request<ProposeCandidateRequest>,
        ) -> Result<tonic::Response<ProposeCandidateResponse>, tonic::Status> {
            let request = request.into_inner();
            let candidate = PolicyCandidate {
                id: "cand-1".to_owned(),
                config_json: request.config_json.clone(),
                state: "proposed".to_owned(),
                diff_json: request.diff_json.clone(),
                ..Default::default()
            };
            self.seen.lock().expect("seen").proposed.push(request);
            Ok(tonic::Response::new(ProposeCandidateResponse {
                candidate: Some(candidate),
            }))
        }

        async fn record_candidate_evaluation(
            &self,
            _request: tonic::Request<RecordCandidateEvaluationRequest>,
        ) -> Result<tonic::Response<RecordCandidateEvaluationResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }

        async fn promote_candidate(
            &self,
            request: tonic::Request<PromoteCandidateRequest>,
        ) -> Result<tonic::Response<PromoteCandidateResponse>, tonic::Status> {
            self.seen
                .lock()
                .expect("seen")
                .promoted
                .push(request.into_inner());
            match &self.promote {
                Ok(config_json) => Ok(tonic::Response::new(PromoteCandidateResponse {
                    policy: Some(RoutingPolicyMessage {
                        config_json: config_json.clone(),
                        version: 3,
                        candidate_id: "cand-1".to_owned(),
                        ..Default::default()
                    }),
                })),
                Err((code, message)) => Err(tonic::Status::new(*code, *message)),
            }
        }

        async fn list_candidates(
            &self,
            _request: tonic::Request<ListCandidatesRequest>,
        ) -> Result<tonic::Response<ListCandidatesResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }

        async fn get_org_routing_overlay(
            &self,
            _request: tonic::Request<GetOrgRoutingOverlayRequest>,
        ) -> Result<tonic::Response<OrgRoutingOverlayMessage>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }

        async fn set_org_routing_overlay(
            &self,
            _request: tonic::Request<SetOrgRoutingOverlayRequest>,
        ) -> Result<tonic::Response<OrgRoutingOverlayMessage>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }
    }

    struct Harness {
        base: String,
        live: Arc<ArcSwap<RoutingPolicy>>,
        seen: Arc<StdMutex<Seen>>,
        _jwks: MockServer,
    }

    async fn harness(promote: Result<String, (tonic::Code, &'static str)>) -> Harness {
        let seen = Arc::new(StdMutex::new(Seen::default()));
        let fake = FakeSessionCore {
            seen: seen.clone(),
            promote,
        };
        let grpc = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("grpc port");
        let grpc_address = grpc.local_addr().expect("grpc address");
        tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(RoutingPolicyServer::new(fake))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(grpc)),
        );
        let client = PolicyClient::from_url(&format!("http://{grpc_address}")).expect("client");

        let (auth, jwks) = verifier().await;
        let chain = FallbackChain::new_with_providers(Vec::new(), 1);
        let live = chain.policy_handle();
        let state = PolicyState {
            auth,
            policy: live.clone(),
            client: Some(Arc::new(client)),
            chain,
        };
        let metrics = metrics_exporter_prometheus::PrometheusBuilder::new()
            .build_recorder()
            .handle();
        let app = router(state, GrpcReadiness::new(), metrics);
        let http = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("http port");
        let address = http.local_addr().expect("http address");
        tokio::spawn(async move { axum::serve(http, app).await });
        Harness {
            base: format!("http://{address}"),
            live,
            seen,
            _jwks: jwks,
        }
    }

    fn proposed_policy() -> RoutingPolicy {
        RoutingPolicy {
            budget_cap_usd: 123.0,
            ..RoutingPolicy::default()
        }
    }

    async fn put(harness: &Harness, token: &str) -> (reqwest::StatusCode, serde_json::Value) {
        let response = reqwest::Client::new()
            .put(format!("{}/internal/v1/router-policy", harness.base))
            .bearer_auth(token)
            .json(&proposed_policy())
            .send()
            .await
            .expect("PUT");
        let status = response.status();
        (status, response.json().await.expect("json body"))
    }

    async fn promote(harness: &Harness, token: &str) -> (reqwest::StatusCode, serde_json::Value) {
        let response = reqwest::Client::new()
            .post(format!(
                "{}/internal/v1/router-policy/candidates/cand-1/promote",
                harness.base
            ))
            .bearer_auth(token)
            .send()
            .await
            .expect("POST promote");
        let status = response.status();
        (status, response.json().await.expect("json body"))
    }

    fn promoted_config() -> String {
        serde_json::to_string(&RoutingPolicy {
            budget_cap_usd: 77.0,
            ..RoutingPolicy::default()
        })
        .expect("config")
    }

    fn admin() -> String {
        admin_token(&["inference:policy:admin"])
    }

    #[tokio::test]
    async fn switch_on_put_answers_202_with_a_candidate_and_never_writes_live() {
        let _env = env_lock().await;
        std::env::set_var(SWITCH, "true");
        let harness = harness(Ok(promoted_config())).await;
        let before = harness.live.load_full().budget_cap_usd;

        let (status, body) = put(&harness, &admin()).await;
        std::env::remove_var(SWITCH);

        assert_eq!(status, reqwest::StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["data"]["candidate_id"], "cand-1", "{body}");
        assert!(body["data"]["diff"].is_object(), "{body}");
        assert!(body.get("error").is_none(), "{body}");
        let seen = harness.seen.lock().expect("seen");
        assert!(seen.set.is_empty(), "the live policy is never written");
        assert_eq!(seen.proposed.len(), 1);
        let sent: RoutingPolicy =
            serde_json::from_str(&seen.proposed[0].config_json).expect("proposed config");
        assert!((sent.budget_cap_usd - 123.0).abs() < f64::EPSILON);
        let sent_diff: serde_json::Value =
            serde_json::from_str(&seen.proposed[0].diff_json).expect("diff json");
        assert_eq!(
            sent_diff, body["data"]["diff"],
            "the stored diff is the one answered"
        );
        assert!(
            (harness.live.load_full().budget_cap_usd - before).abs() < f64::EPSILON,
            "the live copy is unchanged"
        );
    }

    #[tokio::test]
    async fn switch_on_promote_hot_swaps_the_live_policy() {
        let _env = env_lock().await;
        std::env::set_var(SWITCH, "true");
        let harness = harness(Ok(promoted_config())).await;

        let (status, body) = promote(&harness, &admin()).await;
        std::env::remove_var(SWITCH);

        assert_eq!(status, reqwest::StatusCode::OK, "{body}");
        assert_eq!(body["data"]["budget_cap_usd"], 77.0, "{body}");
        assert!((harness.live.load_full().budget_cap_usd - 77.0).abs() < f64::EPSILON);
        let seen = harness.seen.lock().expect("seen");
        assert_eq!(seen.promoted.len(), 1);
        assert_eq!(seen.promoted[0].candidate_id, "cand-1");
        assert!(seen.set.is_empty());
    }

    #[tokio::test]
    async fn switch_on_a_superseded_promotion_is_409_and_leaves_live_alone() {
        let _env = env_lock().await;
        std::env::set_var(SWITCH, "true");
        let harness = harness(Err((tonic::Code::Aborted, "superseded"))).await;
        let before = harness.live.load_full().budget_cap_usd;

        let (status, body) = promote(&harness, &admin()).await;
        std::env::remove_var(SWITCH);

        assert_eq!(status, reqwest::StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"]["code"], "candidate_superseded");
        assert!((harness.live.load_full().budget_cap_usd - before).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn promotion_needs_the_policy_admin_scope() {
        let _env = env_lock().await;
        std::env::set_var(SWITCH, "true");
        let harness = harness(Ok(promoted_config())).await;
        let invoker = admin_token(&["inference:invoke"]);

        let (status, body) = promote(&harness, &invoker).await;
        let (put_status, _) = put(&harness, &invoker).await;
        std::env::remove_var(SWITCH);

        assert_eq!(status, reqwest::StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"]["code"], "authorization_failed");
        assert_eq!(put_status, reqwest::StatusCode::FORBIDDEN);
        let seen = harness.seen.lock().expect("seen");
        assert!(seen.promoted.is_empty() && seen.proposed.is_empty() && seen.set.is_empty());
    }

    #[tokio::test]
    async fn switch_off_put_writes_live_directly_and_promote_is_refused() {
        let _env = env_lock().await;
        std::env::remove_var(SWITCH);
        let harness = harness(Ok(promoted_config())).await;

        let (status, body) = put(&harness, &admin()).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{body}");
        // The stored policy as session-core echoes it: the bare policy, no envelope, as before the switch existed.
        assert_eq!(body["budget_cap_usd"], 123.0, "{body}");
        assert!(body.get("data").is_none(), "{body}");
        assert!((harness.live.load_full().budget_cap_usd - 123.0).abs() < f64::EPSILON);

        let (status, body) = promote(&harness, &admin()).await;
        assert_eq!(status, reqwest::StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_eq!(body["error"]["code"], "candidate_store_disabled");

        let seen = harness.seen.lock().expect("seen");
        assert_eq!(seen.set.len(), 1);
        assert!(seen.proposed.is_empty() && seen.promoted.is_empty());
    }
}
