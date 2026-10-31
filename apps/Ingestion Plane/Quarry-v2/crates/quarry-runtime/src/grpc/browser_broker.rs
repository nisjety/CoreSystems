//! gRPC `GrantValidator` implementation calling Model Plane's
//! `BrowserBroker` over tonic.
//!
//! Quarry brokers browser leases internally, but Model Plane issues *grants*
//! that authorize a specific Model Plane session to use Quarry's browser. The
//! `BrowserBroker` is the authority — Quarry calls `ValidateGrant`
//! before any privileged browser action.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use super::model_plane::v1::browser_broker_client::BrowserBrokerClient;
use super::model_plane::v1::{RevokeGrantRequest, ValidateGrantRequest, ValidateGrantResponse};
use crate::grant_validator::{GrantBudget, GrantValidation, GrantValidator};
use crate::service_tokens::{ServiceTokenRequest, SharedServiceTokenProvider};

/// The service-token request for validating a grant at the broker: the
/// `browser-grant-validate` row of the cross-plane manifest (audience
/// `browser-broker`, scope `browser:read` only). The validator mints it per
/// organization through the shared service-token provider.
pub fn browser_grant_token_request() -> QuarryResult<ServiceTokenRequest> {
    ServiceTokenRequest::from_manifest("browser-grant-validate")
}

#[derive(Clone)]
pub struct GrpcGrantValidator {
    channel: Channel,
    /// Development-only static bearer (`QUARRY_EDGE__MODEL_PLANE_TOKEN` under the
    /// cross-plane dev bypass). Never set together with `service_tokens`.
    auth_token: Option<String>,
    /// The production credential: a service token minted per organization for
    /// audience `browser-broker`, scope `browser:read` (see
    /// [`browser_grant_token_request`]). When this is set, the broker is only
    /// ever called with a token for the organization of the run being checked,
    /// so the broker's own `GetScoped(grant, org)` makes another organization's
    /// grant not found.
    service_tokens: Option<SharedServiceTokenProvider>,
}

/// Transient attempts at the broker for one validation (the first call plus
/// two retries); a refresh after `Unauthenticated` is not one of them.
const MAX_TRANSIENT_ATTEMPTS: usize = 3;
/// Pause before the second and the third attempt.
const RETRY_BACKOFF_MS: [u64; MAX_TRANSIENT_ATTEMPTS - 1] = [50, 200];

