//! Organisation flag values for the signed-in viewer (W5-079-c, F-t3-079).
//!
//! `GET /api/v1/org-flags` answers the active organisation's values for the presentation flags
//! org-core lists in `OrgOverridableFlags`, as `{data:{flags:{name:value}, revision}}`. The read goes
//! to org-core's member route, `GET /orgs/:id/flags` (org:read:self), on the signed-in viewer's
//! delegation, so org-core checks that viewer's own membership. The gateway holds no `:any` scope,
//! so org-core's internal route (org:flags:read:any) is out of its reach. One organisation's
//! answer is reused for 30 seconds through the gateway cache, admitted through `CacheAdmission`
//! like every content cache (a ZDR organisation is read on every request). Every answer is
//! `no-store`. The gateway keeps only each flag's name, value and revision, which org-core gives
//! every member alike, so one cached answer serves each member of the organisation.
//!
//! A read that fails answers `{data:{flags:{}, stale:true}}`, so each presentation flag falls back
//! to its default, which the SPA reads as off. org-core's own 404, from its `ORG_FLAGS_ENABLED`
//! switch being off, passes through, and the SPA reads it as no organisation layer. That switch
//! lives upstream, so the gateway reads no flag of its own.
//!
//! The values are a presentation ceiling only. The SPA ANDs them with its build switch, and no
//! authorization decision reads them.

use std::time::Duration;

