use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use axum::{
    body::{Body, Bytes},
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE, LOCATION, SET_COOKIE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use hmac::{Hmac, KeyInit, Mac};
use reqwest::Method;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};

use crate::{
    auth::actor_with_defaults,
    config::AppState,
    contracts::ActionActor,
    envelope::{error, ok},
    middleware::{AuthenticatedUser, AuthorizedMembership},
};

/// Per-user TTL for the cached session-context lookup. Short enough that an org
/// switch reflects quickly, long enough to collapse the repeated user-core round
/// trips that billing / ingestions / session bootstrap would otherwise each make.
const SESSION_CONTEXT_TTL_SECS: u64 = 60;

/// The org the session context is scoped to: the session's active organization
/// (the org the user is currently acting as, from the validated Better Auth
/// session — never a client header) when present, else `None` so user-core
/// resolves the user's primary membership. Pure, so the cache-key and `x-org-id`
/// derivation below is unit-testable.
pub(crate) fn scope_org_id(user: &AuthenticatedUser) -> Option<&str> {
    user.active_org_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Resolve user-core's session-context for the authenticated user, cached per
/// `(user_id, active_org)` for [`SESSION_CONTEXT_TTL_SECS`]. Returns the unwrapped
/// context object (`{ userId, orgId, role, onboardingStatus, ... }`) or
/// `Value::Null` on failure.
///
/// Scoped to the session's active organization (forwarded to user-core as
/// `x-org-id`) so `role`/`onboardingStatus` reflect the org the user is currently
/// acting as rather than always the primary membership. The cache key includes
/// the active org, so switching orgs never serves a stale primary-org role; it
/// also includes the validated `user_id`, so there is no IDOR surface (a user
/// only reads their own context) and a disabled cache falls back to a live fetch.
pub(crate) async fn resolve_session_context(state: &AppState, user: &AuthenticatedUser) -> Value {
    let scope_org = scope_org_id(user);
    let key = crate::cache::cache_key(
        "session-context",
        &[user.user_id.as_str(), scope_org.unwrap_or("")],
    );
    if let Some(cached) = state
        .cache
        .lookup_within(&key, SESSION_CONTEXT_TTL_SECS)
        .await
    {
        return cached;
    }

    let context = fetch_session_context(state, user).await;
    if !context.is_null() {
        state.cache.store(&key, &context).await;
    }
    context
}

async fn fetch_session_context(state: &AppState, user: &AuthenticatedUser) -> Value {
    let (status, body) = fetch_session_context_response(state, user).await;
    if !status.is_success() {
        return Value::Null;
    }

    crate::envelope::unwrap_data(&body)
}

async fn fetch_session_context_response(
    state: &AppState,
    user: &AuthenticatedUser,
) -> (StatusCode, Value) {
    let scope_org = scope_org_id(user);
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!("{}/api/v1/me/session-context", state.user_core_url);
    // Forward the active org as x-org-id; user-core returns the membership role for
    // THAT org, falling back to the primary membership when `scope_org` is None.
    let (status, Json(body)) = proxy_json(
        state,
        Method::GET,
        &url,
        None,
        scope_org,
        Some(&actor),
        None,
    )
    .await;
    (status, body)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ActiveMembershipResolution {
    Member(AuthorizedMembership),
    Missing,
    AuthorityUnavailable,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MembershipAuthorityContext {
    user_id: String,
    #[serde(default, deserialize_with = "deserialize_present_string")]
    org_id: AuthorityOptionalString,
    #[serde(default, deserialize_with = "deserialize_present_string")]
    role: AuthorityOptionalString,
    onboarding_status: String,
}

#[derive(Debug, Default)]
enum AuthorityOptionalString {
    #[default]
    Missing,
    Value(String),
}

fn deserialize_present_string<'de, D>(deserializer: D) -> Result<AuthorityOptionalString, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(AuthorityOptionalString::Value)
}

fn active_membership_from_authority_body(
    user: &AuthenticatedUser,
    status: StatusCode,
    body: &[u8],
) -> ActiveMembershipResolution {
    if scope_org_id(user).is_none() {
        return ActiveMembershipResolution::Missing;
    }
    if !status.is_success() {
        return ActiveMembershipResolution::AuthorityUnavailable;
    }

    let Ok(context) = serde_json::from_slice::<MembershipAuthorityContext>(body) else {
        return ActiveMembershipResolution::AuthorityUnavailable;
    };
    if context.user_id.trim() != user.user_id.trim()
        || !matches!(
            context.onboarding_status.trim(),
            "CREATED" | "PROFILE_READY" | "COMPLETED"
        )
    {
        return ActiveMembershipResolution::AuthorityUnavailable;
    }

    let (organization_id, role) = match (context.org_id, context.role) {
        (AuthorityOptionalString::Value(organization_id), AuthorityOptionalString::Value(role)) => {
            (organization_id, role)
        }
        pair => {
            return match pair {
                (AuthorityOptionalString::Missing, AuthorityOptionalString::Missing) => {
                    ActiveMembershipResolution::Missing
                }
                _ => ActiveMembershipResolution::AuthorityUnavailable,
            };
        }
    };
    let organization_id = organization_id.trim();
    let role = role.trim().to_ascii_lowercase();
    if organization_id.is_empty()
        || scope_org_id(user) != Some(organization_id)
        || !matches!(role.as_str(), "owner" | "admin" | "member" | "viewer")
    {
        return ActiveMembershipResolution::AuthorityUnavailable;
    }

    ActiveMembershipResolution::Member(AuthorizedMembership {
        organization_id: organization_id.to_owned(),
        role,
    })
}

const MEMBERSHIP_AUTHORITY_MAX_BODY_BYTES: usize = 64 * 1024;

fn membership_authority_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("valid membership-authority HTTP client")
    })
}

pub(crate) async fn resolve_active_membership(
    state: &AppState,
    user: &AuthenticatedUser,
) -> ActiveMembershipResolution {
    if scope_org_id(user).is_none() {
        return ActiveMembershipResolution::Missing;
    }

    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!("{}/api/v1/me/session-context", state.user_core_url);
    let headers = user_core_delegation_headers(
        &state.user_core_service_token,
        &Method::GET,
        &url,
        &[],
        &actor,
        scope_org_id(user),
        user.user_image.as_deref().unwrap_or_default(),
        Utc::now(),
    );
    let mut request = membership_authority_http_client().get(url);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let Ok(response) = send_with_retry(request).await else {
        return ActiveMembershipResolution::AuthorityUnavailable;
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::SERVICE_UNAVAILABLE);
    if !status.is_success() {
        return ActiveMembershipResolution::AuthorityUnavailable;
    }
    if response
        .content_length()
        .is_some_and(|length| length > MEMBERSHIP_AUTHORITY_MAX_BODY_BYTES as u64)
    {
        return ActiveMembershipResolution::AuthorityUnavailable;
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return ActiveMembershipResolution::AuthorityUnavailable;
        };
        if body.len().saturating_add(chunk.len()) > MEMBERSHIP_AUTHORITY_MAX_BODY_BYTES {
            return ActiveMembershipResolution::AuthorityUnavailable;
        }
        body.extend_from_slice(&chunk);
    }
    active_membership_from_authority_body(user, status, &body)
}

/// Re-prime the per-(user, active-org) session-context cache from a live
/// user-core read, returning the fetched context (`Value::Null` on failure —
/// the stale entry was already deleted, so the next read stays a live fetch).
///
/// Deletion alone is not enough after a mutation the caller must observe
/// immediately (onboarding completion): a concurrent request can hold a
/// pre-mutation fetch in flight and re-store the stale context right after the
/// delete. Fetching after the mutation commits and storing that result makes
/// the very next `/session/current` read answer the new state.
pub(crate) async fn refresh_session_context_cache(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Value {
    let scope_org = scope_org_id(user);
    let key = crate::cache::cache_key(
        "session-context",
        &[user.user_id.as_str(), scope_org.unwrap_or("")],
    );
    let context = fetch_session_context(state, user).await;
    if !context.is_null() {
        state.cache.store(&key, &context).await;
    }
    context
}

pub(crate) async fn invalidate_session_context_cache(
    state: &AppState,
    user_id: &str,
    active_org_id: Option<&str>,
) {
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return;
    }

    let primary_key = crate::cache::cache_key("session-context", &[user_id, ""]);
    state.cache.delete(&primary_key).await;

    if let Some(org_id) = active_org_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let scoped_key = crate::cache::cache_key("session-context", &[user_id, org_id]);
        state.cache.delete(&scoped_key).await;
    }
}

/// The authenticated user's authoritative org id from the live membership
/// decision attached by `require_session` (never a client-supplied header or
/// cached session context). Empty only on intentional session-only flows.
pub(crate) async fn authorized_org_id(_state: &AppState, user: &AuthenticatedUser) -> String {
    user.authorized_membership
        .as_ref()
        .map(|membership| membership.organization_id.clone())
        .unwrap_or_default()
}

/// Retry once only when no connection was established. A generic request/send
/// error is ambiguous: the upstream may already have accepted a non-idempotent
/// request, so replaying it can duplicate external actions. Streaming bodies
/// that cannot be cloned also skip the retry.
pub(crate) async fn send_with_retry(
    builder: reqwest::RequestBuilder,
) -> Result<reqwest::Response, reqwest::Error> {
    let retry = builder.try_clone();
    match builder.send().await {
        Ok(resp) => Ok(resp),
        Err(err) => match retry {
            Some(retry_builder) if err.is_connect() => retry_builder.send().await,
            _ => Err(err),
        },
    }
}