impl GrpcGrantValidator {
    /// Connect to the Model Plane browser broker over gRPC.
    /// Endpoint examples: `http://browser-broker:9090`, `https://broker.modelplane.svc:443`.
    ///
    /// Production-grade settings:
    /// - HTTP/2 keep-alive ping every 30s with 10s timeout — detects dead
    ///   connections quickly so the next call doesn't surface a stale-conn
    ///   error.
    /// - `keep_alive_while_idle = true` — keeps the connection alive even
    ///   when no requests are in-flight (otherwise tonic only pings during
    ///   active streams).
    /// - Connect timeout 5s, request timeout 10s.
    /// - Native root CAs for HTTPS endpoints.
    ///
    /// tonic's load-balanced channel automatically reconnects when a
    /// connection breaks, so callers get transparent recovery.
    pub async fn connect(endpoint: impl Into<String>) -> QuarryResult<Self> {
        let endpoint_str = endpoint.into();
        let mut ep = Endpoint::from_shared(endpoint_str.clone()).map_err(|e| {
            QuarryError::new(
                ErrorCode::BadRequest,
                format!("invalid grpc endpoint {endpoint_str}: {e}"),
            )
        })?;
        ep = ep
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .http2_keep_alive_interval(Duration::from_secs(30))
            .keep_alive_timeout(Duration::from_secs(10))
            .keep_alive_while_idle(true)
            .tcp_keepalive(Some(Duration::from_secs(60)));
        if endpoint_str.starts_with("https://") {
            ep = ep
                .tls_config(ClientTlsConfig::new().with_native_roots())
                .map_err(|e| {
                    QuarryError::new(ErrorCode::Internal, format!("tls config failed: {e}"))
                })?;
        }

        let channel = ep.connect().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, format!("grpc connect failed: {e}"))
        })?;

        Ok(Self {
            channel,
            auth_token: None,
            service_tokens: None,
        })
    }

    /// A static bearer for the broker. Development only: the edge refuses to
    /// boot with the cross-plane dev bypass in production, and a validator that
    /// carries service tokens ignores this.
    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        if self.service_tokens.is_none() {
            self.auth_token = Some(token.into());
        }
        self
    }

    /// Authenticate with a service token minted per organization (G1). From
    /// here on `validate` (which has no organization) is refused and
    /// `validate_for_org` is the only way to ask the broker.
    pub fn with_service_tokens(mut self, provider: SharedServiceTokenProvider) -> Self {
        self.service_tokens = Some(provider);
        self.auth_token = None;
        self
    }

    /// Whether this validator mints a service token per organization.
    pub fn uses_service_tokens(&self) -> bool {
        self.service_tokens.is_some()
    }

    fn client(&self) -> BrowserBrokerClient<Channel> {
        BrowserBrokerClient::new(self.channel.clone())
    }

    fn apply_auth<T>(&self, mut req: tonic::Request<T>) -> tonic::Request<T> {
        if let Some(token) = &self.auth_token {
            if let Ok(value) = format!("Bearer {token}").parse() {
                req.metadata_mut().insert("authorization", value);
            }
        }
        req
    }

    /// Revoke a grant (Quarry calls this when a session ends abnormally).
    ///
    /// Revocation needs `browser:write`, which no service principal holds (X-1),
    /// so a validator that carries service tokens refuses here, before any
    /// network call, instead of sending a request the broker must reject.
    pub async fn revoke(&self, grant_id: &str, reason: &str) -> QuarryResult<bool> {
        if self.service_tokens.is_some() {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "a service-token validator holds browser:read only and cannot revoke a grant",
            ));
        }
        let mut client = self.client();
        let request = self.apply_auth(tonic::Request::new(RevokeGrantRequest {
            grant_id: grant_id.to_string(),
            reason: reason.to_string(),
        }));
        let resp = client
            .revoke_grant(request)
            .await
            .map_err(grpc_status_to_error)?
            .into_inner();
        Ok(resp.revoked)
    }

    /// One validation against the broker, with the credential this validator
    /// was built with. `org_id` is `Some` for the org-scoped call.
    ///
    /// - A validator with service tokens mints (or reuses) a token for exactly
    ///   `org_id`; on `Unauthenticated` it drops that token, mints a fresh one
    ///   and asks once more. A second `Unauthenticated` is a refusal. A mint
    ///   failure is returned as it is: the broker is not called without a token.
    /// - `NotFound` (no such grant, or one of another organization) and
    ///   `FailedPrecondition` (the broker's answer for a revoked or expired
    ///   grant) are an inactive grant, which every caller turns into a 403.
    /// - `PermissionDenied` and any other status are errors, never an allow.
    /// - `Unavailable`, `Internal` and `DeadlineExceeded` are retried up to
    ///   three attempts in all (50 ms, then 200 ms between them), then fail
    ///   closed with the broker's status.
    async fn validate_inner(
        &self,
        org_id: Option<&str>,
        grant_id: &str,
    ) -> QuarryResult<GrantValidation> {
        if grant_id.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "grant_id must not be empty",
            ));
        }
        let service = match (&self.service_tokens, org_id) {
            (Some(_), None) => {
                return Err(QuarryError::new(
                    ErrorCode::Forbidden,
                    "browser grant validation needs the organization of the run when service tokens are used",
                ));
            }
            (Some(tokens), Some(org)) => {
                if org.trim().is_empty() {
                    return Err(QuarryError::new(
                        ErrorCode::Forbidden,
                        "verified org_id is required to validate a browser grant",
                    ));
                }
                Some((tokens, org, browser_grant_token_request()?))
            }
            (None, _) => None,
        };

        let mut transient = 0usize;
        let mut refreshed = false;
        let mut force_refresh = false;
        loop {
            let mut request = tonic::Request::new(ValidateGrantRequest {
                grant_id: grant_id.to_string(),
            });
            match &service {
                Some((tokens, org, token_request)) => {
                    let bearer = tokens
                        .token_for_org(org, token_request, force_refresh)
                        .await?;
                    force_refresh = false;
                    let mut value: tonic::metadata::MetadataValue<tonic::metadata::Ascii> =
                        format!("Bearer {}", bearer.expose()).parse().map_err(|_| {
                            QuarryError::new(
                                ErrorCode::Internal,
                                "the broker service token is not a valid header value",
                            )
                        })?;
                    value.set_sensitive(true);
                    request.metadata_mut().insert("authorization", value);
                }
                None => request = self.apply_auth(request),
            }
            let status = match self.client().validate_grant(request).await {
                Ok(response) => return Ok(validation_from_response(response.into_inner())),
                Err(status) => status,
            };
            match status.code() {
                tonic::Code::NotFound | tonic::Code::FailedPrecondition => {
                    return Ok(inactive_validation(grant_id));
                }
                tonic::Code::Unauthenticated if !refreshed && service.is_some() => {
                    refreshed = true;
                    force_refresh = true;
                    if let Some((tokens, org, token_request)) = &service {
                        tokens.invalidate(org, token_request).await;
                    }
                    tracing::warn!("ValidateGrant refused the service token; minting a fresh one");
                }
                code if is_transient(code) => {
                    transient += 1;
                    if transient >= MAX_TRANSIENT_ATTEMPTS {
                        return Err(grpc_status_to_error(status));
                    }
                    tracing::warn!(
                        attempt = transient,
                        code = ?code,
                        "ValidateGrant transient failure; retrying"
                    );
                    tokio::time::sleep(Duration::from_millis(RETRY_BACKOFF_MS[transient - 1]))
                        .await;
                }
                _ => return Err(grpc_status_to_error(status)),
            }
        }
    }
}

