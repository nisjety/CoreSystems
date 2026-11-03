//! Browser page context for a chat turn (E3.3-c): server-side re-resolution of
//! the composer's `@tab` mentions against Quarry's live tab list.
//!
//! The composer parses `@tab` and `@tab:<tabId>` from the draft and resolves them
//! against the tab list it holds, but that result is only a hint: the tab list in
//! the browser can be stale, and a caller can send any id. Before a mention may
//! reach a model, the gateway resolves it again here:
//!
//! 1. the body is checked (camelCase, unknown keys refused, at most eight
//!    mentions, tab id grammar), so no org or session field can ride along;
//! 2. the session is owner-gated by Quarry's own `browser-session` projection
//!    (a session the caller does not own answers as Quarry answers it);
//! 3. a zero-data-retention session is refused, and so is a projection that does
//!    not state `zdr: false` (page context is never attached on ZDR);
//! 4. Quarry's live tabs are read and every mention must resolve to exactly one
//!    tab. One unresolved mention fails the whole request (409 with a reason per
//!    mention): nothing is attached on a partial match.
//!
//! The answer is a list of refs. A ref carries no page content and no grant
//! (`grant: "none"`): a mention never opens, focuses or switches a tab, and never
//! lets the agent act. The tab title is untrusted page-derived text, so it is
//! cleaned of control characters, capped, and marked `labelSource: "page"` with
//! the trust class `browser-scraped`. The target is origin and path only.
//!
//! Off by default: the route is mounted only when `BROWSER_TAB_MENTIONS` is
//! `1|true|yes|on`, read once when the router is built (never in a handler).

use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, error_with_details, ok, unwrap_data},
    middleware::{require_session, AuthenticatedUser},
    rate_limit::{with_rate_class, RateClass},
};

use super::browser::{
    cookie_header, forward_quarry_failure, is_valid_path_segment, owner_browser_session_projection,
    quarry_call, quarry_token,
};

/// Route path of the re-resolution (E3.3-c).
pub(crate) const TAB_MENTIONS_PATH: &str =
    "/api/v1/browser/sessions/{session_id}/context/tab-mentions";
/// Environment switch for the route. Off unless set to 1, true, yes or on.
pub(crate) const TAB_MENTIONS_FLAG: &str = "BROWSER_TAB_MENTIONS";
/// Mentions one turn may carry.
const MENTIONS_MAX: usize = 8;
/// Longest tab label the browser gets back.
const TAB_LABEL_MAX_CHARS: usize = 200;
/// Wire name of `TrustClass::BrowserScraped` (mp-content-safety trust.rs).
const TAB_TRUST_CLASS: &str = "browser-scraped";

// ---------------------------------------------------------------------------
// Router and flag
// ---------------------------------------------------------------------------

fn tab_mentions_flag_on(value: Option<&str>) -> bool {
    matches!(
        value.map(|raw| raw.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

fn tab_mentions_enabled_from_env() -> bool {
    tab_mentions_flag_on(std::env::var(TAB_MENTIONS_FLAG).ok().as_deref())
}

/// The browser-context routes. Wired in `main.rs` next to `browser_evidence`.
#[allow(
    dead_code,
    reason = "main.rs merge line is added by the W23 integration step"
)]
pub(crate) fn router(state: AppState) -> Router<AppState> {
    router_with(state, tab_mentions_enabled_from_env())
}

fn router_with(state: AppState, tab_mentions: bool) -> Router<AppState> {
    // `route_layer` refuses a router without routes, so the off state is a bare
    // empty router (every path answers 404).
    if !tab_mentions {
        return Router::new();
    }
    let routes = Router::new().route(TAB_MENTIONS_PATH, post(resolve_session_tab_mentions));
    with_rate_class(routes, &state.rate_limiter, RateClass::Interactive)
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TabMentionsBody {
    mentions: Vec<TabMentionInput>,
}

/// One mention as the composer sends it (`browserContext.tabMentions[]`):
/// `tabId` null (or absent) means the single active tab (`@tab`), a string
/// means `@tab:<tabId>`. `token` is the text as typed; when present it must
/// spell the same mention, so a token and an id cannot disagree.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct TabMentionInput {
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) tab_id: Option<String>,
}

