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
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
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
/// `x-verevon-org-id` header (which would be an IDOR vector). Empty string if none.
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
    Json(caller_body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let org_id = match require_billing_admin(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let url = format!(
        "{}/api/v1/billing/orgs/{}/checkout-session",
        state.billing_core_url, org_id
    );
    let body = match canonical_checkout_body(
        &caller_body,
        &state.verevon_public_origin,
        "/settings/billing",
    ) {
        Ok(body) => body,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error("invalid_checkout_request", message)),
            )
        }
    };
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

pub(crate) fn canonical_checkout_body(
    caller_body: &Value,
    public_origin: &str,
    landing_path: &str,
) -> Result<Value, &'static str> {
    let plan = caller_body
        .get("plan")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|plan| {
            !plan.is_empty()
                && plan.len() <= 64
                && plan
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .ok_or("A valid billing plan is required.")?;

    let mut success = url::Url::parse(public_origin).map_err(|_| "Checkout is unavailable.")?;
    success.set_path(landing_path);
    success.set_query(None);
    success
        .query_pairs_mut()
        .append_pair("checkout", "success")
        .append_pair("plan", plan);

    let mut cancel = success.clone();
    cancel
        .query_pairs_mut()
        .clear()
        .append_pair("checkout", "cancel")
        .append_pair("plan", plan);

    Ok(json!({
        "plan": plan,
        "success_url": success.as_str(),
        "cancel_url": cancel.as_str(),
    }))
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

    if has_authorized_org_role(user, &["owner", "admin"]) {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn checkout_return_urls_are_derived_from_the_server_origin() {
        let caller = json!({
            "plan": "standard",
            "success_url": "https://attacker.example/success",
            "cancel_url": "https://attacker.example/cancel"
        });
        let body = super::canonical_checkout_body(
            &caller,
            "https://app.verevon.example",
            "/settings/billing",
        )
        .expect("valid checkout request");

        assert_eq!(body["plan"], "standard");
        assert_eq!(
            body["success_url"],
            "https://app.verevon.example/settings/billing?checkout=success&plan=standard"
        );
        assert_eq!(
            body["cancel_url"],
            "https://app.verevon.example/settings/billing?checkout=cancel&plan=standard"
        );
        assert!(!body.to_string().contains("attacker.example"));
    }

    #[test]
    fn checkout_plan_is_a_bounded_opaque_identifier() {
        for plan in [
            "",
            "../admin",
            "standard?next=https://attacker.example",
            &"a".repeat(65),
        ] {
            assert!(super::canonical_checkout_body(
                &json!({ "plan": plan }),
                "https://app.verevon.example",
                "/onboarding",
            )
            .is_err());
        }
    }
}