pub(crate) async fn proxy_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: Option<&str>,
    actor: Option<&ActionActor>,
    content_type: Option<&str>,
) -> (StatusCode, Json<Value>) {
    let actor_present = actor.is_some();
    let actor = actor_with_defaults(actor);
    let is_user_core = same_upstream_origin(url, &state.user_core_url);
    let control_audience = control_service_audience(
        url,
        &state.org_core_url,
        &state.billing_core_url,
        &state.audit_core_url,
    );
    let body_bytes = body
        .as_ref()
        .map(serde_json::to_vec)
        .transpose()
        .unwrap_or_default()
        .unwrap_or_default();
    let mut headers = if is_user_core {
        user_core_delegation_headers(
            &state.user_core_service_token,
            &method,
            url,
            &body_bytes,
            &actor,
            org_id,
            "",
            Utc::now(),
        )
    } else if let Some(audience) = control_audience {
        let service_token = match audience {
            "org-core" => &state.org_core_service_token,
            "billing-core" => &state.billing_core_service_token,
            "audit-core" => &state.audit_core_service_token,
            _ => unreachable!("control audience is allow-listed"),
        };
        let delegated_org = org_id.map(str::trim).unwrap_or_default();
        if actor_present && !actor.user_id.trim().is_empty() && !delegated_org.is_empty() {
            let now = Utc::now();
            control_service_delegation_headers(
                service_token,
                audience,
                &method,
                url,
                &body_bytes,
                &actor,
                delegated_org,
                now,
                &delegation_nonce(now),
            )
        } else {
            BTreeMap::from([
                ("x-service-id".to_owned(), "verevon-gateway".to_owned()),
                ("x-service-token".to_owned(), service_token.to_owned()),
            ])
        }
    } else {
        BTreeMap::from([
            (
                "x-internal-api-key".to_owned(),
                state.internal_api_key.clone(),
            ),
            ("x-user-id".to_owned(), actor.user_id),
        ])
    };
    if !actor.user_email.is_empty() {
        headers.insert("x-user-email".to_owned(), actor.user_email);
    }
    if !actor.user_name.is_empty() {
        headers.insert("x-user-name".to_owned(), actor.user_name);
    }
    if !is_user_core && !actor.user_role.is_empty() {
        headers.insert("x-user-role".to_owned(), actor.user_role);
    }
    if let Some(org_id) = org_id.filter(|value| !value.trim().is_empty()) {
        headers.insert("x-org-id".to_owned(), org_id.trim().to_owned());
    }

    proxy_json_with_headers(state, method, url, body, headers, content_type).await
}

/// Forward an interactive user request with a gateway-minted audience token.
///
/// Unlike [`proxy_json`], this deliberately does not attach the shared internal
/// API key. Data Plane v2 must authorize the verified user claims and scopes in
/// the bearer token; a shared service credential must never broaden the user's
/// document visibility.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn proxy_user_bearer_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: Option<&str>,
    actor: &ActionActor,
    bearer: &str,
    content_type: Option<&str>,
) -> (StatusCode, Json<Value>) {
    proxy_user_bearer_json_with_extra_headers(
        state,
        method,
        url,
        body,
        org_id,
        actor,
        bearer,
        content_type,
        BTreeMap::new(),
    )
    .await
}

/// As `proxy_user_bearer_json`, plus caller-supplied headers.
///
/// `extra` exists for authority the upstream verifies for itself — today the
/// Data Plane `x-space-decision` bearer, which is a Control-signed decision
/// that retrieval-engine verifies against its own key set. It is added AFTER
/// the identity headers and deliberately cannot replace them: an upstream must
/// never learn who the caller is from a value this function was handed.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn proxy_user_bearer_json_with_extra_headers(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: Option<&str>,
    actor: &ActionActor,
    bearer: &str,
    content_type: Option<&str>,
    extra: BTreeMap<String, String>,
) -> (StatusCode, Json<Value>) {
    let bearer = bearer.trim();
    if bearer.is_empty() || bearer.chars().any(char::is_whitespace) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "delegated_auth_unavailable",
                "A scoped upstream authorization token could not be minted.",
            )),
        );
    }

    let mut headers = BTreeMap::from([
        ("authorization".to_owned(), format!("Bearer {bearer}")),
        ("x-user-id".to_owned(), actor.user_id.trim().to_owned()),
    ]);
    if let Some(org_id) = org_id.map(str::trim).filter(|value| !value.is_empty()) {
        headers.insert("x-org-id".to_owned(), org_id.to_owned());
    }
    if !actor.user_email.trim().is_empty() {
        headers.insert(
            "x-user-email".to_owned(),
            actor.user_email.trim().to_owned(),
        );
    }
    if !actor.user_name.trim().is_empty() {
        headers.insert("x-user-name".to_owned(), actor.user_name.trim().to_owned());
    }
    if !actor.user_role.trim().is_empty() {
        headers.insert("x-user-role".to_owned(), actor.user_role.trim().to_owned());
    }
    // Identity is settled above. Anything here is upstream-verified authority,
    // so it may add to the request but never rewrite who is making it.
    for (name, value) in extra {
        let name = name.trim().to_ascii_lowercase();
        if name.starts_with("x-user-")
            || name == "authorization"
            || name == "x-org-id"
            || name == "x-internal-api-key"
        {
            continue;
        }
        headers.insert(name, value);
    }

    proxy_json_with_headers(state, method, url, body, headers, content_type).await
}

#[allow(clippy::too_many_arguments)] // explicit fields mirror the signed cross-language contract
pub(crate) fn user_core_delegation_headers(
    service_token: &str,
    method: &Method,
    url: &str,
    body: &[u8],
    actor: &ActionActor,
    org_id: Option<&str>,
    avatar: &str,
    timestamp: DateTime<Utc>,
) -> BTreeMap<String, String> {
    service_delegation_headers(
        service_token,
        "verevon-gateway",
        "user-core",
        method,
        url,
        body,
        actor,
        org_id,
        avatar,
        timestamp,
        "v1",
        None,
    )
}

#[allow(clippy::too_many_arguments)] // explicit fields mirror the signed cross-language contract
fn service_delegation_headers(
    service_token: &str,
    principal: &str,
    audience: &str,
    method: &Method,
    url: &str,
    body: &[u8],
    actor: &ActionActor,
    org_id: Option<&str>,
    avatar: &str,
    timestamp: DateTime<Utc>,
    delegation_version: &str,
    nonce: Option<&str>,
) -> BTreeMap<String, String> {
    type HmacSha256 = Hmac<Sha256>;

    let timestamp = timestamp.to_rfc3339_opts(SecondsFormat::Secs, false);
    let uri = Url::parse(url)
        .map(|parsed| match parsed.query() {
            Some(query) => format!("{}?{query}", parsed.path()),
            None => parsed.path().to_owned(),
        })
        .unwrap_or_default();
    let org_id = org_id.map(str::trim).unwrap_or_default();
    let body_digest = URL_SAFE_NO_PAD.encode(Sha256::digest(body));
    let mut canonical_fields = vec![delegation_version, principal, audience, timestamp.as_str()];
    if let Some(nonce) = nonce {
        canonical_fields.push(nonce);
    }
    canonical_fields.extend([
        method.as_str(),
        uri.as_str(),
        actor.user_id.trim(),
        org_id,
        actor.user_email.trim(),
        actor.user_name.trim(),
        avatar.trim(),
        body_digest.as_str(),
    ]);
    let canonical = canonical_fields.join("\n");
    let mut mac = HmacSha256::new_from_slice(service_token.as_bytes())
        .expect("HMAC accepts arbitrary key lengths");
    mac.update(canonical.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

    let mut headers = BTreeMap::from([
        ("x-service-token".to_owned(), service_token.to_owned()),
        ("x-service-id".to_owned(), principal.to_owned()),
        ("x-user-id".to_owned(), actor.user_id.trim().to_owned()),
        ("x-org-id".to_owned(), org_id.to_owned()),
        (
            "x-user-email".to_owned(),
            actor.user_email.trim().to_owned(),
        ),
        ("x-user-name".to_owned(), actor.user_name.trim().to_owned()),
        ("x-user-avatar".to_owned(), avatar.trim().to_owned()),
        (
            "x-delegation-version".to_owned(),
            delegation_version.to_owned(),
        ),
        ("x-delegation-timestamp".to_owned(), timestamp),
        (
            "x-delegation-nonce".to_owned(),
            nonce.unwrap_or_default().to_owned(),
        ),
        ("x-delegation-body-sha256".to_owned(), body_digest),
        ("x-delegation-signature".to_owned(), signature),
    ]);
    headers.retain(|_, value| !value.is_empty());
    headers
}

#[allow(clippy::too_many_arguments)] // fields intentionally mirror the Go receiver contract
fn control_service_delegation_headers(
    service_token: &str,
    audience: &str,
    method: &Method,
    url: &str,
    body: &[u8],
    actor: &ActionActor,
    org_id: &str,
    timestamp: DateTime<Utc>,
    nonce: &str,
) -> BTreeMap<String, String> {
    type HmacSha256 = Hmac<Sha256>;

    let timestamp = timestamp.to_rfc3339_opts(SecondsFormat::Secs, false);
    let uri = Url::parse(url)
        .map(|parsed| match parsed.query() {
            Some(query) => format!("{}?{query}", parsed.path()),
            None => parsed.path().to_owned(),
        })
        .unwrap_or_default();
    let body_digest = URL_SAFE_NO_PAD.encode(Sha256::digest(body));
    let canonical = [
        "v3",
        "verevon-gateway",
        audience,
        timestamp.as_str(),
        nonce,
        method.as_str(),
        uri.as_str(),
        actor.user_id.trim(),
        org_id.trim(),
        actor.user_role.trim(),
        body_digest.as_str(),
    ]
    .join("\n");
    let mut mac = HmacSha256::new_from_slice(service_token.as_bytes())
        .expect("HMAC accepts arbitrary key lengths");
    mac.update(canonical.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

    let mut headers = BTreeMap::from([
        ("x-service-id".to_owned(), "verevon-gateway".to_owned()),
        ("x-service-token".to_owned(), service_token.to_owned()),
        ("x-user-id".to_owned(), actor.user_id.trim().to_owned()),
        ("x-org-id".to_owned(), org_id.trim().to_owned()),
        ("x-user-role".to_owned(), actor.user_role.trim().to_owned()),
        ("x-delegation-version".to_owned(), "v3".to_owned()),
        ("x-delegation-timestamp".to_owned(), timestamp),
        ("x-delegation-nonce".to_owned(), nonce.to_owned()),
        ("x-delegation-body-sha256".to_owned(), body_digest),
        ("x-delegation-signature".to_owned(), signature),
    ]);
    headers.retain(|_, value| !value.is_empty());
    headers
}

#[allow(clippy::too_many_arguments)]
fn notification_delegation_headers(
    service_token: &str,
    method: &Method,
    url: &str,
    body: &[u8],
    actor: &ActionActor,
    org_id: &str,
    timestamp: DateTime<Utc>,
    nonce: &str,
) -> BTreeMap<String, String> {
    v2_delegation_headers(
        service_token,
        "notification-core",
        method,
        url,
        body,
        actor,
        org_id,
        timestamp,
        nonce,
    )
}

#[allow(clippy::too_many_arguments)]
fn conversation_delegation_headers(
    service_token: &str,
    method: &Method,
    url: &str,
    body: &[u8],
    actor: &ActionActor,
    org_id: &str,
    timestamp: DateTime<Utc>,
    nonce: &str,
) -> BTreeMap<String, String> {
    v2_delegation_headers(
        service_token,
        "conversation-core",
        method,
        url,
        body,
        actor,
        org_id,
        timestamp,
        nonce,
    )
}

#[allow(clippy::too_many_arguments)]
fn v2_delegation_headers(
    service_token: &str,
    audience: &str,
    method: &Method,
    url: &str,
    body: &[u8],
    actor: &ActionActor,
    org_id: &str,
    timestamp: DateTime<Utc>,
    nonce: &str,
) -> BTreeMap<String, String> {
    type HmacSha256 = Hmac<Sha256>;

    let timestamp = timestamp.to_rfc3339_opts(SecondsFormat::Secs, false);
    let uri = Url::parse(url)
        .map(|parsed| match parsed.query() {
            Some(query) => format!("{}?{query}", parsed.path()),
            None => parsed.path().to_owned(),
        })
        .unwrap_or_default();
    let body_digest = URL_SAFE_NO_PAD.encode(Sha256::digest(body));
    let canonical = [
        "v2",
        "verevon-gateway",
        audience,
        timestamp.as_str(),
        nonce,
        method.as_str(),
        uri.as_str(),
        actor.user_id.trim(),
        org_id.trim(),
        actor.user_role.trim(),
        body_digest.as_str(),
    ]
    .join("\n");
    let mut mac = HmacSha256::new_from_slice(service_token.as_bytes())
        .expect("HMAC accepts arbitrary key lengths");
    mac.update(canonical.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

    let mut headers = BTreeMap::from([
        ("x-service-id".to_owned(), "verevon-gateway".to_owned()),
        ("x-user-id".to_owned(), actor.user_id.trim().to_owned()),
        ("x-org-id".to_owned(), org_id.trim().to_owned()),
        ("x-user-role".to_owned(), actor.user_role.trim().to_owned()),
        ("x-delegation-timestamp".to_owned(), timestamp),
        ("x-delegation-nonce".to_owned(), nonce.to_owned()),
        ("x-delegation-body-sha256".to_owned(), body_digest),
        ("x-delegation-signature".to_owned(), signature),
    ]);
    headers.retain(|_, value| !value.is_empty());
    headers
}

static DELEGATION_NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn notification_http_client() -> &'static reqwest::Client {
    NOTIFICATION_HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(25))
            .pool_idle_timeout(Duration::from_secs(20))
            .tcp_keepalive(Duration::from_secs(20))
            .build()
            .expect("build notification-core HTTP client")
    })
}