fn valid_tab_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// True when `token` spells the mention: `@tab` for the active tab, or
/// `@tab:<id>` for an id (the `@tab` part in any case, the id exactly).
fn token_matches(token: &str, tab_id: Option<&str>) -> bool {
    let Some(prefix) = token.get(..4) else {
        return false;
    };
    if !prefix.eq_ignore_ascii_case("@tab") {
        return false;
    }
    let rest = &token[4..];
    match tab_id {
        None => rest.is_empty(),
        Some(id) => rest.strip_prefix(':') == Some(id),
    }
}

/// The mentions with their ids trimmed, or why the body is refused.
fn checked_mentions(mentions: &[TabMentionInput]) -> Result<Vec<Option<String>>, &'static str> {
    if mentions.is_empty() {
        return Err("at least one mention is required");
    }
    if mentions.len() > MENTIONS_MAX {
        return Err("at most eight mentions may be resolved at once");
    }
    mentions
        .iter()
        .map(|mention| {
            let tab_id = match mention.tab_id.as_deref().map(str::trim) {
                None => None,
                Some(id) if valid_tab_id(id) => Some(id.to_owned()),
                Some(_) => return Err("tabId must be 1 to 64 letters, digits, '-' or '_'"),
            };
            if let Some(token) = mention.token.as_deref() {
                if !token_matches(token.trim(), tab_id.as_deref()) {
                    return Err("token must be @tab or @tab:<tabId> and match tabId");
                }
            }
            Ok(tab_id)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

fn tab_id_of(tab: &Value) -> Option<&str> {
    tab.get("tabId")
        .and_then(Value::as_str)
        .filter(|id| valid_tab_id(id))
}

/// Resolve one mention against the live tabs, failing closed. Tabs without a
/// usable id are ignored, so they can never match.
fn resolve_mention<'a>(tabs: &'a [Value], tab_id: Option<&str>) -> Result<&'a Value, &'static str> {
    let usable: Vec<&Value> = tabs.iter().filter(|tab| tab_id_of(tab).is_some()).collect();
    if usable.is_empty() {
        return Err("no-tabs");
    }
    let matches: Vec<&Value> = match tab_id {
        None => usable
            .into_iter()
            .filter(|tab| tab.get("active").and_then(Value::as_bool) == Some(true))
            .collect(),
        Some(id) => usable
            .into_iter()
            .filter(|tab| tab_id_of(tab) == Some(id))
            .collect(),
    };
    match matches.as_slice() {
        [tab] => Ok(*tab),
        [] if tab_id.is_none() => Err("no-active-tab"),
        [] => Err("unknown-tab"),
        _ => Err("ambiguous-tab"),
    }
}

/// Origin and path of an http(s) URL, with credentials, query and fragment dropped.
fn safe_tab_target(raw: Option<&str>) -> Option<String> {
    let parsed = url::Url::parse(raw?.trim()).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    Some(format!(
        "{}{}",
        parsed.origin().ascii_serialization(),
        parsed.path()
    ))
}

/// Tab title without control characters, at most 200 characters.
fn safe_tab_title(raw: Option<&str>) -> Option<String> {
    let title: String = raw?
        .chars()
        .filter(|ch| !ch.is_control())
        .take(TAB_LABEL_MAX_CHARS)
        .collect();
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

/// The ref one resolved tab contributes. No page content, no grant.
fn tab_ref_view(session_id: &str, tab: &Value) -> Value {
    let tab_id = tab_id_of(tab).unwrap_or_default();
    let target = safe_tab_target(tab.get("url").and_then(Value::as_str));
    let title = safe_tab_title(tab.get("title").and_then(Value::as_str));
    let (label, label_source) = match (&title, &target) {
        (Some(title), _) => (title.clone(), "page"),
        (None, Some(target)) => (target.clone(), "url"),
        (None, None) => (tab_id.to_owned(), "tab-id"),
    };
    json!({
        "kind": "browser-tab",
        "sessionId": session_id,
        "tabId": tab_id,
        "label": label,
        "labelSource": label_source,
        "title": title,
        "target": target,
        "trustClass": TAB_TRUST_CLASS,
        "grant": "none",
    })
}

/// Resolve every mention, or list why each failed one did. Refs are distinct
/// by tab id, in first-seen order.
fn resolve_all(
    session_id: &str,
    tabs: &[Value],
    mentions: &[Option<String>],
) -> Result<Vec<Value>, Vec<Value>> {
    let mut refs: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut failures: Vec<Value> = Vec::new();
    for (index, mention) in mentions.iter().enumerate() {
        match resolve_mention(tabs, mention.as_deref()) {
            Ok(tab) => {
                let id = tab_id_of(tab).unwrap_or_default().to_owned();
                if !seen.contains(&id) {
                    refs.push(tab_ref_view(session_id, tab));
                    seen.push(id);
                }
            }
            Err(reason) => failures.push(json!({
                "index": index,
                "tabId": mention,
                "reason": reason,
            })),
        }
    }
    if failures.is_empty() {
        Ok(refs)
    } else {
        Err(failures)
    }
}

// ---------------------------------------------------------------------------
// Error responses
// ---------------------------------------------------------------------------

fn bad_request(message: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error("invalid_input", message)),
    )
        .into_response()
}

