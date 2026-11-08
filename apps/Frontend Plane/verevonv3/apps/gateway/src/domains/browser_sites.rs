//! WS21-S6: the Frontend Plane's door to the agent browser's site policy (WS21-S3, DEC-19).
//!
//! org-core stores, validates, resolves and explains the policy. The browser broker pulls the
//! resolved answer when it mints a grant, and Quarry stays the final enforcer. This module is only
//! the BFF's door to org-core: an owner or admin reads and replaces the organisation's policy, and
//! any member reads, sets or forgets their own choice for one host. It decides nothing about who
//! may browse, and it forwards no host that a client did not name in the path or the query.
//!
//! The routes exist when `BROWSER_DO_ENABLED` or `BROWSER_SITES_ENABLED` is the literal `true`, read when the
//! browser router is built. Off, they are absent, so an operator cannot set a policy that no run
//! enforces yet. org-core still answers every semantic rule (canonical hosts, the choice limit,
//! revisions); the checks here only keep malformed or unknown fields from reaching it.

use std::collections::HashMap;

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    routing::{get, put},
    Json, Router,
};
use reqwest::Method;
use serde_json::{Map, Value};

use crate::{
    config::AppState, contracts::ActionActor, envelope::error, middleware::AuthenticatedUser,
    upstream::proxy_json,
};

/// The most a browser-site body may weigh, as the model policy's bodies do.
const MAX_BODY_BYTES: usize = 64 << 10;
/// Mirrors org-core's `browsersites.MaxRules`.
const MAX_RULES: usize = 500;
/// Mirrors org-core's `browsersites.MaxNoteLength`.
const MAX_NOTE_CHARS: usize = 200;
/// The DNS limit for a whole hostname.
const MAX_HOST_CHARS: usize = 253;

const POLICY_MODES: [&str; 2] = ["open", "allowlist"];
const RULE_KINDS: [&str; 3] = ["block", "read", "act"];
const CHOICE_LEVELS: [&str; 3] = ["none", "read", "act"];

/// The only fields each body may carry. An unknown field is refused rather than forwarded: a typo
/// such as `levl` would otherwise be dropped by org-core and read by the member as "no choice".
const POLICY_FIELDS: [&str; 3] = ["expected_revision", "mode", "rules"];
const RULE_FIELDS: [&str; 3] = ["host", "rule", "note"];
const CHOICE_FIELDS: [&str; 3] = ["expected_revision", "level", "expires_at"];

type GatewayJsonResponse = (StatusCode, Json<Value>);

/// The browser-site routes, or none at all when the switch is off. They are merged into the browser
/// router, which applies the session layer to them.
pub(crate) fn routes(enabled: bool) -> Router<AppState> {
    if !enabled {
        return Router::new();
    }
    Router::new()
        .route(
            "/api/v1/orgs/{id}/browser-sites",
            get(get_org_policy).put(put_org_policy),
        )
        .route("/api/v1/orgs/{id}/browser-sites/explain", get(explain_site))
        .route("/api/v1/orgs/{id}/browser-sites/me", get(list_my_choices))
        .route(
            "/api/v1/orgs/{id}/browser-sites/me/{host}",
            put(put_my_choice).delete(delete_my_choice),
        )
}

/// Browser-site settings can be enabled for the assistant without enabling chat browser tools.
/// Both switches are exact: only the literal `true` enables the routes.
pub(crate) fn enabled_from_env() -> bool {
    browser_sites_enabled(
        std::env::var("BROWSER_DO_ENABLED").ok().as_deref(),
        std::env::var("BROWSER_SITES_ENABLED").ok().as_deref(),
    )
}

fn browser_sites_enabled(browser_do: Option<&str>, browser_sites: Option<&str>) -> bool {
    switch_enabled(browser_do) || switch_enabled(browser_sites)
}

fn switch_enabled(value: Option<&str>) -> bool {
    value == Some("true")
}

