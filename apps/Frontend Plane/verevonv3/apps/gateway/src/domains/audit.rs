//! Audit read surface.
//!
//! Proxies the Control Plane audit-core read API (internal-key auth) so the SPA
//! Trust Center can aggregate tool-action events (`event=tool_action`, with
//! `details.tool` / `details.data_category`) per connected integration.
//!
//! The org is resolved server-side from the validated session — the browser
//! never gets to pick which org's audit log it reads. Only the `event`,
//! `user_id`, `since`, `until`, `limit`, `plane`, `outcome`, `subject`,
//! `resource_id` and `cursor` filters are forwarded from the query string; `org_id` is always the caller's authorized org.
//!
//! Role gate (WS05-S1): org owners and admins read the whole org trail. Any other
//! member is narrowed to their own events here, before the upstream call, and a
//! foreign `user_id` is refused with 403 `audit_scope_denied`. The one exception
//! is `event=tool_action` with no `user_id`, which audit-core answers with a
//! redacted, content-free org-wide projection (the Trust Center). audit-core
//! re-derives the same decision from the signed role, so this layer is a second
//! check, never the only one.

use axum::{
    extract::{Extension, Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use reqwest::{Method, StatusCode};
use serde::Deserialize;

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error_with_details,
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_json},
};

/// Largest page a gateway caller may ask for; audit-core's own ceiling is higher.
const MAX_AUDIT_LIMIT: u32 = 200;
const TOOL_ACTION_EVENT: &str = "tool_action";
/// Longest filter or cursor value forwarded upstream.
const MAX_FILTER_LEN: usize = 512;

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/audit", get(list_audit))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Read-side audit filters the SPA is allowed to set. `org_id` is intentionally
/// absent — it is always the authorized org, never client-supplied.
#[derive(Debug, Default, Deserialize)]
pub(super) struct AuditQuery {
    event: Option<String>,
    user_id: Option<String>,
    since: Option<String>,
    until: Option<String>,
    limit: Option<u32>,
    plane: Option<String>,
    outcome: Option<String>,
    subject: Option<String>,
    resource_id: Option<String>,
    cursor: Option<String>,
}

