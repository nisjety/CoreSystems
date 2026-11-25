//! Org-scoped member status and agent availability chips: the BFF half of WS11-S10.
//!
//! * `GET /api/v1/spaces/status` reads the caller's own status in their organization, with
//!   `meta.text_allowed` (whether this organization may set status text; unknown reads false);
//!   `PUT /api/v1/spaces/status {state, text?, expires_at?}` replaces it; `DELETE` clears it
//!   (idempotent, and allowed under Zero Data Retention because it keeps no content). A status is one
//!   Application row keyed by (organization, subject), so a text written in one organization is
//!   never visible in another (the Control profile `metadata.status` is global and is untouched).
//! * [`annotate_roster`] adds `status` to every `user` member of `GET /api/v1/spaces/{ref}/roster`
//!   and [`annotate_agents`] adds `activity` (`working`, `waiting`, `idle`) to every agent of
//!   `GET /api/v1/spaces/{ref}/agents`. Both run only after the route has already proved the
//!   caller's membership of the room (Control answers the roster; a non-member never gets here),
//!   and both degrade by leaving the field out: "could not check" is never drawn as "idle".
//! * `spaces.set_status` rides [`put_status_core`], the same function as the route.
//!
//! Content. The status text is the one free-text field of the attention tables. Setting one is a
//! durable write of stored content, so under Zero Data Retention it is the typed 412 before any
//! Application call; a state-only write (closed vocabulary, optional expiry) carries no content
//! and is allowed. Every read passes `includeText` only when the organization's posture is a
//! successful "not ZDR" (an unknown posture fails closed to no text). The text is never copied
//! into prompts, events or notifications by this module.
//!
//! The flag (`SPACE_STATUS_API`) is a rollout control, never an authorization input: the caller's
//! own row, Control's roster and Application's org-membership check decide what is read or written.
//! Flag off answers 404 `space_status_disabled` on the routes and leaves the roster and agent
//! responses unannotated.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_request` / `invalid_status`, 403
//! `active_membership_required`, 404 `space_status_disabled`, 412 `zdr_durable_mutation_forbidden`
//! (text under Zero Data Retention), 503 `status_unavailable` / `zdr_decision_unavailable`.

use axum::{
    extract::{rejection::JsonRejection, Extension, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{convex_gateway_call, room_admin::convex_mutation_message};
use crate::{
    config::AppState,
    envelope::error,
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
    zdr::{org_zdr_lookup, OrgZdrPosture},
};

type Reply = (StatusCode, Json<Value>);

/// The closed state words (the Application `spaceMemberStatus.state` union).
const STATES: [&str; 4] = ["available", "away", "dnd", "offline"];
/// Application answers at most this many subjects per status lookup.
const MAX_STATUS_SUBJECTS: usize = 200;
/// Application answers at most this many agents per activity lookup.
const MAX_ACTIVITY_AGENTS: usize = 100;

/// Registered through `module_routes()` behind `require_session`. A static segment: matchit ranks
/// it above `{space_ref}`, so a room can never shadow it.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/status",
        get(get_status).put(put_status).delete(clear_status),
    )
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn disabled() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "space_status_disabled",
        "Space status is not enabled.",
    )
}

fn membership_required() -> Reply {
    refuse(
        StatusCode::FORBIDDEN,
        "active_membership_required",
        "An active organization membership is required to use a status.",
    )
}

fn unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "status_unavailable",
        "Status is unavailable right now. Nothing was changed.",
    )
}

fn invalid_request() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "The request body is not valid.",
    )
}

fn invalid_status() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_status",
        "state must be available, away, dnd or offline; text at most 80 characters; expiry within 7 days.",
    )
}

/// Flag, then organization. Everything both routes share.
async fn gate(state: &AppState, user: &AuthenticatedUser) -> Result<String, Reply> {
    if !state.space_features.status_api {
        return Err(disabled());
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(membership_required());
    }
    Ok(org_id)
}

