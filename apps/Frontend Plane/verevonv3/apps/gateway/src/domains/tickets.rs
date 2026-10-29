//! Ticketing domain — durable support work queue.
//!
//! Browser-facing `/api/v1/tickets/*` routes are thin authenticated proxies to
//! conversation-core-go. The org scope is derived from the validated session.

use axum::{
    extract::{Extension, Path, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    upstream::{proxy_conversation_json, proxy_json},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/tickets", get(list_tickets).post(create_ticket))
        .route(
            "/api/v1/tickets/conversations/{id}/classifications",
            post(classify_conversation),
        )
        .route("/api/v1/tickets/csat-scorecard", get(get_csat_scorecard))
        .route("/api/v1/tickets/{id}/activity", get(list_ticket_activity))
        .route(
            "/api/v1/tickets/{id}/csat-outcome",
            get(get_ticket_csat_outcome),
        )
        .route("/api/v1/tickets/{id}", get(get_ticket).patch(patch_ticket))
        .route(
            "/api/v1/tickets/{id}/support-recurrence-candidates",
            get(get_support_recurrence_candidates),
        )
        .route(
            "/api/v1/incidents",
            get(list_incidents).post(create_incident),
        )
        .route(
            "/api/v1/incidents/{id}",
            get(get_incident).patch(patch_incident),
        )
        .route("/api/v1/incidents/{id}/tickets", post(link_incident_ticket))
        .route("/api/v1/problems", get(list_problems).post(create_problem))
        .route(
            "/api/v1/problems/{id}",
            get(get_problem).patch(patch_problem),
        )
        .route(
            "/api/v1/ticket-teams",
            get(list_ticket_teams).post(create_ticket_team),
        )
        .route(
            "/api/v1/ticket-teams/{id}",
            axum::routing::patch(patch_ticket_team),
        )
        .route("/api/v1/tickets/{id}/links", post(link_ticket_resource))
        .route(
            "/api/v1/tickets/{id}/macros/{macro_id}/run",
            post(run_ticket_macro),
        )
        .route(
            "/api/v1/tickets/{id}/checklists",
            post(create_ticket_checklist),
        )
        .route(
            "/api/v1/tickets/{id}/checklists/{checklist_id}/items/{item_id}",
            axum::routing::patch(patch_ticket_checklist_item),
        )
        .route(
            "/api/v1/tickets/{id}/side-conversations",
            post(create_ticket_side_conversation),
        )
        .route(
            "/api/v1/tickets/{id}/side-conversations/{side_conversation_id}",
            axum::routing::patch(patch_ticket_side_conversation),
        )
        .route(
            "/api/v1/tickets/{id}/side-conversations/{side_conversation_id}/messages",
            post(add_ticket_side_conversation_message),
        )
        .route(
            "/api/v1/tickets/{id}/chat-handoff",
            post(record_ticket_chat_handoff),
        )
        .route(
            "/api/v1/ticket-views",
            get(list_ticket_views).post(create_ticket_view),
        )
        .route(
            "/api/v1/ticket-views/{id}",
            axum::routing::patch(patch_ticket_view),
        )
        .route(
            "/api/v1/ticket-macros",
            get(list_ticket_macros).post(create_ticket_macro),
        )
        .route(
            "/api/v1/ticket-macros/{id}",
            axum::routing::patch(patch_ticket_macro),
        )
        .route(
            "/api/v1/ticket-automation-rules",
            get(list_ticket_automation_rules).post(create_ticket_automation_rule),
        )
        .route(
            "/api/v1/ticket-automation-rules/{id}",
            axum::routing::patch(patch_ticket_automation_rule),
        )
        .route(
            "/api/v1/sla-policies",
            get(list_sla_policies).post(create_sla_policy),
        )
        .route(
            "/api/v1/sla-policies/{id}",
            axum::routing::patch(patch_sla_policy),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

fn qs(uri: &Uri) -> String {
    uri.query()
        .filter(|q| !q.is_empty())
        .map(|q| format!("?{q}"))
        .unwrap_or_default()
}

async fn list_tickets(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> Response {
    let url = format!("{}/api/v1/tickets{}", state.conversation_core_url, qs(&uri));
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

async fn create_ticket(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(&state, &user, Method::POST, "/api/v1/tickets", Some(body)).await
}

async fn get_ticket(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let path = format!("/api/v1/tickets/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::GET, &path, None).await
}

/// The semantic support-recurrence "similarity candidates" preview
/// (verevon-inbox.md's design gate). Unlike the other proxies in this file,
/// this one is gated on more than an active session: ZDR must be off (this
/// is a read of a semantic-similarity corpus, not just AI-proposal
/// persistence, so it doesn't reuse require_support_ai_review's
/// supportAi.mode gate — only its ZDR check applies here) and the caller's
/// effective capabilities must include support:recurrence:read. Both are
/// checked live against org-core on every request, never cached.
async fn get_support_recurrence_candidates(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = require_support_recurrence_access(&state, &user).await {
        return response;
    }
    let path = format!(
        "/api/v1/tickets/{}/support-recurrence-candidates",
        urlencoding::encode(&id)
    );
    forward_ticket_json(&state, &user, Method::GET, &path, None).await
}

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

async fn require_support_recurrence_access(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<(), Response> {
    let Some(membership) = user.authorized_membership.as_ref() else {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "active_organization_required",
                "Select an active organization first.",
            )),
        )
            .into_response());
    };
    let org_id = membership.organization_id.trim();
    if org_id.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "active_organization_required",
                "Select an active organization first.",
            )),
        )
            .into_response());
    }
    let role = membership.role.trim();

    let actor = actor_for(user);
    let (status, Json(organization)) = proxy_json(
        state,
        Method::GET,
        &format!(
            "{}/api/v1/organizations/{}",
            state.org_core_url,
            urlencoding::encode(org_id)
        ),
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "support_recurrence_policy_unavailable",
                "Similarity candidates are unavailable until the organization policy can be verified.",
            )),
        )
            .into_response());
    }
    let zdr_enabled = organization
        .get("data")
        .unwrap_or(&organization)
        .get("metadata")
        .and_then(|value| value.get("interactiveRetention"))
        .and_then(|retention| retention.get("zdr"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if zdr_enabled {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(error(
                "zdr_recurrence_forbidden",
                "Similarity candidates are not available while Zero Data Retention is enabled.",
            )),
        )
            .into_response());
    }

    let (status, Json(capabilities_response)) = proxy_json(
        state,
        Method::GET,
        &format!(
            "{}/internal/orgs/{}/roles/{}/capabilities",
            state.org_core_url,
            urlencoding::encode(org_id),
            urlencoding::encode(role)
        ),
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "support_recurrence_policy_unavailable",
                "Similarity candidates are unavailable until permissions can be verified.",
            )),
        )
            .into_response());
    }
    let has_capability = capabilities_response
        .get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|capabilities| {
            capabilities
                .iter()
                .any(|value| value.as_str() == Some("support:recurrence:read"))
        });
    if !has_capability {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "support_recurrence_permission_required",
                "This role does not have permission to view similarity candidates.",
            )),
        )
            .into_response());
    }

    Ok(())
}

