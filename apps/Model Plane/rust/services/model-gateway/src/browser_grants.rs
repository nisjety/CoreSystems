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
//!
//! WS8.C adds the `human_browse` kind (open web minus the broker's deny-list,
//! for a person's own browsing) and `POST /v1/browser/grants/:grant_id/renew`,
//! which extends a session or human_browse grant by one broker TTL up to its
//! absolute cap. Both are the broker's decisions; this module only forwards the
//! user's request with the user's browser bearer and maps the answer. A
//! human_browse request names no hosts, and nothing here logs one.
//!
//! INNH-10 MP-3 adds `POST /v1/browser/key-grants` and
//! `DELETE /v1/browser/key-grants/:grant_id`, the door for an `api_key`
//! principal (an API key Auth Core exchanged for a short-lived token). They
//! accept ONLY that principal, with its own `aud=browser-broker` key bearer
//! ([`VerifiedApiKeyBrowserBearer`]); the auth layer refuses a key on every
//! other route. Hosts and a budget are mandatory, there is no `kind`, no mode
//! and no renew; the broker checks both against the key's signed ceilings.
//! Dark behind `MODEL_GATEWAY_BROWSER_KEY_GRANTS` (default off: 404), and the
//! broker's own `BROWSER_BROKER_KEY_GRANTS_ENABLED` still applies (403
//! `api_key_grants_disabled`). Refusals use the typed envelope
//! `{ error: { code, message, details: { reason } } }`.

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    routing::{delete, post},
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{AcquireGrantRequest, RenewGrantRequest, RevokeGrantRequest};
use mp_ids::new_ulid;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::info;

use crate::{
    auth::{Claims, VerifiedApiKeyBrowserBearer, VerifiedBrowserBearer, API_KEY_SCOPE_BROWSER_AGENT},
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
/// The only `kind` a caller may ask for besides the ordinary session grant. It
/// is the broker that decides eligibility (user principal, policy switch); this
/// is an allow-list so an unknown kind is a 400 here, not a broker 5xx.
const KIND_HUMAN_BROWSE: &str = "human_browse";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/browser/grants", post(issue_grant))
        .route("/v1/browser/grants/:grant_id", delete(revoke_grant))
        .route("/v1/browser/grants/:grant_id/renew", post(renew_grant))
        .route("/v1/browser/key-grants", post(issue_key_grant))
        .route("/v1/browser/key-grants/:grant_id", delete(revoke_key_grant))
}

#[derive(Debug, Deserialize)]
struct IssueBrowserGrantRequest {
    /// Required for a session grant, forbidden for a `human_browse` grant.
    #[serde(default)]
    allowed_domains: Vec<String>,
    #[serde(default)]
    mode: Option<String>,
    /// `None` (a session grant) or `"human_browse"`.
    #[serde(default)]
    kind: Option<String>,
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

/// What a valid issue request asks for: the browser mode and the grant kind
/// (`""` for an ordinary session grant, otherwise `KIND_HUMAN_BROWSE`).
struct ValidatedIssue {
    mode: &'static str,
    grant_kind: &'static str,
}

fn validate_request(req: &IssueBrowserGrantRequest) -> Result<ValidatedIssue, ApiError> {
    let grant_kind = match req.kind.as_deref().map(str::trim) {
        None | Some("") => "",
        Some(KIND_HUMAN_BROWSE) => KIND_HUMAN_BROWSE,
        Some(_) => return Err(error(StatusCode::BAD_REQUEST, "unsupported_grant_kind")),
    };
    if grant_kind == KIND_HUMAN_BROWSE {
        // Open web is a policy mode, not a host list: a client that names hosts is
        // asking for a different grant.
        if !req.allowed_domains.is_empty() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "human_browse_takes_no_domains",
            ));
        }
    } else if req.allowed_domains.is_empty()
        || req.allowed_domains.len() > MAX_REQUEST_DOMAINS
        || req
            .allowed_domains
            .iter()
            .any(|domain| domain.trim().is_empty() || domain.len() > MAX_DOMAIN_LEN)
    {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_allowed_domains"));
    }
    let mode = match req.mode.as_deref().map(str::trim) {
        None | Some("") | Some("cloud") => "cloud",
        Some("local") => "local",
        Some(_) => return Err(error(StatusCode::BAD_REQUEST, "unsupported_browser_mode")),
    };
    Ok(ValidatedIssue { mode, grant_kind })
}

/// Map a broker status for a renewal. A grant that cannot be renewed (at its
/// cap, expired, revoked, another kind) and one that is not the caller's look
/// the same: 409 `browser_grant_not_renewable`, so the answer never says whether
/// an id exists. An older broker answers `Unimplemented`, which is a 502.
fn renew_status_to_http(status: &tonic::Status) -> ApiError {
    match status.code() {
        tonic::Code::FailedPrecondition | tonic::Code::NotFound => {
            error(StatusCode::CONFLICT, "browser_grant_not_renewable")
        }
        tonic::Code::InvalidArgument => error(StatusCode::BAD_REQUEST, "invalid_grant_id"),
        _ => issue_status_to_http(status),
    }
}