async fn list_audit(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<AuditQuery>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;

    // audit-core scopes every read by org_id and requires it; without an
    // authorized org there is nothing to read, so return an empty envelope
    // rather than a 400 the SPA would have to special-case.
    if org_id.trim().is_empty() {
        return Json(serde_json::json!({
            "data": [],
            "meta": { "count": 0 },
            "error": null,
        }))
        .into_response();
    }

    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let event = query.event.filter(|v| !v.trim().is_empty());
    let user_id = query.user_id.filter(|v| !v.trim().is_empty());
    let user_id = match scoped_user_id(
        has_authorized_org_role(&user, &["owner", "admin"]),
        &user.user_id,
        event.as_deref(),
        user_id,
    ) {
        Ok(user_id) => user_id,
        Err(()) => {
            return (
                StatusCode::FORBIDDEN,
                Json(error_with_details(
                    "audit_scope_denied",
                    "You can only read your own audit events.",
                    serde_json::json!({ "scope": "self" }),
                )),
            )
                .into_response();
        }
    };
    let mut params: Vec<(&str, String)> = vec![("org_id", org_id.clone())];
    if let Some(event) = event {
        params.push(("event", event));
    }
    if let Some(user_id) = user_id {
        params.push(("user_id", user_id));
    }
    if let Some(since) = query.since.filter(|v| !v.trim().is_empty()) {
        params.push(("since", since));
    }
    if let Some(until) = query.until.filter(|v| !v.trim().is_empty()) {
        params.push(("until", until));
    }
    if let Some(limit) = query.limit {
        params.push(("limit", limit.clamp(1, MAX_AUDIT_LIMIT).to_string()));
    }
    // WS05-S4 filters and keyset cursor. They only narrow; audit-core applies the
    // role scope on top, and the cursor is opaque (audit-core rejects a bad one).
    for (key, value) in [
        ("plane", query.plane),
        ("outcome", query.outcome),
        ("subject", query.subject),
        ("resource_id", query.resource_id),
        ("cursor", query.cursor),
    ] {
        if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
            if value.len() <= MAX_FILTER_LEN {
                params.push((key, value));
            }
        }
    }

    let query_string = params
        .iter()
        .map(|(key, value)| format!("{}={}", key, urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    let url = format!("{}/v1/audit?{}", state.audit_core_url, query_string);

    let (status, body) = proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    (status, body).into_response()
}

/// Decides the `user_id` filter forwarded upstream. Admins keep whatever they
/// asked for. Everyone else is pinned to their own id: a foreign `user_id` is an
/// `Err` (403), and a request with no `user_id` is rewritten to the caller's id
/// unless it is the redacted org-wide `tool_action` view, which audit-core serves.
fn scoped_user_id(
    is_admin: bool,
    caller_id: &str,
    event: Option<&str>,
    requested: Option<String>,
) -> Result<Option<String>, ()> {
    if is_admin {
        return Ok(requested);
    }
    match requested {
        Some(requested) if requested != caller_id => Err(()),
        Some(requested) => Ok(Some(requested)),
        None if event == Some(TOOL_ACTION_EVENT) => Ok(None),
        None => Ok(Some(caller_id.to_string())),
    }
}

#[cfg(test)]
mod tests {
    //! WS05-S1. The pure scope decision is tested directly; the handler is
    //! driven through the REAL router with the real session middleware, and the
    //! upstream doubles answer only a request that carries the gateway's v3
    //! delegation for the session org, user and role.
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{header as wm_header, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const COOKIE: &str = "better-auth.session_token=session-1";

    #[test]
    fn a_non_admin_asking_for_a_foreign_user_id_is_refused() {
        assert_eq!(
            scoped_user_id(false, "user-1", None, Some("user-2".into())),
            Err(())
        );
        assert_eq!(
            scoped_user_id(
                false,
                "user-1",
                Some(TOOL_ACTION_EVENT),
                Some("user-2".into())
            ),
            Err(())
        );
    }

    #[test]
    fn a_non_admin_asking_for_their_own_user_id_is_allowed() {
        assert_eq!(
            scoped_user_id(false, "user-1", None, Some("user-1".into())),
            Ok(Some("user-1".into()))
        );
        assert_eq!(
            scoped_user_id(
                false,
                "user-1",
                Some(TOOL_ACTION_EVENT),
                Some("user-1".into())
            ),
            Ok(Some("user-1".into()))
        );
    }

    #[test]
    fn a_non_admin_tool_action_read_with_no_user_id_stays_unpinned_for_the_redacted_view() {
        assert_eq!(
            scoped_user_id(false, "user-1", Some(TOOL_ACTION_EVENT), None),
            Ok(None)
        );
    }

    #[test]
    fn a_non_admin_read_with_no_user_id_is_pinned_to_the_caller() {
        assert_eq!(
            scoped_user_id(false, "user-1", None, None),
            Ok(Some("user-1".into()))
        );
        assert_eq!(
            scoped_user_id(false, "user-1", Some("login"), None),
            Ok(Some("user-1".into()))
        );
        // Only the exact tool_action event earns the org-wide redacted view.
        assert_eq!(
            scoped_user_id(false, "user-1", Some("tool_action_extra"), None),
            Ok(Some("user-1".into()))
        );
    }

    #[test]
    fn an_admin_keeps_whatever_user_id_filter_they_asked_for() {
        assert_eq!(scoped_user_id(true, "user-1", None, None), Ok(None));
        assert_eq!(
            scoped_user_id(true, "user-1", None, Some("user-2".into())),
            Ok(Some("user-2".into()))
        );
        assert_eq!(
            scoped_user_id(true, "user-1", Some("login"), Some("user-2".into())),
            Ok(Some("user-2".into()))
        );
    }

    struct Harness {
        auth: MockServer,
        user_core: MockServer,
        audit_core: MockServer,
    }

    impl Harness {
        async fn start(org_role: &str) -> Self {
            let auth = MockServer::start().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/auth/get-session"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                    "session": {"activeOrganizationId": "org-1"}
                })))
                .mount(&auth)
                .await;
            let user_core = MockServer::start().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/v1/me/session-context"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "userId": "user-1", "orgId": "org-1", "role": org_role,
                    "onboardingStatus": "COMPLETED"
                })))
                .mount(&user_core)
                .await;
            Self {
                auth,
                user_core,
                audit_core: MockServer::start().await,
            }
        }

        fn state(&self) -> AppState {
            let mut state = crate::tests::test_state(false);
            state.auth_core_url = self.auth.uri();
            state.user_core_url = self.user_core.uri();
            state.audit_core_url = self.audit_core.uri();
            state
        }

        /// Mounts an audit-core double that only answers the delegated read.
        async fn mount_audit(&self, role: &str) {
            Mock::given(wm_method("GET"))
                .and(wm_path("/v1/audit"))
                .and(wm_header("x-service-id", "verevon-gateway"))
                .and(wm_header("x-delegation-version", "v3"))
                .and(wm_header("x-org-id", "org-1"))
                .and(wm_header("x-user-id", "user-1"))
                .and(wm_header("x-user-role", role))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [], "meta": {"count": 0, "scope": "self"}, "error": null
                })))
                .mount(&self.audit_core)
                .await;
        }

        async fn upstream_query(&self) -> Vec<String> {
            self.audit_core
                .received_requests()
                .await
                .expect("recording")
                .iter()
                .map(|request| request.url.query().unwrap_or_default().to_string())
                .collect()
        }

        async fn get(&self, uri: &str) -> (u16, Value) {
            let response = crate::build_router(self.state())
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(uri)
                        .header("cookie", COOKIE)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status().as_u16();
            let parsed: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap_or(Value::Null);
            (status, parsed)
        }
    }

    #[tokio::test]
    async fn a_member_asking_for_a_foreign_user_id_gets_403_and_no_upstream_call() {
        let harness = Harness::start("member").await;
        harness.mount_audit("member").await;
        let (status, body) = harness.get("/api/v1/audit?user_id=user-2&limit=10").await;
        assert_eq!(status, 403, "{body}");
        assert_eq!(body["error"]["code"], "audit_scope_denied");
        assert_eq!(body["error"]["details"]["scope"], "self");
        assert!(
            harness.upstream_query().await.is_empty(),
            "a refused read must not reach audit-core"
        );
    }

    #[tokio::test]
    async fn a_member_read_is_pinned_to_their_own_user_id_and_the_session_org() {
        let harness = Harness::start("member").await;
        harness.mount_audit("member").await;
        let (status, body) = harness
            .get("/api/v1/audit?org_id=org-2&limit=25&bogus=1")
            .await;
        assert_eq!(status, 200, "{body}");
        let queries = harness.upstream_query().await;
        assert_eq!(queries, vec!["org_id=org-1&user_id=user-1&limit=25"]);
    }

    #[tokio::test]
    async fn a_member_tool_action_read_with_no_user_id_is_forwarded_unpinned() {
        let harness = Harness::start("member").await;
        harness.mount_audit("member").await;
        let (status, body) = harness.get("/api/v1/audit?event=tool_action").await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            harness.upstream_query().await,
            vec!["org_id=org-1&event=tool_action"]
        );
    }

    #[tokio::test]
    async fn an_admin_may_read_any_user_and_is_not_pinned() {
        let harness = Harness::start("admin").await;
        harness.mount_audit("admin").await;
        let (status, body) = harness.get("/api/v1/audit?user_id=user-2").await;
        assert_eq!(status, 200, "{body}");
        let (status, body) = harness.get("/api/v1/audit").await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            harness.upstream_query().await,
            vec!["org_id=org-1&user_id=user-2", "org_id=org-1"]
        );
    }

    #[tokio::test]
    async fn the_limit_is_clamped_to_1_through_200() {
        let harness = Harness::start("admin").await;
        harness.mount_audit("admin").await;
        for uri in [
            "/api/v1/audit?limit=0",
            "/api/v1/audit?limit=1",
            "/api/v1/audit?limit=200",
            "/api/v1/audit?limit=201",
            "/api/v1/audit?limit=100000",
        ] {
            let (status, body) = harness.get(uri).await;
            assert_eq!(status, 200, "{uri}: {body}");
        }
        assert_eq!(
            harness.upstream_query().await,
            vec![
                "org_id=org-1&limit=1",
                "org_id=org-1&limit=1",
                "org_id=org-1&limit=200",
                "org_id=org-1&limit=200",
                "org_id=org-1&limit=200",
            ]
        );
    }
}
