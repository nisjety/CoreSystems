//! `/v1/browser/grants` — a signed-in user obtains and releases a browser grant.
//!
//! The grant is minted by BrowserBroker (the only issuer); this module is the
//! user-facing door to it. Three rules shape the code:
//!
//! - **Identity is never taken from the body.** The organization is
//!   `claims.org_id`; the owner is the user behind the delegated
//!   `x-browser-authorization` bearer, which the auth layer has already bound to
//!   the ingress user and tenant. A body that names an `org_id` is ignored.
//! - **The broker decides the policy.** The gateway bounds the request size and
//!   forwards; whether a host may be granted (internal names, counts, caps) is
//!   the broker's `StaticPolicy`, so there is one source of that rule.
//! - **Hosts are not echoed or logged.** What a user browses is theirs. Nothing
//!   here logs `allowed_domains`, and a Zero Data Retention response omits them.
//!
//! Issuing a grant is allowed under ZDR (the grant is ephemeral and carries a
//! `z_` id); only the durable run lifecycle is blocked for ZDR, in
//! `browser_run`.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, post},
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{AcquireGrantRequest, RevokeGrantRequest};
use mp_ids::new_ulid;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::info;

use crate::{
    auth::{Claims, VerifiedBrowserBearer},
    browser_run::authenticated_browser_request,
    state::AppState,
};

type ApiError = (StatusCode, Json<Value>);

/// Most hosts one request may name. The broker applies its own (lower, per
/// deployment) bound; this only keeps an oversized body from reaching it.
const MAX_REQUEST_DOMAINS: usize = 32;
const MAX_DOMAIN_LEN: usize = 253;
const MAX_GRANT_ID_LEN: usize = 128;
const REVOKE_REASON: &str = "session_closed";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/browser/grants", post(issue_grant))
        .route("/v1/browser/grants/:grant_id", delete(revoke_grant))
}

#[derive(Debug, Deserialize)]
struct IssueBrowserGrantRequest {
    allowed_domains: Vec<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    zdr: bool,
}

fn error(status: StatusCode, code: &str) -> ApiError {
    (status, Json(json!({ "error": code })))
}

/// The grant belongs to a person. A service principal never obtains one here,
/// even though the delegated-bearer check already refuses it upstream.
fn require_user(claims: &Claims) -> Result<(), ApiError> {
    let is_user = claims.principal_type.as_deref().unwrap_or("user") == "user"
        && claims.service_id.is_none()
        && !claims.user_id.is_empty()
        && !claims.org_id.is_empty();
    if is_user {
        Ok(())
    } else {
        Err(error(StatusCode::FORBIDDEN, "browser_grant_forbidden"))
    }
}

fn validate_request(req: &IssueBrowserGrantRequest) -> Result<&'static str, ApiError> {
    if req.allowed_domains.is_empty()
        || req.allowed_domains.len() > MAX_REQUEST_DOMAINS
        || req
            .allowed_domains
            .iter()
            .any(|domain| domain.trim().is_empty() || domain.len() > MAX_DOMAIN_LEN)
    {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_allowed_domains"));
    }
    match req.mode.as_deref().map(str::trim) {
        None | Some("") | Some("cloud") => Ok("cloud"),
        Some("local") => Ok("local"),
        Some(_) => Err(error(StatusCode::BAD_REQUEST, "unsupported_browser_mode")),
    }
}

/// Map a broker status for issuance. A refused request is the caller's to fix;
/// anything else is an upstream problem the user cannot act on.
fn issue_status_to_http(status: &tonic::Status) -> ApiError {
    match status.code() {
        tonic::Code::InvalidArgument => {
            error(StatusCode::BAD_REQUEST, "browser_host_not_grantable")
        }
        tonic::Code::PermissionDenied => error(StatusCode::FORBIDDEN, "browser_grant_forbidden"),
        tonic::Code::ResourceExhausted => {
            error(StatusCode::TOO_MANY_REQUESTS, "too_many_browser_grants")
        }
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "browser_broker_unavailable",
        ),
        _ => error(StatusCode::BAD_GATEWAY, "browser_broker_error"),
    }
}