fn is_valid_grant_id(grant_id: &str) -> bool {
    !grant_id.is_empty()
        && grant_id.len() <= MAX_GRANT_ID_LEN
        && grant_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
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
    let ValidatedIssue { mode, grant_kind } = validate_request(&req)?;
    // A grant names no provider or connection, so the resolver reads the token's posture plus the
    // request's own ask. A waived turn is persistent and therefore not ZDR here.
    let zdr = crate::zdr_posture::decide_for(&claims, &crate::zdr_posture::Route::plain(req.zdr))
        .map_err(|refusal| crate::zdr_posture::http(&refusal))?
        .zdr();

    let request = AcquireGrantRequest {
        session_key: format!("ui-{}", new_ulid()),
        mode: mode.to_owned(),
        org_id: claims.org_id.clone(),
        allowed_domains: req.allowed_domains,
        zdr,
        grant_kind: grant_kind.to_owned(),
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
        kind = %granted.kind,
        zdr = granted.zdr,
        "browser grant issued"
    );
    // A broker that predates WS8.C sends no mode: it only issues allow-lists.
    let policy_mode = if granted.policy_mode.is_empty() {
        "allowlist".to_owned()
    } else {
        granted.policy_mode.clone()
    };
    let mut body = json!({
        "grant_id": granted.grant_id,
        "kind": granted.kind,
        "zdr": granted.zdr,
        "expires_at": expires_at_rfc3339(granted.expires_at.as_ref()),
        "absolute_expires_at": expires_at_rfc3339(granted.absolute_expires_at.as_ref()),
        "policy_mode": policy_mode,
        // Config, never a host the user chose: the namespaces an open-web grant
        // may not reach (empty for an allow-list grant).
        "denied_domains": granted.denied_domains,
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
    if !is_valid_grant_id(&grant_id) {
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

/// `POST /v1/browser/grants/:grant_id/renew` — extend a session or
/// human_browse grant by one broker TTL, never past its absolute expiry. The
/// organization is the claims', the owner is the user behind the browser bearer
/// (the broker pins both); a grant that is not renewable is a 409. Allowed under
/// ZDR: a ZDR grant renews in the broker's memory like any other.
async fn renew_grant(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    browser_bearer: VerifiedBrowserBearer,
    Path(grant_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_user(&claims)?;
    if !is_valid_grant_id(&grant_id) {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_grant_id"));
    }
    let mut broker = state.browser_client.clone();
    let renewed = broker
        .renew_grant(authenticated_browser_request(
            RenewGrantRequest {
                grant_id,
                org_id: claims.org_id.clone(),
            },
            &browser_bearer,
        )?)
        .await
        .map_err(|status| renew_status_to_http(&status))?
        .into_inner();
    // No grant id and no host in the log: a keep-alive runs every few minutes.
    info!(org_id = %claims.org_id, "browser grant renewed");
    Ok(Json(json!({
        "expires_at": expires_at_rfc3339(renewed.expires_at.as_ref()),
        "absolute_expires_at": expires_at_rfc3339(renewed.absolute_expires_at.as_ref()),
    })))
}

// ---------------------------------------------------------------------------
// INNH-10 MP-3: key grants for an `api_key` principal.
// ---------------------------------------------------------------------------

/// Shortest and longest lifetime a key-grant request may ask for. The broker
/// clamps to its own key-grant TTL (default 600 s, at most 900 s); a request
/// only ever shortens it.
const MIN_KEY_GRANT_TTL_S: i64 = 30;
const MAX_KEY_GRANT_TTL_S: i64 = 900;
const MAX_SESSION_KEY_LEN: usize = 96;
/// The revoke reason the broker records for a key grant the caller releases.
const KEY_GRANT_REVOKE_REASON: &str = "run_closed";
/// The budget source of every key grant: the caller asked for it, the broker
/// bounds it by the key's signed ceiling.
const KEY_GRANT_BUDGET_SOURCE: &str = "request";

/// ErrorInfo reasons of the broker's key refusals (browser-broker
/// `internal/server/key_grants.go`) and of its site policy (`server.go`).
const BROKER_REASON_KEY_GRANTS_DISABLED: &str = "API_KEY_GRANTS_DISABLED";
const BROKER_REASON_KEY_REVOKED: &str = "API_KEY_REVOKED";
const BROKER_REASON_KEY_HOST_CEILING: &str = "API_KEY_HOST_CEILING";
const BROKER_REASON_KEY_BUDGET_CEILING: &str = "API_KEY_BUDGET_CEILING";
const BROKER_REASON_SITE_DENIED: &str = "BROWSER_SITE_DENIED";
const ERROR_INFO_TYPE_URL: &str = "type.googleapis.com/google.rpc.ErrorInfo";

/// `MODEL_GATEWAY_BROWSER_KEY_GRANTS`: only `1`, `true` or `on` (any case)
/// turns the key-grant routes on. Anything else, and unset, is off.
fn key_grants_flag(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        let value = value.trim();
        ["1", "true", "on"]
            .iter()
            .any(|on| value.eq_ignore_ascii_case(on))
    })
}

fn key_grants_enabled(state: &AppState) -> bool {
    state.browser_key_grants.unwrap_or_else(|| {
        key_grants_flag(std::env::var("MODEL_GATEWAY_BROWSER_KEY_GRANTS").ok().as_deref())
    })
}

/// The typed refusal envelope of the key-grant routes. `message` is fixed text:
/// nothing the caller sent and nothing the broker said rides along.
fn key_error(status: StatusCode, code: &str, reason: &str, message: &str) -> ApiError {
    (
        status,
        Json(json!({
            "error": { "code": code, "message": message, "details": { "reason": reason } }
        })),
    )
}

fn key_bad_request(reason: &str, message: &str) -> ApiError {
    key_error(StatusCode::BAD_REQUEST, "invalid_request", reason, message)
}

/// The key-grant routes do not exist while the gateway flag is off.
fn key_grants_not_found() -> ApiError {
    key_error(
        StatusCode::NOT_FOUND,
        "not_found",
        "not_found",
        "no such route",
    )
}

/// Only a well-formed `api_key` principal holding `browser:agent` obtains a key
/// grant. A user or a service is refused here even if it reached the route.
fn require_api_key(claims: &Claims) -> Result<&str, ApiError> {
    match claims.api_key_id() {
        Some(key_id) if claims.has_scope(API_KEY_SCOPE_BROWSER_AGENT) => Ok(key_id),
        _ => Err(key_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "api_key_principal_required",
            "this route accepts only an api key principal",
        )),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyGrantBudget {
    micro_usd: i64,
    max_steps: i32,
    max_runtime_s: i32,
}

/// `POST /v1/browser/key-grants`. Unknown fields are refused, so `kind`, `mode`,
/// `read_only`, `org_id` or any sensitive scope is a 400, never silently dropped.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueKeyGrantRequest {
    allowed_domains: Vec<String>,
    budget: KeyGrantBudget,
    #[serde(default)]
    zdr: bool,
    #[serde(default)]
    ttl_s: Option<i64>,
    #[serde(default)]
    session_key: Option<String>,
}

fn valid_session_key(session_key: &str) -> bool {
    (1..=MAX_SESSION_KEY_LEN).contains(&session_key.len())
        && session_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

/// The same host bounds as `/v1/browser/grants`, plus a mandatory budget and a
/// TTL that can only shorten. Returns the requested lifetime (0 = broker default).
fn validate_key_request(req: &IssueKeyGrantRequest) -> Result<i32, ApiError> {
    if req.allowed_domains.is_empty()
        || req.allowed_domains.len() > MAX_REQUEST_DOMAINS
        || req
            .allowed_domains
            .iter()
            .any(|domain| domain.trim().is_empty() || domain.len() > MAX_DOMAIN_LEN)
    {
        return Err(key_bad_request(
            "invalid_allowed_domains",
            "allowed_domains must name 1 to 32 bounded hosts",
        ));
    }
    let budget = &req.budget;
    if budget.micro_usd <= 0 || budget.max_steps <= 0 || budget.max_runtime_s <= 0 {
        return Err(key_bad_request(
            "api_key_grant_budget_required",
            "budget.micro_usd, budget.max_steps and budget.max_runtime_s must all be positive",
        ));
    }
    let ttl_seconds = match req.ttl_s {
        None => 0,
        Some(ttl) if (MIN_KEY_GRANT_TTL_S..=MAX_KEY_GRANT_TTL_S).contains(&ttl) => {
            i32::try_from(ttl).map_err(|_| key_bad_request("invalid_ttl", "ttl_s is out of range"))?
        }
        Some(_) => {
            return Err(key_bad_request(
                "invalid_ttl",
                "ttl_s must be between 30 and 900 seconds",
            ))
        }
    };
    if req
        .session_key
        .as_deref()
        .is_some_and(|session_key| !valid_session_key(session_key))
    {
        return Err(key_bad_request(
            "invalid_session_key",
            "session_key must be 1 to 96 characters of [A-Za-z0-9_.:-]",
        ));
    }
    Ok(ttl_seconds)
}

/// `google.rpc.Status` as the broker's `grpc-status-details-bin` carries it.
#[derive(Clone, PartialEq, prost::Message)]
struct RpcStatusDetails {
    #[prost(int32, tag = "1")]
    code: i32,
    #[prost(string, tag = "2")]
    message: String,
    #[prost(message, repeated, tag = "3")]
    details: Vec<prost_types::Any>,
}

/// `google.rpc.ErrorInfo` (only the fields read here; `metadata` is skipped).
#[derive(Clone, PartialEq, prost::Message)]
struct ErrorInfoDetail {
    #[prost(string, tag = "1")]
    reason: String,
    #[prost(string, tag = "2")]
    domain: String,
}

/// The ErrorInfo reason a broker refusal carries, if any.
fn error_info_reason(status: &tonic::Status) -> Option<String> {
    use prost::Message as _;
    let details = RpcStatusDetails::decode(status.details()).ok()?;
    details
        .details
        .iter()
        .filter(|any| any.type_url == ERROR_INFO_TYPE_URL)
        .find_map(|any| ErrorInfoDetail::decode(any.value.as_slice()).ok())
        .map(|info| info.reason)
        .filter(|reason| !reason.is_empty())
}

/// Map a broker status for a key-grant issue. Each key refusal has its own
/// reason; nothing the broker said is echoed.
fn key_issue_status_to_http(status: &tonic::Status) -> ApiError {
    let reason = error_info_reason(status);
    match (status.code(), reason.as_deref()) {
        (tonic::Code::PermissionDenied, Some(BROKER_REASON_KEY_GRANTS_DISABLED)) => key_error(
            StatusCode::FORBIDDEN,
            "api_key_grants_disabled",
            "api_key_grants_disabled",
            "api key browser grants are disabled",
        ),
        (tonic::Code::PermissionDenied, Some(BROKER_REASON_KEY_REVOKED)) => key_error(
            StatusCode::FORBIDDEN,
            "api_key_revoked",
            "api_key_revoked",
            "the api key is revoked",
        ),
        (tonic::Code::PermissionDenied, Some(BROKER_REASON_KEY_HOST_CEILING)) => key_error(
            StatusCode::FORBIDDEN,
            "api_key_host_ceiling",
            "api_key_host_ceiling",
            "allowed_domains are outside the api key's host ceiling",
        ),
        (tonic::Code::PermissionDenied, Some(BROKER_REASON_KEY_BUDGET_CEILING)) => key_error(
            StatusCode::FORBIDDEN,
            "api_key_budget_ceiling",
            "api_key_budget_ceiling",
            "the budget is over the api key's budget ceiling",
        ),
        (tonic::Code::PermissionDenied, Some(BROKER_REASON_SITE_DENIED)) => key_error(
            StatusCode::FORBIDDEN,
            "browser_grant_forbidden",
            "browser_site_denied",
            "the organization's site policy refuses these hosts",
        ),
        (tonic::Code::PermissionDenied, _) => key_error(
            StatusCode::FORBIDDEN,
            "browser_grant_forbidden",
            "browser_grant_forbidden",
            "the browser grant is refused",
        ),
        (tonic::Code::InvalidArgument | tonic::Code::FailedPrecondition, _) => {
            key_bad_request("browser_host_not_grantable", "the broker refused the request")
        }
        (tonic::Code::ResourceExhausted, _) => key_error(
            StatusCode::TOO_MANY_REQUESTS,
            "per_key_grant_cap",
            "per_key_grant_cap",
            "too many live browser grants for this api key or its owner",
        ),
        (tonic::Code::Unavailable | tonic::Code::DeadlineExceeded, _) => key_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "browser_broker_unavailable",
            "browser_broker_unavailable",
            "the browser broker is unavailable",
        ),
        _ => key_broker_error("browser_broker_error"),
    }
}

fn key_broker_error(reason: &str) -> ApiError {
    key_error(
        StatusCode::BAD_GATEWAY,
        "browser_broker_error",
        reason,
        "the browser broker answered unexpectedly",
    )
}

fn authenticated_key_browser_request<T>(
    message: T,
    bearer: &VerifiedApiKeyBrowserBearer,
) -> Result<tonic::Request<T>, ApiError> {
    let authorization = format!("Bearer {}", bearer.as_str())
        .parse()
        .map_err(|_| {
            key_error(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "invalid_browser_credential",
                "invalid browser credential",
            )
        })?;
    let mut request = tonic::Request::new(message);
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

fn missing_key_bearer() -> ApiError {
    key_error(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "browser_bearer_required",
        "x-browser-authorization with the api key's browser-broker token is required",
    )
}

/// `POST /v1/browser/key-grants` — issue an allow-list session grant for the
/// calling API key. The org is the token's; the owner and the key are the
/// broker's reading of the key bearer. Allowed under ZDR (the grant is
/// ephemeral, `z_` id); a ZDR response never names the hosts.
async fn issue_key_grant(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    key_bearer: Option<VerifiedApiKeyBrowserBearer>,
    body: Result<Json<IssueKeyGrantRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    if !key_grants_enabled(&state) {
        return Err(key_grants_not_found());
    }
    let key_id = require_api_key(&claims)?.to_owned();
    let key_bearer = key_bearer.ok_or_else(missing_key_bearer)?;
    let Json(req) = body.map_err(|_| {
        key_bad_request(
            "invalid_body",
            "the body must be { allowed_domains, budget, zdr?, ttl_s?, session_key? }",
        )
    })?;
    let ttl_seconds = validate_key_request(&req)?;
    // The key token carries the org's standing posture raised by the key's own flag; the
    // request can raise it again, never lower it. A waiver has no place on this route.
    let zdr = crate::zdr_posture::decide_for(&claims, &crate::zdr_posture::Route::plain(req.zdr))
        .map_err(|refusal| crate::zdr_posture::http(&refusal))?
        .zdr();

    let session_key = format!(
        "apikey-{}",
        req.session_key.unwrap_or_else(new_ulid)
    );
    let request = AcquireGrantRequest {
        session_key,
        mode: "cloud".to_owned(),
        org_id: claims.org_id.clone(),
        allowed_domains: req.allowed_domains,
        zdr,
        grant_kind: String::new(),
        budget_micro_usd: req.budget.micro_usd,
        budget_max_steps: req.budget.max_steps,
        budget_max_runtime_s: req.budget.max_runtime_s,
        budget_source: KEY_GRANT_BUDGET_SOURCE.to_owned(),
        ttl_seconds,
        ..Default::default()
    };
    let mut broker = state.browser_client.clone();
    let granted = broker
        .acquire_grant(authenticated_key_browser_request(request, &key_bearer)?)
        .await
        .map_err(|status| key_issue_status_to_http(&status))?
        .into_inner();

    // The broker stamps the key from the verified key bearer. A grant that does not carry this
    // key, or is not an ordinary allow-list session grant, is never handed out: released at once.
    let shape_ok = granted.key_id == key_id
        && matches!(granted.kind.as_str(), "" | "session")
        && matches!(granted.policy_mode.as_str(), "" | "allowlist");
    if !shape_ok {
        if let Ok(release) = authenticated_key_browser_request(
            RevokeGrantRequest {
                grant_id: granted.grant_id.clone(),
                reason: KEY_GRANT_REVOKE_REASON.to_owned(),
            },
            &key_bearer,
        ) {
            let _ = broker.revoke_grant(release).await;
        }
        tracing::warn!(org_id = %claims.org_id, "broker returned a key grant of another key or kind; released");
        return Err(key_broker_error("key_grant_mismatch"));
    }

    let zdr = zdr || granted.zdr;
    if zdr {
        // Under ZDR neither the grant id nor the key id is logged (the broker's own rule).
        info!(org_id = %claims.org_id, zdr = true, "browser key grant issued");
    } else {
        info!(
            org_id = %claims.org_id,
            grant_id = %granted.grant_id,
            key_id = %key_id,
            zdr = false,
            "browser key grant issued"
        );
    }
    let mut data = json!({
        "grant_id": granted.grant_id,
        "expires_at": expires_at_rfc3339(granted.expires_at.as_ref()),
        "budget": {
            "micro_usd": granted.budget_micro_usd,
            "max_steps": granted.budget_max_steps,
            "max_runtime_s": granted.budget_max_runtime_s,
        },
        "key_id": granted.key_id,
    });
    if !zdr {
        data["allowed_domains"] = json!(granted.allowed_domains);
    }
    Ok((StatusCode::CREATED, Json(json!({ "data": data }))))
}

/// `DELETE /v1/browser/key-grants/:grant_id` — release one of the calling key's
/// grants. 204 whether it was live, already gone, or never this key's: the
/// broker answers NotFound for a grant of another key or owner, and this route
/// never says whether an id exists.
async fn revoke_key_grant(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    key_bearer: Option<VerifiedApiKeyBrowserBearer>,
    Path(grant_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if !key_grants_enabled(&state) {
        return Err(key_grants_not_found());
    }
    require_api_key(&claims)?;
    let key_bearer = key_bearer.ok_or_else(missing_key_bearer)?;
    if !is_valid_grant_id(&grant_id) {
        return Err(key_bad_request("invalid_grant_id", "the grant id is malformed"));
    }
    let mut broker = state.browser_client.clone();
    let result = broker
        .revoke_grant(authenticated_key_browser_request(
            RevokeGrantRequest {
                grant_id,
                reason: KEY_GRANT_REVOKE_REASON.to_owned(),
            },
            &key_bearer,
        )?)
        .await;
    match result {
        Ok(_) => {
            info!(org_id = %claims.org_id, "browser key grant released");
            Ok(StatusCode::NO_CONTENT)
        }
        Err(status) if status.code() == tonic::Code::NotFound => Ok(StatusCode::NO_CONTENT),
        Err(status) => Err(match status.code() {
            tonic::Code::PermissionDenied
            | tonic::Code::Unavailable
            | tonic::Code::DeadlineExceeded => key_issue_status_to_http(&status),
            _ => key_broker_error("browser_broker_error"),
        }),
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
        DeriveRunGrantResponse, RenewGrantResponse, RevokeGrantResponse, ValidateGrantRequest,
        ValidateGrantResponse,
    };
    use tower::ServiceExt;

    #[derive(Clone, Default)]
    struct Recorder {
        acquired: Arc<Mutex<Vec<AcquireGrantRequest>>>,
        revoked: Arc<Mutex<Vec<RevokeGrantRequest>>>,
        renewed: Arc<Mutex<Vec<RenewGrantRequest>>>,
        authorizations: Arc<Mutex<Vec<String>>>,
        acquire_failure: Option<tonic::Code>,
        revoke_failure: Option<tonic::Code>,
        renew_failure: Option<tonic::Code>,
        /// An ErrorInfo reason attached to `acquire_failure` / `revoke_failure`.
        failure_reason: Option<&'static str>,
        /// The `key_id` the fake broker stamps on an acquired grant (empty when unset).
        respond_key_id: Option<String>,
    }

    /// A broker refusal the way the Go broker sends it: a status with an ErrorInfo detail.
    fn broker_status(code: tonic::Code, reason: Option<&str>) -> tonic::Status {
        use prost::Message as _;
        let Some(reason) = reason else {
            return tonic::Status::new(code, "test broker rejection");
        };
        let details = RpcStatusDetails {
            code: code as i32,
            message: "test broker rejection".to_owned(),
            details: vec![prost_types::Any {
                type_url: ERROR_INFO_TYPE_URL.to_owned(),
                value: ErrorInfoDetail {
                    reason: reason.to_owned(),
                    domain: String::new(),
                }
                .encode_to_vec(),
            }],
        };
        tonic::Status::with_details(
            code,
            "test broker rejection",
            tonic::codegen::Bytes::from(details.encode_to_vec()),
        )
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
                return Err(broker_status(code, self.failure_reason));
            }
            let id = if request.zdr { "z_abc" } else { "abc" };
            let human = request.grant_kind == "human_browse";
            Ok(tonic::Response::new(AcquireGrantResponse {
                grant_id: id.to_owned(),
                kind: if human { "human_browse" } else { "session" }.to_owned(),
                zdr: request.zdr,
                allowed_domains: request.allowed_domains,
                expires_at: Some(prost_types::Timestamp {
                    seconds: 1_800_000_000,
                    nanos: 0,
                }),
                policy_mode: if human { "open_web" } else { "allowlist" }.to_owned(),
                denied_domains: if human {
                    vec![".internal".to_owned(), "metadata.goog".to_owned()]
                } else {
                    Vec::new()
                },
                absolute_expires_at: Some(prost_types::Timestamp {
                    seconds: if human { 1_800_014_400 } else { 1_800_000_000 },
                    nanos: 0,
                }),
                budget_micro_usd: request.budget_micro_usd,
                budget_max_steps: request.budget_max_steps,
                budget_max_runtime_s: request.budget_max_runtime_s,
                key_id: self.respond_key_id.clone().unwrap_or_default(),
                ..Default::default()
            }))
        }

        async fn renew_grant(
            &self,
            request: tonic::Request<RenewGrantRequest>,
        ) -> Result<tonic::Response<RenewGrantResponse>, tonic::Status> {
            if let Some(value) = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
            {
                self.authorizations.lock().unwrap().push(value.to_owned());
            }
            self.renewed.lock().unwrap().push(request.into_inner());
            match self.renew_failure {
                Some(code) => Err(tonic::Status::new(code, "test broker rejection")),
                None => Ok(tonic::Response::new(RenewGrantResponse {
                    expires_at: Some(prost_types::Timestamp {
                        seconds: 1_800_000_900,
                        nanos: 0,
                    }),
                    absolute_expires_at: Some(prost_types::Timestamp {
                        seconds: 1_800_014_400,
                        nanos: 0,
                    }),
                })),
            }
        }

        async fn revoke_grant(
            &self,
            request: tonic::Request<RevokeGrantRequest>,
        ) -> Result<tonic::Response<RevokeGrantResponse>, tonic::Status> {
            if let Some(value) = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
            {
                self.authorizations.lock().unwrap().push(value.to_owned());
            }
            self.revoked.lock().unwrap().push(request.into_inner());
            match self.revoke_failure {
                Some(code) => Err(broker_status(code, self.failure_reason)),
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
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "auth-core".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            org_zdr: None,
            zdr_waiver: None,
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
    async fn issue_human_browse_passes_kind_and_binds_org() {
        let recorder = Recorder::default();
        let router = app(recorder.clone(), user_claims(false), true).await;
        let (status, body) = send(
            router,
            "POST",
            "/v1/browser/grants",
            json!({
                "kind": "human_browse",
                "org_id": "org-attacker",
                "session_key": "forged"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let seen = recorder.acquired.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].grant_kind, "human_browse");
        assert_eq!(seen[0].org_id, "org-a");
        assert!(seen[0].session_key.starts_with("ui-") && seen[0].session_key != "forged");
        assert!(seen[0].allowed_domains.is_empty(), "open web names no hosts");
        assert!(seen[0].parent_grant_id.is_empty() && seen[0].allowed_actions.is_empty());
        assert_eq!(
            recorder.authorizations.lock().unwrap().as_slice(),
            ["Bearer browser-token"]
        );
        assert_eq!(body["kind"], "human_browse");
        assert_eq!(body["policy_mode"], "open_web");
        assert_eq!(body["denied_domains"], json!([".internal", "metadata.goog"]));
        assert_eq!(body["expires_at"], "2027-01-15T08:00:00Z");
        assert_eq!(body["absolute_expires_at"], "2027-01-15T12:00:00Z");
    }

    #[tokio::test]
    async fn issue_session_grant_reports_an_allowlist_and_sends_no_kind() {
        let recorder = Recorder::default();
        let router = app(recorder.clone(), user_claims(false), true).await;
        let (status, body) = send(router, "POST", "/v1/browser/grants", issue_body()).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(recorder.acquired.lock().unwrap()[0].grant_kind, "");
        assert_eq!(body["policy_mode"], "allowlist");
        assert_eq!(body["denied_domains"], json!([]));
    }

    #[tokio::test]
    async fn issue_rejects_an_unknown_kind_and_human_browse_with_hosts() {
        for body in [
            json!({ "kind": "admin", "allowed_domains": ["example.com"] }),
            json!({ "kind": "attach" }),
            json!({ "kind": "human_browse", "allowed_domains": ["example.com"] }),
        ] {
            let recorder = Recorder::default();
            let router = app(recorder.clone(), user_claims(false), true).await;
            let (status, _) = send(router, "POST", "/v1/browser/grants", body.clone()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert!(recorder.acquired.lock().unwrap().is_empty(), "{body}");
        }
    }

    #[tokio::test]
    async fn issue_human_browse_is_allowed_under_zdr_and_stays_ephemeral() {
        let recorder = Recorder::default();
        let router = app(recorder.clone(), user_claims(true), true).await;
        let (status, body) = send(
            router,
            "POST",
            "/v1/browser/grants",
            json!({ "kind": "human_browse" }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["zdr"], true);
        assert_eq!(body["grant_id"], "z_abc");
        assert!(recorder.acquired.lock().unwrap()[0].zdr);
    }

    #[tokio::test]
    async fn renew_extends_with_the_browser_bearer_and_the_claims_org() {
        let recorder = Recorder::default();
        let router = app(recorder.clone(), user_claims(false), true).await;
        let (status, body) = send(
            router,
            "POST",
            "/v1/browser/grants/abc-123_x/renew",
            json!({ "org_id": "org-attacker", "grant_id": "forged" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["expires_at"], "2027-01-15T08:15:00Z");
        assert_eq!(body["absolute_expires_at"], "2027-01-15T12:00:00Z");
        assert!(
            body.get("grant_id").is_none(),
            "the response never echoes the grant id"
        );
        let seen = recorder.renewed.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].grant_id, "abc-123_x", "the id is the path's");
        assert_eq!(seen[0].org_id, "org-a", "the org is the claims', never the body's");
        assert_eq!(
            recorder.authorizations.lock().unwrap().as_slice(),
            ["Bearer browser-token"]
        );
    }

    #[tokio::test]
    async fn renew_maps_precondition_to_409() {
        for (code, want, error_code) in [
            (
                tonic::Code::FailedPrecondition,
                StatusCode::CONFLICT,
                "browser_grant_not_renewable",
            ),
            (
                tonic::Code::NotFound,
                StatusCode::CONFLICT,
                "browser_grant_not_renewable",
            ),
            (
                tonic::Code::PermissionDenied,
                StatusCode::FORBIDDEN,
                "browser_grant_forbidden",
            ),
            (
                tonic::Code::Unavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "browser_broker_unavailable",
            ),
            (
                tonic::Code::Unimplemented,
                StatusCode::BAD_GATEWAY,
                "browser_broker_error",
            ),
        ] {
            let recorder = Recorder {
                renew_failure: Some(code),
                ..Default::default()
            };
            let router = app(recorder, user_claims(false), true).await;
            let (status, body) =
                send(router, "POST", "/v1/browser/grants/abc/renew", Value::Null).await;
            assert_eq!(status, want, "{code:?}");
            assert_eq!(body["error"], error_code, "{code:?}");
        }
    }

    #[tokio::test]
    async fn renew_requires_browser_bearer() {
        let recorder = Recorder::default();
        let router = app(recorder.clone(), user_claims(false), false).await;
        let (status, _) = send(router, "POST", "/v1/browser/grants/abc/renew", Value::Null).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(recorder.renewed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn renew_rejects_service_principals_and_bad_ids_before_the_broker() {
        let recorder = Recorder::default();
        let router = app(recorder.clone(), service_claims(), true).await;
        let (status, _) = send(router, "POST", "/v1/browser/grants/abc/renew", Value::Null).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let router = app(recorder.clone(), user_claims(false), true).await;
        let (status, _) = send(router, "POST", "/v1/browser/grants/a%20b/renew", Value::Null).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(recorder.renewed.lock().unwrap().is_empty());
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

    // -----------------------------------------------------------------------
    // INNH-10 MP-3: key grants.
    // -----------------------------------------------------------------------

    fn key_claims(zdr: bool) -> Claims {
        Claims {
            sub: "apikey:key-1".to_owned(),
            user_id: String::new(),
            principal_type: Some("api_key".to_owned()),
            key_id: Some("key-1".to_owned()),
            acting_user_id: Some("user-a".to_owned()),
            scopes: vec!["browser:agent".to_owned()],
            ..user_claims(zdr)
        }
    }

    fn key_recorder() -> Recorder {
        Recorder {
            respond_key_id: Some("key-1".to_owned()),
            ..Default::default()
        }
    }

    async fn key_app(recorder: Recorder, claims: Claims, with_key_bearer: bool, flag: bool) -> Router {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(BrowserBrokerServer::new(recorder))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await;
        });
        let mut state = AppState::new();
        state.browser_key_grants = Some(flag);
        state.browser_client = BrowserBrokerClient::connect(format!("http://{address}"))
            .await
            .unwrap();
        let mut router = router().with_state(state).layer(Extension(claims));
        if with_key_bearer {
            router = router.layer(Extension(VerifiedApiKeyBrowserBearer::for_test("key-browser-token")));
        }
        router
    }

    fn key_body() -> Value {
        json!({
            "allowed_domains": ["example.com"],
            "budget": { "micro_usd": 500_000, "max_steps": 20, "max_runtime_s": 300 }
        })
    }

    #[tokio::test]
    async fn key_grants_flag_defaults_off_and_only_the_literals_enable_it() {
        for off in [None, Some(""), Some("0"), Some("off"), Some("false"), Some("yes"), Some("enabled")] {
            assert!(!key_grants_flag(off), "{off:?}");
        }
        for on in ["1", "true", "TRUE", "on", " On "] {
            assert!(key_grants_flag(Some(on)), "{on}");
        }
        assert_eq!(AppState::new().browser_key_grants, None, "production reads the env, default off");
    }

    #[tokio::test]
    async fn key_grants_accept_only_api_key_principal() {
        let recorder = key_recorder();
        let router = key_app(recorder.clone(), key_claims(false), true, true).await;
        let mut body = key_body();
        body["ttl_s"] = json!(120);
        body["session_key"] = json!("run-7");
        let (status, body) = send(router, "POST", "/v1/browser/key-grants", body).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let seen = recorder.acquired.lock().unwrap();
        assert_eq!(seen.len(), 1);
        let sent = &seen[0];
        assert_eq!(sent.org_id, "org-a", "the org is the token's");
        assert_eq!(sent.mode, "cloud");
        assert_eq!(sent.grant_kind, "", "an ordinary allow-list session grant");
        assert_eq!(sent.session_key, "apikey-run-7");
        assert_eq!(sent.allowed_domains, ["example.com"]);
        assert_eq!(
            (sent.budget_micro_usd, sent.budget_max_steps, sent.budget_max_runtime_s),
            (500_000, 20, 300)
        );
        assert_eq!(sent.budget_source, "request");
        assert_eq!(sent.ttl_seconds, 120);
        assert!(!sent.zdr && !sent.read_only);
        assert!(sent.parent_grant_id.is_empty() && sent.allowed_actions.is_empty());
        assert_eq!(
            recorder.authorizations.lock().unwrap().as_slice(),
            ["Bearer key-browser-token"],
            "the broker is called with the key's own browser-broker bearer"
        );
        assert_eq!(
            body,
            json!({ "data": {
                "grant_id": "abc",
                "expires_at": "2027-01-15T08:00:00Z",
                "allowed_domains": ["example.com"],
                "budget": { "micro_usd": 500_000, "max_steps": 20, "max_runtime_s": 300 },
                "key_id": "key-1",
            }})
        );
    }

    #[tokio::test]
    async fn user_and_service_are_refused_on_key_grants() {
        let unscoped = Claims {
            scopes: Vec::new(),
            ..key_claims(false)
        };
        let malformed = Claims {
            sub: "apikey:key-2".to_owned(),
            ..key_claims(false)
        };
        for (case, claims) in [
            ("user", user_claims(false)),
            ("service", service_claims()),
            ("key without browser:agent", unscoped),
            ("malformed key token", malformed),
        ] {
            for (method, path, body) in [
                ("POST", "/v1/browser/key-grants", key_body()),
                ("DELETE", "/v1/browser/key-grants/abc", Value::Null),
            ] {
                let recorder = key_recorder();
                let router = key_app(recorder.clone(), claims.clone(), true, true).await;
                let (status, body) = send(router, method, path, body).await;
                assert_eq!(status, StatusCode::FORBIDDEN, "{case} {method}");
                assert_eq!(body["error"]["details"]["reason"], "api_key_principal_required", "{case}");
                assert!(recorder.acquired.lock().unwrap().is_empty(), "{case}");
                assert!(recorder.revoked.lock().unwrap().is_empty(), "{case}");
            }
        }
    }

    #[tokio::test]
    async fn require_user_keeps_refusing_an_api_key() {
        assert!(require_user(&key_claims(false)).is_err());
        for path in ["/v1/browser/grants", "/v1/browser/grants/abc/renew"] {
            let recorder = Recorder::default();
            let router = app(recorder.clone(), key_claims(false), true).await;
            let (status, _) = send(router, "POST", path, issue_body()).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
            assert!(recorder.acquired.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn bounds_match_user_grants() {
        let budget = json!({ "micro_usd": 1, "max_steps": 1, "max_runtime_s": 1 });
        for (body, reason) in [
            (json!({ "allowed_domains": [], "budget": budget }), "invalid_allowed_domains"),
            (json!({ "allowed_domains": [""], "budget": budget }), "invalid_allowed_domains"),
            (json!({ "allowed_domains": ["  "], "budget": budget }), "invalid_allowed_domains"),
            (json!({ "allowed_domains": vec!["a.example.com"; 33], "budget": budget }), "invalid_allowed_domains"),
            (json!({ "allowed_domains": ["x".repeat(254)], "budget": budget }), "invalid_allowed_domains"),
            (json!({ "budget": budget }), "invalid_body"),
            (json!({ "allowed_domains": ["example.com"] }), "invalid_body"),
            (json!({ "allowed_domains": ["example.com"], "budget": { "micro_usd": 1, "max_steps": 1 } }), "invalid_body"),
            (json!({ "allowed_domains": ["example.com"], "budget": { "micro_usd": 0, "max_steps": 1, "max_runtime_s": 1 } }), "api_key_grant_budget_required"),
            (json!({ "allowed_domains": ["example.com"], "budget": { "micro_usd": 1, "max_steps": -1, "max_runtime_s": 1 } }), "api_key_grant_budget_required"),
            (json!({ "allowed_domains": ["example.com"], "budget": { "micro_usd": 1, "max_steps": 1, "max_runtime_s": 0 } }), "api_key_grant_budget_required"),
            (json!({ "allowed_domains": ["example.com"], "budget": budget, "ttl_s": 29 }), "invalid_ttl"),
            (json!({ "allowed_domains": ["example.com"], "budget": budget, "ttl_s": 901 }), "invalid_ttl"),
            (json!({ "allowed_domains": ["example.com"], "budget": budget, "session_key": "a b" }), "invalid_session_key"),
            // No kind, no mode, no read-only, no sensitive scope, no forged identity.
            (json!({ "allowed_domains": ["example.com"], "budget": budget, "kind": "human_browse" }), "invalid_body"),
            (json!({ "allowed_domains": ["example.com"], "budget": budget, "mode": "local" }), "invalid_body"),
            (json!({ "allowed_domains": ["example.com"], "budget": budget, "read_only": true }), "invalid_body"),
            (json!({ "allowed_domains": ["example.com"], "budget": budget, "org_id": "org-attacker" }), "invalid_body"),
            (json!({ "allowed_domains": ["example.com"], "budget": budget, "allowed_actions": ["download_ref"] }), "invalid_body"),
            (json!({ "allowed_domains": ["example.com"], "budget": budget, "parent_grant_id": "g" }), "invalid_body"),
        ] {
            let recorder = key_recorder();
            let router = key_app(recorder.clone(), key_claims(false), true, true).await;
            let (status, response) = send(router, "POST", "/v1/browser/key-grants", body.clone()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(response["error"]["code"], "invalid_request", "{body}");
            assert_eq!(response["error"]["details"]["reason"], reason, "{body}");
            assert!(recorder.acquired.lock().unwrap().is_empty(), "{body}");
        }
        // The edges of the user route's bounds are admitted here too.
        let recorder = key_recorder();
        let router = key_app(recorder.clone(), key_claims(false), true, true).await;
        let body = json!({
            "allowed_domains": vec!["a.example.com"; 32],
            "budget": budget,
            "ttl_s": 900,
        });
        let (status, response) = send(router, "POST", "/v1/browser/key-grants", body).await;
        assert_eq!(status, StatusCode::CREATED, "{response}");
    }

    #[tokio::test]
    async fn zdr_omits_hosts() {
        for (claims_zdr, request_zdr) in [(true, false), (false, true)] {
            let recorder = key_recorder();
            let router = key_app(recorder.clone(), key_claims(claims_zdr), true, true).await;
            let mut body = key_body();
            body["allowed_domains"] = json!(["secret-client.example.com"]);
            body["zdr"] = json!(request_zdr);
            let (status, body) = send(router, "POST", "/v1/browser/key-grants", body).await;
            assert_eq!(status, StatusCode::CREATED, "{body}");
            assert_eq!(body["data"]["grant_id"], "z_abc");
            assert!(body["data"].get("allowed_domains").is_none());
            assert!(
                !body.to_string().contains("secret-client"),
                "a ZDR response must not echo hosts: {body}"
            );
            assert!(recorder.acquired.lock().unwrap()[0].zdr, "the posture floor reaches the broker");
        }
    }

    #[tokio::test]
    async fn delete_is_idempotent_and_scoped_to_own_key() {
        for failure in [None, Some(tonic::Code::NotFound)] {
            let recorder = Recorder {
                revoke_failure: failure,
                ..key_recorder()
            };
            let router = key_app(recorder.clone(), key_claims(false), true, true).await;
            let (status, body) =
                send(router, "DELETE", "/v1/browser/key-grants/abc-123_x", Value::Null).await;
            assert_eq!(status, StatusCode::NO_CONTENT, "{failure:?}");
            assert_eq!(body, Value::Null, "nothing says whether the id existed");
            let seen = recorder.revoked.lock().unwrap();
            assert_eq!(seen[0].grant_id, "abc-123_x");
            assert_eq!(seen[0].reason, "run_closed");
            assert_eq!(
                recorder.authorizations.lock().unwrap().as_slice(),
                ["Bearer key-browser-token"],
                "the broker scopes the revoke to the key behind its own bearer"
            );
        }
        // Bad id and missing key bearer never reach the broker; other failures are typed.
        let recorder = key_recorder();
        let router = key_app(recorder.clone(), key_claims(false), true, true).await;
        let (status, _) = send(router, "DELETE", "/v1/browser/key-grants/a%20b", Value::Null).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let router = key_app(recorder.clone(), key_claims(false), false, true).await;
        let (status, body) = send(router, "DELETE", "/v1/browser/key-grants/abc", Value::Null).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["details"]["reason"], "browser_bearer_required");
        assert!(recorder.revoked.lock().unwrap().is_empty());
        let router = key_app(
            Recorder {
                revoke_failure: Some(tonic::Code::Unavailable),
                ..key_recorder()
            },
            key_claims(false),
            true,
            true,
        )
        .await;
        let (status, _) = send(router, "DELETE", "/v1/browser/key-grants/abc", Value::Null).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn flag_off_returns_403() {
        // The broker's own switch off: both routes answer the typed 403.
        for (method, path, body) in [
            ("POST", "/v1/browser/key-grants", key_body()),
            ("DELETE", "/v1/browser/key-grants/abc", Value::Null),
        ] {
            let recorder = Recorder {
                acquire_failure: Some(tonic::Code::PermissionDenied),
                revoke_failure: Some(tonic::Code::PermissionDenied),
                failure_reason: Some("API_KEY_GRANTS_DISABLED"),
                ..key_recorder()
            };
            let router = key_app(recorder, key_claims(false), true, true).await;
            let (status, body) = send(router, method, path, body).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method}");
            assert_eq!(body["error"]["code"], "api_key_grants_disabled", "{method}");
            assert_eq!(body["error"]["details"]["reason"], "api_key_grants_disabled");
        }
    }

    #[tokio::test]
    async fn gateway_flag_off_returns_404_before_anything_else() {
        for claims in [key_claims(false), user_claims(false)] {
            for (method, path, body) in [
                ("POST", "/v1/browser/key-grants", key_body()),
                ("DELETE", "/v1/browser/key-grants/abc", Value::Null),
            ] {
                let recorder = key_recorder();
                let router = key_app(recorder.clone(), claims.clone(), false, false).await;
                let (status, _) = send(router, method, path, body).await;
                assert_eq!(status, StatusCode::NOT_FOUND, "{method}");
                assert!(recorder.acquired.lock().unwrap().is_empty());
                assert!(recorder.revoked.lock().unwrap().is_empty());
            }
        }
    }

    #[tokio::test]
    async fn key_issue_requires_the_key_bearer() {
        let recorder = key_recorder();
        let router = key_app(recorder.clone(), key_claims(false), false, true).await;
        let (status, body) = send(router, "POST", "/v1/browser/key-grants", key_body()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["details"]["reason"], "browser_bearer_required");
        // A user's browser bearer is a different type and never satisfies the key route.
        let router = key_app(recorder.clone(), key_claims(false), false, true)
            .await
            .layer(Extension(VerifiedBrowserBearer::for_test("user-browser-token")));
        let (status, _) = send(router, "POST", "/v1/browser/key-grants", key_body()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(recorder.acquired.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn broker_key_refusals_map_to_typed_errors() {
        for (code, reason, want, error_code, detail) in [
            (tonic::Code::PermissionDenied, Some("API_KEY_REVOKED"), StatusCode::FORBIDDEN, "api_key_revoked", "api_key_revoked"),
            (tonic::Code::PermissionDenied, Some("API_KEY_HOST_CEILING"), StatusCode::FORBIDDEN, "api_key_host_ceiling", "api_key_host_ceiling"),
            (tonic::Code::PermissionDenied, Some("API_KEY_BUDGET_CEILING"), StatusCode::FORBIDDEN, "api_key_budget_ceiling", "api_key_budget_ceiling"),
            (tonic::Code::PermissionDenied, Some("BROWSER_SITE_DENIED"), StatusCode::FORBIDDEN, "browser_grant_forbidden", "browser_site_denied"),
            (tonic::Code::PermissionDenied, None, StatusCode::FORBIDDEN, "browser_grant_forbidden", "browser_grant_forbidden"),
            (tonic::Code::PermissionDenied, Some("SOMETHING_NEW"), StatusCode::FORBIDDEN, "browser_grant_forbidden", "browser_grant_forbidden"),
            (tonic::Code::InvalidArgument, None, StatusCode::BAD_REQUEST, "invalid_request", "browser_host_not_grantable"),
            (tonic::Code::ResourceExhausted, None, StatusCode::TOO_MANY_REQUESTS, "per_key_grant_cap", "per_key_grant_cap"),
            (tonic::Code::Unavailable, None, StatusCode::SERVICE_UNAVAILABLE, "browser_broker_unavailable", "browser_broker_unavailable"),
            (tonic::Code::Unauthenticated, None, StatusCode::BAD_GATEWAY, "browser_broker_error", "browser_broker_error"),
            (tonic::Code::Internal, Some("API_KEY_REVOKED"), StatusCode::BAD_GATEWAY, "browser_broker_error", "browser_broker_error"),
        ] {
            let recorder = Recorder {
                acquire_failure: Some(code),
                failure_reason: reason,
                ..key_recorder()
            };
            let router = key_app(recorder, key_claims(false), true, true).await;
            let (status, body) = send(router, "POST", "/v1/browser/key-grants", key_body()).await;
            assert_eq!(status, want, "{code:?} {reason:?}");
            assert_eq!(body["error"]["code"], error_code, "{code:?} {reason:?}");
            assert_eq!(body["error"]["details"]["reason"], detail, "{code:?} {reason:?}");
            assert!(
                !body.to_string().contains("test broker rejection"),
                "the broker's message is never echoed: {body}"
            );
        }
    }

    #[tokio::test]
    async fn a_grant_of_another_key_is_released_and_refused() {
        for respond in [None, Some("key-2")] {
            let recorder = Recorder {
                respond_key_id: respond.map(str::to_owned),
                ..Default::default()
            };
            let router = key_app(recorder.clone(), key_claims(false), true, true).await;
            let (status, body) = send(router, "POST", "/v1/browser/key-grants", key_body()).await;
            assert_eq!(status, StatusCode::BAD_GATEWAY, "{respond:?}");
            assert_eq!(body["error"]["details"]["reason"], "key_grant_mismatch");
            assert!(body.get("data").is_none());
            let revoked = recorder.revoked.lock().unwrap();
            assert_eq!(revoked.len(), 1, "the stray grant is released at once");
            assert_eq!(revoked[0].grant_id, "abc");
        }
    }
}