fn conversation_http_client() -> &'static reqwest::Client {
    // Both signed Application Plane clients require the same transport policy:
    // pinned caller-side origins and no redirect following with authority headers.
    notification_http_client()
}

fn delegation_nonce(now: DateTime<Utc>) -> String {
    let counter = DELEGATION_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = format!(
        "{}:{}:{}",
        now.timestamp_nanos_opt().unwrap_or_default(),
        std::process::id(),
        counter
    );
    URL_SAFE_NO_PAD.encode(Sha256::digest(seed.as_bytes()))
}

pub(crate) async fn proxy_notification_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: &str,
    actor: &ActionActor,
) -> (StatusCode, Json<Value>) {
    if !notification_target_is_configured_origin(url, &state.notification_core_url) {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "notification_upstream_target_rejected",
                "Notification request target does not match the configured notification-core origin",
            )),
        );
    }
    let body_bytes = body
        .as_ref()
        .map(serde_json::to_vec)
        .transpose()
        .unwrap_or_default()
        .unwrap_or_default();
    let now = Utc::now();
    let nonce = delegation_nonce(now);
    let headers = notification_delegation_headers(
        &state.notification_core_service_token,
        &method,
        url,
        &body_bytes,
        actor,
        org_id,
        now,
        &nonce,
    );
    proxy_json_with_client_and_headers(notification_http_client(), method, url, body, headers, None)
        .await
}

pub(crate) async fn proxy_conversation_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    user: &AuthenticatedUser,
    content_type: Option<&str>,
) -> (StatusCode, Json<Value>) {
    if !conversation_target_is_configured_origin(url, &state.conversation_core_url) {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "conversation_upstream_target_rejected",
                "Conversation request target does not match the configured conversation-core origin",
            )),
        );
    }

    // `require_session` attaches a fresh canonical membership on every sensitive
    // request. Reuse it here so Inbox does not perform a second authority lookup.
    let Some(membership) = user.authorized_membership.as_ref() else {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "conversation_membership_required",
                "An explicit active organization membership is required.",
            )),
        );
    };
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: membership.role.clone(),
    };
    let body_bytes = match body.as_ref().map(serde_json::to_vec).transpose() {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error(
                    "invalid_json",
                    "Conversation request body could not be serialized.",
                )),
            )
        }
    };
    let now = Utc::now();
    let nonce = delegation_nonce(now);
    let headers = conversation_delegation_headers(
        &state.conversation_core_service_token,
        &method,
        url,
        body_bytes.as_deref().unwrap_or_default(),
        &actor,
        &membership.organization_id,
        now,
        &nonce,
    );
    proxy_json_bytes_with_client_and_headers(
        conversation_http_client(),
        method,
        url,
        body_bytes.as_deref(),
        headers,
        content_type,
    )
    .await
}

fn notification_target_is_configured_origin(target: &str, configured_base: &str) -> bool {
    same_upstream_origin(target, configured_base)
}

fn conversation_target_is_configured_origin(target: &str, configured_base: &str) -> bool {
    same_upstream_origin(target, configured_base)
}

pub(crate) fn same_upstream_origin(target: &str, configured_base: &str) -> bool {
    let (Ok(target), Ok(base)) = (Url::parse(target), Url::parse(configured_base)) else {
        return false;
    };
    target.scheme() == base.scheme()
        && target.host_str() == base.host_str()
        && target.port_or_known_default() == base.port_or_known_default()
}

fn control_service_audience(
    target: &str,
    org_core_url: &str,
    billing_core_url: &str,
    audit_core_url: &str,
) -> Option<&'static str> {
    if same_upstream_origin(target, org_core_url) {
        Some("org-core")
    } else if same_upstream_origin(target, billing_core_url) {
        Some("billing-core")
    } else if same_upstream_origin(target, audit_core_url) {
        Some("audit-core")
    } else {
        None
    }
}

fn session_service_headers(
    service_token: &str,
    method: &Method,
    url: &str,
    body: &[u8],
    actor: &ActionActor,
    timestamp: DateTime<Utc>,
    nonce: &str,
) -> BTreeMap<String, String> {
    service_delegation_headers(
        service_token,
        "verevon-gateway",
        "session-core",
        method,
        url,
        body,
        actor,
        None,
        "",
        timestamp,
        "v2",
        Some(nonce),
    )
}

pub(crate) async fn proxy_session_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    actor: Option<&ActionActor>,
) -> (StatusCode, Json<Value>) {
    let actor = actor_with_defaults(actor);
    let body_bytes = body
        .as_ref()
        .map(serde_json::to_vec)
        .transpose()
        .unwrap_or_default()
        .unwrap_or_default();
    let now = Utc::now();
    let headers = session_service_headers(
        &state.session_core_service_token,
        &method,
        url,
        &body_bytes,
        &actor,
        now,
        &delegation_nonce(now),
    );
    proxy_json_with_headers(state, method, url, body, headers, None).await
}

async fn proxy_json_with_headers(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    headers: BTreeMap<String, String>,
    content_type: Option<&str>,
) -> (StatusCode, Json<Value>) {
    proxy_json_with_client_and_headers(&state.client, method, url, body, headers, content_type)
        .await
}

async fn proxy_json_with_client_and_headers(
    client: &reqwest::Client,
    method: Method,
    url: &str,
    body: Option<Value>,
    headers: BTreeMap<String, String>,
    content_type: Option<&str>,
) -> (StatusCode, Json<Value>) {
    let mut request = client.request(method, url);
    for (name, value) in headers {
        if !value.trim().is_empty() {
            request = request.header(name, value);
        }
    }
    if let Some(content_type) = content_type {
        request = request.header("content-type", content_type);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }

    match send_with_retry(request).await {
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let body = response.json::<Value>().await.unwrap_or(Value::Null);
            // On a non-success status with an empty / non-JSON upstream body, synthesize
            // a proper {error:{code,message}} envelope instead of forwarding `{}` with an
            // error status — otherwise the SPA gets a failing status it cannot explain.
            let needs_envelope = !status.is_success()
                && (body.is_null() || body.as_object().map(|o| o.is_empty()).unwrap_or(false));
            if needs_envelope {
                (
                    status,
                    Json(error(
                        "upstream_error",
                        format!("Upstream returned {}", status.as_u16()),
                    )),
                )
            } else {
                (status, Json(if body.is_null() { json!({}) } else { body }))
            }
        }
        Err(err) => {
            // The concrete reqwest error is the ONLY signal that distinguishes
            // connect-refused from timeout from a malformed response. Discarding
            // it turned a one-line diagnosis into hours of inference.
            tracing::error!(
                error = %err,
                is_connect = err.is_connect(),
                is_timeout = err.is_timeout(),
                is_request = err.is_request(),
                is_body = err.is_body(),
                url = %url,
                "upstream request failed before a response was received"
            );
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "upstream_unavailable",
                    "The upstream service is unavailable.",
                )),
            )
        }
    }
}