#[async_trait]
impl GrantValidator for GrpcGrantValidator {
    /// Validate without an organization. With a static development bearer this
    /// is the whole check; with service tokens it is refused (fail closed),
    /// because a token for no organization would let the broker answer for any
    /// grant. Every agent route uses [`GrantValidator::validate_for_org`].
    async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
        self.validate_inner(None, grant_id).await
    }

    /// Validate `grant_id` with a broker token minted for `org_id`, the
    /// organization of the verified run. See [`GrpcGrantValidator::validate_inner`]
    /// for the status mapping and the bounded retries.
    async fn validate_for_org(
        &self,
        org_id: &str,
        grant_id: &str,
    ) -> QuarryResult<GrantValidation> {
        self.validate_inner(Some(org_id), grant_id).await
    }
}

/// What the broker's `NotFound` and `FailedPrecondition` (a revoked or expired
/// grant) both mean to Quarry: a grant that authorizes nothing.
pub(crate) fn inactive_validation(grant_id: &str) -> GrantValidation {
    GrantValidation {
        grant_id: grant_id.to_string(),
        active: false,
        expires_at: None,
        allowed_domains: Vec::new(),
        allowed_actions: Vec::new(),
        allowed_frame_ids: Vec::new(),
        allowed_dialog_ids: Vec::new(),
        allowed_artifact_ids: Vec::new(),
        parent_grant_id: None,
        kind: String::new(),
        owner_id: String::new(),
        zdr: false,
        budget: None,
        policy_mode: String::new(),
        denied_domains: Vec::new(),
        absolute_expires_at: None,
        resource_id: String::new(),
        key_id: String::new(),
    }
}

