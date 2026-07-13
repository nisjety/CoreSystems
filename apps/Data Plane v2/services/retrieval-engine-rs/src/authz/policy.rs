//! Policy client for Control Plane's versioned Data authorization decision.
//!
//! - [`NoopPolicyClient`]: explicit insecure-local pass-through only.
//!
//! - [`HttpPolicyClient`]: calls auth-core's canonical Better Auth membership
//!   decision endpoint with a dedicated caller credential. Backend errors and
//!   misconfiguration always fail closed.

use std::time::Duration;

use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

use super::context::EffectiveAcl;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementMode {
    Off,
    Strict,
    Permissive,
}

impl EnforcementMode {
    pub fn from_env() -> Self {
        match std::env::var("CONTROL_PLANE_ENFORCEMENT").as_deref() {
            Ok("strict") => EnforcementMode::Strict,
            Ok("permissive") => EnforcementMode::Permissive,
            Ok("off") => EnforcementMode::Off,
            _ => EnforcementMode::Strict,
        }
    }
}

/// What a policy lookup returned. `is_member=false` means the caller
/// claims an org they are not a member of and must be denied; the audit
/// log records the cause.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub is_member: bool,
    pub acl: EffectiveAcl,
    pub cause: String,
}

impl PolicyDecision {
    pub fn allow_all() -> Self {
        Self {
            is_member: true,
            acl: EffectiveAcl::allow_all(),
            cause: "ok".into(),
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            is_member: false,
            acl: EffectiveAcl::default(),
            cause: reason.into(),
        }
    }
}

/// Async trait — kept simple (no generics) so callers can use `Arc<dyn …>`.
#[async_trait::async_trait]
pub trait PolicyClient: Send + Sync {
    async fn resolve(&self, user_id: &str, org_id: &str) -> PolicyDecision;
}

/// Local-dev / v2.3 back-compat: always allow.
pub struct NoopPolicyClient;

#[async_trait::async_trait]
impl PolicyClient for NoopPolicyClient {
    async fn resolve(&self, _user_id: &str, _org_id: &str) -> PolicyDecision {
        PolicyDecision::allow_all()
    }
}

/// Production client — one versioned Control Plane decision contract. Decisions
/// are cached briefly; once expired, a Control outage cannot reuse a stale allow.
pub struct HttpPolicyClient {
    http: reqwest::Client,
    token_url: String,
    decision_url: String,
    service_id: String,
    service_api_key: String,
    cache: moka::future::Cache<(String, String), PolicyDecision>,
    cache_enabled: bool,
}

impl HttpPolicyClient {
    const AUDIENCE: &'static str = "control-policy";
    const REQUIRED_SCOPE: &'static str = "data:authorization:decide";
    const TOKEN_REASON: &'static str = "authorize retrieval request";
    const MAX_TOKEN_TTL_SECONDS: i64 = 300;

    pub fn new(
        token_url: impl Into<String>,
        decision_url: impl Into<String>,
        service_id: impl Into<String>,
        service_api_key: impl Into<String>,
    ) -> Self {
        Self::new_with_cache_ttl(
            token_url,
            decision_url,
            service_id,
            service_api_key,
            Duration::from_secs(30),
        )
    }

    fn new_with_cache_ttl(
        token_url: impl Into<String>,
        decision_url: impl Into<String>,
        service_id: impl Into<String>,
        service_api_key: impl Into<String>,
        cache_ttl: Duration,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            // Never forward the service credential or short-lived bearer to a
            // redirected host. Control routes are configured as exact URLs.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build reqwest client");
        let cache = moka::future::Cache::builder()
            .time_to_live(if cache_ttl.is_zero() {
                Duration::from_secs(1)
            } else {
                cache_ttl
            })
            .max_capacity(50_000)
            .build();
        Self {
            http,
            token_url: token_url.into(),
            decision_url: decision_url.into(),
            service_id: service_id.into(),
            service_api_key: service_api_key.into(),
            cache,
            cache_enabled: !cache_ttl.is_zero(),
        }
    }

