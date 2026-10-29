//! Lead-builder (W1) surface — metered and audited.
//!
//! Filtered Enhetsregisteret company search, sub-entity/branch and financials
//! enrichment, the governed `leads.build_list` action, saved org-scoped lists,
//! and CSV export, proxied to leads-core. The org is resolved server-side via
//! `authorized_org_id` (never a client header), so every route is IDOR-clean.
//! The CSV export and `build_list` are METERED: both are gated on the org's
//! billing-core `leads` entitlement and 402 when not entitled (leads-core emits
//! the per-action audit event). COMPANY DATA ONLY — leads-core never returns
//! person/role/birth-number data.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    middleware::{require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_json},
};

/// billing-core feature key gating lead export (the metered action).
const LEADS_FEATURE: &str = "leads";

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/leads/search", post(search))
        .route("/api/v1/leads/companies/{orgnr}/branches", get(branches))
        .route(
            "/api/v1/leads/companies/{orgnr}/financials",
            get(financials),
        )
        .route("/api/v1/leads/build_list", post(build_list))
        .route("/api/v1/leads/lists", get(list_lists).post(create_list))
        .route(
            "/api/v1/leads/lists/{id}",
            get(get_list).delete(delete_list),
        )
        .route("/api/v1/leads/lists/{id}/export.csv", get(export_csv))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn no_active_org() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": { "code": "no_active_org", "message": "No active organization." } })),
    )
        .into_response()
}

/// Whether a billing-core entitlement response grants the feature: a 2xx status
/// AND `allowed == true`. Anything else (402, missing field, error) denies.
fn entitlement_allowed(status: StatusCode, body: &Value) -> bool {
    status.is_success()
        && body
            .get("allowed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

/// Fail-closed metering gate for the `leads` add-on. Returns `Ok(())` when the
/// org is entitled, or a ready-to-send 402 response otherwise. The org is the
/// server-resolved `authorized_org_id` — never a client value.
async fn require_leads_entitlement(
    state: &AppState,
    org_id: &str,
    actor: &ActionActor,
) -> Result<(), Response> {
    let ent_url = format!(
        "{}/api/v1/billing/orgs/{}/entitlements/{}",
        state.billing_core_url,
        org_id.trim(),
        LEADS_FEATURE
    );
    let (ent_status, Json(ent_body)) = proxy_json(
        state,
        Method::GET,
        &ent_url,
        None,
        Some(org_id),
        Some(actor),
        None,
    )
    .await;
    if entitlement_allowed(ent_status, &ent_body) {
        Ok(())
    } else {
        Err((
            StatusCode::PAYMENT_REQUIRED,
            Json(json!({
                "error": { "code": "entitlement_required", "message": "The lead-builder add-on is required for this action." },
                "billing": ent_body,
            })),
        )
            .into_response())
    }
}

async fn search(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let url = format!("{}/api/v1/leads/search", state.leads_core_url);
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
    .into_response()
}

/// `branches` — a company's sub-entities (`/underenheter`). Read-only, company
/// data only. Org resolved server-side.
async fn branches(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(orgnr): Path<String>,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let url = format!(
        "{}/api/v1/leads/companies/{}/branches",
        state.leads_core_url, orgnr
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
    .into_response()
}

/// `financials` — a company's filed annual accounts (`/regnskap`). Read-only,
/// aggregate company figures only. Org resolved server-side.
async fn financials(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(orgnr): Path<String>,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let url = format!(
        "{}/api/v1/leads/companies/{}/financials",
        state.leads_core_url, orgnr
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
    .into_response()
}

/// `build_list` — the governed lead-builder action: search → (branch enrich) →
/// dedupe → save an org-scoped list. METERED (gated on the `leads` entitlement)
/// and AUDITED (leads-core records the action via the proxied actor). The org is
/// the server-resolved `authorized_org_id` and is forwarded as `x-org-id` by
/// `proxy_json`; leads-core ignores any org in the body — so a client/model can
/// never redirect the write to another tenant (IDOR-clean).
async fn build_list(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let actor = actor_for(&user);

    if let Err(resp) = require_leads_entitlement(&state, &org_id, &actor).await {
        return resp;
    }

    let url = format!("{}/api/v1/leads/build_list", state.leads_core_url);
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await
    .into_response()
}

async fn list_lists(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return Json(json!({ "data": [], "meta": { "count": 0 }, "error": null })).into_response();
    }
    let url = format!("{}/api/v1/leads/lists", state.leads_core_url);
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
    .into_response()
}

async fn create_list(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let url = format!("{}/api/v1/leads/lists", state.leads_core_url);
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
    .into_response()
}

async fn get_list(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let url = format!("{}/api/v1/leads/lists/{}", state.leads_core_url, id);
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
    .into_response()
}

async fn delete_list(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let url = format!("{}/api/v1/leads/lists/{}", state.leads_core_url, id);
    proxy_json(
        &state,
        Method::DELETE,
        &url,
        None,
        Some(&org_id),
        Some(&actor_for(&user)),
        None,
    )
    .await
    .into_response()
}

async fn export_csv(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let actor = actor_for(&user);

    // Metering: gate the export on the org's billing-core `leads` entitlement.
    if let Err(resp) = require_leads_entitlement(&state, &org_id, &actor).await {
        return resp;
    }

    // Raw CSV passthrough (proxy_json would mangle the non-JSON body). leads-core
    // emits the per-export audit event on this call.
    let url = format!(
        "{}/api/v1/leads/lists/{}/export.csv",
        state.leads_core_url, id
    );
    let request = state
        .client
        .get(&url)
        .header("x-internal-api-key", &state.internal_api_key)
        .header("x-org-id", org_id.trim())
        .header("x-user-id", &actor.user_id);

    match request.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let disposition = resp
                .headers()
                .get("content-disposition")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("attachment; filename=\"leads.csv\"")
                .to_string();
            let bytes = resp.bytes().await.unwrap_or_default();
            if !status.is_success() {
                return (status, bytes).into_response();
            }
            let mut headers = HeaderMap::new();
            headers.insert(
                "content-type",
                HeaderValue::from_static("text/csv; charset=utf-8"),
            );
            if let Ok(value) = HeaderValue::from_str(&disposition) {
                headers.insert("content-disposition", value);
            }
            (status, headers, bytes).into_response()
        }
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entitlement_allowed_only_on_2xx_and_allowed_true() {
        assert!(entitlement_allowed(
            StatusCode::OK,
            &json!({ "allowed": true })
        ));
        assert!(!entitlement_allowed(
            StatusCode::OK,
            &json!({ "allowed": false })
        ));
        // 402 (not allowed) denies even if the body somehow says allowed.
        assert!(!entitlement_allowed(
            StatusCode::PAYMENT_REQUIRED,
            &json!({ "allowed": true })
        ));
        // Missing field denies (fail-closed for the metered feature).
        assert!(!entitlement_allowed(StatusCode::OK, &json!({})));
    }
}
