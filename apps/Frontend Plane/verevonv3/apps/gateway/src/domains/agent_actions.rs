//! Agent "specialized actions" surface for the chat composer's `/` menu:
//! skills and capabilities. Both are Model-Plane catalogs fronted by
//! model-gateway (`/v1/skills`, `/v1/capabilities`, which it in turn proxies to
//! capability-core). The SPA only ever talks to this gateway, so these proxies
//! are what let the `/` menu surface real, org-scoped skills/capabilities.
//!
//! Reads (skills/capabilities catalogs) are open to any authenticated member so
//! the `/` menu can surface them; activation happens client-side by threading
//! the chosen action into the chat invoke `tools` array. Skill AUTHORING
//! (create/update/delete) is admin-gated here (defense in depth) and mutates the
//! durable org skill registry (model-gateway `/v1/skills` → capability-core),
//! which the chat path injects into the live prompt via `MatchSkills`.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState,
    domains::chat::shared,
    envelope::error,
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
};

static CRON_CREATE_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/skills", get(list_skills).post(create_skill))
        .route(
            "/api/v1/skills/{skill_id}",
            post(update_skill).delete(delete_skill),
        )
        .route("/api/v1/plugins", get(list_plugins).post(create_plugin))
        .route(
            "/api/v1/plugins/{plugin_id}",
            post(update_plugin).delete(delete_plugin),
        )
        .route("/api/v1/cron", get(list_cron).post(create_cron))
        .route(
            "/api/v1/cron/{cron_id}",
            post(update_cron).delete(delete_cron),
        )
        .route("/api/v1/cron/{cron_id}/fires", get(list_cron_fires))
        .route("/api/v1/capabilities", get(list_capabilities))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Whether the validated session user may author org skills. Mirrors
/// `mcp.rs::is_org_admin`: a platform `admin`/`superadmin` short-circuits,
/// else the active org's session-context role must be `owner`/`admin`.
/// model-gateway/capability-core re-derive write authority from the verified
/// capability token; this is a defense-in-depth gate that also lets the SPA
/// surface a clear error.
async fn can_author_skills(state: &AppState, user: &AuthenticatedUser) -> bool {
    let _ = state;
    has_authorized_org_role(user, &["owner", "admin"])
}

/// Whether the caller may create/update/delete THIS cron schedule (chat-wave2
/// "schedule" spec §2.2, owner ruling 2026-09-28 #1: "schedules may be
/// created by the creator in their own personal Space, admins keep org-wide
/// control").
///
/// `schedule_creator_subject_id` is `None` for a create call (the schedule
/// does not exist yet) and `Some` for update/delete (the resolved owner of
/// the existing row). An org admin/owner always passes, unchanged from
/// before; a non-admin passes only when they are that exact schedule's own
/// creator.
fn can_author_own_schedule(user: &AuthenticatedUser, schedule_creator_subject_id: Option<&str>) -> bool {
    has_authorized_org_role(user, &["owner", "admin"])
        || schedule_creator_subject_id.is_some_and(|id| id == user.user_id)
}

/// A cron-create body naming an explicit `space_ref` — a Space Room, since
/// the chat-facing surface (`schedule-from-chat.ts`) never sends this field
/// and always resolves to the caller's own personal Space — still requires
/// the admin gate. Only the personal-Space path (no `space_ref` at all) is
/// relaxed to "any org member, for their own schedule": that relaxation is
/// exactly what `inject_personal_schedule_create_context` already verifies
/// server-side before minting the Control decision (it resolves the caller's
/// OWN personal Space, never an arbitrary one), so this is a narrowing of an
/// over-broad BFF-side pre-check, not a new authorization surface.
fn cron_create_names_explicit_space(body: &Value) -> bool {
    body.get("space_ref")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
}

/// Rebuild a forwardable query string from the incoming params, dropping empty
/// values. Upstream handlers read `q`, `kind`, `limit`, `after_id`.
fn build_query(params: &HashMap<String, String>) -> String {
    let pairs: Vec<String> = params
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(key, value)| {
            format!(
                "{}={}",
                urlencoding::encode(key),
                urlencoding::encode(value)
            )
        })
        .collect();
    if pairs.is_empty() {
        String::new()
    } else {
        format!("?{}", pairs.join("&"))
    }
}