use axum::{
    extract::{Extension, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::{
    cache_admission::{valid_org_scope, CacheAdmission},
    config::AppState,
    contracts::ActionActor,
    middleware::{require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_json},
};

/// The cache namespace of one organisation's answer, and how long that answer is reused.
const CACHE_NAMESPACE: &str = "org-flags";
const CACHE_TTL_SECS: u64 = 30;

/// Bound on the org-core read. A presentation flag never holds a page for longer than this.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// The longest flag name or value this route passes on.
const MAX_TOKEN_LENGTH: usize = 64;

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/org-flags", get(get_org_flags))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

async fn get_org_flags(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    // The organisation comes from the live membership the session resolved, never from the request.
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        // No organisation: nothing is set for the viewer, so every flag keeps its default.
        return answer(
            StatusCode::OK,
            json!({ "data": { "flags": {}, "revision": 0 } }),
        );
    }
    let Some(scope) = valid_org_scope(&org_id) else {
        return stale_answer();
    };
    // Content admission decides whether this organisation's answer may be read from or written to
    // the cache at all (a ZDR organisation, an unknown posture or a switched-off cache may not).
    // A refused admission reads org-core on every request; it never changes the answer.
    let admission = CacheAdmission::resolve(&state, &user, &headers, &Value::Null, false).await;
    admission.record_bypass(CACHE_NAMESPACE);
    // The key carries the organisation id in clear and hashes the rest, so one organisation's
    // answer is never served for another.
    let key = admission.key(CACHE_NAMESPACE, &["flags"]);
    if let (Some(key), true) = (key.as_deref(), admission.may_read) {
        if let Some(data) = state
            .cache
            .lookup_within_scoped(key, scope, CACHE_TTL_SECS)
            .await
        {
            return answer(StatusCode::OK, json!({ "data": data }));
        }
    }

    // The member route, on the signed-in viewer's delegation: org-core checks that viewer's
    // membership of this organisation. The internal route needs org:flags:read:any, which the
    // gateway can never hold.
    let url = format!("{}/orgs/{scope}/flags", state.org_core_url);
    let viewer = viewer_actor(&user);
    let read = tokio::time::timeout(
        READ_TIMEOUT,
        proxy_json(
            &state,
            Method::GET,
            &url,
            None,
            Some(scope),
            Some(&viewer),
            None,
        ),
    )
    .await;
    let Ok((status, Json(body))) = read else {
        return stale_answer();
    };
    if status == StatusCode::NOT_FOUND {
        // org-core's switch is off, or the organisation is unknown: there is no layer to read.
        return answer(status, body);
    }
    match org_flags_from_core(&body) {
        Some(data) if status.is_success() => {
            if let (Some(key), true) = (key.as_deref(), admission.may_write) {
                state.cache.store_scoped(key, scope, &data).await;
            }
            answer(StatusCode::OK, json!({ "data": data }))
        }
        _ => stale_answer(),
    }
}

/// An answer the browser must not keep.
fn answer(status: StatusCode, body: Value) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

/// The answer when the organisation's values could not be read: every presentation flag falls back
/// to its default, which is off.
fn stale_answer() -> Response {
    answer(
        StatusCode::OK,
        json!({ "data": { "flags": {}, "stale": true } }),
    )
}

/// The signed-in viewer as the actor of the org-core read, built the same way `actor_for` builds it
/// in `domains/orgs/shared.rs` for the other org-core routes.
fn viewer_actor(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

/// org-core's `{data:{flags:[...]}}` reshaped to `{flags:{name:value}, revision}`. An entry that is
/// not a flag name and value is left out, so that flag keeps its default. The revision is the sum of
/// the stored per-flag revisions, which moves on every write.
fn org_flags_from_core(body: &Value) -> Option<Value> {
    let entries = body.get("data")?.get("flags")?.as_array()?;
    let mut flags = Map::new();
    let mut revision: u64 = 0;
    for entry in entries {
        let name = entry.get("name").and_then(Value::as_str);
        let value = entry.get("value").and_then(Value::as_str);
        let (Some(name), Some(value)) = (name, value) else {
            continue;
        };
        if !is_token(name) || !is_token(value) {
            continue;
        }
        flags.insert(name.to_owned(), Value::String(value.to_owned()));
        revision =
            revision.saturating_add(entry.get("revision").and_then(Value::as_u64).unwrap_or(0));
    }
    Some(json!({ "flags": flags, "revision": revision }))
}

fn is_token(raw: &str) -> bool {
    !raw.is_empty()
        && raw.len() <= MAX_TOKEN_LENGTH
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[cfg(test)]
mod tests {
    //! W5-079-c tests: the read goes to org-core's member route on the viewer's delegation.
    //! They run against a wiremock org-core. The test state's cache is disabled, so every read
    //! reaches it.
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use wiremock::matchers::{
        header as wm_header, header_exists as wm_header_exists, method as wm_method,
        path as wm_path,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const MEMBER_PATH: &str = "/orgs/org-1/flags";
    const INTERNAL_PATH: &str = "/internal/orgs/org-1/flags";

    fn viewer() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    fn state_for(org_core: &MockServer) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.org_core_url = org_core.uri();
        state
    }

    /// The route, called as the signed-in viewer of org-1: the status, the headers and the body.
    async fn read_flags(state: &AppState) -> (StatusCode, HeaderMap, Value) {
        let response =
            get_org_flags(State(state.clone()), Extension(viewer()), HeaderMap::new()).await;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (
            status,
            headers,
            serde_json::from_slice(&bytes).expect("json"),
        )
    }

    #[tokio::test]
    async fn the_read_is_the_member_route_on_the_viewers_delegation() {
        let org_core = MockServer::start().await;
        let state = state_for(&org_core);
        let answer = json!({"data": {"flags": [
            {"name": "VITE_CHAT_READER_POLISH", "value": "true", "revision": 2}
        ]}});
        Mock::given(wm_method("GET"))
            .and(wm_path(MEMBER_PATH))
            .and(wm_header("x-service-id", "verevon-gateway"))
            .and(wm_header(
                "x-service-token",
                state.org_core_service_token.as_str(),
            ))
            .and(wm_header("x-user-id", "user-1"))
            .and(wm_header("x-org-id", "org-1"))
            .and(wm_header("x-user-role", "member"))
            .and(wm_header("x-delegation-version", "v3"))
            .and(wm_header_exists("x-delegation-timestamp"))
            .and(wm_header_exists("x-delegation-nonce"))
            .and(wm_header_exists("x-delegation-body-sha256"))
            .and(wm_header_exists("x-delegation-signature"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer))
            .expect(1)
            .mount(&org_core)
            .await;

        let (status, headers, body) = read_flags(&state).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-store")
        );
        assert_eq!(
            body,
            json!({"data": {"flags": {"VITE_CHAT_READER_POLISH": "true"}, "revision": 2}})
        );
        let requests = org_core.received_requests().await.unwrap_or_default();
        assert!(requests
            .iter()
            .all(|request| request.url.path() != INTERNAL_PATH));
    }

    #[tokio::test]
    async fn org_core_switched_off_passes_its_404_through() {
        let org_core = MockServer::start().await;
        let not_found = json!({"error": {"code": "not_found", "message": "not found"}});
        Mock::given(wm_method("GET"))
            .and(wm_path(MEMBER_PATH))
            .respond_with(ResponseTemplate::new(404).set_body_json(not_found.clone()))
            .mount(&org_core)
            .await;

        let (status, _, body) = read_flags(&state_for(&org_core)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, not_found);
    }

    #[tokio::test]
    async fn a_refused_read_answers_stale_so_every_flag_reads_off() {
        let org_core = MockServer::start().await;
        let refused = json!({"error": {"code": "forbidden", "message": "not a member"}});
        Mock::given(wm_method("GET"))
            .and(wm_path(MEMBER_PATH))
            .respond_with(ResponseTemplate::new(403).set_body_json(refused))
            .mount(&org_core)
            .await;

        let (status, _, body) = read_flags(&state_for(&org_core)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({"data": {"flags": {}, "stale": true}}));
    }
}