async fn proxy_json_bytes_with_client_and_headers(
    client: &reqwest::Client,
    method: Method,
    url: &str,
    body: Option<&[u8]>,
    headers: BTreeMap<String, String>,
    content_type: Option<&str>,
) -> (StatusCode, Json<Value>) {
    let mut request = client.request(method, url);
    for (name, value) in headers {
        if !value.trim().is_empty() {
            request = request.header(name, value);
        }
    }
    if body.is_some() {
        request = request.header("content-type", content_type.unwrap_or("application/json"));
    } else if let Some(content_type) = content_type {
        request = request.header("content-type", content_type);
    }
    if let Some(body) = body {
        request = request.body(body.to_vec());
    }

    match send_with_retry(request).await {
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let body = response.json::<Value>().await.unwrap_or(Value::Null);
            let needs_envelope = !status.is_success()
                && (body.is_null() || body.as_object().map(|o| o.is_empty()).unwrap_or(false));
            if needs_envelope {
                (
                    status,
                    Json(error(
                        "upstream_error",
                        format!("Upstream returned {}", status.as_u16()),
                    )),
                )
            } else {
                (status, Json(if body.is_null() { json!({}) } else { body }))
            }
        }
        Err(err) => {
            // The concrete reqwest error is the ONLY signal that distinguishes
            // connect-refused from timeout from a malformed response. Discarding
            // it turned a one-line diagnosis into hours of inference.
            tracing::error!(
                error = %err,
                is_connect = err.is_connect(),
                is_timeout = err.is_timeout(),
                is_request = err.is_request(),
                is_body = err.is_body(),
                url = %url,
                "upstream request failed before a response was received"
            );
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "upstream_unavailable",
                    "The upstream service is unavailable.",
                )),
            )
        }
    }
}

/// Proxy an auth request to auth-core, forwarding session cookies and returning Set-Cookie headers.
/// Session tokens are stripped from the response body — they live only in HttpOnly cookies.
pub(crate) async fn proxy_auth(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    cookie_header: Option<&str>,
    browser_origin: Option<&str>,
) -> Response {
    proxy_auth_with_headers(state, method, url, body, cookie_header, browser_origin, &[]).await
}

/// Proxy an auth request to auth-core with an explicit, tiny allowlist of extra
/// browser-originated headers. Keep this narrow: auth routes handle session
/// cookies, so arbitrary client headers must not become upstream authority.
pub(crate) async fn proxy_auth_with_headers(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    cookie_header: Option<&str>,
    browser_origin: Option<&str>,
    extra_headers: &[(&'static str, String)],
) -> Response {
    let mut req = state.client.request(method, url);

    if let Some(cookie) = cookie_header.filter(|c| !c.is_empty()) {
        req = req.header("cookie", cookie);
    }
    if let Some(origin) = browser_origin.filter(|value| !value.trim().is_empty()) {
        req = req.header("origin", origin.trim());
    }
    for (name, value) in extra_headers {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            req = req.header(*name, trimmed);
        }
    }

    if let Some(body) = body {
        req = req.json(&body);
    }

    match req.send().await {
        Ok(upstream) => {
            let status =
                StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

            let set_cookies: Vec<_> = upstream
                .headers()
                .get_all(SET_COOKIE)
                .iter()
                .cloned()
                .collect();

            let mut body = upstream.json::<Value>().await.unwrap_or_else(|_| json!({}));

            // Strip session tokens from auth response bodies — they belong only in
            // HttpOnly cookies. Better Auth's get-session nests the live token under
            // `session.token`, so strip the top-level keys (sign-in/up shape) AND the
            // nested ones; otherwise the same secret in the HttpOnly cookie is readable
            // by any page script (XSS exfiltration / session replay).
            strip_session_tokens(&mut body);

            let mut response = (status, Json(body)).into_response();
            for cookie_val in set_cookies {
                response.headers_mut().append(SET_COOKIE, cookie_val);
            }
            response
        }
        Err(err) => {
            tracing::error!(
                error = %err,
                is_connect = err.is_connect(),
                is_timeout = err.is_timeout(),
                is_request = err.is_request(),
                is_body = err.is_body(),
                url = %url,
                "upstream request failed before a response was received"
            );
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "upstream_unavailable",
                    "The upstream service is unavailable.",
                )),
            )
                .into_response()
        }
    }
}

const AUTH_CALLBACK_MAX_RESPONSE_BYTES: usize = 1024 * 1024;

fn auth_callback_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            // Provider authorization codes are single-use. Never follow or
            // retry callback redirects inside the gateway.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("valid auth callback HTTP client")
    })
}

/// Forward an opaque OAuth/OIDC/SAML callback to the configured Auth Core only.
/// The allowlists here are intentionally separate from the JSON auth proxy: a
/// callback must preserve redirects, form bodies, and every Set-Cookie header,
/// while never inheriting caller-supplied identity or internal-authority headers.
pub(crate) async fn proxy_auth_callback(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Bytes>,
    cookie_header: Option<&str>,
    content_type: Option<&str>,
) -> Response {
    if !same_upstream_origin(url, &state.auth_core_url) {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "auth_callback_target_rejected",
                "Authentication callback target was rejected.",
            )),
        )
            .into_response();
    }

    let mut request = auth_callback_http_client().request(method, url);
    if let Some(cookie) = cookie_header.filter(|value| !value.trim().is_empty()) {
        request = request.header("cookie", cookie.trim());
    }
    if let Some(content_type) = content_type.filter(|value| !value.trim().is_empty()) {
        request = request.header(CONTENT_TYPE, content_type.trim());
    }
    if let Some(body) = body {
        request = request.body(body);
    }

    let upstream = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            // reqwest's Display output can contain the full request URL, including
            // single-use OAuth codes/state. Record only non-sensitive categories.
            tracing::error!(
                timeout = error.is_timeout(),
                connect = error.is_connect(),
                request = error.is_request(),
                "auth callback: auth-core unavailable"
            );
            return (
                StatusCode::BAD_GATEWAY,
                Json(crate::envelope::error(
                    "auth_callback_unavailable",
                    "Authentication callback service is unavailable.",
                )),
            )
                .into_response();
        }
    };

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let headers = upstream.headers().clone();
    if upstream
        .content_length()
        .is_some_and(|length| length > AUTH_CALLBACK_MAX_RESPONSE_BYTES as u64)
    {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "auth_callback_response_too_large",
                "Authentication callback returned an invalid response.",
            )),
        )
            .into_response();
    }

    let mut stream = upstream.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "auth_callback_response_invalid",
                    "Authentication callback returned an invalid response.",
                )),
            )
                .into_response();
        };
        if body.len().saturating_add(chunk.len()) > AUTH_CALLBACK_MAX_RESPONSE_BYTES {
            return (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "auth_callback_response_too_large",
                    "Authentication callback returned an invalid response.",
                )),
            )
                .into_response();
        }
        body.extend_from_slice(&chunk);
    }

    let mut response = Response::builder()
        .status(status)
        .body(Body::from(body))
        .unwrap_or_else(|_| {
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "auth_callback_response_invalid",
                    "Authentication callback returned an invalid response.",
                )),
            )
                .into_response()
        });
    for name in [LOCATION, CONTENT_TYPE, CACHE_CONTROL] {
        if let Some(value) = headers.get(&name) {
            response.headers_mut().insert(name, value.clone());
        }
    }
    for cookie in headers.get_all(SET_COOKIE).iter() {
        response.headers_mut().append(SET_COOKIE, cookie.clone());
    }
    response
}

/// Normalize the browser origin for Better Auth's CSRF/origin checks.
///
/// Prefer the explicit `Origin` header. If the browser/proxy reports `null` or
/// omits it, fall back to the request `Referer` origin. Auth-core still applies
/// its own trusted-origin allowlist; this just preserves the public SPA origin
/// across the same-origin BFF hop.
pub(crate) fn browser_origin(headers: &HeaderMap) -> Option<String> {
    header_origin(headers, "origin").or_else(|| header_origin(headers, "referer"))
}

fn header_origin(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_http_origin)
}

fn normalize_http_origin(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return None;
    }

    let parsed = Url::parse(trimmed).ok()?;
    match parsed.scheme() {
        "http" | "https" => Some(parsed.origin().ascii_serialization()),
        _ => None,
    }
}

/// Remove session tokens from an auth response body in place — both top-level
/// `token`/`sessionToken` (sign-in/up shape) and nested `session.token`/
/// `session.sessionToken` (Better Auth get-session shape). Non-secret session
/// metadata (e.g. `expiresAt`) is preserved.
fn strip_session_tokens(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    obj.remove("token");
    obj.remove("sessionToken");
    if let Some(session) = obj.get_mut("session").and_then(Value::as_object_mut) {
        session.remove("token");
        session.remove("sessionToken");
    }
}

/// Proxy a request to integration-corev2 and normalize its `{success, data|error}` envelope
/// to the gateway's standard `{data}` / `{error:{code,message}}` shape.
/// Also maps 402/403 plan-gate errors to `PLAN_REQUIRED` error code.
pub(crate) async fn proxy_integration_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    user_id: &str,
) -> (StatusCode, Json<Value>) {
    let (status, Json(raw)) =
        proxy_bearer_json(state, method, url, body, bearer_token, user_id).await;

    if let Some(success) = raw.get("success").and_then(|v| v.as_bool()) {
        if success {
            let data = raw.get("data").cloned().unwrap_or_else(|| raw.clone());
            return (status, Json(ok(data)));
        }

        let msg = integration_error_message(&raw);
        let code = integration_error_code(&raw, status, &msg);
        return (status, Json(error(code, msg)));
    }

    (status, Json(raw))
}

/// Integration-core error codes the SPA is allowed to branch on by code
/// rather than by message. Everything else still collapses into the opaque
/// `integration_error` category, so an internal failure name never becomes a
/// browser-visible contract.
///
/// An entry belongs here only when the code is (a) stable and owned by
/// integration-corev2, (b) an expected state of a healthy workspace rather
/// than a fault, and (c) actionable in the UI. `no_sources_registered` is the
/// 409 that `POST /connections/{id}/sync` returns for a Microsoft connection
/// with no SharePoint/OneDrive library registered yet — the normal state of a
/// brand-new connection, which the SPA turns into "velg bibliotek" instead of
/// a generic failure.
const INTEGRATION_ERROR_CODE_ALLOWLIST: &[&str] = &["no_sources_registered"];

/// Choose the browser-visible error code for an integration-core failure. The
/// plan gate keeps precedence over the allow-list: a 402/403 must always route
/// the user to upgrade, whatever code the upstream attached.
fn integration_error_code(raw: &Value, status: StatusCode, message: &str) -> &'static str {
    if status.as_u16() == 402
        || status.as_u16() == 403
        || message.to_ascii_lowercase().contains("plan")
    {
        return "PLAN_REQUIRED";
    }
    let upstream_code = raw
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    INTEGRATION_ERROR_CODE_ALLOWLIST
        .iter()
        .copied()
        .find(|allowed| allowed.eq_ignore_ascii_case(upstream_code))
        .unwrap_or("integration_error")
}