/// What a `ValidateGrantResponse` says, in Quarry's own type. The budget claim
/// is carried verbatim (WS6.E); whether it is applied is the edge's decision.
pub(crate) fn validation_from_response(resp: ValidateGrantResponse) -> GrantValidation {
    GrantValidation {
        grant_id: resp.grant_id,
        active: resp.active,
        expires_at: resp.expires_at.and_then(prost_ts_to_chrono),
        allowed_domains: resp.allowed_domains,
        allowed_actions: resp.allowed_actions,
        allowed_frame_ids: resp.allowed_frame_ids,
        allowed_dialog_ids: resp.allowed_dialog_ids,
        allowed_artifact_ids: resp.allowed_artifact_ids,
        parent_grant_id: (!resp.parent_grant_id.is_empty()).then_some(resp.parent_grant_id),
        kind: resp.kind,
        owner_id: resp.owner_id,
        zdr: resp.zdr,
        budget: GrantBudget::from_wire(
            resp.budget_micro_usd,
            resp.budget_max_steps,
            resp.budget_max_runtime_s,
            resp.budget_source,
            resp.budget_minted_at_unix,
        ),
        policy_mode: resp.policy_mode,
        denied_domains: resp.denied_domains,
        absolute_expires_at: resp.absolute_expires_at.and_then(prost_ts_to_chrono),
        resource_id: resp.resource_id,
        // INNH-10: the vendored `proto/browser.proto` has no `key_id` on `ValidateGrantResponse`
        // yet (the Model Plane owner picks the field number, ask MP-2). Until it is vendored this
        // is always empty, so no API-key run is admitted through the gRPC validator: it fails closed.
        key_id: String::new(),
    }
}

/// gRPC status codes considered transient for retry purposes.
pub(crate) fn is_transient(code: tonic::Code) -> bool {
    matches!(
        code,
        tonic::Code::Unavailable | tonic::Code::Internal | tonic::Code::DeadlineExceeded
    )
}

fn prost_ts_to_chrono(ts: prost_types::Timestamp) -> Option<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp(ts.seconds, ts.nanos.max(0) as u32)
}