fn invalid_session() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error(
            "invalid_browser_session",
            "The requested browser session id is invalid.",
        )),
    )
        .into_response()
}

fn zdr_tab_context_refused() -> Response {
    (
        StatusCode::CONFLICT,
        Json(error(
            "browser_tab_context_refused_zdr",
            "Page context from a browser tab is not attached in a zero-data-retention session.",
        )),
    )
        .into_response()
}

fn mentions_unresolved(failures: Vec<Value>) -> Response {
    (
        StatusCode::CONFLICT,
        Json(error_with_details(
            "browser_tab_mention_unresolved",
            "A tab mention could not be resolved to exactly one live tab, so nothing was attached.",
            json!({ "mentions": failures }),
        )),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Owner gate, Quarry read and the shared entry point
// ---------------------------------------------------------------------------

/// A projection that does not state `zdr: false` is refused like a ZDR session.
fn tab_context_refused(projection: &Value) -> bool {
    projection.get("zdr").and_then(Value::as_bool) != Some(false)
}

/// Re-resolve a turn's mentions for a session the caller owns. This is the one
/// entry point: the route below and, later, the chat turn path call it, so a
/// mention can never reach a model without passing the same gate. Returns the
/// refs, or the response to send instead.
pub(crate) async fn resolve_turn_tab_mentions(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    session_id: &str,
    mentions: &[TabMentionInput],
) -> Result<Vec<Value>, Response> {
    // Everything that needs no upstream is checked first.
    if !is_valid_path_segment(session_id) {
        return Err(invalid_session());
    }
    let mentions = checked_mentions(mentions).map_err(bad_request)?;

    let cookie = cookie_header(headers);
    let token = quarry_token(state, user, &cookie).await;
    let projection =
        owner_browser_session_projection(state, user, token.as_deref(), session_id).await?;
    // Quarry's tab list is not read at all for a refused session.
    if tab_context_refused(&projection) {
        return Err(zdr_tab_context_refused());
    }

    let (status, body) = quarry_call(
        state,
        Method::GET,
        &format!("/v1/agent/runs/{}/tabs", urlencoding::encode(session_id)),
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return Err(forward_quarry_failure(status, body));
    }
    let tabs: Vec<Value> = unwrap_data(&body)
        .get("tabs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    resolve_all(session_id, &tabs, &mentions).map_err(mentions_unresolved)
}

// ---------------------------------------------------------------------------
// Route
// ---------------------------------------------------------------------------

/// `POST /api/v1/browser/sessions/{session_id}/context/tab-mentions` with
/// `{ mentions: [{ tabId: string | null }] }` (E3.3-c, mounted only with
/// `BROWSER_TAB_MENTIONS`). Answers `{ data: { refs: [...] } }`, uncached.
async fn resolve_session_tab_mentions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    body: Bytes,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return invalid_session();
    }
    let input: TabMentionsBody = match serde_json::from_slice(&body) {
        Ok(input) => input,
        Err(_) => return bad_request("The body must be JSON with mentions only."),
    };
    match resolve_turn_tab_mentions(&state, &user, &headers, &session_id, &input.mentions).await {
        Ok(refs) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "private, no-store")],
            Json(ok(json!({ "refs": refs }))),
        )
            .into_response(),
        Err(response) => response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;
    use wiremock::{
        matchers::{method as wm_method, path as wm_path},
        Mock, MockServer, ResponseTemplate,
    };

    use crate::domains::browser::tests::test_app_state;
    use crate::middleware::AuthorizedMembership;

    const URI: &str = "/api/v1/browser/sessions/run-1/context/tab-mentions";

    fn test_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-a".to_owned(),
            user_email: "user-a@example.test".to_owned(),
            user_name: "user-a".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: None,
            active_org_id: Some("org_a".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org_a".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    async fn insert_test_user(
        mut request: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> Response {
        request.extensions_mut().insert(test_user());
        next.run(request).await
    }

    /// The route behind a stand-in session layer for `user-a`.
    fn context_app(state: AppState) -> axum::Router {
        axum::Router::new()
            .route(TAB_MENTIONS_PATH, post(resolve_session_tab_mentions))
            .layer(axum::middleware::from_fn(insert_test_user))
            .with_state(state)
    }

    async fn send(app: axum::Router, uri: &str, body: &str) -> (StatusCode, Option<String>, Value) {
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let cache = response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, cache, value)
    }

    /// Quarry knows run-1 for the caller; `zdr` is left out when null.
    async fn mount_owned_run(quarry: &MockServer, zdr: Value) {
        let mut data = json!({ "runId": "run-1", "controlMode": "agent_control" });
        if !zdr.is_null() {
            data["zdr"] = zdr;
        }
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/browser-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": data })))
            .mount(quarry)
            .await;
    }

    async fn mount_tabs(quarry: &MockServer, tabs: Value, expected_calls: u64) {
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/tabs"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "data": { "tabs": tabs } })),
            )
            .expect(expected_calls)
            .mount(quarry)
            .await;
    }

    async fn state_for(quarry: &MockServer) -> AppState {
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();
        state
    }

    fn live_tabs() -> Value {
        json!([
            { "tabId": "tab_a", "active": true, "title": "Invoice\u{0007} 42", "url": "https://user:pw@shop.example/orders/42?token=secret#top" },
            { "tabId": "tab_b", "active": false, "title": "", "url": "https://docs.example/guide" },
            { "tabId": "tab_c", "active": false, "url": "about:blank" }
        ])
    }

    #[test]
    fn tab_mentions_flag_defaults_off() {
        assert!(!tab_mentions_flag_on(None));
        assert!(!tab_mentions_flag_on(Some("")));
        assert!(!tab_mentions_flag_on(Some("0")));
        assert!(!tab_mentions_flag_on(Some("off")));
        assert!(tab_mentions_flag_on(Some("1")));
        assert!(tab_mentions_flag_on(Some(" TRUE ")));
        assert!(tab_mentions_flag_on(Some("on")));
    }

    #[test]
    fn mention_body_is_checked_before_any_upstream() {
        assert!(checked_mentions(&[]).is_err());
        let mention = |token: Option<&str>, tab_id: Option<&str>| TabMentionInput {
            token: token.map(str::to_owned),
            tab_id: tab_id.map(str::to_owned),
        };
        let many = vec![mention(None, None); MENTIONS_MAX + 1];
        assert!(checked_mentions(&many).is_err());
        assert!(checked_mentions(&[mention(None, Some("a/b"))]).is_err());
        let good = [
            mention(Some("@Tab"), None),
            mention(Some("@tab:tab_a"), Some(" tab_a ")),
            mention(None, Some("tab_b")),
        ];
        assert_eq!(
            checked_mentions(&good).unwrap(),
            vec![None, Some("tab_a".to_owned()), Some("tab_b".to_owned())]
        );
        // A token that names another tab, or is not a mention, is refused.
        assert!(checked_mentions(&[mention(Some("@tab:tab_b"), Some("tab_a"))]).is_err());
        assert!(checked_mentions(&[mention(Some("@tab"), Some("tab_a"))]).is_err());
        assert!(checked_mentions(&[mention(Some("@tab:tab_a"), None)]).is_err());
        assert!(checked_mentions(&[mention(Some("@table"), None)]).is_err());
        assert!(checked_mentions(&[mention(Some("@"), None)]).is_err());
        assert!(checked_mentions(&[mention(Some("@tæb"), None)]).is_err());
    }

    #[test]
    fn mention_resolution_fails_closed() {
        let tabs = vec![
            json!({ "tabId": "tab_a", "active": true }),
            json!({ "tabId": "tab_b", "active": false }),
            json!({ "tabId": "tab_c", "active": false }),
            json!({ "tabId": "tab_c", "active": false }),
            json!({ "tabId": "bad/id", "active": true }),
        ];
        assert!(resolve_mention(&tabs, None).is_ok());
        assert!(resolve_mention(&tabs, Some("tab_b")).is_ok());
        assert_eq!(
            resolve_mention(&tabs, Some("tab_c")).unwrap_err(),
            "ambiguous-tab"
        );
        assert_eq!(
            resolve_mention(&tabs, Some("tab_z")).unwrap_err(),
            "unknown-tab"
        );
        assert_eq!(
            resolve_mention(&tabs, Some("bad/id")).unwrap_err(),
            "unknown-tab"
        );
        assert_eq!(resolve_mention(&[], None).unwrap_err(), "no-tabs");

        let none_active = vec![json!({ "tabId": "tab_a", "active": false })];
        assert_eq!(
            resolve_mention(&none_active, None).unwrap_err(),
            "no-active-tab"
        );
        let two_active = vec![
            json!({ "tabId": "tab_a", "active": true }),
            json!({ "tabId": "tab_b", "active": true }),
        ];
        assert_eq!(
            resolve_mention(&two_active, None).unwrap_err(),
            "ambiguous-tab"
        );
    }

    #[test]
    fn refs_are_distinct_and_one_failure_fails_all() {
        let tabs = vec![
            json!({ "tabId": "tab_a", "active": true, "title": "A" }),
            json!({ "tabId": "tab_b", "active": false }),
        ];
        let refs = resolve_all(
            "run-1",
            &tabs,
            &[None, Some("tab_a".to_owned()), Some("tab_b".to_owned())],
        )
        .unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0]["tabId"], "tab_a");
        assert_eq!(refs[1]["tabId"], "tab_b");

        let failures = resolve_all("run-1", &tabs, &[None, Some("tab_z".to_owned())]).unwrap_err();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0]["index"], 1);
        assert_eq!(failures[0]["reason"], "unknown-tab");
    }

    #[test]
    fn tab_ref_labels_page_text_and_drops_url_secrets() {
        let reference = tab_ref_view(
            "run-1",
            &json!({ "tabId": "tab_a", "title": "  Pay\u{0000}ment  ", "url": "https://u:p@x.example/a?q=1#f" }),
        );
        assert_eq!(reference["label"], "Payment");
        assert_eq!(reference["labelSource"], "page");
        assert_eq!(reference["target"], "https://x.example/a");
        assert_eq!(reference["trustClass"], "browser-scraped");
        assert_eq!(reference["grant"], "none");

        let by_url = tab_ref_view(
            "run-1",
            &json!({ "tabId": "tab_b", "url": "https://d.example/g" }),
        );
        assert_eq!(by_url["label"], "https://d.example/g");
        assert_eq!(by_url["labelSource"], "url");

        let bare = tab_ref_view(
            "run-1",
            &json!({ "tabId": "tab_c", "url": "javascript:alert(1)" }),
        );
        assert_eq!(bare["label"], "tab_c");
        assert_eq!(bare["labelSource"], "tab-id");
        assert!(bare["target"].is_null());

        let long = tab_ref_view(
            "run-1",
            &json!({ "tabId": "tab_d", "title": "x".repeat(500) }),
        );
        assert_eq!(
            long["label"].as_str().unwrap().chars().count(),
            TAB_LABEL_MAX_CHARS
        );
    }

    #[tokio::test]
    async fn route_is_not_mounted_with_the_flag_off() {
        let state = test_app_state();
        let app = router_with(state.clone(), false).with_state(state);
        let (status, _, _) = send(app, URI, r#"{"mentions":[{"tabId":null}]}"#).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn mentions_resolve_against_live_tabs() {
        let quarry = MockServer::start().await;
        mount_owned_run(&quarry, json!(false)).await;
        mount_tabs(&quarry, live_tabs(), 1).await;
        let app = context_app(state_for(&quarry).await);

        let (status, cache, body) = send(
            app,
            URI,
            r#"{"mentions":[{"token":"@tab","tabId":null},{"token":"@tab:tab_b","tabId":"tab_b"},{"tabId":"tab_a"}]}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(cache.as_deref(), Some("private, no-store"));
        let refs = body["data"]["refs"].as_array().unwrap();
        assert_eq!(refs.len(), 2, "tab_a is mentioned twice but attached once");
        assert_eq!(refs[0]["tabId"], "tab_a");
        assert_eq!(refs[0]["label"], "Invoice 42");
        assert_eq!(refs[0]["target"], "https://shop.example/orders/42");
        assert_eq!(refs[0]["grant"], "none");
        assert_eq!(refs[1]["tabId"], "tab_b");
        assert_eq!(refs[1]["labelSource"], "url");
        let text = body.to_string();
        assert!(!text.contains("secret"), "{text}");
        assert!(!text.contains("pw@"), "{text}");
    }

    #[tokio::test]
    async fn an_unknown_tab_fails_the_whole_request() {
        let quarry = MockServer::start().await;
        mount_owned_run(&quarry, json!(false)).await;
        mount_tabs(&quarry, live_tabs(), 1).await;
        let app = context_app(state_for(&quarry).await);

        let (status, _, body) = send(
            app,
            URI,
            r#"{"mentions":[{"tabId":null},{"tabId":"tab_gone"}]}"#,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"]["code"], "browser_tab_mention_unresolved");
        assert_eq!(body["error"]["details"]["mentions"][0]["index"], 1);
        assert_eq!(
            body["error"]["details"]["mentions"][0]["reason"],
            "unknown-tab"
        );
        assert!(body.get("data").is_none());
    }

    #[tokio::test]
    async fn zdr_and_unstated_zdr_are_refused_without_reading_tabs() {
        for zdr in [json!(true), Value::Null] {
            let quarry = MockServer::start().await;
            mount_owned_run(&quarry, zdr.clone()).await;
            mount_tabs(&quarry, live_tabs(), 0).await;
            let app = context_app(state_for(&quarry).await);

            let (status, _, body) = send(app, URI, r#"{"mentions":[{"tabId":null}]}"#).await;
            assert_eq!(status, StatusCode::CONFLICT, "zdr {zdr}: {body}");
            assert_eq!(body["error"]["code"], "browser_tab_context_refused_zdr");
        }
    }

    #[tokio::test]
    async fn foreign_session_and_bad_input_never_read_tabs() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/browser-session"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": { "code": "NOT_FOUND", "message": "agent run not found" }
            })))
            .mount(&quarry)
            .await;
        mount_tabs(&quarry, live_tabs(), 0).await;
        let state = state_for(&quarry).await;

        let (status, _, _) = send(
            context_app(state.clone()),
            URI,
            r#"{"mentions":[{"tabId":null}]}"#,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        for body in [
            r#"{"mentions":[{"tabId":null}],"orgId":"org_b"}"#,
            r#"{"mentions":[{"tabId":"a/b"}]}"#,
            r#"{"mentions":[{"token":"@tab:tab_b","tabId":"tab_a"}]}"#,
            r#"{"mentions":[]}"#,
            r#"{"mentions":[{"tabId":null,"grant":"act"}]}"#,
            "not json",
        ] {
            let (status, _, response) = send(context_app(state.clone()), URI, body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {response}");
        }

        let (status, _, response) = send(
            context_app(state),
            "/api/v1/browser/sessions/r!/context/tab-mentions",
            r#"{"mentions":[{"tabId":null}]}"#,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    }
}
