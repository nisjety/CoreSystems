use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

use super::shared::cookie_header;

pub(super) async fn list_api_keys(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let org_id = match session_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };

    let payload = match post_auth_core(
        &state,
        "api-keys/list",
        json!({
            "organizationId": org_id,
            "limit": 100,
            "offset": 0,
            "includeExpired": false,
        }),
        &cookie_header(&headers),
    )
    .await
    {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    let api_keys = payload
        .get("apiKeys")
        .and_then(Value::as_array)
        .map(|keys| {
            keys.iter()
                .map(|key| normalize_api_key(key, None))
                .collect::<Vec<Value>>()
        })
        .unwrap_or_default();

    (StatusCode::OK, Json(ok(api_keys)))
}

pub(crate) async fn create_api_key(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    let org_id = match session_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_api_key_name", "API key name is required.")),
        );
    }

    let mut request_body = json!({
        "name": name,
        "organizationId": org_id,
        "scopes": ["read"],
    });
    if let Some(expires_at) = body
        .get("expiresAt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request_body["expiresAt"] = Value::String(expires_at.to_owned());
    }

    let payload = match post_auth_core(
        &state,
        "api-keys/create",
        request_body,
        &cookie_header(&headers),
    )
    .await
    {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    let api_key = payload.get("apiKey").unwrap_or(&Value::Null);
    let secret = api_key
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if secret.is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "api_key_create_failed",
                "auth-core did not return the API key secret.",
            )),
        );
    }

    (
        StatusCode::OK,
        Json(ok(normalize_api_key(api_key, Some(secret)))),
    )
}

pub(crate) async fn delete_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl axum::response::IntoResponse {
    let key_id = id.trim();
    if key_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_api_key_id", "API key id is required.")),
        );
    }

    match post_auth_core(
        &state,
        "api-keys/delete",
        json!({ "keyId": key_id }),
        &cookie_header(&headers),
    )
    .await
    {
        Ok(_) => (StatusCode::OK, Json(ok(Value::Null))),
        Err(response) => response,
    }
}

async fn session_org_id(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<String, (StatusCode, Json<Value>)> {
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error("no_active_org", "No active organization found.")),
        ));
    }
    Ok(org_id)
}

async fn post_auth_core(
    state: &AppState,
    path: &str,
    body: Value,
    cookie: &str,
) -> Result<Value, (StatusCode, Json<Value>)> {
    let url = format!("{}/api/v2/auth/{}", state.auth_core_url, path);
    let mut request = state.client.post(url).json(&body);
    if !cookie.trim().is_empty() {
        request = request.header("cookie", cookie);
    }

    let response = request.send().await.map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        )
    })?;
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let payload = response.json::<Value>().await.unwrap_or_else(|_| json!({}));

    if !status.is_success() {
        return Err((status, Json(auth_core_error(&payload, "api_key_error"))));
    }

    if payload.get("success").and_then(Value::as_bool) == Some(false) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(auth_core_error(&payload, "api_key_error")),
        ));
    }

    Ok(payload)
}

fn auth_core_error(payload: &Value, code: &'static str) -> Value {
    let message = payload
        .get("error")
        .and_then(Value::as_str)
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .unwrap_or("auth-core API key operation failed.");
    error(code, message)
}

fn normalize_api_key(api_key: &Value, secret: Option<&str>) -> Value {
    let id = string_value(api_key, "id");
    let name = string_value(api_key, "name");
    let key_prefix = secret
        .map(prefix_from_secret)
        .or_else(|| optional_string_value(api_key, "prefix"))
        .unwrap_or_else(|| id.chars().take(12).collect());
    let mut normalized = json!({
        "id": id,
        "name": if name.is_empty() { "API key" } else { name.as_str() },
        "prefix": key_prefix,
        "createdAt": string_value(api_key, "createdAt"),
    });

    if let Some(expires_at) = optional_string_value(api_key, "expiresAt") {
        normalized["expiresAt"] = Value::String(expires_at);
    }
    if let Some(secret) = secret {
        normalized["secret"] = Value::String(secret.to_owned());
    }
    normalized
}

fn string_value(value: &Value, key: &str) -> String {
    optional_string_value(value, key).unwrap_or_default()
}

fn optional_string_value(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn prefix_from_secret(secret: &str) -> String {
    secret.chars().take(12).collect()
}