pub(crate) fn grpc_status_to_error(status: tonic::Status) -> QuarryError {
    let code = match status.code() {
        tonic::Code::Unauthenticated | tonic::Code::PermissionDenied => ErrorCode::Forbidden,
        tonic::Code::NotFound => ErrorCode::NotFound,
        tonic::Code::InvalidArgument => ErrorCode::BadRequest,
        tonic::Code::DeadlineExceeded => ErrorCode::Timeout,
        tonic::Code::ResourceExhausted => ErrorCode::RateLimited,
        _ => ErrorCode::DriverFailed,
    };
    QuarryError::new(
        code,
        format!("grpc {}: {}", status.code(), status.message()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_claim_on_a_validate_response_is_carried_verbatim() {
        let resp = ValidateGrantResponse {
            grant_id: "g1".into(),
            active: true,
            kind: "run".into(),
            budget_micro_usd: 1_500_000,
            budget_max_steps: 25,
            budget_max_runtime_s: 300,
            budget_source: "org_default".into(),
            budget_minted_at_unix: 1_760_000_000,
            ..Default::default()
        };
        let grant = validation_from_response(resp);
        assert_eq!(
            grant.budget,
            Some(GrantBudget {
                micro_usd: 1_500_000,
                max_steps: 25,
                max_runtime_s: 300,
                source: "org_default".into(),
                minted_at_unix: 1_760_000_000,
            })
        );
        assert_eq!(grant.kind, "run");
    }

    #[test]
    fn a_validate_response_with_no_bound_carries_no_claim() {
        // An old broker (all five fields zero) and a broker that sent only a label.
        for resp in [
            ValidateGrantResponse::default(),
            ValidateGrantResponse {
                budget_source: "request".into(),
                budget_minted_at_unix: 1,
                ..Default::default()
            },
            // A negative bound is not a bound (the broker never sends one).
            ValidateGrantResponse {
                budget_micro_usd: -5,
                budget_max_steps: -1,
                budget_max_runtime_s: -1,
                ..Default::default()
            },
        ] {
            assert_eq!(validation_from_response(resp).budget, None);
        }
    }

    #[test]
    fn prost_ts_to_chrono_handles_zero() {
        let ts = prost_types::Timestamp {
            seconds: 0,
            nanos: 0,
        };
        let dt = prost_ts_to_chrono(ts).unwrap();
        assert_eq!(dt.timestamp(), 0);
    }

    #[test]
    fn prost_ts_to_chrono_handles_negative_nanos() {
        // protobuf spec says nanos must be in [0, 999_999_999], but we handle
        // accidental negatives without crashing.
        let ts = prost_types::Timestamp {
            seconds: 1_700_000_000,
            nanos: -10,
        };
        let dt = prost_ts_to_chrono(ts).unwrap();
        assert_eq!(dt.timestamp(), 1_700_000_000);
    }

    #[test]
    fn grpc_status_to_error_maps_codes() {
        let err = grpc_status_to_error(tonic::Status::not_found("missing"));
        assert_eq!(err.code, ErrorCode::NotFound);

        let err = grpc_status_to_error(tonic::Status::deadline_exceeded("slow"));
        assert_eq!(err.code, ErrorCode::Timeout);

        let err = grpc_status_to_error(tonic::Status::resource_exhausted("rate"));
        assert_eq!(err.code, ErrorCode::RateLimited);

        let err = grpc_status_to_error(tonic::Status::permission_denied("nope"));
        assert_eq!(err.code, ErrorCode::Forbidden);
    }

    // ---- the validator against a real gRPC peer (G1, G6) -------------------

    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use tonic::{Request, Response, Status};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::model_plane::v1::browser_broker_server::{BrowserBroker, BrowserBrokerServer};
    use super::super::model_plane::v1::{
        AcquireGrantRequest, AcquireGrantResponse, BrowserHealthRequest, BrowserHealthResponse,
        RevokeGrantResponse,
    };
    use crate::service_tokens::{AuthCoreServiceTokenProvider, SharedServiceTokenProvider};

    /// The broker, scripted: each `ValidateGrant` pops the next answer and
    /// records the `authorization` header it arrived with.
    #[derive(Clone, Default)]
    struct ScriptedBroker {
        answers: Arc<Mutex<VecDeque<Result<ValidateGrantResponse, Status>>>>,
        seen: Arc<Mutex<Vec<(Option<String>, String)>>>,
    }

    impl ScriptedBroker {
        fn answering(answers: Vec<Result<ValidateGrantResponse, Status>>) -> Self {
            Self {
                answers: Arc::new(Mutex::new(answers.into())),
                seen: Arc::default(),
            }
        }

        fn calls(&self) -> Vec<(Option<String>, String)> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[tonic::async_trait]
    impl BrowserBroker for ScriptedBroker {
        async fn acquire_grant(
            &self,
            _request: Request<AcquireGrantRequest>,
        ) -> Result<Response<AcquireGrantResponse>, Status> {
            Err(Status::unimplemented("not part of this test"))
        }

        async fn revoke_grant(
            &self,
            _request: Request<RevokeGrantRequest>,
        ) -> Result<Response<RevokeGrantResponse>, Status> {
            Err(Status::unimplemented("not part of this test"))
        }

        async fn validate_grant(
            &self,
            request: Request<ValidateGrantRequest>,
        ) -> Result<Response<ValidateGrantResponse>, Status> {
            let bearer = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let grant_id = request.into_inner().grant_id;
            self.seen.lock().unwrap().push((bearer, grant_id));
            self.answers
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(Status::internal("the script ran out of answers")))
                .map(Response::new)
        }

        async fn health(
            &self,
            _request: Request<BrowserHealthRequest>,
        ) -> Result<Response<BrowserHealthResponse>, Status> {
            Err(Status::unimplemented("not part of this test"))
        }
    }

    /// Serve `broker` on a loopback port and connect a validator to it.
    async fn validator_for(broker: ScriptedBroker) -> GrpcGrantValidator {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(BrowserBrokerServer::new(broker))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener)),
        );
        GrpcGrantValidator::connect(format!("http://{address}"))
            .await
            .expect("the validator connects to the fake broker")
    }

    /// An Auth Core that mints `tokens` in order, one per request, and records
    /// the request bodies. The registry's `browser-broker` mint path.
    async fn auth_core_minting(tokens: &[&str]) -> MockServer {
        let server = MockServer::start().await;
        for (index, token) in tokens.iter().enumerate() {
            Mock::given(method("POST"))
                .and(path("/api/browser-broker/internal-token"))
                .and(header("x-service-id", "quarry-edge"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "token": token,
                    "expiresInSeconds": 300,
                })))
                .up_to_n_times(1)
                .with_priority((index + 1) as u8)
                .mount(&server)
                .await;
        }
        server
    }

    fn provider_for(auth_core: &MockServer) -> SharedServiceTokenProvider {
        Arc::new(
            AuthCoreServiceTokenProvider::new(auth_core.uri(), "credential-long-enough").unwrap(),
        )
    }

    fn active(grant_id: &str) -> ValidateGrantResponse {
        ValidateGrantResponse {
            grant_id: grant_id.into(),
            active: true,
            kind: "run".into(),
            owner_id: "user-1".into(),
            allowed_domains: vec!["example.com".into()],
            ..Default::default()
        }
    }

    async fn mint_bodies(auth_core: &MockServer) -> Vec<serde_json::Value> {
        auth_core
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn a_service_token_validator_asks_the_broker_with_a_token_minted_for_the_runs_org() {
        let auth_core = auth_core_minting(&["broker-token-org-a", "broker-token-org-b"]).await;
        let broker = ScriptedBroker::answering(vec![
            Ok(active("grant_1")),
            Ok(active("grant_2")),
            Ok(active("grant_1")),
        ]);
        let validator = validator_for(broker.clone())
            .await
            .with_service_tokens(provider_for(&auth_core));

        let first = validator.validate_for_org("org-a", "grant_1").await.unwrap();
        assert!(first.is_usable());
        assert_eq!(first.allowed_domains, ["example.com"]);
        let second = validator.validate_for_org("org-b", "grant_2").await.unwrap();
        assert!(second.is_usable());
        // The same organization again reuses its cached token: no third mint.
        validator.validate_for_org("org-a", "grant_1").await.unwrap();

        let calls = broker.calls();
        assert_eq!(
            calls
                .iter()
                .map(|(bearer, grant)| (bearer.as_deref(), grant.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (Some("Bearer broker-token-org-a"), "grant_1"),
                (Some("Bearer broker-token-org-b"), "grant_2"),
                (Some("Bearer broker-token-org-a"), "grant_1"),
            ],
            "each call carries the token minted for its own organization"
        );
        let bodies = mint_bodies(&auth_core).await;
        assert_eq!(bodies.len(), 2, "one mint per organization, then the cache");
        for (body, org) in bodies.iter().zip(["org-a", "org-b"]) {
            assert_eq!(body["orgId"], org);
            assert_eq!(body["scopes"], serde_json::json!(["browser:read"]));
            assert_eq!(body["reason"], "validate verified browser grant");
            assert!(
                body.get("zdr").is_none(),
                "the request never names a posture: {body}"
            );
        }
    }

    #[tokio::test]
    async fn the_org_blind_validate_is_refused_when_service_tokens_are_configured() {
        let auth_core = auth_core_minting(&["never-used"]).await;
        let broker = ScriptedBroker::answering(vec![Ok(active("grant_1"))]);
        let validator = validator_for(broker.clone())
            .await
            .with_service_tokens(provider_for(&auth_core))
            .with_bearer_token("static-dev-token");
        assert!(validator.uses_service_tokens());

        let refused = validator.validate("grant_1").await.unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden);
        assert!(broker.calls().is_empty(), "the broker is never asked");
        assert!(
            mint_bodies(&auth_core).await.is_empty(),
            "no token is minted for no organization"
        );
        // An empty organization is refused before any network call too.
        let refused = validator.validate_for_org("  ", "grant_1").await.unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden);
        assert!(broker.calls().is_empty());
    }

    #[tokio::test]
    async fn an_unauthenticated_answer_mints_a_fresh_token_once_and_asks_again() {
        let auth_core = auth_core_minting(&["stale-token", "fresh-token"]).await;
        let broker = ScriptedBroker::answering(vec![
            Err(Status::unauthenticated("token rejected")),
            Ok(active("grant_1")),
        ]);
        let validator = validator_for(broker.clone())
            .await
            .with_service_tokens(provider_for(&auth_core));

        let grant = validator.validate_for_org("org-a", "grant_1").await.unwrap();
        assert!(grant.is_usable());
        let bearers: Vec<_> = broker.calls().into_iter().map(|(bearer, _)| bearer).collect();
        assert_eq!(
            bearers,
            vec![
                Some("Bearer stale-token".to_owned()),
                Some("Bearer fresh-token".to_owned())
            ]
        );
        assert_eq!(mint_bodies(&auth_core).await.len(), 2);
    }

    #[tokio::test]
    async fn a_second_unauthenticated_answer_is_a_refusal_not_a_loop() {
        let auth_core = auth_core_minting(&["token-1", "token-2", "token-3"]).await;
        let broker = ScriptedBroker::answering(vec![
            Err(Status::unauthenticated("no")),
            Err(Status::unauthenticated("still no")),
            Ok(active("grant_1")),
        ]);
        let validator = validator_for(broker.clone())
            .await
            .with_service_tokens(provider_for(&auth_core));

        let error = validator.validate_for_org("org-a", "grant_1").await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(broker.calls().len(), 2, "one refresh, no more");
        assert_eq!(mint_bodies(&auth_core).await.len(), 2);
    }

    #[tokio::test]
    async fn a_revoked_expired_or_unknown_grant_is_inactive_never_an_upstream_error() {
        // G6: the broker answers FailedPrecondition for a revoked or expired
        // grant and NotFound for an unknown one or another organization's. Both
        // are a grant that authorizes nothing, so the caller answers 403 and not 502.
        for status in [
            Status::failed_precondition("grant revoked"),
            Status::failed_precondition("grant expired"),
            Status::not_found("grant not found"),
        ] {
            let auth_core = auth_core_minting(&["broker-token"]).await;
            let broker = ScriptedBroker::answering(vec![Err(status.clone())]);
            let validator = validator_for(broker.clone())
                .await
                .with_service_tokens(provider_for(&auth_core));
            let grant = validator
                .validate_for_org("org-a", "grant_1")
                .await
                .unwrap_or_else(|error| panic!("{status:?} became an error: {error}"));
            assert!(!grant.active, "{status:?}");
            assert!(!grant.is_usable(), "{status:?}");
            assert_eq!(grant.grant_id, "grant_1");
            assert!(grant.allowed_domains.is_empty(), "{status:?}");
            assert_eq!(broker.calls().len(), 1, "{status:?} is not retried");
        }
    }

    #[tokio::test]
    async fn the_static_bearer_path_maps_a_revoked_grant_the_same_way() {
        let broker = ScriptedBroker::answering(vec![
            Err(Status::failed_precondition("grant revoked")),
            Ok(active("grant_1")),
        ]);
        let validator = validator_for(broker.clone())
            .await
            .with_bearer_token("static-dev-token");
        assert!(!validator.uses_service_tokens());
        assert!(!validator.validate("grant_1").await.unwrap().active);
        // The org-aware call falls back to the same static path.
        assert!(validator.validate_for_org("org-a", "grant_1").await.unwrap().active);
        assert_eq!(
            broker.calls().into_iter().map(|(bearer, _)| bearer).collect::<Vec<_>>(),
            vec![
                Some("Bearer static-dev-token".to_owned()),
                Some("Bearer static-dev-token".to_owned())
            ]
        );
    }

    #[tokio::test]
    async fn a_permission_denied_answer_is_a_403_error_and_is_not_retried() {
        let auth_core = auth_core_minting(&["broker-token"]).await;
        let broker = ScriptedBroker::answering(vec![
            Err(Status::permission_denied("scope browser:read required")),
            Ok(active("grant_1")),
        ]);
        let validator = validator_for(broker.clone())
            .await
            .with_service_tokens(provider_for(&auth_core));
        let error = validator.validate_for_org("org-a", "grant_1").await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(broker.calls().len(), 1);
    }

    #[tokio::test]
    async fn an_unavailable_broker_is_retried_three_times_in_all_and_then_fails_closed() {
        let auth_core = auth_core_minting(&["broker-token", "broker-token-2"]).await;
        let broker = ScriptedBroker::answering(vec![
            Err(Status::unavailable("restarting")),
            Err(Status::unavailable("restarting")),
            Err(Status::unavailable("restarting")),
            Ok(active("grant_1")),
        ]);
        let validator = validator_for(broker.clone())
            .await
            .with_service_tokens(provider_for(&auth_core));
        let error = validator.validate_for_org("org-a", "grant_1").await.unwrap_err();
        assert_eq!(error.code, ErrorCode::DriverFailed);
        assert_eq!(broker.calls().len(), 3, "bounded: the fourth answer is never asked for");
        assert_eq!(mint_bodies(&auth_core).await.len(), 1, "one token served all three");

        // A transient failure that clears inside the budget is served.
        let broker = ScriptedBroker::answering(vec![
            Err(Status::unavailable("restarting")),
            Ok(active("grant_1")),
        ]);
        let validator = validator_for(broker.clone())
            .await
            .with_service_tokens(provider_for(&auth_core));
        assert!(validator.validate_for_org("org-a", "grant_1").await.unwrap().active);
        assert_eq!(broker.calls().len(), 2);
    }

    #[tokio::test]
    async fn a_mint_failure_fails_closed_and_the_broker_is_never_called() {
        // Auth Core refuses the mint (a directory miss or a registry without the
        // row): the call ends in 403 before any request reaches the broker.
        let auth_core = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/browser-broker/internal-token"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&auth_core)
            .await;
        let broker = ScriptedBroker::answering(vec![Ok(active("grant_1"))]);
        let validator = validator_for(broker.clone())
            .await
            .with_service_tokens(provider_for(&auth_core));
        let error = validator.validate_for_org("org-a", "grant_1").await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert!(broker.calls().is_empty());

        // Auth Core down is an error as well, never an allow.
        let down = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&down)
            .await;
        let validator = validator_for(ScriptedBroker::answering(vec![Ok(active("grant_1"))]))
            .await
            .with_service_tokens(provider_for(&down));
        assert!(validator.validate_for_org("org-a", "grant_1").await.is_err());
    }

    #[tokio::test]
    async fn a_service_token_validator_cannot_revoke() {
        let auth_core = auth_core_minting(&["broker-token"]).await;
        let broker = ScriptedBroker::answering(vec![]);
        let validator = validator_for(broker.clone())
            .await
            .with_service_tokens(provider_for(&auth_core));
        let error = validator.revoke("grant_1", "done").await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert!(mint_bodies(&auth_core).await.is_empty());
    }

    #[test]
    fn the_validator_requests_exactly_the_manifest_row() {
        let request = browser_grant_token_request().unwrap();
        assert_eq!(request.scopes(), ["browser:read"]);
        assert_eq!(request.reason(), "validate verified browser grant");
    }
}
