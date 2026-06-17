use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    upstream::proxy_json,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/billing/account", get(billing_account))
        .route(
            "/api/v1/billing/entitlements/:feature",
            get(billing_entitlement),
        )
        .route("/api/v1/billing/quotas/:metric", get(billing_quota))
        .route("/api/v1/billing/checkout", post(billing_checkout))
        .route(
            "/api/v1/billing/checkout/confirm",
            post(billing_checkout_confirm),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

/// The authenticated user's authoritative org id — cached per-user in `upstream`
/// and resolved from user-core's session-context, never a client-supplied
/// `x-velion-org-id` header (which would be an IDOR vector). Empty string if none.
async fn authorized_org_id(state: &AppState, user: &AuthenticatedUser) -> String {
    crate::upstream::authorized_org_id(state, user).await
}

// ── Billing routes ────────────────────────────────────────────────────────────

async fn billing_account(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> (StatusCode, Json<Value>) {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.is_empty() {
        return no_active_org_response();
    }

    let url = format!(
        "{}/api/v1/billing/orgs/{}/account",
        state.billing_core_url, org_id
    );
    let response = proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor_for(&user)),
        None,
    )
    .await;

    if response.0.is_server_error()
        && billing_account_fallback_enabled(&state)
        && !org_id.is_empty()
    {
        return (
            StatusCode::OK,
            Json(default_billing_account(&org_id, "billing_core_unavailable")),
        );
    }

    response
}

async fn billing_entitlement(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(feature): Path<String>,
) -> (StatusCode, Json<Value>) {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.is_empty() {
        return no_active_org_response();
    }

    let url = format!(
        "{}/api/v1/billing/orgs/{}/entitlements/{}",
        state.billing_core_url,
        org_id,
        urlencoding::encode(&feature)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn billing_quota(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(metric): Path<String>,
) -> (StatusCode, Json<Value>) {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.is_empty() {
        return no_active_org_response();
    }

    let url = format!(
        "{}/api/v1/billing/orgs/{}/quotas/{}",
        state.billing_core_url,
        org_id,
        urlencoding::encode(&metric)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn billing_checkout(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let org_id = match require_billing_admin(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let url = format!(
        "{}/api/v1/billing/orgs/{}/checkout-session",
        state.billing_core_url, org_id
    );
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&org_id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn billing_checkout_confirm(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let org_id = match require_billing_admin(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let url = format!(
        "{}/api/v1/billing/orgs/{}/checkout-session/confirm",
        state.billing_core_url, org_id
    );
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&org_id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn require_billing_admin(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<String, (StatusCode, Json<Value>)> {
    let org_id = authorized_org_id(state, user).await;
    if org_id.is_empty() {
        return Err(no_active_org_response());
    }

    if has_any_role(user.auth_role.as_deref(), &["admin", "superadmin"]) {
        return Ok(org_id);
    }

    let org_role = crate::upstream::resolve_session_context(state, user)
        .await
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if has_any_role(Some(org_role.as_str()), &["owner", "admin"]) {
        Ok(org_id)
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Billing changes require an organization owner or admin.",
            )),
        ))
    }
}

fn no_active_org_response() -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(error(
            "no_active_org",
            "An active organization is required for billing.",
        )),
    )
}

fn has_any_role(value: Option<&str>, allowed: &[&str]) -> bool {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|role| {
            allowed
                .iter()
                .any(|allowed_role| role.eq_ignore_ascii_case(allowed_role))
        })
}

fn billing_account_fallback_enabled(state: &AppState) -> bool {
    cfg!(debug_assertions) || state.allow_dev_actor_headers || state.allow_dev_auth_bypass
}

fn default_billing_account(org_id: &str, reason: &str) -> Value {
    json!({
        "org_id": org_id,
        "plan": "free",
        "subscription_state": "active",
        "credits": 0,
        "products": {},
        "feature_flags": {},
        "entitlements": {
            "feature.chat": true,
            "feature.api_keys": true,
            "feature.audit_logs": true,
            "feature.integrations": false,
            "feature.sso": false
        },
        "quota_limits": {
            "api_calls": 1000,
            "users": 5,
            "storage_mb": 1000
        },
        "provider_customer_id": {},
        "metadata": {
            "fallback": reason
        }
    })
}