    async fn mint_token(&self, org_id: &str) -> Result<String, ()> {
        if self.token_url.trim().is_empty()
            || self.service_id.trim().is_empty()
            || self.service_api_key.trim().is_empty()
        {
            tracing::error!("Control policy token issuer or caller identity is not configured");
            return Err(());
        }

        let response = self
            .http
            .post(&self.token_url)
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", &self.service_api_key)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": [Self::REQUIRED_SCOPE],
                "reason": Self::TOKEN_REASON,
            }))
            .send()
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "Control policy token endpoint unavailable");
            })?;
        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), "Control policy token issuance failed");
            return Err(());
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct TokenResponse {
            token: String,
            expires_at: String,
            expires_in_seconds: i64,
            issuer: String,
            audience: String,
        }

        #[derive(Deserialize)]
        struct TokenClaims {
            iss: String,
            aud: String,
            sub: String,
            exp: i64,
            org_id: String,
            principal_type: String,
            service_id: String,
            scopes: Vec<String>,
            reason: String,
        }

        let bundle: TokenResponse = response.json().await.map_err(|error| {
            tracing::warn!(error = %error, "Control policy token response was malformed");
        })?;
        if bundle.audience != Self::AUDIENCE
            || bundle.expires_in_seconds <= 0
            || bundle.expires_in_seconds > Self::MAX_TOKEN_TTL_SECONDS
            || bundle.token.trim().is_empty()
            || bundle.issuer.trim().is_empty()
        {
            tracing::warn!("Control policy token response violated the bounded-token contract");
            return Err(());
        }

        let header = decode_header(&bundle.token).map_err(|_| {
            tracing::warn!("Control policy token was malformed");
        })?;
        if header.alg != Algorithm::RS256 || header.typ.as_deref() != Some("JWT") {
            tracing::warn!("Control policy token did not use the required JWT profile");
            return Err(());
        }

        // This decode only validates the mint response before forwarding it.
        // The decision endpoint is the cryptographic verifier and rechecks the
        // RS256 signature, issuer, audience, service identity, tenant and scope.
        let mut validation = Validation::new(Algorithm::RS256);
        validation.insecure_disable_signature_validation();
        validation.set_audience(&[Self::AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "aud", "sub"]);
        let token_data =
            decode::<TokenClaims>(&bundle.token, &DecodingKey::from_secret(&[]), &validation)
                .map_err(|_| {
                    tracing::warn!("Control policy token claims were invalid");
                })?;
        let claims = token_data.claims;
        let expected_subject = format!("service:{}", self.service_id);
        if claims.aud != Self::AUDIENCE
            || claims.org_id != org_id
            || claims.principal_type != "service"
            || claims.sub != expected_subject
            || claims.service_id != expected_subject
            || claims.scopes.as_slice() != [Self::REQUIRED_SCOPE]
            || claims.reason != Self::TOKEN_REASON
            || claims.iss != bundle.issuer
        {
            tracing::warn!("Control policy token claims did not match the requested authority");
            return Err(());
        }

        let now = Utc::now();
        let expires_at = DateTime::parse_from_rfc3339(&bundle.expires_at)
            .map_err(|_| {
                tracing::warn!("Control policy token expiry was malformed");
            })?
            .with_timezone(&Utc);
        let remaining_claim_seconds = claims.exp - now.timestamp();
        let remaining_bundle_seconds = (expires_at - now).num_seconds();
        if remaining_claim_seconds <= 0
            || remaining_claim_seconds > Self::MAX_TOKEN_TTL_SECONDS
            || remaining_bundle_seconds <= 0
            || remaining_bundle_seconds > Self::MAX_TOKEN_TTL_SECONDS
            || (claims.exp - expires_at.timestamp()).abs() > 5
            || (bundle.expires_in_seconds - remaining_claim_seconds).abs() > 5
        {
            tracing::warn!("Control policy token was expired or exceeded the maximum lifetime");
            return Err(());
        }

        Ok(bundle.token)
    }

    async fn fetch_decision(&self, user_id: &str, org_id: &str) -> Result<PolicyDecision, ()> {
        if self.decision_url.trim().is_empty()
            || user_id.trim().is_empty()
            || org_id.trim().is_empty()
        {
            tracing::error!("Control decision URL or policy subject is not configured");
            return Err(());
        }

        let token = self.mint_token(org_id).await?;

        let resp = self
            .http
            .post(&self.decision_url)
            .bearer_auth(token)
            .json(&serde_json::json!({
                "userId": user_id,
                "orgId": org_id,
                "action": "data.read",
            }))
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "Control authorization decision unavailable");
            })?;
        if !resp.status().is_success() {
            tracing::warn!(status = %resp.status(), "Control authorization decision failed");
            return Err(());
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct DecisionResponse {
            version: String,
            allowed: bool,
            role: Option<String>,
            permissions: Vec<String>,
            membership_revision: Option<String>,
            reason: String,
        }
        let decision: DecisionResponse = resp.json().await.map_err(|e| {
            tracing::warn!(error = %e, "Control authorization decision decode failed");
        })?;
        if decision.version != "v1" {
            tracing::warn!(version = %decision.version, "unsupported Control decision version");
            return Err(());
        }
        if !decision.allowed {
            // `resolve` asks only for `data.read`; its sole valid denial shape is
            // a non-member with no role, permissions, or membership revision.
            if decision.reason != "not_member"
                || decision.role.is_some()
                || !decision.permissions.is_empty()
                || decision.membership_revision.is_some()
            {
                tracing::warn!("Control denial response violated the decision contract");
                return Err(());
            }
            tracing::info!(reason = %decision.reason, "Control authorization denied membership");
            return Ok(PolicyDecision::deny("denied:no_membership"));
        }
        if decision.reason != "member"
            || decision.role.as_deref().is_none_or(str::is_empty)
            || decision
                .membership_revision
                .as_deref()
                .is_none_or(str::is_empty)
            || !decision
                .permissions
                .iter()
                .any(|value| value == "data:read")
        {
            tracing::warn!("Control allow response violated the decision contract");
            return Err(());
        }
        tracing::debug!(
            revision = decision.membership_revision.as_deref().unwrap_or("none"),
            permissions = ?decision.permissions,
            "Control authorization allowed membership"
        );
        Ok(PolicyDecision {
            is_member: true,
            acl: EffectiveAcl {
                can_read: true,
                ..Default::default()
            },
            cause: "ok".into(),
        })
    }
}