/// Whether text may be returned: only a successful "not ZDR" answer. Unknown fails closed.
async fn text_allowed(state: &AppState, user: &AuthenticatedUser) -> bool {
    org_zdr_lookup(state, user).await == OrgZdrPosture::Disabled
}

/// The wire shape of one status: `{state, text, expires_at}`.
fn status_json(value: &Value) -> Value {
    json!({
        "state": value.get("state").cloned().unwrap_or(Value::Null),
        "text": value.get("text").cloned().unwrap_or(Value::Null),
        "expires_at": value.get("expiresAt").cloned().unwrap_or(Value::Null),
    })
}

// ── the routes ───────────────────────────────────────────────────────────────────────────────────

async fn get_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Reply {
    let org_id = match gate(&state, &user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let include_text = text_allowed(&state, &user).await;
    // `meta.text_allowed` states the same posture the read used, so the status menu draws the text
    // field only for an organization positively known not to be ZDR (unknown fails closed to false).
    // It is display only: the write path refuses text on its own (`durable_write_refusal`).
    let meta = json!({"text_allowed": include_text});
    match convex_gateway_call(
        &state,
        "query",
        "spaceMemberStatus:getOwnStatusForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "includeText": include_text,
        }),
    )
    .await
    {
        // No row, or an expired one, reads as cleared: every field null.
        Ok(value) if value.is_null() => (
            StatusCode::OK,
            Json(json!({"data": {"state": null, "text": null, "expires_at": null}, "meta": meta})),
        ),
        Ok(value) => (
            StatusCode::OK,
            Json(json!({"data": status_json(&value), "meta": meta})),
        ),
        Err(()) => unavailable(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusBody {
    state: String,
    text: Option<String>,
    /// Epoch milliseconds; Application bounds it to the future and at most 7 days ahead.
    expires_at: Option<i64>,
}

async fn put_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Reply {
    let Ok(Json(body)) = body else {
        return invalid_request();
    };
    put_status_core(&state, &user, Some(&headers), &body).await
}

/// The status write, shared by the route and the `spaces.set_status` action. Replaces the caller's
/// own row entirely (an absent `text` or `expires_at` clears it). Text is refused typed for a
/// Zero Data Retention organization before any Application call.
pub(crate) async fn put_status_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: Option<&HeaderMap>,
    body: &Value,
) -> Reply {
    let org_id = match gate(state, user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let Ok(body) = serde_json::from_value::<StatusBody>(body.clone()) else {
        return invalid_request();
    };
    if !STATES.contains(&body.state.as_str()) {
        return invalid_status();
    }
    let has_text = body
        .text
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty());
    if has_text {
        if let Some(refusal) = crate::zdr::durable_write_refusal(state, user, headers, None).await {
            return refusal;
        }
    }
    let mut args = Map::new();
    args.insert("externalAuthId".to_owned(), json!(user.user_id));
    args.insert("externalOrgId".to_owned(), json!(org_id));
    args.insert("state".to_owned(), json!(body.state));
    if let Some(text) = &body.text {
        args.insert("text".to_owned(), json!(text));
    }
    if let Some(expires_at) = body.expires_at {
        args.insert("expiresAt".to_owned(), json!(expires_at));
    }
    match convex_mutation_message(state, "spaceMemberStatus:setStatusForGateway", Value::Object(args))
        .await
    {
        Ok(value) => (StatusCode::OK, Json(json!({"data": status_json(&value)}))),
        Err(Some(message)) if message.contains("invalid_status") => invalid_status(),
        Err(_) => unavailable(),
    }
}

/// `DELETE /api/v1/spaces/status`: removes the caller's own status in this organization (WS11-S10,
/// the clear route of the status menu). Idempotent: an absent row is cleared too. Clearing keeps no
/// content and only deletes, so it is allowed under Zero Data Retention; no org posture lookup and no
/// durable-write refusal apply. Answers the cleared shape a GET answers for no status: every field null.
async fn clear_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Reply {
    let org_id = match gate(&state, &user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    match convex_mutation_message(
        &state,
        "spaceMemberStatus:clearStatusForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
        }),
    )
    .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"data": {"state": null, "text": null, "expires_at": null}})),
        ),
        Err(_) => unavailable(),
    }
}