fn expires_at_rfc3339(timestamp: Option<&prost_types::Timestamp>) -> Option<String> {
    let timestamp = timestamp?;
    let nanos = u32::try_from(timestamp.nanos).ok()?;
    chrono::DateTime::from_timestamp(timestamp.seconds, nanos)
        .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// `POST /v1/browser/grants` — issue a session grant for the caller.
async fn issue_grant(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    browser_bearer: VerifiedBrowserBearer,
    Json(req): Json<IssueBrowserGrantRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    require_user(&claims)?;
    let mode = validate_request(&req)?;
    let zdr = claims.effective_zdr(req.zdr);

    let request = AcquireGrantRequest {
        session_key: format!("ui-{}", new_ulid()),
        mode: mode.to_owned(),
        org_id: claims.org_id.clone(),
        allowed_domains: req.allowed_domains,
        zdr,
        ..Default::default()
    };
    let mut broker = state.browser_client.clone();
    let granted = broker
        .acquire_grant(authenticated_browser_request(request, &browser_bearer)?)
        .await
        .map_err(|status| issue_status_to_http(&status))?
        .into_inner();

    info!(
        org_id = %claims.org_id,
        grant_id = %granted.grant_id,
        zdr = granted.zdr,
        "browser grant issued"
    );
    let mut body = json!({
        "grant_id": granted.grant_id,
        "kind": granted.kind,
        "zdr": granted.zdr,
        "expires_at": expires_at_rfc3339(granted.expires_at.as_ref()),
    });
    if !granted.zdr {
        body["allowed_domains"] = json!(granted.allowed_domains);
    }
    Ok((StatusCode::CREATED, Json(body)))
}

/// `DELETE /v1/browser/grants/:grant_id` — release a grant and everything
/// derived from it. Idempotent: a grant that is already gone is a 200 with
/// `revoked: false`, never a 404 that would tell a caller whether an id exists.
async fn revoke_grant(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    browser_bearer: VerifiedBrowserBearer,
    Path(grant_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_user(&claims)?;
    let valid_id = !grant_id.is_empty()
        && grant_id.len() <= MAX_GRANT_ID_LEN
        && grant_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if !valid_id {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_grant_id"));
    }

    let mut broker = state.browser_client.clone();
    let result = broker
        .revoke_grant(authenticated_browser_request(
            RevokeGrantRequest {
                grant_id: grant_id.clone(),
                reason: REVOKE_REASON.to_owned(),
            },
            &browser_bearer,
        )?)
        .await;
    match result {
        Ok(response) => {
            info!(org_id = %claims.org_id, grant_id = %grant_id, "browser grant revoked");
            Ok(Json(json!({ "revoked": response.into_inner().revoked })))
        }
        Err(status) if status.code() == tonic::Code::NotFound => {
            Ok(Json(json!({ "revoked": false })))
        }
        Err(status) => Err(issue_status_to_http(&status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use axum::{body::Body, http::Request};
    use mp_contracts::model_plane::v1::{
        browser_broker_client::BrowserBrokerClient,
        browser_broker_server::{BrowserBroker, BrowserBrokerServer},
        AcquireGrantResponse, BrowserHealthRequest, BrowserHealthResponse, DeriveRunGrantRequest,
        DeriveRunGrantResponse, RevokeGrantResponse, ValidateGrantRequest, ValidateGrantResponse,
    };
    use tower::ServiceExt;

    #[derive(Clone, Default)]
    struct Recorder {
        acquired: Arc<Mutex<Vec<AcquireGrantRequest>>>,
        revoked: Arc<Mutex<Vec<RevokeGrantRequest>>>,
        authorizations: Arc<Mutex<Vec<String>>>,
        acquire_failure: Option<tonic::Code>,
        revoke_failure: Option<tonic::Code>,
    }

    #[tonic::async_trait]
    impl BrowserBroker for Recorder {
        async fn acquire_grant(
            &self,
            request: tonic::Request<AcquireGrantRequest>,
        ) -> Result<tonic::Response<AcquireGrantResponse>, tonic::Status> {
            if let Some(value) = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
            {
                self.authorizations.lock().unwrap().push(value.to_owned());
            }
            let request = request.into_inner();
            self.acquired.lock().unwrap().push(request.clone());
            if let Some(code) = self.acquire_failure {
                return Err(tonic::Status::new(code, "test broker rejection"));
            }
            let id = if request.zdr { "z_abc" } else { "abc" };
            Ok(tonic::Response::new(AcquireGrantResponse {
                grant_id: id.to_owned(),
                kind: "session".to_owned(),
                zdr: request.zdr,
                allowed_domains: request.allowed_domains,
                expires_at: Some(prost_types::Timestamp {
                    seconds: 1_800_000_000,
                    nanos: 0,
                }),
                ..Default::default()
            }))
        }

        async fn revoke_grant(
            &self,
            request: tonic::Request<RevokeGrantRequest>,
        ) -> Result<tonic::Response<RevokeGrantResponse>, tonic::Status> {
            self.revoked.lock().unwrap().push(request.into_inner());
            match self.revoke_failure {
                Some(code) => Err(tonic::Status::new(code, "test broker rejection")),
                None => Ok(tonic::Response::new(RevokeGrantResponse { revoked: true })),
            }
        }

        async fn validate_grant(
            &self,
            _request: tonic::Request<ValidateGrantRequest>,
        ) -> Result<tonic::Response<ValidateGrantResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }

        async fn derive_run_grant(
            &self,
            _request: tonic::Request<DeriveRunGrantRequest>,
        ) -> Result<tonic::Response<DeriveRunGrantResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }

        async fn health(
            &self,
            _request: tonic::Request<BrowserHealthRequest>,
        ) -> Result<tonic::Response<BrowserHealthResponse>, tonic::Status> {
            Ok(tonic::Response::new(BrowserHealthResponse {
                status: "ok".to_owned(),
            }))
        }
    }

    fn user_claims(zdr: bool) -> Claims {
        Claims {
            sub: "user-a".to_owned(),
            iss: "auth-core".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            sovereign: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    fn service_claims() -> Claims {
        Claims {
            sub: "service:quarry".to_owned(),
            user_id: String::new(),
            principal_type: Some("service".to_owned()),
            service_id: Some("service:quarry".to_owned()),
            reason: Some("test service".to_owned()),
            ..user_claims(false)
        }
    }

    async fn app(recorder: Recorder, claims: Claims, with_bearer: bool) -> Router {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(BrowserBrokerServer::new(recorder))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await;
        });
        let mut state = AppState::new();
        state.browser_client = BrowserBrokerClient::connect(format!("http://{address}"))
            .await
            .unwrap();
        let mut router = router().with_state(state).layer(Extension(claims));
        if with_bearer {
            router = router.layer(Extension(VerifiedBrowserBearer::for_test("browser-token")));
        }
        router
    }

    async fn send(router: Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let response = router
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn issue_body() -> Value {
        json!({ "allowed_domains": ["example.com"] })
    }

    #[tokio::test]
    async fn issue_requires_browser_bearer() {
        let recorder = Recorder::default();
        let router = app(recorder.clone(), user_claims(false), false).await;
        let (status, _) = send(router, "POST", "/v1/browser/grants", issue_body()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(recorder.acquired.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn issue_binds_org_from_claims_not_body() {
        let recorder = Recorder::default();
        let router = app(recorder.clone(), user_claims(false), true).await;
        let (status, body) = send(
            router,
            "POST",
            "/v1/browser/grants",
            json!({
                "allowed_domains": ["example.com"],
                "org_id": "org-attacker",
                "session_key": "forged",
                "parent_grant_id": "someone-elses",
                "allowed_actions": ["download_ref"]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let seen = recorder.acquired.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].org_id, "org-a");
        assert!(seen[0].session_key.starts_with("ui-") && seen[0].session_key != "forged");
        // Only an ordinary session grant can be requested from here.
        assert!(seen[0].parent_grant_id.is_empty() && seen[0].allowed_actions.is_empty());
        assert_eq!(
            recorder.authorizations.lock().unwrap().as_slice(),
            ["Bearer browser-token"],
            "the broker is called with the browser-audience bearer"
        );
        assert_eq!(body["grant_id"], "abc");
        assert_eq!(body["kind"], "session");
        assert_eq!(body["expires_at"], "2027-01-15T08:00:00Z");
        assert_eq!(body["allowed_domains"], json!(["example.com"]));
    }

    #[tokio::test]
    async fn issue_rejects_service_principal_with_browser_header() {
        let recorder = Recorder::default();
        let router = app(recorder.clone(), service_claims(), true).await;
        let (status, _) = send(router, "POST", "/v1/browser/grants", issue_body()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(recorder.acquired.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn issue_maps_broker_status_codes() {
        for (code, want, error_code) in [
            (
                tonic::Code::InvalidArgument,
                StatusCode::BAD_REQUEST,
                "browser_host_not_grantable",
            ),
            (
                tonic::Code::PermissionDenied,
                StatusCode::FORBIDDEN,
                "browser_grant_forbidden",
            ),
            (
                tonic::Code::ResourceExhausted,
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_browser_grants",
            ),
            (
                tonic::Code::Unavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "browser_broker_unavailable",
            ),
            (
                tonic::Code::DeadlineExceeded,
                StatusCode::SERVICE_UNAVAILABLE,
                "browser_broker_unavailable",
            ),
            (
                tonic::Code::Unauthenticated,
                StatusCode::BAD_GATEWAY,
                "browser_broker_error",
            ),
            (
                tonic::Code::Internal,
                StatusCode::BAD_GATEWAY,
                "browser_broker_error",
            ),
        ] {
            let recorder = Recorder {
                acquire_failure: Some(code),
                ..Default::default()
            };
            let router = app(recorder, user_claims(false), true).await;
            let (status, body) = send(router, "POST", "/v1/browser/grants", issue_body()).await;
            assert_eq!(status, want, "{code:?}");
            assert_eq!(body["error"], error_code, "{code:?}");
        }
    }

    #[tokio::test]
    async fn issue_validates_the_request_before_calling_the_broker() {
        for body in [
            json!({ "allowed_domains": [] }),
            json!({ "allowed_domains": [""] }),
            json!({ "allowed_domains": ["  "] }),
            json!({ "allowed_domains": vec!["a.example.com"; 33] }),
            json!({ "allowed_domains": ["x".repeat(254)] }),
            json!({ "allowed_domains": ["example.com"], "mode": "teleport" }),
        ] {
            let recorder = Recorder::default();
            let router = app(recorder.clone(), user_claims(false), true).await;
            let (status, _) = send(router, "POST", "/v1/browser/grants", body.clone()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert!(recorder.acquired.lock().unwrap().is_empty(), "{body}");
        }
    }

    #[tokio::test]
    async fn issue_response_never_contains_domains_when_zdr() {
        for (claims_zdr, request_zdr) in [(true, false), (false, true)] {
            let recorder = Recorder::default();
            let router = app(recorder.clone(), user_claims(claims_zdr), true).await;
            let (status, body) = send(
                router,
                "POST",
                "/v1/browser/grants",
                json!({ "allowed_domains": ["secret-client.example.com"], "zdr": request_zdr }),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED);
            assert_eq!(body["zdr"], true);
            assert_eq!(body["grant_id"], "z_abc");
            assert!(
                !body.to_string().contains("secret-client"),
                "a ZDR response must not echo hosts: {body}"
            );
            assert_eq!(recorder.acquired.lock().unwrap()[0].zdr, true);
        }
    }

    #[tokio::test]
    async fn revoke_uses_the_session_closed_reason_and_the_browser_bearer() {
        let recorder = Recorder::default();
        let router = app(recorder.clone(), user_claims(false), true).await;
        let (status, body) = send(
            router,
            "DELETE",
            "/v1/browser/grants/abc-123_x",
            Value::Null,
        )
        .await;
        assert_eq!((status, body), (StatusCode::OK, json!({ "revoked": true })));
        let seen = recorder.revoked.lock().unwrap();
        assert_eq!(seen[0].grant_id, "abc-123_x");
        assert_eq!(seen[0].reason, "session_closed");
    }

    #[tokio::test]
    async fn revoke_is_idempotent_on_not_found() {
        let recorder = Recorder {
            revoke_failure: Some(tonic::Code::NotFound),
            ..Default::default()
        };
        let router = app(recorder, user_claims(false), true).await;
        let (status, body) = send(router, "DELETE", "/v1/browser/grants/gone", Value::Null).await;
        assert_eq!(
            (status, body),
            (StatusCode::OK, json!({ "revoked": false }))
        );
    }

    #[tokio::test]
    async fn revoke_maps_other_failures_and_rejects_bad_ids_and_non_users() {
        let router = app(
            Recorder {
                revoke_failure: Some(tonic::Code::Unavailable),
                ..Default::default()
            },
            user_claims(false),
            true,
        )
        .await;
        let (status, _) = send(router, "DELETE", "/v1/browser/grants/abc", Value::Null).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

        let recorder = Recorder::default();
        let router = app(recorder.clone(), user_claims(false), true).await;
        let (status, _) = send(router, "DELETE", "/v1/browser/grants/a%20b", Value::Null).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let router = app(recorder.clone(), service_claims(), true).await;
        let (status, _) = send(router, "DELETE", "/v1/browser/grants/abc", Value::Null).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let router = app(recorder.clone(), user_claims(false), false).await;
        let (status, _) = send(router, "DELETE", "/v1/browser/grants/abc", Value::Null).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(recorder.revoked.lock().unwrap().is_empty());
    }
}