/// GET /api/v1/orgs/:id/browser-sites: the organisation's policy, for the admin settings view.
async fn get_org_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&user, &id) {
        return response;
    }
    proxy_json(
        &state,
        Method::GET,
        &org_core_url(&state, &id, ""),
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// PUT /api/v1/orgs/:id/browser-sites: replace the whole policy. The body carries the revision it
/// was edited from; org-core answers 409 `revision_conflict` to a stale editor and that is passed
/// through, so no edit is ever silently overwritten.
async fn put_org_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&user, &id) {
        return response;
    }
    if let Some(refusal) = refuse_body(&body, validate_policy_body) {
        return refusal;
    }
    proxy_json(
        &state,
        Method::PUT,
        &org_core_url(&state, &id, ""),
        Some(body),
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// GET /api/v1/orgs/:id/browser-sites/explain?host=: why one site is or is not open to the caller.
/// The answer names the admin's rule and the caller's own choice, and nothing about other members.
async fn explain_site(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> GatewayJsonResponse {
    let host = query.get("host").map_or("", |host| host.trim());
    explain_site_for_member(&state, &user, &id, host).await
}

pub(super) async fn explain_site_for_member(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    host: &str,
) -> GatewayJsonResponse {
    if let Err(response) = require_active_org(user, org_id) {
        return response;
    }
    if !valid_host(host) {
        return invalid("host is required and must be a hostname");
    }
    let suffix = format!("/explain?host={}", urlencoding::encode(host));
    proxy_json(
        state,
        Method::GET,
        &org_core_url(state, org_id, &suffix),
        None,
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await
}

/// GET /api/v1/orgs/:id/browser-sites/me: the caller's own choices and the policy revision they
/// were read against.
async fn list_my_choices(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_active_org(&user, &id) {
        return response;
    }
    proxy_json(
        &state,
        Method::GET,
        &org_core_url(&state, &id, "/me"),
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// PUT /api/v1/orgs/:id/browser-sites/me/:host: the caller records their own choice for one host.
/// The host comes from the path alone, and the choice must name the revision the caller read.
async fn put_my_choice(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, host)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_active_org(&user, &id) {
        return response;
    }
    let host = host.trim();
    if !valid_host(host) {
        return invalid("host must be a hostname");
    }
    if let Some(refusal) = refuse_body(&body, validate_choice_body) {
        return refusal;
    }
    let suffix = format!("/me/{}", urlencoding::encode(host));
    proxy_json(
        &state,
        Method::PUT,
        &org_core_url(&state, &id, &suffix),
        Some(body),
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// DELETE /api/v1/orgs/:id/browser-sites/me/:host: forgets the caller's own choice for one host, so
/// the next use asks again.
async fn delete_my_choice(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, host)): Path<(String, String)>,
) -> GatewayJsonResponse {
    if let Err(response) = require_active_org(&user, &id) {
        return response;
    }
    let host = host.trim();
    if !valid_host(host) {
        return invalid("host must be a hostname");
    }
    let suffix = format!("/me/{}", urlencoding::encode(host));
    proxy_json(
        &state,
        Method::DELETE,
        &org_core_url(&state, &id, &suffix),
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

fn org_core_url(state: &AppState, org_id: &str, suffix: &str) -> String {
    format!(
        "{}/api/v1/organizations/{}/browser-sites{suffix}",
        state.org_core_url,
        urlencoding::encode(org_id)
    )
}

/// The caller must hold the organisation's live membership. The forwarded role is only a hint, so
/// this checks the membership the session middleware resolved for the active organisation.
fn require_active_org(
    user: &AuthenticatedUser,
    requested_org_id: &str,
) -> Result<(), GatewayJsonResponse> {
    match user.authorized_membership.as_ref() {
        Some(membership) if membership.organization_id == requested_org_id.trim() => Ok(()),
        _ => Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "The requested organization is not authorized for this session.",
            )),
        )),
    }
}

/// The policy is the organisation's, so changing it needs an owner or an admin. This is the same
/// rule as `domains::orgs::shared::require_org_admin`, which is private to that module.
fn require_org_admin(user: &AuthenticatedUser, org_id: &str) -> Result<(), GatewayJsonResponse> {
    require_active_org(user, org_id)?;
    let is_admin = user.authorized_membership.as_ref().is_some_and(|membership| {
        membership
            .role
            .split(',')
            .map(str::trim)
            .any(|role| role.eq_ignore_ascii_case("owner") || role.eq_ignore_ascii_case("admin"))
    });
    if is_admin {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Organization administration requires an owner or admin role.",
            )),
        ))
    }
}

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn invalid(message: &str) -> GatewayJsonResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(error("validation_error", message)),
    )
}