async fn list_skills(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let url = format!(
        "{}/v1/skills{}",
        state.model_gateway_url,
        build_query(&params)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}

/// Create a durable org skill. Body (`name`, `description`, `content`,
/// `trigger_keywords`, ...) is forwarded to model-gateway `/v1/skills`
/// (→ capability-core `agent_skills`). Admin-gated; org id is never injected —
/// it is derived from the verified capability token's claims.
async fn create_skill(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Only organization admins can author skills.",
            )),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!("{}/v1/skills", state.model_gateway_url);
    shared::proxy_model_json_with_capability(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Update a durable org skill (enable/disable, edit content/description).
/// Admin-gated; proxied to model-gateway `/v1/skills/{id}` (PATCH).
async fn update_skill(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Only organization admins can edit skills.",
            )),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!(
        "{}/v1/skills/{}",
        state.model_gateway_url,
        urlencoding::encode(&skill_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::PATCH,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Delete a durable org skill. Admin-gated; proxied to model-gateway
/// `/v1/skills/{id}` (DELETE).
async fn delete_skill(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Only organization admins can delete skills.",
            )),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!(
        "{}/v1/skills/{}",
        state.model_gateway_url,
        urlencoding::encode(&skill_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// List the org's plugin packages (open to members so the UI can show them).
async fn list_plugins(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let url = format!(
        "{}/v1/plugins{}",
        state.model_gateway_url,
        build_query(&params)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}

/// Register a plugin package (`name`, `version`, `description`, `manifest_json`).
/// Admin-gated; proxied to model-gateway `/v1/plugins` → capability-core. New
/// plugins default disabled until an admin enables them.
async fn create_plugin(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Only organization admins can register plugins.",
            )),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!("{}/v1/plugins", state.model_gateway_url);
    shared::proxy_model_json_with_capability(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Update a plugin (enable/disable, pin, rollout state). Admin-gated.
async fn update_plugin(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(plugin_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Only organization admins can change plugins.",
            )),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!(
        "{}/v1/plugins/{}",
        state.model_gateway_url,
        urlencoding::encode(&plugin_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::PATCH,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Delete (soft) a plugin package. Admin-gated.
async fn delete_plugin(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(plugin_id): Path<String>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Only organization admins can delete plugins.",
            )),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!(
        "{}/v1/plugins/{}",
        state.model_gateway_url,
        urlencoding::encode(&plugin_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// List the org's cron schedules (open to members).
async fn list_cron(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let url = format!(
        "{}/v1/cron{}",
        state.model_gateway_url,
        build_query(&params)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}

/// The recent fires of one schedule, for "See results". Creator-scoped like
/// update and delete: an org admin/owner, or the schedule's own creator. The
/// fires themselves are read from capability-core for the verified org.
async fn list_cron_fires(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(cron_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    if let Err(response) = authorize_own_schedule(
        &state,
        &user,
        &cron_id,
        token.as_deref(),
        &capability,
        "Only an organization admin or this schedule's own creator can see its results.",
    )
    .await
    {
        return response.into_response();
    }
    let url = format!(
        "{}/v1/cron/{}/fires{}",
        state.model_gateway_url,
        urlencoding::encode(&cron_id),
        build_query(&params)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// SCHEDULE_CONTRACT_ENABLED (DEC-16). Read strictly: only the literal `true`
/// lets a create carry a stored-consent contract to Control, so anything else
/// leaves schedules draft-only.
// Read per request, not from AppState: the state is not owned by this change.
fn schedule_contract_enabled() -> bool {
    std::env::var("SCHEDULE_CONTRACT_ENABLED").is_ok_and(|value| value == "true")
}

/// Create a cron schedule (`name`, `schedule_expr`, `timezone`, `task_template`,
/// `enabled`). Proxied to model-gateway `/v1/cron` → capability-core, which
/// validates the cron expression and seeds next_fire_at.
///
/// Admin-gated for an explicit (Space Room) `space_ref`, unchanged. Relaxed
/// for the personal-Space path (no `space_ref` at all): any authenticated org
/// member may create their OWN personal schedule from chat (owner ruling
/// 2026-09-28 #1) — see `can_author_own_schedule`'s doc.
async fn create_cron(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> impl IntoResponse {
    if cron_create_names_explicit_space(&body) && !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Only organization admins can create cron schedules in a shared Space.",
            )),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let Some(org_id) = user
        .active_org_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to create a Space schedule.",
            )),
        )
            .into_response();
    };
    let counter = CRON_CREATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let schedule_id = format!("cron_{}_{}_{}", user.user_id, nanos, counter);
    let idempotency_key = format!("space-cron-create:{schedule_id}");
    if let Err(response) = crate::domains::spaces::inject_personal_schedule_create_context(
        &state,
        &user,
        org_id,
        &mut body,
        &schedule_id,
        &idempotency_key,
        schedule_contract_enabled(),
    )
    .await
    {
        return response.into_response();
    }
    let url = format!("{}/v1/cron", state.model_gateway_url);
    shared::proxy_model_json_with_capability(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Resolve `cron_id`'s `creator_subject_id` via a cheap `GET /v1/cron/{id}`
/// (already proxied 1:1 to capability-core) and apply
/// `can_author_own_schedule`. An org admin/owner short-circuits without the
/// lookup, unchanged from before. Returns `Err` with the exact response to
/// send back: either the lookup's own failure propagated as-is (a 404 for an
/// id that does not exist or is outside the caller's org stays a 404, not a
/// misleading 403), or a 403 when the caller is neither an admin nor this
/// schedule's own creator.
async fn authorize_own_schedule(
    state: &AppState,
    user: &AuthenticatedUser,
    cron_id: &str,
    token: Option<&str>,
    capability: &str,
    forbidden_message: &'static str,
) -> Result<(), (StatusCode, Json<Value>)> {
    if has_authorized_org_role(user, &["owner", "admin"]) {
        return Ok(());
    }
    let url = format!(
        "{}/v1/cron/{}",
        state.model_gateway_url,
        urlencoding::encode(cron_id)
    );
    let (status, Json(schedule)) = shared::proxy_model_json_with_capability(
        state,
        Method::GET,
        &url,
        None,
        token,
        Some(capability),
        user,
    )
    .await;
    if !status.is_success() {
        return Err((status, Json(schedule)));
    }
    let creator = schedule.get("creator_subject_id").and_then(Value::as_str);
    if can_author_own_schedule(user, creator) {
        return Ok(());
    }
    Err((
        StatusCode::FORBIDDEN,
        Json(error("forbidden", forbidden_message)),
    ))
}

/// Update a cron schedule (enable/disable, edit expression). Admin-gated for
/// anyone else's schedule; relaxed for the caller's own (owner ruling
/// 2026-09-28 #1) — see `authorize_own_schedule`.
async fn update_cron(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(cron_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    if let Err(response) = authorize_own_schedule(
        &state,
        &user,
        &cron_id,
        token.as_deref(),
        &capability,
        "Only an organization admin or this schedule's own creator can change it.",
    )
    .await
    {
        return response.into_response();
    }
    let url = format!(
        "{}/v1/cron/{}",
        state.model_gateway_url,
        urlencoding::encode(&cron_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::PATCH,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Delete a cron schedule. Admin-gated for anyone else's schedule; relaxed
/// for the caller's own (owner ruling 2026-09-28 #1) — see
/// `authorize_own_schedule`.
async fn delete_cron(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(cron_id): Path<String>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    if let Err(response) = authorize_own_schedule(
        &state,
        &user,
        &cron_id,
        token.as_deref(),
        &capability,
        "Only an organization admin or this schedule's own creator can delete it.",
    )
    .await
    {
        return response.into_response();
    }
    let url = format!(
        "{}/v1/cron/{}",
        state.model_gateway_url,
        urlencoding::encode(&cron_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

async fn list_capabilities(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let url = format!(
        "{}/v1/capabilities{}",
        state.model_gateway_url,
        build_query(&params)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{can_author_own_schedule, cron_create_names_explicit_space};
    use crate::middleware::AuthenticatedUser;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn member(user_id: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: user_id.to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    fn owner(user_id: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            // has_authorized_org_role reads the live Control membership
            // decision, never auth_role or a cached session role — see its
            // own doc comment.
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "owner".to_owned(),
            }),
            ..member(user_id)
        }
    }

    // can_author_own_schedule — the shared gate behind create_cron's
    // personal-Space path and authorize_own_schedule's update/delete lookup.

    #[test]
    fn own_schedule_author_check_allows_an_admin_for_any_schedule() {
        assert!(can_author_own_schedule(&owner("admin-1"), Some("someone-else")));
        assert!(can_author_own_schedule(&owner("admin-1"), None));
    }

    #[test]
    fn own_schedule_author_check_allows_the_exact_creator() {
        assert!(can_author_own_schedule(&member("user-1"), Some("user-1")));
    }

    #[test]
    fn own_schedule_author_check_refuses_a_member_on_someone_elses_schedule() {
        assert!(!can_author_own_schedule(&member("user-1"), Some("user-2")));
    }

    #[test]
    fn own_schedule_author_check_refuses_a_member_when_no_creator_is_known_yet() {
        // The create-time shape: nothing to compare against, so a non-admin
        // must be refused rather than let through by a missing comparison.
        assert!(!can_author_own_schedule(&member("user-1"), None));
    }

    // cron_create_names_explicit_space — what routes create_cron through the
    // relaxed personal-Space path vs. the unchanged admin-gated Space-Room path.

    #[test]
    fn create_cron_with_no_space_ref_is_the_personal_space_path() {
        assert!(!cron_create_names_explicit_space(&json!({"name": "x"})));
        assert!(!cron_create_names_explicit_space(&json!({"space_ref": ""})));
        assert!(!cron_create_names_explicit_space(&json!({"space_ref": "   "})));
    }

    #[test]
    fn create_cron_with_an_explicit_space_ref_still_requires_admin() {
        assert!(cron_create_names_explicit_space(&json!({"space_ref": "space-1"})));
    }

    // list_cron_fires — router-through: the session decides the org and the
    // creator gate, and a forged org header never reaches model-gateway.
    async fn fires_test_servers(creator: &str) -> (MockServer, MockServer, MockServer) {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for path in ["/api/model-plane/token", "/api/capability-core/token"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(path))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "delegated-token"})),
                )
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let model = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/cron/cron_1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cron_1", "creator_subject_id": creator
            })))
            .mount(&model)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/cron/cron_1/fires"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [{
                "id": "fire_1", "status": "completed", "outcome": "completed",
                "run_id": "run_1", "thread_id": "thread_1", "withheld_effects": 0
            }]})))
            .mount(&model)
            .await;
        (auth, user_core, model)
    }

    fn fires_test_state(
        auth: &MockServer,
        user_core: &MockServer,
        model: &MockServer,
    ) -> crate::config::AppState {
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model.uri();
        state
    }

    #[tokio::test]
    async fn list_cron_fires_forwards_org_scope() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (auth, user_core, model) = fires_test_servers("user-1").await;
        let state = fires_test_state(&auth, &user_core, &model);
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/cron/cron_1/fires?limit=5")
                    .header("cookie", "better-auth.session_token=session-1")
                    .header("x-verevon-org-id", "org-forged")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["data"][0]["thread_id"], "thread_1");
        let model_requests = model.received_requests().await.unwrap();
        let fires = model_requests
            .iter()
            .find(|request| request.url.path() == "/v1/cron/cron_1/fires")
            .expect("fires request reaches model-gateway");
        assert_eq!(fires.url.query(), Some("limit=5"));
        assert!(fires.headers.get("x-verevon-org-id").is_none());
    }

    #[tokio::test]
    async fn list_cron_fires_refuses_a_member_who_did_not_create_the_schedule() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (auth, user_core, model) = fires_test_servers("user-2").await;
        let state = fires_test_state(&auth, &user_core, &model);
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/cron/cron_1/fires")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
        let model_requests = model.received_requests().await.unwrap();
        assert!(model_requests
            .iter()
            .all(|request| request.url.path() != "/v1/cron/cron_1/fires"));
    }
}