#[async_trait::async_trait]
impl PolicyClient for HttpPolicyClient {
    async fn resolve(&self, user_id: &str, org_id: &str) -> PolicyDecision {
        let key = (user_id.to_string(), org_id.to_string());
        if self.cache_enabled {
            if let Some(cached) = self.cache.get(&key).await {
                return cached;
            }
        }

        let decision = self
            .fetch_decision(user_id, org_id)
            .await
            .unwrap_or_else(|_| PolicyDecision::deny("denied:control_plane_unavailable"));
        // Cache positive membership only and for at most 30 seconds. Backend
        // errors and explicit denials are rechecked on the next request, while
        // expired positive entries can never be reused during an outage.
        if self.cache_enabled && decision.is_member {
            self.cache.insert(key, decision.clone()).await;
        }
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
        Json, Router,
    };
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use rand::thread_rng;
    use rsa::{
        pkcs8::{EncodePrivateKey, LineEnding},
        RsaPrivateKey,
    };
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicI8, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    };

    const SERVICE_ID: &str = "retrieval-engine";
    const SERVICE_CREDENTIAL: &str = "local-test-credential";

    #[derive(Debug, Clone, Copy)]
    enum TokenProblem {
        None,
        MalformedResponse,
        WrongAudience,
        MissingScope,
        WrongOrg,
        Expired,
    }

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        headers: HeaderMap,
        body: serde_json::Value,
    }

    struct MockPolicyState {
        token_problem: TokenProblem,
        decision_mode: AtomicI8,
        token_requests: AtomicUsize,
        decision_requests: AtomicUsize,
        recorded_tokens: Mutex<Vec<RecordedRequest>>,
        recorded_decisions: Mutex<Vec<RecordedRequest>>,
    }

    impl MockPolicyState {
        fn new(token_problem: TokenProblem, decision_mode: i8) -> Self {
            Self {
                token_problem,
                decision_mode: AtomicI8::new(decision_mode),
                token_requests: AtomicUsize::new(0),
                decision_requests: AtomicUsize::new(0),
                recorded_tokens: Mutex::new(Vec::new()),
                recorded_decisions: Mutex::new(Vec::new()),
            }
        }
    }

    fn encoding_key() -> &'static EncodingKey {
        static KEY: OnceLock<EncodingKey> = OnceLock::new();
        KEY.get_or_init(|| {
            let private =
                RsaPrivateKey::new(&mut thread_rng(), 2048).expect("generate policy test key");
            let private_pem = private
                .to_pkcs8_pem(LineEnding::LF)
                .expect("encode policy test key");
            EncodingKey::from_rsa_pem(private_pem.as_bytes()).expect("load policy test key")
        })
    }

    #[derive(Serialize)]
    struct MockTokenClaims {
        iss: &'static str,
        aud: String,
        sub: String,
        iat: i64,
        nbf: i64,
        exp: i64,
        org_id: String,
        principal_type: &'static str,
        service_id: String,
        scopes: Vec<String>,
        reason: &'static str,
    }

    async fn token(
        State(state): State<Arc<MockPolicyState>>,
        headers: HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        state.token_requests.fetch_add(1, Ordering::SeqCst);
        state
            .recorded_tokens
            .lock()
            .expect("token records lock")
            .push(RecordedRequest {
                headers,
                body: body.clone(),
            });

        if matches!(state.token_problem, TokenProblem::MalformedResponse) {
            return (StatusCode::OK, Json(json!({"audience": "control-policy"})));
        }

        // The shared key is prewarmed before the mock server starts, keeping its
        // CPU-heavy generation outside the client's three-second HTTP timeout.
        let signing_key = encoding_key();
        let now = chrono::Utc::now().timestamp();
        let expires_in = if matches!(state.token_problem, TokenProblem::Expired) {
            -30
        } else {
            60
        };
        let requested_org = body["orgId"].as_str().unwrap_or_default();
        let org_id = if matches!(state.token_problem, TokenProblem::WrongOrg) {
            "different-org"
        } else {
            requested_org
        };
        let audience = if matches!(state.token_problem, TokenProblem::WrongAudience) {
            "data-plane"
        } else {
            "control-policy"
        };
        let scopes = if matches!(state.token_problem, TokenProblem::MissingScope) {
            vec!["data:read".to_string()]
        } else {
            vec!["data:authorization:decide".to_string()]
        };
        let claims = MockTokenClaims {
            iss: "http://control.test/api/convex-auth",
            aud: audience.to_string(),
            sub: format!("service:{SERVICE_ID}"),
            iat: now,
            nbf: now - 5,
            exp: now + expires_in,
            org_id: org_id.to_string(),
            principal_type: "service",
            service_id: format!("service:{SERVICE_ID}"),
            scopes,
            reason: "authorize retrieval request",
        };
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".to_string());
        let jwt = encode(&header, &claims, signing_key).expect("mint mock policy token");
        let expires_at = chrono::DateTime::from_timestamp(now + expires_in, 0)
            .expect("valid mock expiry")
            .to_rfc3339();
        (
            StatusCode::OK,
            Json(json!({
                "token": jwt,
                "expiresAt": expires_at,
                "expiresInSeconds": expires_in,
                "issuer": "http://control.test/api/convex-auth",
                "audience": audience,
            })),
        )
    }

    async fn decision(
        State(state): State<Arc<MockPolicyState>>,
        headers: HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        state.decision_requests.fetch_add(1, Ordering::SeqCst);
        state
            .recorded_decisions
            .lock()
            .expect("decision records lock")
            .push(RecordedRequest { headers, body });
        match state.decision_mode.load(Ordering::SeqCst) {
            -2 => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "unavailable"})),
            ),
            -1 => (StatusCode::OK, Json(json!({"version": "v1"}))),
            allowed => (
                StatusCode::OK,
                Json(json!({
                    "version": "v1",
                    "allowed": allowed == 1,
                    "role": if allowed == 1 { Some("member") } else { None },
                    "permissions": if allowed == 1 { vec!["data:read"] } else { Vec::<&str>::new() },
                    "membershipRevision": if allowed == 1 { Some("rev-1") } else { None },
                    "reason": if allowed == 1 { "member" } else { "not_member" }
                })),
            ),
        }
    }

    async fn start_policy_server(
        token_problem: TokenProblem,
        decision_mode: i8,
    ) -> (String, String, Arc<MockPolicyState>) {
        // RSA key generation can take longer than the production client's
        // three-second timeout under default-parallel test load. Complete shared
        // fixture initialization before exposing an endpoint to that timeout.
        let _ = encoding_key();
        let state = Arc::new(MockPolicyState::new(token_problem, decision_mode));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind policy test server");
        let addr = listener.local_addr().expect("policy test addr");
        let app = Router::new()
            .route("/api/control-policy/internal-token", post(token))
            .route(
                "/api/v1/internal/authorization/data-plane/decision",
                post(decision),
            )
            .with_state(state.clone());
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve policy test");
        });
        (
            format!("http://{addr}/api/control-policy/internal-token"),
            format!("http://{addr}/api/v1/internal/authorization/data-plane/decision"),
            state,
        )
    }

    fn client(token_url: String, decision_url: String) -> HttpPolicyClient {
        HttpPolicyClient::new(token_url, decision_url, SERVICE_ID, SERVICE_CREDENTIAL)
    }

    fn assert_no_legacy_policy_key(headers: &HeaderMap) {
        assert!(headers.get("x-data-plane-policy-key").is_none());
    }

    #[tokio::test]
    async fn scoped_service_token_is_minted_per_org_and_used_as_decision_bearer() {
        let (token_url, decision_url, state) = start_policy_server(TokenProblem::None, 1).await;
        let decision = client(token_url, decision_url)
            .resolve("user-1", "org-a")
            .await;
        assert!(decision.is_member);

        let token_requests = state.recorded_tokens.lock().expect("token records lock");
        assert_eq!(token_requests.len(), 1);
        let mint = &token_requests[0];
        assert_eq!(
            mint.headers
                .get("x-service-id")
                .and_then(|value| value.to_str().ok()),
            Some(SERVICE_ID)
        );
        assert_eq!(
            mint.headers
                .get("x-service-api-key")
                .and_then(|value| value.to_str().ok()),
            Some(SERVICE_CREDENTIAL)
        );
        assert_no_legacy_policy_key(&mint.headers);
        assert_eq!(mint.body["orgId"], "org-a");
        assert_eq!(mint.body["scopes"], json!(["data:authorization:decide"]));
        assert_eq!(mint.body["reason"], "authorize retrieval request");
        drop(token_requests);

        let decision_requests = state
            .recorded_decisions
            .lock()
            .expect("decision records lock");
        assert_eq!(decision_requests.len(), 1);
        let request = &decision_requests[0];
        assert_no_legacy_policy_key(&request.headers);
        assert!(request
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("Bearer ")));
        assert_eq!(
            request.body,
            json!({"userId":"user-1", "orgId":"org-a", "action":"data.read"})
        );
    }

    #[tokio::test]
    async fn decisions_are_cached_by_user_and_org_but_never_across_tenants() {
        let (token_url, decision_url, state) = start_policy_server(TokenProblem::None, 1).await;
        let client = client(token_url, decision_url);
        assert!(client.resolve("user-1", "org-a").await.is_member);
        assert!(client.resolve("user-1", "org-a").await.is_member);
        assert!(client.resolve("user-1", "org-b").await.is_member);
        assert_eq!(state.token_requests.load(Ordering::SeqCst), 2);
        assert_eq!(state.decision_requests.load(Ordering::SeqCst), 2);
        let orgs: Vec<String> = state
            .recorded_tokens
            .lock()
            .expect("token records lock")
            .iter()
            .map(|request| request.body["orgId"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(orgs, vec!["org-a", "org-b"]);
    }

    #[tokio::test]
    async fn token_and_decision_endpoint_failures_are_denied_and_not_cached() {
        let unavailable = HttpPolicyClient::new(
            "http://127.0.0.1:9/api/control-policy/internal-token",
            "http://127.0.0.1:9/api/v1/internal/authorization/data-plane/decision",
            SERVICE_ID,
            SERVICE_CREDENTIAL,
        );
        assert_eq!(
            unavailable.resolve("user-1", "org-a").await.cause,
            "denied:control_plane_unavailable"
        );

        let (token_url, decision_url, malformed_token) =
            start_policy_server(TokenProblem::MalformedResponse, 1).await;
        let decision = client(token_url, decision_url)
            .resolve("user-1", "org-a")
            .await;
        assert!(!decision.is_member);
        assert_eq!(malformed_token.decision_requests.load(Ordering::SeqCst), 0);

        let (token_url, decision_url, unavailable_decision) =
            start_policy_server(TokenProblem::None, -2).await;
        let decision = client(token_url, decision_url)
            .resolve("user-1", "org-a")
            .await;
        assert!(!decision.is_member);
        assert_eq!(decision.cause, "denied:control_plane_unavailable");
        assert_eq!(
            unavailable_decision
                .decision_requests
                .load(Ordering::SeqCst),
            1
        );

        let (token_url, decision_url, malformed_decision) =
            start_policy_server(TokenProblem::None, -1).await;
        let client = client(token_url, decision_url);
        assert!(!client.resolve("user-1", "org-a").await.is_member);
        malformed_decision.decision_mode.store(1, Ordering::SeqCst);
        assert!(client.resolve("user-1", "org-a").await.is_member);
        assert_eq!(
            malformed_decision.decision_requests.load(Ordering::SeqCst),
            2
        );
    }

    #[tokio::test]
    async fn minted_token_must_be_unexpired_scoped_and_bound_to_control_policy_and_org() {
        for problem in [
            TokenProblem::WrongAudience,
            TokenProblem::MissingScope,
            TokenProblem::WrongOrg,
            TokenProblem::Expired,
        ] {
            let (token_url, decision_url, state) = start_policy_server(problem, 1).await;
            let decision = client(token_url, decision_url)
                .resolve("user-1", "org-a")
                .await;
            assert!(!decision.is_member, "token problem {problem:?} must deny");
            assert_eq!(state.decision_requests.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn expired_membership_cache_never_reuses_a_stale_allow() {
        let (token_url, decision_url, state) = start_policy_server(TokenProblem::None, 1).await;
        let client = HttpPolicyClient::new_with_cache_ttl(
            token_url,
            decision_url,
            SERVICE_ID,
            SERVICE_CREDENTIAL,
            Duration::ZERO,
        );
        assert!(client.resolve("user-1", "org-a").await.is_member);
        state.decision_mode.store(0, Ordering::SeqCst);
        let second = client.resolve("user-1", "org-a").await;
        assert!(!second.is_member);
        assert_eq!(second.cause, "denied:no_membership");
        assert_eq!(state.decision_requests.load(Ordering::SeqCst), 2);
    }
    #[tokio::test]
    async fn noop_client_always_allows() {
        let c = NoopPolicyClient;
        let d = c.resolve("user-1", "org-a").await;
        assert!(d.is_member);
        assert!(d.acl.can_read);
        assert_eq!(d.cause, "ok");
    }

    #[test]
    fn enforcement_mode_parsing() {
        std::env::set_var("CONTROL_PLANE_ENFORCEMENT", "strict");
        assert_eq!(EnforcementMode::from_env(), EnforcementMode::Strict);
        std::env::set_var("CONTROL_PLANE_ENFORCEMENT", "permissive");
        assert_eq!(EnforcementMode::from_env(), EnforcementMode::Permissive);
        std::env::set_var("CONTROL_PLANE_ENFORCEMENT", "off");
        assert_eq!(EnforcementMode::from_env(), EnforcementMode::Off);
        std::env::remove_var("CONTROL_PLANE_ENFORCEMENT");
        assert_eq!(EnforcementMode::from_env(), EnforcementMode::Strict);
    }
}