/// The size limit and the structural check of one body, or the refusal to forward it.
fn refuse_body(body: &Value, check: fn(&Value) -> Result<(), String>) -> Option<GatewayJsonResponse> {
    if body.to_string().len() > MAX_BODY_BYTES {
        return Some((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(error(
                "body_too_large",
                "The browser site body is larger than 64 KB.",
            )),
        ));
    }
    check(body).err().map(|message| invalid(&message))
}

/// A hostname as a client may name it: ASCII letters, digits, dots and hyphens, within the DNS
/// length limit. org-core canonicalises case and the trailing dot and refuses what it cannot
/// canonicalise. This check only keeps path and query text that is not a host from reaching it.
fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= MAX_HOST_CHARS
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
}

/// Shape of an admin's PUT body: the fields, the revision it was read at, and each rule.
fn validate_policy_body(body: &Value) -> Result<(), String> {
    let fields = object(body, "The request body must be a JSON object.")?;
    reject_unknown_fields("the request body", fields, &POLICY_FIELDS)?;
    require_revision(fields)?;
    match fields.get("mode").and_then(Value::as_str) {
        Some(mode) if POLICY_MODES.contains(&mode) => {}
        _ => return Err("mode must be open or allowlist.".to_owned()),
    }
    let Some(rules) = fields.get("rules").and_then(Value::as_array) else {
        return Err("rules is required and must be an array.".to_owned());
    };
    if rules.len() > MAX_RULES {
        return Err(format!("At most {MAX_RULES} site rules are allowed."));
    }
    rules.iter().try_for_each(validate_rule)
}

fn validate_rule(rule: &Value) -> Result<(), String> {
    let fields = object(rule, "Each site rule must be a JSON object.")?;
    reject_unknown_fields("a site rule", fields, &RULE_FIELDS)?;
    let host = fields.get("host").and_then(Value::as_str).unwrap_or("");
    if !valid_host(host) {
        return Err("Each site rule needs a hostname.".to_owned());
    }
    match fields.get("rule").and_then(Value::as_str) {
        Some(kind) if RULE_KINDS.contains(&kind) => {}
        _ => return Err("A site rule must be block, read or act.".to_owned()),
    }
    match fields.get("note") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(note)) if note.chars().count() <= MAX_NOTE_CHARS => Ok(()),
        Some(_) => Err(format!(
            "A site note is text of at most {MAX_NOTE_CHARS} characters."
        )),
    }
}

/// Shape of a member's PUT body: the revision the choice was made against and the level.
fn validate_choice_body(body: &Value) -> Result<(), String> {
    let fields = object(body, "The request body must be a JSON object.")?;
    reject_unknown_fields("the request body", fields, &CHOICE_FIELDS)?;
    require_revision(fields)?;
    match fields.get("level").and_then(Value::as_str) {
        Some(level) if CHOICE_LEVELS.contains(&level) => {}
        _ => return Err("level must be none, read or act.".to_owned()),
    }
    match fields.get("expires_at") {
        None | Some(Value::Null) | Some(Value::String(_)) => Ok(()),
        Some(_) => Err("expires_at must be a time or null.".to_owned()),
    }
}

fn object<'a>(value: &'a Value, message: &str) -> Result<&'a Map<String, Value>, String> {
    value.as_object().ok_or_else(|| message.to_owned())
}

fn require_revision(fields: &Map<String, Value>) -> Result<(), String> {
    match fields.get("expected_revision").and_then(Value::as_i64) {
        Some(revision) if revision >= 0 => Ok(()),
        _ => Err(
            "expected_revision is required and must be an integer, zero or greater (0 for the first write)."
                .to_owned(),
        ),
    }
}

