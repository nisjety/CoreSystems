//! WS03-S4: the org-admin Space effect-policy routes (phase 1).
//!
//! user-core owns the policy row, the canonical-role check (auth-core), the write
//! switch, the audit rows and every rule about which entitlement may be on. This
//! module is only the Frontend Plane's door to it: flag, session role (defence in
//! depth), structure check, then a proxy that signs the v1 delegation. It evaluates
//! no policy and holds no state, and Control's typed body and status pass through.
//!
//! Order on every route: flag (404 `space_policy_admin_disabled`), `require_org_admin`,
//! shape check, `proxy_json`. The flag is a rollout control, never an authorization input.

use std::collections::HashMap;

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::proxy_json,
};

use super::shared::{actor_for, require_org_admin, GatewayJsonResponse};

const CODE_INVALID: &str = "space_policy_invalid_request";
const MAX_ENTITLEMENT_KEYS: usize = 32;
const MAX_KEY_LENGTH: usize = 48;
const MAX_PRESET_ID_LENGTH: usize = 48;
const IDEMPOTENCY_MIN: usize = 8;
const IDEMPOTENCY_MAX: usize = 128;
const EVENTS_MAX_LIMIT: u32 = 100;
const MAX_CURSOR_DIGITS: usize = 19;

/// The only fields a PUT body may carry. Org, actor and role come from the signed
/// delegation, never from the body, so a stray one is refused rather than forwarded.
const PUT_FIELDS: [&str; 4] = [
    "idempotency_key",
    "expected_entitlement_revision",
    "preset_id",
    "entitlements",
];

fn disabled() -> GatewayJsonResponse {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "space_policy_admin_disabled",
            "Space policy administration is not enabled.",
        )),
    )
}

fn invalid(field: &str, message: &str) -> GatewayJsonResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            CODE_INVALID,
            message,
            json!({ "field": field }),
        )),
    )
}

fn control_url(state: &AppState, suffix: &str) -> String {
    format!(
        "{}/api/v1/internal/spaces/org-policy{suffix}",
        state.user_core_url
    )
}

/// `^[a-z_]{1,48}$`. Control owns the closed key set, so a new catalogue row needs
/// no gateway edit; this only keeps the shape sane.
fn is_entitlement_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_LENGTH
        && key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

/// Structure check of a PUT body. Returns the body rebuilt from the allowed fields,
/// or the refusal. Never generates an idempotency key: a retry must reuse the
/// caller's, so the gateway cannot mint a fresh one per attempt.
fn validate_put_body(body: &Value) -> Result<Value, GatewayJsonResponse> {
    let Some(object) = body.as_object() else {
        return Err(invalid("body", "The request body must be a JSON object."));
    };
    if object.keys().any(|key| !PUT_FIELDS.contains(&key.as_str())) {
        return Err(invalid("body", "The request body has an unsupported field."));
    }
    let key = object
        .get("idempotency_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if key.len() < IDEMPOTENCY_MIN || key.len() > IDEMPOTENCY_MAX {
        return Err(invalid(
            "idempotency_key",
            "idempotency_key must be 8 to 128 characters.",
        ));
    }
    let Some(revision) = object
        .get("expected_entitlement_revision")
        .and_then(Value::as_i64)
        .filter(|revision| *revision >= 0)
    else {
        return Err(invalid(
            "expected_entitlement_revision",
            "expected_entitlement_revision must be a non-negative integer.",
        ));
    };
    let preset = object.get("preset_id").filter(|value| !value.is_null());
    let entitlements = object.get("entitlements").filter(|value| !value.is_null());
    let mut forwarded = Map::new();
    forwarded.insert("idempotency_key".to_owned(), json!(key));
    forwarded.insert("expected_entitlement_revision".to_owned(), json!(revision));
    match (preset, entitlements) {
        (Some(preset), None) => {
            let Some(preset_id) = preset
                .as_str()
                .map(str::trim)
                .filter(|id| !id.is_empty() && id.len() <= MAX_PRESET_ID_LENGTH)
            else {
                return Err(invalid("preset_id", "preset_id must be a short string."));
            };
            forwarded.insert("preset_id".to_owned(), json!(preset_id));
        }
        (None, Some(entitlements)) => {
            let Some(map) = entitlements.as_object() else {
                return Err(invalid(
                    "entitlements",
                    "entitlements must be an object of booleans.",
                ));
            };
            if map.len() > MAX_ENTITLEMENT_KEYS
                || !map
                    .iter()
                    .all(|(key, value)| is_entitlement_key(key) && value.is_boolean())
            {
                return Err(invalid(
                    "entitlements",
                    "entitlements must hold at most 32 boolean flags with lowercase names.",
                ));
            }
            forwarded.insert("entitlements".to_owned(), entitlements.clone());
        }
        _ => {
            return Err(invalid(
                "preset_id",
                "Send exactly one of preset_id or entitlements.",
            ));
        }
    }
    Ok(Value::Object(forwarded))
}

/// `?limit=` 1..=100 and `?cursor=` (Control's cursor is a decimal event id). Anything
/// else is refused here so a malformed value never reaches the signed URL.
fn events_query(params: &HashMap<String, String>) -> Result<String, GatewayJsonResponse> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(raw) = params.get("limit").map(|value| value.trim()).filter(|v| !v.is_empty()) {
        match raw.parse::<u32>() {
            Ok(limit) if (1..=EVENTS_MAX_LIMIT).contains(&limit) => {
                parts.push(format!("limit={limit}"));
            }
            _ => return Err(invalid("limit", "limit must be 1 to 100.")),
        }
    }
    if let Some(raw) = params.get("cursor").map(|value| value.trim()).filter(|v| !v.is_empty()) {
        if raw.len() > MAX_CURSOR_DIGITS || !raw.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("cursor", "cursor is not valid."));
        }
        parts.push(format!("cursor={raw}"));
    }
    Ok(if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    })
}

/// GET /api/v1/orgs/:id/space-policy: the org's effective Space entitlements.
pub(crate) async fn get_space_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if !state.space_features.policy_admin_api {
        return disabled();
    }
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    proxy_json(
        &state,
        Method::GET,
        &control_url(&state, ""),
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// PUT /api/v1/orgs/:id/space-policy/entitlements: apply a preset or the eight keys.
pub(crate) async fn put_space_policy_entitlements(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    apply_space_policy_entitlements(&state, &user, &id, &body).await
}

/// Shared by the route above and the `spaces.set_effect_entitlements` /
/// `spaces.apply_effect_preset` action dispatchers, so both ride the same flag,
/// admin check and shape check.
pub(crate) async fn apply_space_policy_entitlements(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    body: &Value,
) -> GatewayJsonResponse {
    if !state.space_features.policy_admin_api {
        return disabled();
    }
    if let Err(response) = require_org_admin(state, user, org_id).await {
        return response;
    }
    let forwarded = match validate_put_body(body) {
        Ok(forwarded) => forwarded,
        Err(response) => return response,
    };
    proxy_json(
        state,
        Method::PUT,
        &control_url(state, "/entitlements"),
        Some(forwarded),
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await
}

/// GET /api/v1/orgs/:id/space-policy/events: the change history, newest first.
pub(crate) async fn list_space_policy_events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> GatewayJsonResponse {
    if !state.space_features.policy_admin_api {
        return disabled();
    }
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    let query = match events_query(&params) {
        Ok(query) => query,
        Err(response) => return response,
    };
    proxy_json(
        &state,
        Method::GET,
        &control_url(&state, &format!("/events{query}")),
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}