/// Extract only the bounded, public message from integration-core's error
/// envelope. Core returns either a legacy error string or a structured
/// `{ code, message }` object. Never expose topology-bearing messages through
/// the browser gateway: callers still receive the stable integration category.
fn integration_error_message(raw: &Value) -> String {
    let message = raw
        .get("error")
        .and_then(|error| {
            error
                .as_str()
                .or_else(|| error.get("message").and_then(Value::as_str))
        })
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .filter(|message| !message.contains("http://") && !message.contains("https://"));

    message.unwrap_or("Integration service error").to_owned()
}

/// Proxy a JSON request with a Bearer token and x-user-id. Used for quarry-edge and similar
/// services that accept JWT auth rather than internal API key auth.
pub(crate) async fn proxy_bearer_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    user_id: &str,
) -> (StatusCode, Json<Value>) {
    proxy_bearer_json_with_headers(state, method, url, body, bearer_token, user_id, &[]).await
}

/// As `proxy_bearer_json`, with explicitly supplied server-owned headers. This
/// exists for target-bound authority envelopes; callers must never copy a
/// browser header into this list.
pub(crate) async fn proxy_bearer_json_with_headers(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    user_id: &str,
    extra_headers: &[(String, String)],
) -> (StatusCode, Json<Value>) {
    let mut req = state
        .client
        .request(method, url)
        .header("x-user-id", user_id);

    for (name, value) in extra_headers {
        req = req.header(name, value);
    }

    if let Some(token) = bearer_token {
        req = req.bearer_auth(token);
    }

    if let Some(b) = body {
        req = req.json(&b);
    }

    match send_with_retry(req).await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let b = resp.json::<Value>().await.unwrap_or_else(|_| json!({}));
            (status, Json(b))
        }
        Err(err) => {
            // The concrete reqwest error is the ONLY signal that distinguishes
            // connect-refused from timeout from a malformed response. Discarding
            // it turned a one-line diagnosis into hours of inference.
            tracing::error!(
                error = %err,
                is_connect = err.is_connect(),
                is_timeout = err.is_timeout(),
                is_request = err.is_request(),
                is_body = err.is_body(),
                url = %url,
                "upstream request failed before a response was received"
            );
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "upstream_unavailable",
                    "The upstream service is unavailable.",
                )),
            )
        }
    }
}

/// Proxy an SSE stream from model-gateway to the browser without buffering.
/// Injects a Bearer token and optional `Last-Event-ID` / ZDR headers before forwarding.
/// Returns a proper JSON error envelope if the upstream returns a non-2xx status.
#[allow(clippy::too_many_arguments)] // cohesive SSE-proxy request context
pub(crate) async fn proxy_sse_stream(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    last_event_id: Option<&str>,
    actor: Option<(&str, &str)>,
    zdr: bool,
) -> Response {
    proxy_sse_stream_with_data_plane(
        state,
        method,
        url,
        body,
        bearer_token,
        None,
        None,
        None,
        None,
        None,
        None,
        last_event_id,
        actor,
        zdr,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn proxy_sse_stream_with_session(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    session_bearer: Option<&str>,
    last_event_id: Option<&str>,
    actor: Option<(&str, &str)>,
    zdr: bool,
) -> Response {
    proxy_sse_stream_with_data_plane(
        state,
        method,
        url,
        body,
        bearer_token,
        None,
        None,
        None,
        None,
        session_bearer,
        None,
        last_event_id,
        actor,
        zdr,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn proxy_sse_stream_with_data_plane(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    data_plane_bearer: Option<&str>,
    inference_bearer: Option<&str>,
    execution_bearer: Option<&str>,
    cost_bearer: Option<&str>,
    session_bearer: Option<&str>,
    ingestion_bearer: Option<&str>,
    last_event_id: Option<&str>,
    actor: Option<(&str, &str)>,
    zdr: bool,
) -> Response {
    let mut req = state.streaming_client.request(method, url);

    if let Some(token) = bearer_token {
        req = req.bearer_auth(token);
    }
    if let Some(token) = data_plane_bearer
        .map(str::trim)
        .filter(|token| !token.is_empty() && !token.chars().any(char::is_whitespace))
    {
        req = req.header("x-data-plane-authorization", format!("Bearer {token}"));
    }
    if let Some(token) = ingestion_bearer
        .map(str::trim)
        .filter(|token| !token.is_empty() && !token.chars().any(char::is_whitespace))
    {
        req = req.header("x-ingestion-authorization", format!("Bearer {token}"));
    }
    if let Some(token) = cost_bearer
        .map(str::trim)
        .filter(|token| !token.is_empty() && !token.chars().any(char::is_whitespace))
    {
        req = req.header("x-cost-authorization", format!("Bearer {token}"));
    }
    if let Some(token) = inference_bearer
        .map(str::trim)
        .filter(|token| !token.is_empty() && !token.chars().any(char::is_whitespace))
    {
        req = req.header("x-inference-authorization", format!("Bearer {token}"));
    }
    if let Some(token) = execution_bearer
        .map(str::trim)
        .filter(|token| !token.is_empty() && !token.chars().any(char::is_whitespace))
    {
        req = req.header("x-execution-authorization", format!("Bearer {token}"));
    }
    if let Some(token) = session_bearer
        .map(str::trim)
        .filter(|token| !token.is_empty() && !token.chars().any(char::is_whitespace))
    {
        req = req.header("x-session-authorization", format!("Bearer {token}"));
    }

    if let Some((user_id, org_id)) = actor {
        req = req.header("x-user-id", user_id);
        if !org_id.trim().is_empty() {
            req = req.header("x-org-id", org_id);
        }
    }

    if let Some(lei) = last_event_id.filter(|v| !v.is_empty()) {
        req = req.header("last-event-id", lei);
    }

    if zdr {
        req = req.header("x-zdr", "true");
    }

    if let Some(b) = body {
        req = req.json(&b);
    }

    match req.send().await {
        Ok(resp) => {
            let status_u16 = resp.status().as_u16();
            if status_u16 >= 400 {
                let code = if status_u16 == 401 {
                    "unauthorized"
                } else {
                    "model_error"
                };
                let msg = format!("Model gateway returned {status_u16}");
                return (
                    StatusCode::from_u16(status_u16).unwrap_or(StatusCode::BAD_GATEWAY),
                    Json(error(code, msg)),
                )
                    .into_response();
            }

            // Forward upstream SSE bytes verbatim, interleaving a `: keep-alive`
            // comment every 15s of idle so intermediaries (nginx, the Docker bridge)
            // don't reap an idle stream. Comment lines (leading `:`) are ignored by
            // EventSource and by the SPA's sse.ts parser.
            let upstream = resp.bytes_stream();
            let body = Body::from_stream(async_stream::stream! {
                futures_util::pin_mut!(upstream);
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                ticker.tick().await; // discard the immediate first tick
                loop {
                    tokio::select! {
                        chunk = upstream.next() => match chunk {
                            Some(Ok(bytes)) => yield Ok::<Bytes, std::io::Error>(bytes),
                            Some(Err(_)) => {
                                yield Ok::<Bytes, std::io::Error>(sse_transport_error_event());
                                break;
                            }
                            None => break,
                        },
                        _ = ticker.tick() => {
                            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b": keep-alive\n\n"));
                        }
                    }
                }
            });
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .header("x-accel-buffering", "no")
                .body(body)
                .expect("infallible static headers")
        }
        Err(err) => {
            tracing::error!(
                error = %err,
                is_connect = err.is_connect(),
                is_timeout = err.is_timeout(),
                is_request = err.is_request(),
                is_body = err.is_body(),
                url = %url,
                "upstream request failed before a response was received"
            );
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "upstream_unavailable",
                    "The upstream service is unavailable.",
                )),
            )
                .into_response()
        }
    }
}

