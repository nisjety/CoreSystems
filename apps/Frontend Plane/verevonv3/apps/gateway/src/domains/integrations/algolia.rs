//! Narrow user-delegated Algolia onboarding. Provider tokens never leave
//! Integration Core; even an organization switch during token minting fails closed.
use crate::{
    audience_tokens::get_audience_token,
    config::AppState,
    envelope::error,
    middleware::{has_authorized_org_role, AuthenticatedUser},
};
use axum::{
    body::Bytes,
    extract::{Extension, OriginalUri, Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::Engine;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) enum Operation {
    Connections,
    Connect,
    Status(String),
    Applications(String),
    Indices(String, String),
}
fn fail(status: StatusCode) -> Response {
    (
        status,
        [
            ("cache-control", "no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        Json(error(
            "algolia_unavailable",
            "Algolia setup is unavailable. Check workspace permissions, provider setup and retry.",
        )),
    )
        .into_response()
}
fn identifier(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|tail| {
        !tail.is_empty()
            && tail.len() <= 100
            && tail
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    })
}
fn app(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|c| c.is_ascii_alphanumeric())
}
fn index(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
}
fn text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 200 && !s.chars().any(char::is_control))
}
fn timestamp(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|s| s.len() <= 40 && chrono::DateTime::parse_from_rfc3339(s).is_ok())
}
pub(super) fn authorization_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    let allowed = [
        "response_type",
        "client_id",
        "redirect_uri",
        "scope",
        "state",
        "code_challenge",
        "code_challenge_method",
        "resource",
    ];
    let pairs: Vec<_> = url.query_pairs().collect();
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_ref())
            .unwrap_or("")
    };
    value.len() <= 4096
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
        && url.origin().ascii_serialization() == "https://dashboard.algolia.com"
        && url.path() == "/2/oauth/authorize"
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && pairs.len() == allowed.len()
        && allowed
            .iter()
            .all(|key| pairs.iter().filter(|(k, _)| k == key).count() == 1)
        && get("response_type") == "code"
        && get("scope") == "public"
        && get("resource") == "https://mcp.algolia.com/mcp"
        && get("redirect_uri") == "http://localhost:3026/api/v1/providers/algolia/callback"
        && get("code_challenge_method") == "S256"
        && (43..=128).contains(&get("code_challenge").len())
        && get("code_challenge")
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        && !get("state").is_empty()
        && get("state").len() <= 256
        && get("state")
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._~-".contains(&c))
        && !get("client_id").is_empty()
        && get("client_id").len() <= 200
        && !get("client_id").chars().any(char::is_control)
}
pub(super) fn project(op: &Operation, value: &Value, org: &str) -> Option<Value> {
    match op {
        Operation::Connections => {
            let rows = value.get("connections")?.as_array()?;
            if rows.len() > 64 {
                return None;
            }
            let mut ids = std::collections::HashSet::new();
            let mut result = Vec::new();
            for row in rows {
                let id = row.get("id")?.as_str()?;
                if !identifier(id, "conn_")
                    || !ids.insert(id)
                    || row.get("providerKey")?.as_str()? != "algolia"
                    || row
                        .get("organizationId")
                        .is_some_and(|v| v.as_str() != Some(org))
                {
                    return None;
                }
                let status = match row.get("status")?.as_str()? {
                    "active" => "active",
                    "needs_refresh" | "reconnect_required" => "reconnect_required",
                    _ => "inactive",
                };
                let mut safe = json!({"id":id,"status":status});
                if let Some(name) = row.get("displayName") {
                    safe["displayName"] = Value::String(text(name)?.into())
                }
                result.push(safe);
            }
            Some(json!({"connections":result}))
        }
        Operation::Connect => {
            let id = value.get("sessionToken")?.as_str()?;
            let auth = value.get("authorizationUrl")?.as_str()?;
            if !identifier(id, "cs_")
                || value.get("providerConfigKey")?.as_str()? != "algolia"
                || value.get("authMode")?.as_str()? != "direct-oauth"
                || !authorization_url(auth)
            {
                return None;
            }
            Some(
                json!({"id":id,"authorizationUrl":auth,"expiresAt":timestamp(value.get("expiresAt")?)?}),
            )
        }
        Operation::Status(id) => {
            if value.get("id")?.as_str()? != id || value.get("providerKey")?.as_str()? != "algolia"
            {
                return None;
            }
            let mut status = value.get("status")?.as_str()?;
            if !["pending", "completed", "failed", "expired"].contains(&status) {
                return None;
            }
            let expiry = timestamp(value.get("expiresAt")?)?;
            if status == "pending"
                && chrono::DateTime::parse_from_rfc3339(expiry)
                    .ok()?
                    .with_timezone(&chrono::Utc)
                    <= chrono::Utc::now()
            {
                status = "expired"
            }
            Some(json!({"id":id,"status":status,"expiresAt":expiry}))
        }
        Operation::Applications(_) | Operation::Indices(_, _) => {
            let (key, field, limit) = if matches!(op, Operation::Applications(_)) {
                ("applications", "id", 128)
            } else {
                ("indices", "name", 5000)
            };
            let rows = value.get(key)?.as_array()?;
            if rows.len() > limit {
                return None;
            }
            let mut seen = std::collections::HashSet::new();
            let mut result = Vec::new();
            for row in rows {
                let name = row.get(field)?.as_str()?;
                if !(if field == "id" {
                    app(name)
                } else {
                    index(name)
                }) || !seen.insert(name)
                {
                    return None;
                }
                result.push(json!({field:name}));
            }
            Some(json!({key:result}))
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConnectInput {
    return_url: String,
}
fn input(
    op: &Operation,
    body: &Bytes,
    headers: &HeaderMap,
    state: &AppState,
) -> Result<Option<Value>, StatusCode> {
    match op {
        Operation::Connect => {
            if headers.get("origin").and_then(|v| v.to_str().ok())
                != Some(state.verevon_public_origin.as_str())
            {
                return Err(StatusCode::FORBIDDEN);
            }
            if headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(';').next())
                .map(str::trim)
                != Some("application/json")
            {
                return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
            }
            if body.len() > 2048 {
                return Err(StatusCode::BAD_REQUEST);
            }
            let data: ConnectInput =
                serde_json::from_slice(body).map_err(|_| StatusCode::BAD_REQUEST)?;
            if data.return_url != format!("{}/studio/pulse", state.verevon_public_origin) {
                return Err(StatusCode::BAD_REQUEST);
            }
            Ok(Some(
                json!({"returnUrl":data.return_url,"capabilities":["search.analytics.read"]}),
            ))
        }
        Operation::Connections if body.is_empty() => Ok(None),
        Operation::Status(id) if body.is_empty() && identifier(id, "cs_") => Ok(None),
        Operation::Applications(id) if body.is_empty() && identifier(id, "conn_") => Ok(None),
        Operation::Indices(id, application)
            if body.is_empty() && identifier(id, "conn_") && app(application) =>
        {
            Ok(None)
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}
pub(super) async fn proxy(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    op: Operation,
    body: Bytes,
) -> Response {
    if !has_authorized_org_role(user, &["owner", "admin"]) {
        return fail(StatusCode::FORBIDDEN);
    }
    let Some(org) = user.active_org_id.as_deref().filter(|s| !s.is_empty()) else {
        return fail(StatusCode::FORBIDDEN);
    };
    let command = match input(&op, &body, headers, state) {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    let cookie = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let Some(token) = get_audience_token(state, &user.user_id, cookie, "ingestion").await else {
        return fail(StatusCode::SERVICE_UNAVAILABLE);
    };
    let parts: Vec<_> = token.split('.').collect();
    let claims = if parts.len() == 3 && token.len() <= 16384 {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .ok()
            .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
    } else {
        None
    };
    if claims
        .as_ref()
        .and_then(|v| v.get("org_id"))
        .and_then(Value::as_str)
        != Some(org)
    {
        return fail(StatusCode::FORBIDDEN);
    }
    let suffix = match &op {
        Operation::Connections => "/api/v1/connections?providerKey=algolia".into(),
        Operation::Connect => "/api/v1/providers/algolia/connect-session".into(),
        Operation::Status(id) => format!("/api/v1/connect-sessions/{id}/status"),
        Operation::Applications(id) => {
            format!("/api/v1/providers/algolia/connections/{id}/applications")
        }
        Operation::Indices(id, app) => {
            format!("/api/v1/providers/algolia/connections/{id}/indices?applicationId={app}")
        }
    };
    let mut request = state
        .client
        .request(
            if command.is_some() {
                Method::POST
            } else {
                Method::GET
            },
            format!(
                "{}{}",
                state.integration_core_url.trim_end_matches('/'),
                suffix
            ),
        )
        .bearer_auth(token)
        .header("accept", "application/json");
    if let Some(value) = command {
        request = request.json(&value)
    }
    let Ok(upstream) = request.send().await else {
        return fail(StatusCode::SERVICE_UNAVAILABLE);
    };
    if upstream.status() != StatusCode::OK {
        let code = match upstream.status().as_u16() {
            400 => StatusCode::BAD_REQUEST,
            401 | 403 => StatusCode::FORBIDDEN,
            404 => StatusCode::NOT_FOUND,
            429 => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        };
        return fail(code);
    }
    if !upstream
        .headers()
        .get("content-type")
        .and_then(|s| s.to_str().ok())
        .is_some_and(|s| s.starts_with("application/json"))
    {
        return fail(StatusCode::BAD_GATEWAY);
    }
    let maximum = if matches!(op, Operation::Indices(_, _)) {
        1_048_576
    } else {
        131_072
    };
    let mut chunks = upstream.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(part) = chunks.next().await {
        let Ok(part) = part else {
            return fail(StatusCode::BAD_GATEWAY);
        };
        if bytes.len() + part.len() > maximum {
            return fail(StatusCode::BAD_GATEWAY);
        }
        bytes.extend_from_slice(&part)
    }
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return fail(StatusCode::BAD_GATEWAY);
    };
    if value.get("success") != Some(&Value::Bool(true)) {
        return fail(StatusCode::BAD_GATEWAY);
    }
    let Some(data) = value.get("data").and_then(|v| project(&op, v, org)) else {
        return fail(StatusCode::BAD_GATEWAY);
    };
    (
        [
            ("cache-control", "no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        Json(json!({"data":data})),
    )
        .into_response()
}
pub(super) async fn connections(
    State(s): State<AppState>,
    Extension(u): Extension<AuthenticatedUser>,
    h: HeaderMap,
    OriginalUri(uri): OriginalUri,
    b: Bytes,
) -> Response {
    if uri.query().is_some() {
        return fail(StatusCode::BAD_REQUEST);
    }
    proxy(&s, &u, &h, Operation::Connections, b).await
}
pub(super) async fn connect(
    State(s): State<AppState>,
    Extension(u): Extension<AuthenticatedUser>,
    h: HeaderMap,
    OriginalUri(uri): OriginalUri,
    b: Bytes,
) -> Response {
    if uri.query().is_some() {
        return fail(StatusCode::BAD_REQUEST);
    }
    proxy(&s, &u, &h, Operation::Connect, b).await
}
pub(super) async fn status(
    State(s): State<AppState>,
    Extension(u): Extension<AuthenticatedUser>,
    h: HeaderMap,
    Path(id): Path<String>,
    OriginalUri(uri): OriginalUri,
    b: Bytes,
) -> Response {
    if uri.query().is_some() {
        return fail(StatusCode::BAD_REQUEST);
    }
    proxy(&s, &u, &h, Operation::Status(id), b).await
}
pub(super) async fn applications(
    State(s): State<AppState>,
    Extension(u): Extension<AuthenticatedUser>,
    h: HeaderMap,
    Path(id): Path<String>,
    OriginalUri(uri): OriginalUri,
    b: Bytes,
) -> Response {
    if uri.query().is_some() {
        return fail(StatusCode::BAD_REQUEST);
    }
    proxy(&s, &u, &h, Operation::Applications(id), b).await
}
pub(super) async fn indices(
    State(s): State<AppState>,
    Extension(u): Extension<AuthenticatedUser>,
    h: HeaderMap,
    Path(id): Path<String>,
    OriginalUri(uri): OriginalUri,
    b: Bytes,
) -> Response {
    let pairs: Vec<_> = url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes()).collect();
    if pairs.len() != 1 || pairs[0].0 != "applicationId" {
        return fail(StatusCode::BAD_REQUEST);
    }
    proxy(
        &s,
        &u,
        &h,
        Operation::Indices(id, pairs[0].1.to_string()),
        b,
    )
    .await
}
pub(super) async fn generic_connect(
    s: &AppState,
    u: &AuthenticatedUser,
    h: &HeaderMap,
    value: Value,
) -> Response {
    let Ok(raw) = serde_json::to_vec(&value) else {
        return fail(StatusCode::BAD_REQUEST);
    };
    proxy(s, u, h, Operation::Connect, Bytes::from(raw)).await
}