async fn list_ticket_activity(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    uri: Uri,
) -> Response {
    let path = format!(
        "/api/v1/tickets/{}/activity{}",
        urlencoding::encode(&id),
        qs(&uri)
    );
    forward_ticket_json(&state, &user, Method::GET, &path, None).await
}

async fn get_ticket_csat_outcome(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let path = format!("/api/v1/tickets/{}/csat-outcome", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::GET, &path, None).await
}

async fn get_csat_scorecard(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(&state, &user, Method::GET, "/api/v1/csat-scorecard", None).await
}

async fn patch_ticket(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/tickets/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn list_incidents(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(&state, &user, Method::GET, "/api/v1/incidents", None).await
}

async fn create_incident(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(&state, &user, Method::POST, "/api/v1/incidents", Some(body)).await
}

async fn get_incident(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let path = format!("/api/v1/incidents/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::GET, &path, None).await
}

async fn patch_incident(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/incidents/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn link_incident_ticket(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/incidents/{}/tickets", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::POST, &path, Some(body)).await
}

async fn list_problems(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(&state, &user, Method::GET, "/api/v1/problems", None).await
}

async fn create_problem(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(&state, &user, Method::POST, "/api/v1/problems", Some(body)).await
}

async fn get_problem(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let path = format!("/api/v1/problems/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::GET, &path, None).await
}

async fn patch_problem(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/problems/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn link_ticket_resource(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/tickets/{}/links", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::POST, &path, Some(body)).await
}

async fn run_ticket_macro(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, macro_id)): Path<(String, String)>,
) -> Response {
    let path = format!(
        "/api/v1/tickets/{}/macros/{}/run",
        urlencoding::encode(&id),
        urlencoding::encode(&macro_id)
    );
    forward_ticket_json(&state, &user, Method::POST, &path, None).await
}

async fn create_ticket_checklist(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/tickets/{}/checklists", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::POST, &path, Some(body)).await
}

async fn patch_ticket_checklist_item(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, checklist_id, item_id)): Path<(String, String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!(
        "/api/v1/tickets/{}/checklists/{}/items/{}",
        urlencoding::encode(&id),
        urlencoding::encode(&checklist_id),
        urlencoding::encode(&item_id)
    );
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn create_ticket_side_conversation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!(
        "/api/v1/tickets/{}/side-conversations",
        urlencoding::encode(&id)
    );
    forward_ticket_json(&state, &user, Method::POST, &path, Some(body)).await
}

async fn patch_ticket_side_conversation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, side_conversation_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!(
        "/api/v1/tickets/{}/side-conversations/{}",
        urlencoding::encode(&id),
        urlencoding::encode(&side_conversation_id),
    );
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn add_ticket_side_conversation_message(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, side_conversation_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!(
        "/api/v1/tickets/{}/side-conversations/{}/messages",
        urlencoding::encode(&id),
        urlencoding::encode(&side_conversation_id),
    );
    forward_ticket_json(&state, &user, Method::POST, &path, Some(body)).await
}

async fn record_ticket_chat_handoff(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let path = format!("/api/v1/tickets/{}/chat-handoff", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::POST, &path, None).await
}

async fn list_ticket_views(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(&state, &user, Method::GET, "/api/v1/ticket-views", None).await
}

async fn create_ticket_view(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(
        &state,
        &user,
        Method::POST,
        "/api/v1/ticket-views",
        Some(body),
    )
    .await
}

async fn patch_ticket_view(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/ticket-views/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn list_ticket_teams(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(&state, &user, Method::GET, "/api/v1/ticket-teams", None).await
}

async fn create_ticket_team(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(
        &state,
        &user,
        Method::POST,
        "/api/v1/ticket-teams",
        Some(body),
    )
    .await
}

async fn patch_ticket_team(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/ticket-teams/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn list_ticket_macros(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(&state, &user, Method::GET, "/api/v1/ticket-macros", None).await
}

async fn create_ticket_macro(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(
        &state,
        &user,
        Method::POST,
        "/api/v1/ticket-macros",
        Some(body),
    )
    .await
}

async fn patch_ticket_macro(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/ticket-macros/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn list_ticket_automation_rules(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(
        &state,
        &user,
        Method::GET,
        "/api/v1/ticket-automation-rules",
        None,
    )
    .await
}

async fn create_ticket_automation_rule(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(
        &state,
        &user,
        Method::POST,
        "/api/v1/ticket-automation-rules",
        Some(body),
    )
    .await
}

async fn patch_ticket_automation_rule(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!(
        "/api/v1/ticket-automation-rules/{}",
        urlencoding::encode(&id)
    );
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn list_sla_policies(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(&state, &user, Method::GET, "/api/v1/sla-policies", None).await
}

async fn create_sla_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(
        &state,
        &user,
        Method::POST,
        "/api/v1/sla-policies",
        Some(body),
    )
    .await
}

async fn patch_sla_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/sla-policies/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn classify_conversation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!(
        "/api/v1/conversations/{}/ticket-classifications",
        urlencoding::encode(&id)
    );
    forward_ticket_json(&state, &user, Method::POST, &path, Some(body)).await
}

async fn forward_ticket_json(
    state: &AppState,
    user: &AuthenticatedUser,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Response {
    let url = format!("{}{}", state.conversation_core_url, path);
    proxy_conversation_json(state, method, &url, body, user, None)
        .await
        .into_response()
}

#[cfg(test)]
mod support_recurrence_access_tests {
    use super::*;
    use http_body_util::BodyExt;
    use wiremock::{
        matchers::{method as wm_method, path as wm_path},
        Mock, MockServer, ResponseTemplate,
    };

    fn member_user(role: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some(role.to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: role.to_owned(),
            }),
        }
    }

    fn user_without_membership() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: None,
            active_org_id: None,
            authorized_membership: None,
        }
    }

    async fn state_for(org_core: &MockServer) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.org_core_url = org_core.uri();
        state
    }

    async fn mock_org_core(zdr: bool, capabilities: &[&str]) -> MockServer {
        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "org-1",
                "metadata": { "interactiveRetention": { "zdr": zdr } }
            })))
            .mount(&org_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/internal/orgs/org-1/roles/member/capabilities"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "capabilities": capabilities
            })))
            .mount(&org_core)
            .await;
        org_core
    }

    async fn error_code(response: Response) -> String {
        let body: Value = serde_json::from_slice(
            &response
                .into_body()
                .collect()
                .await
                .expect("response body")
                .to_bytes(),
        )
        .expect("JSON response");
        body["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    #[tokio::test]
    async fn no_active_membership_is_forbidden_before_any_upstream_call() {
        let org_core = MockServer::start().await; // no mocks registered -- must not be called
        let state = state_for(&org_core).await;
        let err = require_support_recurrence_access(&state, &user_without_membership())
            .await
            .expect_err("expected rejection");
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
        assert_eq!(error_code(err).await, "active_organization_required");
    }

    #[tokio::test]
    async fn zdr_enabled_is_precondition_failed() {
        let org_core = mock_org_core(true, &["support:recurrence:read"]).await;
        let state = state_for(&org_core).await;
        let err = require_support_recurrence_access(&state, &member_user("member"))
            .await
            .expect_err("expected rejection");
        assert_eq!(err.status(), StatusCode::PRECONDITION_FAILED);
        assert_eq!(error_code(err).await, "zdr_recurrence_forbidden");
    }

    #[tokio::test]
    async fn missing_capability_is_forbidden() {
        let org_core = mock_org_core(false, &["org:read"]).await;
        let state = state_for(&org_core).await;
        let err = require_support_recurrence_access(&state, &member_user("member"))
            .await
            .expect_err("expected rejection");
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            error_code(err).await,
            "support_recurrence_permission_required"
        );
    }

    #[tokio::test]
    async fn non_zdr_org_with_capability_is_allowed() {
        let org_core = mock_org_core(false, &["support:recurrence:read"]).await;
        let state = state_for(&org_core).await;
        require_support_recurrence_access(&state, &member_user("member"))
            .await
            .expect("expected access to be granted");
    }

    #[tokio::test]
    async fn org_core_unavailable_is_service_unavailable() {
        let org_core = MockServer::start().await; // no mocks -- every call fails/404s
        let state = state_for(&org_core).await;
        let err = require_support_recurrence_access(&state, &member_user("member"))
            .await
            .expect_err("expected rejection");
        assert_eq!(err.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            error_code(err).await,
            "support_recurrence_policy_unavailable"
        );
    }
}