fn sse_transport_error_event() -> Bytes {
    Bytes::from_static(
        b"event: error\ndata: {\"error\":{\"code\":\"upstream_unavailable\",\"message\":\"The upstream service is unavailable.\"}}\n\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use wiremock::{
        matchers::{header as wm_header, method as wm_method, path as wm_path},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn midstream_sse_transport_failure_is_an_opaque_event() {
        let event = sse_transport_error_event();
        let text = std::str::from_utf8(&event).expect("static utf8");
        assert_eq!(
            text,
            "event: error\ndata: {\"error\":{\"code\":\"upstream_unavailable\",\"message\":\"The upstream service is unavailable.\"}}\n\n"
        );
        assert!(!text.contains("http://"));
        assert!(!text.contains("https://"));
    }

    #[test]
    fn integration_errors_preserve_bounded_structured_messages() {
        assert_eq!(
            integration_error_message(&json!({
                "success": false,
                "error": {
                    "code": "auth_core_unconfigured",
                    "message": "Integration authentication is temporarily unavailable."
                }
            })),
            "Integration authentication is temporarily unavailable."
        );
        assert_eq!(
            integration_error_message(&json!({
                "success": false,
                "error": { "message": "provider failed at https://internal.example.test" }
            })),
            "Integration service error"
        );
    }

    /// The allow-listed code reaches the SPA verbatim so Settings, Knowledge
    /// and onboarding can offer "pick a library" instead of a generic failure.
    #[test]
    fn allow_listed_integration_codes_reach_the_spa_verbatim() {
        let raw = json!({
            "success": false,
            "error": {
                "code": "no_sources_registered",
                "message": "No SharePoint or OneDrive library is registered for this organization yet."
            }
        });
        let message = integration_error_message(&raw);
        assert_eq!(
            integration_error_code(&raw, StatusCode::CONFLICT, &message),
            "no_sources_registered"
        );
    }

    /// Everything outside the allow-list keeps collapsing into the opaque
    /// category: an internal failure name is not a browser contract.
    #[test]
    fn unlisted_integration_codes_stay_opaque() {
        for code in [
            "sync_queue_failed",
            "token_broker_failed",
            "connections_list_failed",
        ] {
            let raw = json!({ "success": false, "error": { "code": code, "message": "Sync could not be queued." } });
            let message = integration_error_message(&raw);
            assert_eq!(
                integration_error_code(&raw, StatusCode::INTERNAL_SERVER_ERROR, &message),
                "integration_error",
                "code {code} must not reach the SPA"
            );
        }
        let legacy = json!({ "success": false, "error": "connection not found" });
        let message = integration_error_message(&legacy);
        assert_eq!(
            integration_error_code(&legacy, StatusCode::NOT_FOUND, &message),
            "integration_error"
        );
    }

    /// The plan gate keeps precedence: a 402/403 (or a plan-worded message)
    /// must route the user to upgrade even if an allow-listed code rides along.
    #[test]
    fn plan_gate_outranks_the_allow_list() {
        let raw = json!({ "success": false, "error": { "code": "no_sources_registered", "message": "Library sync requires a higher plan." } });
        let message = integration_error_message(&raw);
        assert_eq!(
            integration_error_code(&raw, StatusCode::FORBIDDEN, &message),
            "PLAN_REQUIRED"
        );
        assert_eq!(
            integration_error_code(&raw, StatusCode::CONFLICT, &message),
            "PLAN_REQUIRED",
            "a plan-worded message still routes to upgrade"
        );
    }
    use crate::middleware::AuthenticatedUser;
    use axum::http::{HeaderMap, HeaderValue};

    #[tokio::test]
    async fn notification_client_does_not_follow_redirects() {
        use axum::{response::Redirect, routing::get, Router};

        let app = Router::new()
            .route(
                "/redirect",
                get(|| async { Redirect::temporary("/target") }),
            )
            .route(
                "/target",
                get(|| async { "delegation must not reach here" }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind redirect test server");
        let address = listener.local_addr().expect("redirect test address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve redirect test")
        });

        let response = notification_http_client()
            .get(format!("http://{address}/redirect"))
            .send()
            .await
            .expect("notification request");
        server.abort();

        assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
    }

    #[tokio::test]
    async fn json_proxy_preserves_success_and_synthesizes_bounded_failure_envelopes() {
        let upstream = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/json"))
            .and(wm_header("x-scoped-test", "verified"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "created": true })))
            .expect(1)
            .mount(&upstream)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/empty-error"))
            .respond_with(ResponseTemplate::new(502).set_body_raw("bad gateway", "text/plain"))
            .expect(1)
            .mount(&upstream)
            .await;

        let client = reqwest::Client::new();
        let headers = BTreeMap::from([
            ("x-scoped-test".to_owned(), "verified".to_owned()),
            ("x-empty".to_owned(), "  ".to_owned()),
        ]);
        let (status, Json(body)) = proxy_json_with_client_and_headers(
            &client,
            Method::POST,
            &format!("{}/json", upstream.uri()),
            Some(json!({ "name": "example" })),
            headers,
            Some("application/json"),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body, json!({ "created": true }));

        let (status, Json(body)) = proxy_json_with_client_and_headers(
            &client,
            Method::GET,
            &format!("{}/empty-error", upstream.uri()),
            None,
            BTreeMap::new(),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(
            body.pointer("/error/code").and_then(Value::as_str),
            Some("upstream_error")
        );

        let (status, Json(body)) = proxy_json_with_client_and_headers(
            &client,
            Method::GET,
            "http://127.0.0.1:1/unavailable",
            None,
            BTreeMap::new(),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(
            body.pointer("/error/message").and_then(Value::as_str),
            Some("The upstream service is unavailable.")
        );
    }

    #[tokio::test]
    async fn byte_proxy_sets_content_type_and_keeps_error_details_opaque() {
        let upstream = MockServer::start().await;
        Mock::given(wm_method("PUT"))
            .and(wm_path("/bytes"))
            .and(wm_header("content-type", "application/octet-stream"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "stored": true })))
            .expect(1)
            .mount(&upstream)
            .await;

        let client = reqwest::Client::new();
        let (status, Json(body)) = proxy_json_bytes_with_client_and_headers(
            &client,
            Method::PUT,
            &format!("{}/bytes", upstream.uri()),
            Some(b"opaque payload"),
            BTreeMap::new(),
            Some("application/octet-stream"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({ "stored": true }));

        let (status, Json(body)) = proxy_json_bytes_with_client_and_headers(
            &client,
            Method::POST,
            "http://127.0.0.1:1/secret-path",
            Some(b"secret-body"),
            BTreeMap::new(),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(!body.to_string().contains("secret"));
        assert!(!body.to_string().contains("127.0.0.1"));
    }

    #[tokio::test]
    async fn bearer_proxy_forwards_verified_identity_and_fails_closed_on_transport_error() {
        let upstream = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/bearer"))
            .and(wm_header("authorization", "Bearer scoped-token"))
            .and(wm_header("x-user-id", "verified-user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "allowed": true })))
            .expect(1)
            .mount(&upstream)
            .await;
        let state = crate::tests::test_state(false);

        let (status, Json(body)) = proxy_bearer_json(
            &state,
            Method::GET,
            &format!("{}/bearer", upstream.uri()),
            None,
            Some("scoped-token"),
            "verified-user",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({ "allowed": true }));

        let (status, Json(body)) = proxy_bearer_json(
            &state,
            Method::POST,
            "http://127.0.0.1:1/unavailable",
            Some(json!({ "secret": true })),
            None,
            "verified-user",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(
            body.pointer("/error/code").and_then(Value::as_str),
            Some("upstream_unavailable")
        );
    }

    #[tokio::test]
    async fn auth_proxy_strips_body_tokens_and_preserves_http_only_cookie_headers() {
        let auth = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/sign-in"))
            .and(wm_header("cookie", "better-auth.session=verified"))
            .and(wm_header("origin", "https://verevon.example"))
            .and(wm_header("x-captcha-response", "captcha-proof"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("set-cookie", "session=one; HttpOnly; Secure")
                    .append_header("set-cookie", "csrf=two; HttpOnly; Secure")
                    .set_body_json(json!({
                        "token": "top-secret",
                        "sessionToken": "top-session-secret",
                        "session": {
                            "token": "nested-secret",
                            "sessionToken": "nested-session-secret",
                            "expiresAt": "2026-07-15T00:00:00Z"
                        }
                    })),
            )
            .expect(1)
            .mount(&auth)
            .await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();

        let response = proxy_auth_with_headers(
            &state,
            Method::POST,
            &format!("{}/sign-in", auth.uri()),
            Some(json!({ "email": "user@example.com" })),
            Some("better-auth.session=verified"),
            Some("https://verevon.example"),
            &[("x-captcha-response", " captcha-proof ".to_owned())],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get_all(SET_COOKIE).iter().count(), 2);
        let body: Value = serde_json::from_slice(
            &response
                .into_body()
                .collect()
                .await
                .expect("auth body")
                .to_bytes(),
        )
        .unwrap();
        assert!(body.get("token").is_none());
        assert!(body.pointer("/session/token").is_none());
        assert_eq!(
            body.pointer("/session/expiresAt").and_then(Value::as_str),
            Some("2026-07-15T00:00:00Z")
        );

        let unavailable = proxy_auth(
            &state,
            Method::GET,
            "http://127.0.0.1:1/unavailable",
            None,
            None,
            None,
        )
        .await;
        assert_eq!(unavailable.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn auth_callback_is_origin_pinned_bounded_and_redirect_transparent() {
        let auth = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/callback"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", "https://verevon.example/auth/complete")
                    .insert_header("content-type", "text/plain")
                    .append_header("set-cookie", "session=verified; HttpOnly; Secure")
                    .set_body_string("redirecting"),
            )
            .expect(1)
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/large"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![
                b'x';
                AUTH_CALLBACK_MAX_RESPONSE_BYTES
                    + 1
            ]))
            .expect(1)
            .mount(&auth)
            .await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();

        let rejected = proxy_auth_callback(
            &state,
            Method::GET,
            "https://attacker.example/callback",
            None,
            None,
            None,
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::BAD_GATEWAY);

        let response = proxy_auth_callback(
            &state,
            Method::POST,
            &format!("{}/callback", auth.uri()),
            Some(Bytes::from_static(b"code=opaque&state=opaque")),
            Some("better-auth.session=verified"),
            Some("application/x-www-form-urlencoded"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(
            response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("https://verevon.example/auth/complete")
        );
        assert_eq!(response.headers().get_all(SET_COOKIE).iter().count(), 1);

        let oversized = proxy_auth_callback(
            &state,
            Method::GET,
            &format!("{}/large", auth.uri()),
            None,
            None,
            None,
        )
        .await;
        assert_eq!(oversized.status(), StatusCode::BAD_GATEWAY);
    }

    fn user_with_active_org(active: Option<&str>) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "u@example.com".to_owned(),
            user_name: "U".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: None,
            active_org_id: active.map(str::to_owned),
            authorized_membership: None,
        }
    }

    #[test]
    fn scope_org_id_uses_active_org_when_set() {
        assert_eq!(
            scope_org_id(&user_with_active_org(Some("org-active"))),
            Some("org-active")
        );
    }

    #[test]
    fn scope_org_id_trims_and_treats_blank_as_none() {
        assert_eq!(
            scope_org_id(&user_with_active_org(Some("  org-x  "))),
            Some("org-x")
        );
        assert_eq!(scope_org_id(&user_with_active_org(Some("   "))), None);
        assert_eq!(scope_org_id(&user_with_active_org(None)), None);
    }

    #[test]
    fn session_context_cache_key_differs_by_active_org() {
        // Role/onboarding now differ per active org, so the cache must not collapse
        // two different active orgs (or active vs primary) onto one entry — that was
        // the stale-role bug. This mirrors the key built in resolve_session_context.
        let key = |user: &AuthenticatedUser| {
            crate::cache::cache_key(
                "session-context",
                &[user.user_id.as_str(), scope_org_id(user).unwrap_or("")],
            )
        };
        let primary = key(&user_with_active_org(None));
        let org_a = key(&user_with_active_org(Some("org-a")));
        let org_b = key(&user_with_active_org(Some("org-b")));
        assert_ne!(primary, org_a);
        assert_ne!(org_a, org_b);
        // A blank active org collapses onto the primary (None) key.
        assert_eq!(primary, key(&user_with_active_org(Some("  "))));
    }

    #[test]
    fn active_membership_uses_control_role_and_requires_exact_scope() {
        let user = AuthenticatedUser {
            auth_role: Some("superadmin".to_owned()),
            ..user_with_active_org(Some("org-active"))
        };
        let context = json!({
            "userId": "user-1",
            "orgId": "org-active",
            "role": "member",
            "onboardingStatus": "COMPLETED"
        });
        let ActiveMembershipResolution::Member(membership) = active_membership_from_authority_body(
            &user,
            StatusCode::OK,
            &serde_json::to_vec(&context).unwrap(),
        ) else {
            panic!("matching Control membership must be accepted")
        };
        assert_eq!(membership.organization_id, "org-active");
        assert_eq!(membership.role, "member");

        let wrong_org = json!({
            "userId": "user-1",
            "orgId": "org-primary",
            "role": "owner",
            "onboardingStatus": "COMPLETED"
        });
        assert_eq!(
            active_membership_from_authority_body(
                &user,
                StatusCode::OK,
                &serde_json::to_vec(&wrong_org).unwrap(),
            ),
            ActiveMembershipResolution::AuthorityUnavailable
        );
    }

    #[test]
    fn active_membership_requires_an_explicit_active_organization() {
        let user = user_with_active_org(None);
        let context = json!({
            "userId": "user-1",
            "orgId": "org-primary",
            "role": "owner",
            "onboardingStatus": "COMPLETED"
        });

        assert_eq!(
            active_membership_from_authority_body(
                &user,
                StatusCode::OK,
                &serde_json::to_vec(&context).unwrap(),
            ),
            ActiveMembershipResolution::Missing
        );
    }

    #[test]
    fn active_membership_non_success_is_authority_unavailable() {
        let user = user_with_active_org(Some("org-active"));
        for authority_status in [
            StatusCode::TEMPORARY_REDIRECT,
            StatusCode::UNAUTHORIZED,
            StatusCode::BAD_GATEWAY,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::SERVICE_UNAVAILABLE,
        ] {
            assert_eq!(
                active_membership_from_authority_body(&user, authority_status, b""),
                ActiveMembershipResolution::AuthorityUnavailable
            );
        }
    }

    #[test]
    fn active_membership_absence_is_distinct_from_authority_failure() {
        let user = user_with_active_org(Some("org-active"));
        assert_eq!(
            active_membership_from_authority_body(
                &user,
                StatusCode::OK,
                br#"{"userId":"user-1","onboardingStatus":"COMPLETED"}"#,
            ),
            ActiveMembershipResolution::Missing
        );
    }

    #[test]
    fn active_membership_malformed_success_is_authority_unavailable() {
        let user = user_with_active_org(Some("org-active"));
        for body in [
            br#"{"unexpected":"shape"}"#.as_slice(),
            br#"{"userId":"user-1","orgId":null,"role":"member","onboardingStatus":"COMPLETED"}"#.as_slice(),
            br#"{"userId":"user-1","orgId":"org-active","role":"member","onboardingStatus":"COMPLETED","extra":true}"#.as_slice(),
            br#"{"userId":"user-1","orgId":"org-active","role":"member","onboardingStatus":"COMPLETED"}{}"#.as_slice(),
        ] {
            assert_eq!(
                active_membership_from_authority_body(&user, StatusCode::OK, body),
                ActiveMembershipResolution::AuthorityUnavailable
            );
        }
    }

    #[test]
    fn active_membership_rejects_mismatch_partial_and_unknown_role() {
        let user = user_with_active_org(Some("org-1"));
        for context in [
            json!({"userId":"user-1","orgId":"","role":"member","onboardingStatus":"COMPLETED"}),
            json!({"userId":"user-1","orgId":"org-1","onboardingStatus":"COMPLETED"}),
            json!({"userId":"user-1","orgId":"org-1","role":"","onboardingStatus":"COMPLETED"}),
            json!({"userId":"user-1","orgId":"org-1","role":"superadmin","onboardingStatus":"COMPLETED"}),
            json!({"userId":"other-user","orgId":"org-1","role":"owner","onboardingStatus":"COMPLETED"}),
        ] {
            assert_eq!(
                active_membership_from_authority_body(
                    &user,
                    StatusCode::OK,
                    &serde_json::to_vec(&context).unwrap(),
                ),
                ActiveMembershipResolution::AuthorityUnavailable
            );
        }
    }

    #[test]
    fn session_service_headers_use_scoped_credential_without_shared_authority() {
        use chrono::{TimeZone, Utc};

        let actor = ActionActor {
            user_id: "verified-user".to_owned(),
            user_email: "verified@example.com".to_owned(),
            user_name: "Verified User".to_owned(),
            user_role: "admin".to_owned(),
        };

        let timestamp = Utc
            .with_ymd_and_hms(2026, 7, 11, 2, 0, 0)
            .single()
            .expect("fixed timestamp");
        let headers = session_service_headers(
            "0123456789abcdef0123456789abcdef",
            &Method::GET,
            "http://session-core:3017/api/v1/sessions/current",
            &[],
            &actor,
            timestamp,
            "nonce-fixed-vector-000000001",
        );

        assert_eq!(
            headers.get("x-service-token"),
            Some(&"0123456789abcdef0123456789abcdef".to_owned())
        );
        assert_eq!(headers.get("x-user-id"), Some(&"verified-user".to_owned()));
        assert!(headers.contains_key("x-delegation-timestamp"));
        assert_eq!(
            headers.get("x-delegation-nonce").map(String::as_str),
            Some("nonce-fixed-vector-000000001")
        );
        assert!(headers.contains_key("x-delegation-body-sha256"));
        assert_eq!(
            headers.get("x-delegation-signature").map(String::as_str),
            Some("JMYH35hoU13_zzSt39WJPqRIH3neTFfrsPI1HwqIZ2U")
        );
        assert!(!headers.contains_key("x-internal-api-key"));
        assert!(!headers.contains_key("x-user-role"));
    }

    #[test]
    fn user_core_self_delegation_signature_matches_cross_language_contract() {
        use chrono::{TimeZone, Utc};

        let actor = ActionActor {
            user_id: "user-1".to_owned(),
            user_email: "verified@example.com".to_owned(),
            user_name: "Verified User".to_owned(),
            user_role: "admin".to_owned(),
        };
        let body = br#"{"theme":"dark"}"#;
        let timestamp = Utc
            .with_ymd_and_hms(2026, 7, 11, 2, 0, 0)
            .single()
            .expect("fixed timestamp");

        let headers = user_core_delegation_headers(
            "0123456789abcdef0123456789abcdef",
            &Method::PATCH,
            "http://user-core:3012/api/v1/preferences?view=all",
            body,
            &actor,
            Some("org-1"),
            "",
            timestamp,
        );

        assert_eq!(
            headers.get("x-delegation-body-sha256").map(String::as_str),
            Some("D0-H20VnIyp_F1aqFTTsExR3eznDv1IJ-Hz5c5Mhzdw")
        );
        assert_eq!(
            headers.get("x-delegation-signature").map(String::as_str),
            Some("YscmqUN5kNSEAFIU2hafAF2UV87glPCdL_CnjII5V5k")
        );
        assert_eq!(
            headers.get("x-delegation-timestamp").map(String::as_str),
            Some("2026-07-11T02:00:00+00:00")
        );
        assert!(!headers.contains_key("x-user-role"));
    }

    #[test]
    fn control_service_v3_delegation_binds_audience_role_query_body_and_nonce() {
        use chrono::{TimeZone, Utc};

        let actor = ActionActor {
            user_id: "user-1".to_owned(),
            user_email: "verified@example.com".to_owned(),
            user_name: "Verified User".to_owned(),
            user_role: "admin".to_owned(),
        };
        let body = br#"{"plan":"pro"}"#;
        let timestamp = Utc
            .with_ymd_and_hms(2026, 7, 14, 20, 0, 0)
            .single()
            .expect("fixed timestamp");

        let headers = control_service_delegation_headers(
            "0123456789abcdef0123456789abcdef",
            "billing-core",
            &Method::POST,
            "http://billing-core:3014/api/v1/billing/orgs/org-1/checkout-session?mode=embed",
            body,
            &actor,
            "org-1",
            timestamp,
            "fixed-nonce-1234567890",
        );

        assert_eq!(
            headers.get("x-service-id").map(String::as_str),
            Some("verevon-gateway")
        );
        assert_eq!(
            headers.get("x-service-token").map(String::as_str),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            headers.get("x-delegation-version").map(String::as_str),
            Some("v3")
        );
        assert_eq!(
            headers.get("x-user-role").map(String::as_str),
            Some("admin")
        );
        assert_eq!(
            headers.get("x-delegation-body-sha256").map(String::as_str),
            Some("ApxA0uXOJFNQhvraZ-s-yFofgWfVqZ6reRfsBXYSbpk")
        );
        assert_eq!(
            headers.get("x-delegation-signature").map(String::as_str),
            Some("mHWFgO2sgeHul--jSM9dYL7mOs0mt_x3rxHc58wCgrk")
        );
        assert!(!headers.contains_key("x-internal-api-key"));
    }

    #[test]
    fn notification_delegation_binds_scope_role_nonce_and_body_without_shared_key() {
        use chrono::{TimeZone, Utc};

        let actor = ActionActor {
            user_id: "user-1".to_owned(),
            user_email: "verified@example.com".to_owned(),
            user_name: "Verified User".to_owned(),
            user_role: "admin".to_owned(),
        };
        let body = br#"{"organization_id":"org-1","recipient":{"kind":"user","id":"user-1"}}"#;
        let timestamp = Utc
            .with_ymd_and_hms(2026, 7, 13, 12, 0, 0)
            .single()
            .expect("fixed timestamp");

        let headers = notification_delegation_headers(
            "0123456789abcdef0123456789abcdef",
            &Method::POST,
            "http://notification-core:3140/api/v1/notification-requests",
            body,
            &actor,
            "org-1",
            timestamp,
            "fixed-nonce-1234567890",
        );

        assert_eq!(
            headers.get("x-service-id").map(String::as_str),
            Some("verevon-gateway")
        );
        assert_eq!(headers.get("x-org-id").map(String::as_str), Some("org-1"));
        assert_eq!(
            headers.get("x-user-role").map(String::as_str),
            Some("admin")
        );
        assert_eq!(
            headers.get("x-delegation-nonce").map(String::as_str),
            Some("fixed-nonce-1234567890")
        );
        assert_eq!(
            headers.get("x-delegation-body-sha256").map(String::as_str),
            Some("KSAem_coGl1Xx_rU84ulYwo1-4Joui0ynxMypC4vHSk")
        );
        assert_eq!(
            headers.get("x-delegation-signature").map(String::as_str),
            Some("iZbxn0GTXuuy-tZveuwQcAN-0a2bND6iR_9VB07LwIM")
        );
        assert!(!headers.contains_key("x-internal-api-key"));
        assert!(!headers.contains_key("x-service-token"));
    }

    #[test]
    fn conversation_delegation_matches_go_cross_language_contract() {
        use chrono::{TimeZone, Utc};

        let actor = ActionActor {
            user_id: "user-1".to_owned(),
            user_email: "verified@example.com".to_owned(),
            user_name: "Verified User".to_owned(),
            user_role: "admin".to_owned(),
        };
        let body = br#"{"body_text":"hello"}"#;
        let timestamp = Utc
            .with_ymd_and_hms(2026, 7, 13, 12, 0, 0)
            .single()
            .expect("fixed timestamp");
        let headers = conversation_delegation_headers(
            "0123456789abcdef0123456789abcdef",
            &Method::POST,
            "http://conversation-core:3160/api/v1/conversations/conversation-1/messages?source=inbox",
            body,
            &actor,
            "org-1",
            timestamp,
            "fixed-nonce-1234567890",
        );

        assert_eq!(
            headers.get("x-delegation-body-sha256").map(String::as_str),
            Some("zIWXrcFcB2V6qcMYvMSL9BWhHM37zJ5N96fr8I-wyRI")
        );
        assert_eq!(
            headers.get("x-delegation-signature").map(String::as_str),
            Some("YTL2cFsFBP8k-7kctwC-GVYdLxpTCX3pkABl_f-eTvE")
        );
        assert_eq!(
            headers.get("x-user-role").map(String::as_str),
            Some("admin")
        );
        assert!(!headers.contains_key("x-internal-api-key"));
        assert!(!headers.contains_key("x-service-token"));
    }

    #[test]
    fn conversation_signing_is_restricted_to_configured_origin() {
        assert!(conversation_target_is_configured_origin(
            "http://conversation-core:3160/api/v1/inboxes",
            "http://conversation-core:3160"
        ));
        assert!(!conversation_target_is_configured_origin(
            "http://conversation-core.attacker:3160/api/v1/inboxes",
            "http://conversation-core:3160"
        ));
    }

    #[test]
    fn notification_signing_is_restricted_to_the_configured_notification_origin() {
        assert!(notification_target_is_configured_origin(
            "http://notification-core:3140/notifications",
            "http://notification-core:3140"
        ));
        assert!(!notification_target_is_configured_origin(
            "http://dpv2-retrieval-engine:8004/v1/search",
            "http://notification-core:3140"
        ));
        assert!(!notification_target_is_configured_origin(
            "http://notification-core.attacker:3140/notifications",
            "http://notification-core:3140"
        ));
    }

    #[test]
    fn user_core_origin_is_matched_exactly() {
        assert!(same_upstream_origin(
            "http://user-core:3012/api/v1/users/me",
            "http://user-core:3012"
        ));
        assert!(!same_upstream_origin(
            "http://user-core.attacker:3012/api/v1/users/me",
            "http://user-core:3012"
        ));
    }

    #[test]
    fn control_service_audience_is_selected_only_for_exact_configured_origin() {
        assert_eq!(
            control_service_audience(
                "http://org-core:8080/orgs/org-1",
                "http://org-core:8080",
                "http://billing-core:3014",
                "http://audit-core:8187",
            ),
            Some("org-core")
        );
        assert_eq!(
            control_service_audience(
                "http://billing-core:3014/api/v1/billing/orgs/org-1/account",
                "http://org-core:8080",
                "http://billing-core:3014",
                "http://audit-core:8187",
            ),
            Some("billing-core")
        );
        assert_eq!(
            control_service_audience(
                "http://audit-core:8187/v1/audit?org_id=org-1",
                "http://org-core:8080",
                "http://billing-core:3014",
                "http://audit-core:8187",
            ),
            Some("audit-core")
        );
        assert_eq!(
            control_service_audience(
                "http://org-core.attacker:8080/orgs/org-1",
                "http://org-core:8080",
                "http://billing-core:3014",
                "http://audit-core:8187",
            ),
            None
        );
    }

    #[test]
    fn browser_origin_prefers_explicit_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://localhost:5173"));
        headers.insert(
            "referer",
            HeaderValue::from_static("http://localhost:5199/login"),
        );

        assert_eq!(
            browser_origin(&headers),
            Some("http://localhost:5173".to_owned())
        );
    }

    #[test]
    fn browser_origin_falls_back_to_referer_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("null"));
        headers.insert(
            "referer",
            HeaderValue::from_static("http://localhost:5173/login?next=/onboarding"),
        );

        assert_eq!(
            browser_origin(&headers),
            Some("http://localhost:5173".to_owned())
        );
    }

    #[test]
    fn browser_origin_ignores_non_http_values() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("file:///tmp/index.html"));
        headers.insert("referer", HeaderValue::from_static("about:blank"));

        assert_eq!(browser_origin(&headers), None);
    }
}