// ── roster and agent annotations ─────────────────────────────────────────────────────────────────

/// Adds `status` (`{state, text, expires_at}`, or `null` when none or expired) to every `user`
/// member of a successful Control roster body. Called after Control has proved the caller's
/// membership of the room. Flag off, a failed lookup or a non-success body leave it untouched.
pub(super) async fn annotate_roster(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    status: StatusCode,
    body: &mut Value,
) {
    if !status.is_success() || !state.space_features.status_api || org_id.trim().is_empty() {
        return;
    }
    let Some(members) = body
        .get_mut("data")
        .and_then(|data| data.get_mut("members"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    let mut subjects: Vec<String> = Vec::new();
    for member in members.iter() {
        if member.get("subject_type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        if let Some(id) = member.get("subject_id").and_then(Value::as_str) {
            if !id.trim().is_empty() && !subjects.iter().any(|seen| seen == id) {
                subjects.push(id.to_owned());
            }
        }
    }
    if subjects.is_empty() {
        return;
    }
    let include_text = text_allowed(state, user).await;
    let mut found: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    for chunk in subjects.chunks(MAX_STATUS_SUBJECTS) {
        let Ok(value) = convex_gateway_call(
            state,
            "query",
            "spaceMemberStatus:statusesForGateway",
            json!({
                "externalAuthId": user.user_id,
                "externalOrgId": org_id,
                "subjectIds": chunk,
                "includeText": include_text,
            }),
        )
        .await
        else {
            // A failed read leaves `status` out of the response entirely: absent means "not
            // checked", `null` means "checked, none".
            return;
        };
        for row in value.as_array().cloned().unwrap_or_default() {
            if let Some(id) = row.get("externalAuthId").and_then(Value::as_str) {
                found.insert(id.to_owned(), status_json(&row));
            }
        }
    }
    for member in members.iter_mut() {
        if member.get("subject_type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let Some(id) = member.get("subject_id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        if let Some(object) = member.as_object_mut() {
            object.insert(
                "status".to_owned(),
                found.get(&id).cloned().unwrap_or(Value::Null),
            );
        }
    }
}

/// Adds `activity` (`working`, `waiting`, `idle`) to every agent of the room's agent list. A stale
/// or unknown row is `idle` by Application's rule; a failed lookup leaves the field out.
pub(super) async fn annotate_agents(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    agents: &mut [Value],
) {
    if !state.space_features.status_api || org_id.trim().is_empty() || agents.is_empty() {
        return;
    }
    let mut ids: Vec<String> = Vec::new();
    for agent in agents.iter() {
        if let Some(id) = agent.get("subject_id").and_then(Value::as_str) {
            if !id.trim().is_empty() && !ids.iter().any(|seen| seen == id) {
                ids.push(id.to_owned());
            }
        }
    }
    let mut found: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for chunk in ids.chunks(MAX_ACTIVITY_AGENTS) {
        let Ok(value) = convex_gateway_call(
            state,
            "query",
            "spaceMemberStatus:agentActivityForGateway",
            json!({
                "externalAuthId": user.user_id,
                "externalOrgId": org_id,
                "spaceRef": space_ref,
                "agentIds": chunk,
            }),
        )
        .await
        else {
            return;
        };
        for row in value.as_array().cloned().unwrap_or_default() {
            if let (Some(id), Some(chip)) = (
                row.get("agentId").and_then(Value::as_str),
                row.get("state").and_then(Value::as_str),
            ) {
                found.insert(id.to_owned(), chip.to_owned());
            }
        }
    }
    for agent in agents.iter_mut() {
        let Some(id) = agent.get("subject_id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        if let Some(object) = agent.as_object_mut() {
            let chip = found.get(&id).map_or("idle", String::as_str);
            object.insert("activity".to_owned(), json!(chip));
        }
    }
}

#[cfg(test)]
mod tests {
    //! WS11-S10.gw tests: the status routes through the real router, and the roster and agent
    //! annotations called directly, with wiremock for auth-core, Control (user-core), org-core and
    //! Application (Convex). No model is called.
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::{body_partial_json, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Fx {
        state: AppState,
        _auth: MockServer,
        _user_core: MockServer,
        _org: MockServer,
        application: MockServer,
    }

    /// `zdr`: org-core's answer for `org-1` (`None` is an outage, so an unknown posture). The
    /// status flag is on.
    async fn fx(zdr: Option<bool>) -> Fx {
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
                "userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(match zdr {
                Some(zdr) => ResponseTemplate::new(200).set_body_json(
                    json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}),
                ),
                None => ResponseTemplate::new(500),
            })
            .mount(&org)
            .await;
        let application = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org.uri();
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.status_api = true;
        Fx {
            state,
            _auth: auth,
            _user_core: user_core,
            _org: org,
            application,
        }
    }

    async fn mount_convex(fx: &Fx, route: &str, function: &str, template: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(wm_path(route))
            .and(body_partial_json(json!({"path": function})))
            .respond_with(template)
            .mount(&fx.application)
            .await;
    }

    fn value(value: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": value}))
    }

    fn convex_error(message: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": message}))
    }

    async fn call(fx: &Fx, method: &str, body: Option<Value>, zdr_header: bool) -> (u16, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri("/api/v1/spaces/status")
            .header("cookie", "better-auth.session_token=session-1");
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        if zdr_header {
            request = request.header("x-zdr", "true");
        }
        let response = crate::build_router(fx.state.clone())
            .oneshot(
                request
                    .body(match &body {
                        Some(value) => Body::from(value.to_string()),
                        None => Body::empty(),
                    })
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let payload: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, payload)
    }

    /// Bodies of the Convex calls to one function.
    async fn convex_calls(fx: &Fx, function: &str) -> Vec<Value> {
        fx.application
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
            .filter(|body| body["path"] == function)
            .collect()
    }

    async fn convex_request_count(fx: &Fx) -> usize {
        fx.application.received_requests().await.unwrap().len()
    }

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User".to_owned(),
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

    fn roster() -> Value {
        json!({"data": {"members": [
            {"subject_type": "user", "subject_id": "u-1", "role": "owner"},
            {"subject_type": "user", "subject_id": "u-2", "role": "viewer"},
            {"subject_type": "user", "subject_id": "u-1", "role": "owner"},
            {"subject_type": "agent", "subject_id": "agent-1", "role": "editor"},
            {"subject_type": "user", "role": "viewer"},
        ]}})
    }

    // ---- the flag --------------------------------------------------------------------------------

    #[tokio::test]
    async fn flag_off_is_404_space_status_disabled_and_the_annotations_leave_the_answers_alone() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fx(Some(false)).await;
        fx.state.space_features.status_api = false;
        let (status, body) = call(&fx, "GET", None, false).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "space_status_disabled");
        let (status, body) = call(&fx, "PUT", Some(json!({"state": "away"})), false).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "space_status_disabled");

        let mut members = roster();
        annotate_roster(&fx.state, &user(), "org-1", StatusCode::OK, &mut members).await;
        assert_eq!(members, roster(), "flag off leaves the roster unannotated");
        let mut agents = vec![json!({"subject_id": "agent-1"})];
        annotate_agents(&fx.state, &user(), "org-1", "space-room", &mut agents).await;
        assert_eq!(agents, vec![json!({"subject_id": "agent-1"})]);
        assert_eq!(convex_request_count(&fx).await, 0, "no Application function is reached");
    }

    // ---- PUT -------------------------------------------------------------------------------------

    #[tokio::test]
    async fn put_status_validates_state_and_maps_invalid_status() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false)).await;
        mount_convex(
            &fx,
            "/api/mutation",
            "spaceMemberStatus:setStatusForGateway",
            value(json!({"state": "away", "text": null, "expiresAt": 1_900_000_000_000_i64})),
        )
        .await;
        let (status, body) = call(
            &fx,
            "PUT",
            Some(json!({"state": "away", "expires_at": 1_900_000_000_000_i64})),
            false,
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["data"],
            json!({"state": "away", "text": null, "expires_at": 1_900_000_000_000_i64})
        );
        let writes = convex_calls(&fx, "spaceMemberStatus:setStatusForGateway").await;
        assert_eq!(writes[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(writes[0]["args"]["externalOrgId"], "org-1");
        assert_eq!(writes[0]["args"]["state"], "away");
        assert_eq!(writes[0]["args"]["expiresAt"], 1_900_000_000_000_i64);
        assert!(writes[0]["args"].get("text").is_none(), "no text was sent, so none is forwarded");

        for (body, code) in [
            (json!({"state": "busy"}), "invalid_status"),
            (json!({"state": ""}), "invalid_status"),
            (json!({"state": "away", "org_id": "forged"}), "invalid_request"),
            (json!({"state": "away", "user_id": "forged"}), "invalid_request"),
            (json!({}), "invalid_request"),
            (json!({"state": 3}), "invalid_request"),
            (json!({"state": "away", "expires_at": "tomorrow"}), "invalid_request"),
            (json!("away"), "invalid_request"),
        ] {
            let (status, got) = call(&fx, "PUT", Some(body.clone()), false).await;
            assert_eq!(status, 400, "{body}");
            assert_eq!(got["error"]["code"], code, "{body}");
        }
        assert_eq!(
            convex_calls(&fx, "spaceMemberStatus:setStatusForGateway").await.len(),
            1,
            "no refused write reached Application"
        );

        // Application's own bound on the text or the expiry is the same typed 400; anything else is
        // a 503 that says nothing changed and carries none of Application's words.
        for (message, want_status, want_code) in [
            ("Uncaught Error: invalid_status", 400, "invalid_status"),
            ("Uncaught Error: internal stack trace", 503, "status_unavailable"),
        ] {
            let failing = self::fx(Some(false)).await;
            mount_convex(
                &failing,
                "/api/mutation",
                "spaceMemberStatus:setStatusForGateway",
                convex_error(message),
            )
            .await;
            let (status, got) = call(&failing, "PUT", Some(json!({"state": "dnd"})), false).await;
            assert_eq!(status, want_status, "{message}");
            assert_eq!(got["error"]["code"], want_code, "{message}");
            assert!(!got.to_string().contains("stack trace"));
        }
    }

    #[tokio::test]
    async fn follow_and_status_text_refused_under_zdr_with_typed_412() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let write = "spaceMemberStatus:setStatusForGateway";

        // A Zero Data Retention organization: the text is a durable write of content, refused
        // typed before any Application call; a state-only write carries no content and goes on.
        let zdr = fx(Some(true)).await;
        mount_convex(&zdr, "/api/mutation", write, value(json!({"state": "dnd", "text": null, "expiresAt": null}))).await;
        let (status, body) = call(&zdr, "PUT", Some(json!({"state": "away", "text": "In a meeting"})), false).await;
        assert_eq!(status, 412, "{body}");
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert!(convex_calls(&zdr, write).await.is_empty());
        let (status, body) = call(&zdr, "PUT", Some(json!({"state": "dnd"})), false).await;
        assert_eq!(status, 200, "{body}");
        let (status, _) = call(&zdr, "PUT", Some(json!({"state": "dnd", "text": "   "})), false).await;
        assert_eq!(status, 200, "a blank text is no content");
        assert_eq!(convex_calls(&zdr, write).await.len(), 2);

        // An org-core outage is an unknown posture: text fails closed in its own words.
        let unknown = fx(None).await;
        mount_convex(&unknown, "/api/mutation", write, value(json!({"state": "away"}))).await;
        let (status, body) =
            call(&unknown, "PUT", Some(json!({"state": "away", "text": "Lunch"})), false).await;
        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"]["code"], "zdr_decision_unavailable");
        assert!(convex_calls(&unknown, write).await.is_empty());

        // A request-level ZDR header alone refuses the text, even for an org that is not ZDR.
        let open = fx(Some(false)).await;
        mount_convex(
            &open,
            "/api/mutation",
            write,
            value(json!({"state": "away", "text": "Lunch", "expiresAt": null})),
        )
        .await;
        let (status, body) =
            call(&open, "PUT", Some(json!({"state": "away", "text": "Lunch"})), true).await;
        assert_eq!(status, 412, "{body}");
        assert!(convex_calls(&open, write).await.is_empty());

        // A positively non-ZDR org reaches Application with the text.
        let (status, body) =
            call(&open, "PUT", Some(json!({"state": "away", "text": "Lunch"})), false).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["text"], "Lunch");
        assert_eq!(convex_calls(&open, write).await[0]["args"]["text"], "Lunch");
    }

    // ---- GET -------------------------------------------------------------------------------------

    #[tokio::test]
    async fn get_status_includes_text_only_when_org_not_zdr() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let read = "spaceMemberStatus:getOwnStatusForGateway";
        // (org-core answer, includeText Application is asked for): an unknown posture fails closed.
        for (zdr, include_text) in [(Some(false), true), (Some(true), false), (None, false)] {
            let fx = fx(zdr).await;
            mount_convex(
                &fx,
                "/api/query",
                read,
                value(json!({"state": "away", "text": "brb", "expiresAt": 1_900_000_000_000_i64})),
            )
            .await;
            let (status, body) = call(&fx, "GET", None, false).await;
            assert_eq!(status, 200, "{zdr:?}: {body}");
            assert_eq!(
                body["data"],
                json!({"state": "away", "text": "brb", "expires_at": 1_900_000_000_000_i64})
            );
            // The menu's text field follows this: only a positive "not ZDR" offers it.
            assert_eq!(body["meta"]["text_allowed"], include_text, "{zdr:?}");
            let asked = convex_calls(&fx, read).await;
            assert_eq!(asked[0]["args"]["includeText"], include_text, "{zdr:?}");
            assert_eq!(asked[0]["args"]["externalAuthId"], "user-1");
            assert_eq!(asked[0]["args"]["externalOrgId"], "org-1");
        }

        // No row, or an expired one, reads as cleared.
        let fx = fx(Some(false)).await;
        mount_convex(&fx, "/api/query", read, value(Value::Null)).await;
        let (status, body) = call(&fx, "GET", None, false).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"], json!({"state": null, "text": null, "expires_at": null}));
        assert_eq!(body["meta"], json!({"text_allowed": true}));

        // Application failing is a 503 that says so, never a cleared status.
        let down = self::fx(Some(false)).await;
        mount_convex(&down, "/api/query", read, ResponseTemplate::new(500)).await;
        let (status, body) = call(&down, "GET", None, false).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "status_unavailable");
    }

    // ---- the roster annotation -------------------------------------------------------------------

    #[tokio::test]
    async fn roster_annotation_adds_status_to_user_members_only() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let lookup = "spaceMemberStatus:statusesForGateway";
        let fx = fx(Some(false)).await;
        mount_convex(
            &fx,
            "/api/query",
            lookup,
            value(json!([
                {"externalAuthId": "u-1", "state": "away", "text": "brb", "expiresAt": 1_900_000_000_000_i64}
            ])),
        )
        .await;
        let mut body = roster();
        annotate_roster(&fx.state, &user(), "org-1", StatusCode::OK, &mut body).await;
        let members = body["data"]["members"].as_array().unwrap();
        let away = json!({"state": "away", "text": "brb", "expires_at": 1_900_000_000_000_i64});
        assert_eq!(members[0]["status"], away);
        assert!(members[1]["status"].is_null(), "a member with no row reads as null, not as absent");
        assert!(members[1].as_object().unwrap().contains_key("status"));
        assert_eq!(members[2]["status"], away, "a repeated id is annotated too");
        assert!(
            members[3].get("status").is_none(),
            "a service member is never annotated or asked about"
        );
        assert!(members[4].get("status").is_none(), "a user row with no id is left alone");
        let asked = convex_calls(&fx, lookup).await;
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0]["args"]["subjectIds"], json!(["u-1", "u-2"]));
        assert_eq!(asked[0]["args"]["includeText"], true);
        assert_eq!(asked[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(asked[0]["args"]["externalOrgId"], "org-1");

        // A Zero Data Retention org, or an unknown posture, never asks for the text.
        for zdr in [Some(true), None] {
            let fx = self::fx(zdr).await;
            mount_convex(&fx, "/api/query", lookup, value(json!([]))).await;
            let mut body = roster();
            annotate_roster(&fx.state, &user(), "org-1", StatusCode::OK, &mut body).await;
            assert_eq!(convex_calls(&fx, lookup).await[0]["args"]["includeText"], false, "{zdr:?}");
        }
    }

    #[tokio::test]
    async fn a_failed_status_lookup_leaves_the_field_out_and_other_inputs_are_left_alone() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let lookup = "spaceMemberStatus:statusesForGateway";
        let down = fx(Some(false)).await;
        mount_convex(&down, "/api/query", lookup, ResponseTemplate::new(500)).await;
        let mut body = roster();
        annotate_roster(&down.state, &user(), "org-1", StatusCode::OK, &mut body).await;
        assert_eq!(body, roster(), "could not check is never drawn as checked and none");

        let quiet = fx(Some(false)).await;
        mount_convex(&quiet, "/api/query", lookup, value(json!([]))).await;
        // A refused roster, a blank org and a body with no members are each left untouched, with
        // no lookup.
        let mut refused = roster();
        annotate_roster(&quiet.state, &user(), "org-1", StatusCode::FORBIDDEN, &mut refused).await;
        assert_eq!(refused, roster());
        let mut blank_org = roster();
        annotate_roster(&quiet.state, &user(), "  ", StatusCode::OK, &mut blank_org).await;
        assert_eq!(blank_org, roster());
        let mut no_members = json!({"data": {"count": 0}});
        annotate_roster(&quiet.state, &user(), "org-1", StatusCode::OK, &mut no_members).await;
        assert_eq!(no_members, json!({"data": {"count": 0}}));
        let mut only_agents = json!({"data": {"members": [{"subject_type": "agent", "subject_id": "a-1"}]}});
        annotate_roster(&quiet.state, &user(), "org-1", StatusCode::OK, &mut only_agents).await;
        assert_eq!(only_agents, json!({"data": {"members": [{"subject_type": "agent", "subject_id": "a-1"}]}}));
        assert_eq!(convex_request_count(&quiet).await, 0);
    }

    // ---- the agent annotation --------------------------------------------------------------------

    #[tokio::test]
    async fn agents_annotation_adds_activity_a_missing_id_reads_idle_and_a_failed_lookup_leaves_it_out() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let lookup = "spaceMemberStatus:agentActivityForGateway";
        let fx = fx(Some(false)).await;
        mount_convex(
            &fx,
            "/api/query",
            lookup,
            value(json!([{"agentId": "agent-1", "state": "working"}, {"agentId": "agent-3", "state": "waiting"}])),
        )
        .await;
        let mut agents = vec![
            json!({"subject_id": "agent-1", "name": "One"}),
            json!({"subject_id": "agent-2", "name": "Two"}),
            json!({"subject_id": "agent-1", "name": "One again"}),
            json!({"name": "no id"}),
        ];
        annotate_agents(&fx.state, &user(), "org-1", "space-room", &mut agents).await;
        assert_eq!(agents[0]["activity"], "working");
        assert_eq!(agents[1]["activity"], "idle", "an agent Application has no row for is idle");
        assert_eq!(agents[2]["activity"], "working");
        assert!(agents[3].get("activity").is_none(), "an agent with no id is left alone");
        assert_eq!(agents[0]["name"], "One", "the rest of the agent is untouched");
        let asked = convex_calls(&fx, lookup).await;
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0]["args"]["agentIds"], json!(["agent-1", "agent-2"]));
        assert_eq!(asked[0]["args"]["spaceRef"], "space-room");
        assert_eq!(asked[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(asked[0]["args"]["externalOrgId"], "org-1");

        // A failed lookup leaves the field out entirely: "unknown" is never drawn as "idle".
        let down = self::fx(Some(false)).await;
        mount_convex(&down, "/api/query", lookup, ResponseTemplate::new(500)).await;
        let mut agents = vec![json!({"subject_id": "agent-1"})];
        annotate_agents(&down.state, &user(), "org-1", "space-room", &mut agents).await;
        assert_eq!(agents, vec![json!({"subject_id": "agent-1"})]);

        // A blank org and an empty list ask nothing.
        let quiet = self::fx(Some(false)).await;
        mount_convex(&quiet, "/api/query", lookup, value(json!([]))).await;
        let mut agents = vec![json!({"subject_id": "agent-1"})];
        annotate_agents(&quiet.state, &user(), " ", "space-room", &mut agents).await;
        assert_eq!(agents, vec![json!({"subject_id": "agent-1"})]);
        let mut none: Vec<Value> = Vec::new();
        annotate_agents(&quiet.state, &user(), "org-1", "space-room", &mut none).await;
        assert!(none.is_empty());
        assert_eq!(convex_request_count(&quiet).await, 0);
    }

    // ---- DELETE (the clear route) ----------------------------------------------------------------

    #[tokio::test]
    async fn delete_status_clears_the_own_row_under_zdr_too_and_reads_the_cleared_shape() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // A Zero Data Retention organization: clearing keeps no content, so it is still allowed.
        let fx = fx(Some(true)).await;
        let clear = "spaceMemberStatus:clearStatusForGateway";
        mount_convex(&fx, "/api/mutation", clear, value(json!({"cleared": true}))).await;
        let (status, body) = call(&fx, "DELETE", None, false).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body,
            json!({"data": {"state": null, "text": null, "expires_at": null}})
        );
        let calls = convex_calls(&fx, clear).await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(calls[0]["args"]["externalOrgId"], "org-1");
        assert!(
            calls[0]["args"].get("text").is_none(),
            "a clear sends no text"
        );
    }

    #[tokio::test]
    async fn delete_status_is_idempotent_when_there_is_nothing_to_clear() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false)).await;
        let clear = "spaceMemberStatus:clearStatusForGateway";
        mount_convex(
            &fx,
            "/api/mutation",
            clear,
            value(json!({"cleared": false})),
        )
        .await;
        let (status, body) = call(&fx, "DELETE", None, false).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["state"], Value::Null);
        assert_eq!(convex_calls(&fx, clear).await.len(), 1);
    }

    #[tokio::test]
    async fn delete_status_is_404_when_the_flag_is_off_and_reaches_no_application_function() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fx(Some(false)).await;
        fx.state.space_features.status_api = false;
        let (status, body) = call(&fx, "DELETE", None, false).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "space_status_disabled");
        assert_eq!(convex_request_count(&fx).await, 0);
    }

    #[tokio::test]
    async fn delete_status_is_503_when_application_fails_and_never_a_cleared_answer() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false)).await;
        mount_convex(
            &fx,
            "/api/mutation",
            "spaceMemberStatus:clearStatusForGateway",
            convex_error("temporarily unavailable"),
        )
        .await;
        let (status, body) = call(&fx, "DELETE", None, false).await;
        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"]["code"], "status_unavailable");
        assert!(
            body.get("data").is_none(),
            "a failed clear must not read as cleared"
        );
    }
}