fn reject_unknown_fields(
    path: &str,
    fields: &Map<String, Value>,
    allowed: &[&str],
) -> Result<(), String> {
    match fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(unknown) => Err(format!("Unknown field '{unknown}' in {path}.")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request, Router};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{
        body_json as wm_body_json, header as wm_header, method as wm_method, path as wm_path,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::config::AppState;
    use crate::middleware::require_session;

    use super::{
        browser_sites_enabled, routes, switch_enabled, valid_host, validate_choice_body, validate_policy_body,
    };

    const COOKIE: &str = "better-auth.session_token=session-1";
    const ORG_CORE_SITES: &str = "/api/v1/organizations/org-1/browser-sites";

    /// Auth, user-core and org-core stand-ins for one signed-in user of org-1 with session `role`.
    struct Upstreams {
        auth: MockServer,
        user_core: MockServer,
        org_core: MockServer,
    }

    impl Upstreams {
        async fn start(role: &str) -> Self {
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
                    "userId": "user-1", "orgId": "org-1", "role": role,
                    "onboardingStatus": "COMPLETED"
                })))
                .mount(&user_core)
                .await;
            Self {
                auth,
                user_core,
                org_core: MockServer::start().await,
            }
        }

        fn state(&self) -> AppState {
            let mut state = crate::tests::test_state(false);
            state.auth_core_url = self.auth.uri();
            state.user_core_url = self.user_core.uri();
            state.org_core_url = self.org_core.uri();
            state
        }

        /// org-core answers only a request that carries the gateway's delegated identity for this
        /// organisation and user, on the given path, and the exact body when one is given.
        async fn org_core(
            &self,
            method: &str,
            path: &str,
            body: Option<Value>,
            response: ResponseTemplate,
        ) {
            let mut mock = Mock::given(wm_method(method))
                .and(wm_path(path))
                .and(wm_header("x-service-id", "verevon-gateway"))
                .and(wm_header("x-delegation-version", "v3"))
                .and(wm_header("x-org-id", "org-1"))
                .and(wm_header("x-user-id", "user-1"));
            if let Some(body) = body {
                mock = mock.and(wm_body_json(body));
            }
            mock.respond_with(response).mount(&self.org_core).await;
        }

        async fn org_core_requests(&self) -> usize {
            self.org_core
                .received_requests()
                .await
                .expect("recording")
                .len()
        }
    }

    fn app(state: AppState) -> Router {
        routes(true)
            .route_layer(axum::middleware::from_fn_with_state(
                state.clone(),
                require_session,
            ))
            .with_state(state)
    }

    async fn send(state: AppState, method: &str, uri: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", COOKIE);
        let body = match body {
            Some(body) => {
                request = request.header("content-type", "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = app(state)
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let parsed: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, parsed)
    }

    fn policy_body() -> Value {
        json!({
            "expected_revision": 3,
            "mode": "allowlist",
            "rules": [{"host": "example.com", "rule": "act", "note": "ordering portal"}]
        })
    }

    #[tokio::test]
    async fn a_member_cannot_read_or_write_the_organisation_policy() {
        let upstreams = Upstreams::start("member").await;
        let (get_status, _) = send(
            upstreams.state(),
            "GET",
            "/api/v1/orgs/org-1/browser-sites",
            None,
        )
        .await;
        let (put_status, _) = send(
            upstreams.state(),
            "PUT",
            "/api/v1/orgs/org-1/browser-sites",
            Some(policy_body()),
        )
        .await;
        assert_eq!(get_status, 403);
        assert_eq!(put_status, 403);
        assert_eq!(upstreams.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn an_admin_reads_the_policy_through_org_core() {
        let upstreams = Upstreams::start("admin").await;
        upstreams
            .org_core(
                "GET",
                ORG_CORE_SITES,
                None,
                ResponseTemplate::new(200)
                    .set_body_json(json!({"organization_id": "org-1", "revision": 3})),
            )
            .await;
        let (status, body) = send(
            upstreams.state(),
            "GET",
            "/api/v1/orgs/org-1/browser-sites",
            None,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["revision"], 3);
    }

    #[tokio::test]
    async fn a_policy_body_with_an_unknown_field_never_reaches_org_core() {
        let upstreams = Upstreams::start("owner").await;
        let mut body = policy_body();
        body["modee"] = json!("open");
        let (status, _) = send(
            upstreams.state(),
            "PUT",
            "/api/v1/orgs/org-1/browser-sites",
            Some(body),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(upstreams.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn a_policy_write_is_forwarded_and_a_stale_revision_is_passed_through() {
        let upstreams = Upstreams::start("admin").await;
        upstreams
            .org_core(
                "PUT",
                ORG_CORE_SITES,
                Some(policy_body()),
                ResponseTemplate::new(409).set_body_json(json!({
                    "code": "revision_conflict", "current_revision": 4
                })),
            )
            .await;
        let (status, body) = send(
            upstreams.state(),
            "PUT",
            "/api/v1/orgs/org-1/browser-sites",
            Some(policy_body()),
        )
        .await;
        assert_eq!(status, 409);
        assert_eq!(body["code"], "revision_conflict");
    }

    #[tokio::test]
    async fn a_member_reads_their_own_choices_and_never_the_policy() {
        let upstreams = Upstreams::start("member").await;
        upstreams
            .org_core(
                "GET",
                &format!("{ORG_CORE_SITES}/me"),
                None,
                ResponseTemplate::new(200).set_body_json(json!({"revision": 3, "prefs": []})),
            )
            .await;
        let (status, body) = send(
            upstreams.state(),
            "GET",
            "/api/v1/orgs/org-1/browser-sites/me",
            None,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["revision"], 3);
    }

    #[tokio::test]
    async fn a_choice_is_written_for_the_host_in_the_path_and_nothing_else() {
        let upstreams = Upstreams::start("member").await;
        let choice = json!({"expected_revision": 3, "level": "read"});
        upstreams
            .org_core(
                "PUT",
                &format!("{ORG_CORE_SITES}/me/example.com"),
                Some(choice.clone()),
                ResponseTemplate::new(200)
                    .set_body_json(json!({"host": "example.com", "level": "read"})),
            )
            .await;
        let (status, body) = send(
            upstreams.state(),
            "PUT",
            "/api/v1/orgs/org-1/browser-sites/me/example.com",
            Some(choice),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["level"], "read");
    }

    #[tokio::test]
    async fn a_choice_with_an_unknown_level_or_an_odd_host_never_reaches_org_core() {
        let upstreams = Upstreams::start("member").await;
        let (level_status, _) = send(
            upstreams.state(),
            "PUT",
            "/api/v1/orgs/org-1/browser-sites/me/example.com",
            Some(json!({"expected_revision": 3, "level": "all"})),
        )
        .await;
        let (host_status, _) = send(
            upstreams.state(),
            "PUT",
            "/api/v1/orgs/org-1/browser-sites/me/exa_mple.com",
            Some(json!({"expected_revision": 3, "level": "read"})),
        )
        .await;
        assert_eq!(level_status, 400);
        assert_eq!(host_status, 400);
        assert_eq!(upstreams.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn forgetting_a_choice_forwards_a_delete_for_that_host() {
        let upstreams = Upstreams::start("member").await;
        upstreams
            .org_core(
                "DELETE",
                &format!("{ORG_CORE_SITES}/me/example.com"),
                None,
                ResponseTemplate::new(204),
            )
            .await;
        let (status, _) = send(
            upstreams.state(),
            "DELETE",
            "/api/v1/orgs/org-1/browser-sites/me/example.com",
            None,
        )
        .await;
        assert_eq!(status, 204);
    }

    #[tokio::test]
    async fn explain_needs_a_host_and_forwards_it_encoded() {
        let upstreams = Upstreams::start("member").await;
        upstreams
            .org_core(
                "GET",
                &format!("{ORG_CORE_SITES}/explain"),
                None,
                ResponseTemplate::new(200).set_body_json(json!({"decision": {"effective": "ask"}})),
            )
            .await;
        let (missing, _) = send(
            upstreams.state(),
            "GET",
            "/api/v1/orgs/org-1/browser-sites/explain",
            None,
        )
        .await;
        let (status, body) = send(
            upstreams.state(),
            "GET",
            "/api/v1/orgs/org-1/browser-sites/explain?host=example.com",
            None,
        )
        .await;
        assert_eq!(missing, 400);
        assert_eq!(status, 200);
        assert_eq!(body["decision"]["effective"], "ask");
        let recorded = upstreams.org_core.received_requests().await.unwrap();
        assert!(recorded
            .iter()
            .any(|request| request.url.query() == Some("host=example.com")));
    }

    #[test]
    fn assistant_site_settings_do_not_require_chat_browser_tools() {
        assert!(browser_sites_enabled(None, Some("true")));
        assert!(browser_sites_enabled(Some("true"), None));
        assert!(!browser_sites_enabled(None, None));
        assert!(!browser_sites_enabled(Some("false"), Some("TRUE")));
    }

    #[test]
    fn the_switch_is_the_literal_true_and_nothing_else() {
        assert!(switch_enabled(Some("true")));
        assert!(!switch_enabled(Some("TRUE")));
        assert!(!switch_enabled(Some("1")));
        assert!(!switch_enabled(Some(" true")));
        assert!(!switch_enabled(None));
    }

    #[test]
    fn a_host_is_ascii_letters_digits_dots_and_hyphens_within_the_dns_limit() {
        assert!(valid_host("example.com"));
        assert!(valid_host("Shop-2.example.co.uk."));
        assert!(!valid_host(""));
        assert!(!valid_host("exa_mple.com"));
        assert!(!valid_host("example.com/path"));
        assert!(!valid_host("*.example.com"));
        assert!(!valid_host("bücher.example"));
        assert!(!valid_host(&"a".repeat(254)));
    }

    #[test]
    fn a_policy_body_is_checked_for_shape_before_it_is_forwarded() {
        assert!(validate_policy_body(&policy_body()).is_ok());
        assert!(validate_policy_body(&json!({"mode": "open", "rules": []})).is_err());
        assert!(validate_policy_body(&json!({"expected_revision": -1, "mode": "open", "rules": []})).is_err());
        assert!(validate_policy_body(&json!({"expected_revision": 0, "mode": "closed", "rules": []})).is_err());
        assert!(validate_policy_body(&json!({"expected_revision": 0, "mode": "open"})).is_err());
        assert!(validate_policy_body(&json!({
            "expected_revision": 0, "mode": "open",
            "rules": [{"host": "example.com", "rule": "allow"}]
        }))
        .is_err());
        assert!(validate_policy_body(&json!({
            "expected_revision": 0, "mode": "open",
            "rules": [{"host": "example.com", "rule": "block", "note": "x".repeat(201)}]
        }))
        .is_err());
        let too_many: Vec<Value> = (0..501)
            .map(|index| json!({"host": format!("site{index}.example"), "rule": "read"}))
            .collect();
        assert!(validate_policy_body(&json!({
            "expected_revision": 0, "mode": "allowlist", "rules": too_many
        }))
        .is_err());
    }

    #[test]
    fn a_choice_body_names_a_known_level_and_an_optional_expiry() {
        assert!(validate_choice_body(&json!({"expected_revision": 0, "level": "none"})).is_ok());
        assert!(validate_choice_body(&json!({
            "expected_revision": 2, "level": "act", "expires_at": null
        }))
        .is_ok());
        assert!(validate_choice_body(&json!({"expected_revision": 2, "level": "root"})).is_err());
        assert!(validate_choice_body(&json!({"level": "read"})).is_err());
        assert!(validate_choice_body(&json!({
            "expected_revision": 2, "level": "read", "expires_at": 90
        }))
        .is_err());
        assert!(validate_choice_body(&json!({
            "expected_revision": 2, "level": "read", "remember": true
        }))
        .is_err());
    }
}