/// Largest artifact this gateway will relay to a browser.
///
/// A screenshot is tens to hundreds of KB; a full-page one can be a few MB. The
/// cap exists so a browser-run artifact of unexpected size cannot be pulled
/// through the SPA's origin in one response.
const ARTIFACT_RELAY_MAX_BYTES: usize = 12 * 1024 * 1024;

/// Content types this gateway will let a browser render from the SPA's OWN
/// origin.
///
/// # Why an allowlist and not a passthrough
///
/// quarry-edge reports the artifact's real type now, and its artifact kinds
/// include `text/html` (a captured page) and `application/json`. Relaying
/// `text/html` from the SPA's origin would make any stored page a same-origin
/// document — a stored-XSS vector with the user's session attached, reachable by
/// anyone who can get a page captured. So renderable types are allowlisted and
/// everything else is downgraded to an opaque attachment: still retrievable,
/// never executable.
const RENDERABLE_ARTIFACT_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp"];

/// Relay an artifact's BYTES from an upstream plane to the browser.
///
/// The caller supplies a token minted for the upstream's audience **on behalf of
/// the end user**, never a service credential: the upstream (Quarry-v2) decides
/// access from the token's own org claim, so a service identity would collapse
/// every tenant's artifacts into one readable set.
pub(crate) async fn proxy_artifact_bytes(
    state: &AppState,
    url: &str,
    bearer_token: Option<&str>,
    user_id: &str,
) -> Response {
    let mut req = state.client.get(url).header("x-user-id", user_id);
    if let Some(token) = bearer_token {
        req = req.bearer_auth(token);
    }

    let resp = match send_with_retry(req).await {
        Ok(resp) => resp,
        Err(err) => {
            tracing::warn!(error = %err, %url, "artifact relay upstream request failed");
            return (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "artifact_unavailable",
                    "Could not load the artifact.",
                )),
            )
                .into_response();
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    // The upstream's type, before any decision about whether to honour it.
    let upstream_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or(value).trim().to_owned())
        .unwrap_or_default();

    if !status.is_success() {
        // Do not relay an upstream error BODY: it is the other plane's prose and
        // may name internal paths. The status is the useful part.
        return (
            status,
            Json(error(
                "artifact_unavailable",
                "Could not load the artifact.",
            )),
        )
            .into_response();
    }

    let bytes = match resp.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!(error = %err, %url, "artifact relay body read failed");
            return (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "artifact_unavailable",
                    "Could not load the artifact.",
                )),
            )
                .into_response();
        }
    };
    if bytes.len() > ARTIFACT_RELAY_MAX_BYTES {
        tracing::warn!(
            bytes = bytes.len(),
            cap = ARTIFACT_RELAY_MAX_BYTES,
            "artifact exceeds the relay cap"
        );
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(error(
                "artifact_too_large",
                "That artifact is too large to display.",
            )),
        )
            .into_response();
    }

    let renderable = RENDERABLE_ARTIFACT_TYPES.contains(&upstream_type.as_str());
    let (content_type, disposition) = if renderable {
        (upstream_type.clone(), "inline")
    } else {
        // Retrievable, never executable from this origin.
        ("application/octet-stream".to_owned(), "attachment")
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(reqwest::header::CONTENT_TYPE, content_type)
        // Belt and braces with the allowlist: the declared type is now accurate,
        // and the browser is still forbidden from guessing a different one.
        .header("x-content-type-options", "nosniff")
        .header("content-disposition", disposition)
        // Private: an artifact is tenant-scoped, so no shared cache may keep it.
        .header(reqwest::header::CACHE_CONTROL, "private, max-age=30")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error(
                    "artifact_unavailable",
                    "Could not load the artifact.",
                )),
            )
                .into_response()
        })
}

