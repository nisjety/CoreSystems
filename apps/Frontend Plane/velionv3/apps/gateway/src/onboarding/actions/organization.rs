use axum::{extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse, Json};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    auth::actor_from_request,
    config::AppState,
    contracts::CreateOrganizationRequest,
    envelope::error,
    upstream::proxy_json,
    utils::{slugify, trim_opt},
};

pub(crate) async fn create_organization(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateOrganizationRequest>,
) -> impl IntoResponse {
    if input.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_name", "Organization name is required.")),
        );
    }

    let body = json!({
        "name": input.name.trim(),
        "slug": slugify(input.name.trim()),
        "plan": input.plan.unwrap_or_else(|| "trial".into()),
        "org_number": trim_opt(input.org_number),
        "brreg_data": input.brreg_data,
        "metadata": input.metadata,
    });
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );

    let (status, Json(created)) = proxy_json(
        &state,
        Method::POST,
        &format!("{}/orgs", state.org_core_url),
        Some(body),
        None,
        Some(&actor),
        None,
    )
    .await;

    // Make the freshly-created org the session's active organization so that
    // subsequent session-context / billing / knowledge calls are scoped to it.
    // Best-effort: a failure here must never fail organization creation.
    if status.is_success() {
        if let Some(org_id) = extract_org_id(&created) {
            switch_active_org(&state, &headers, &org_id).await;
        }
    }

    (status, Json(created))
}

/// Pull the new organization id out of org-core's create response, which returns
/// the org object at the root (or under `organization` in the brreg-warning branch).
fn extract_org_id(created: &Value) -> Option<String> {
    ["/id", "/organization/id", "/data/id", "/org/id"]
        .into_iter()
        .find_map(|ptr| created.pointer(ptr).and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

/// Set the session's active organization via auth-core (Better Auth set-active).
/// Server-to-server with the caller's cookie; the response is intentionally
/// ignored — the active org is persisted server-side on the session row.
async fn switch_active_org(state: &AppState, headers: &HeaderMap, org_id: &str) {
    let cookie = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if cookie.is_empty() {
        return;
    }
    let url = format!("{}/api/auth/organization/set-active", state.auth_core_url);
    let _ = state
        .client
        .post(&url)
        .header("cookie", cookie)
        .json(&json!({ "organizationId": org_id }))
        .send()
        .await;
}