#[cfg(test)]
mod artifact_relay_tests {
    use super::{ARTIFACT_RELAY_MAX_BYTES, RENDERABLE_ARTIFACT_TYPES};

    /// The guard that matters: a captured HTML page must never be renderable
    /// from the SPA's own origin, because that is stored XSS with the user's
    /// session attached.
    #[test]
    fn html_and_json_are_never_renderable() {
        for hostile in [
            "text/html",
            "application/xhtml+xml",
            "image/svg+xml",
            "application/json",
        ] {
            assert!(
                !RENDERABLE_ARTIFACT_TYPES.contains(&hostile),
                "{hostile} must not be served inline from this origin"
            );
        }
    }

    /// SVG deserves its own note: it IS an image, and it can carry script. It is
    /// deliberately absent above; this pins that decision so a future "add the
    /// other image types" change cannot quietly include it.
    #[test]
    fn svg_is_excluded_on_purpose() {
        assert!(!RENDERABLE_ARTIFACT_TYPES.contains(&"image/svg+xml"));
    }

    #[test]
    fn screenshots_are_renderable() {
        assert!(RENDERABLE_ARTIFACT_TYPES.contains(&"image/png"));
    }

    #[test]
    fn the_relay_cap_is_sane_for_a_full_page_screenshot() {
        const { assert!(ARTIFACT_RELAY_MAX_BYTES >= 4 * 1024 * 1024) };
    }
}
