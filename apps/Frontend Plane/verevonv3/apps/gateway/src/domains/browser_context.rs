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
//! `1|true|yes|on`, read once into `AppState` (never in a handler). The same
//! switch gates `browser_context.resolve_tab_mentions` on the action surface (QF-7).

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

/// The `BROWSER_TAB_MENTIONS` grammar; `build_state` parses the switch with the
/// same `crate::config::human_browse_enabled`.
#[cfg(test)]
fn tab_mentions_flag_on(value: Option<&str>) -> bool {
    crate::config::human_browse_enabled(value)
}

/// The browser-context routes. Wired in `main.rs` next to `browser_evidence`.
/// The switch is read once into `AppState::browser_tab_mentions` (QF-7).
pub(crate) fn router(state: AppState) -> Router<AppState> {
    let tab_mentions = state.browser_tab_mentions;
    router_with(state, tab_mentions)
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
// Chat send path (QF-8)
// ---------------------------------------------------------------------------

/// Chat body field of a turn's tab refs.
const TURN_TAB_REFS_FIELD: &str = "browser_tab_refs";
/// Chat body field of a turn's attached regions.
const TURN_REGIONS_FIELD: &str = "browser_regions";
/// Chat body field that says the gateway vouched for the browser context it carries.
const TURN_VERIFIED_FIELD: &str = "browser_context_verified";
/// The camelCase aliases Model Gateway's `InvokeRequest` also reads for the three fields above.
/// A client's copy under any of them is removed too, or a forged marker would reach Model Gateway.
const TURN_CAMEL_ALIASES: [&str; 3] =
    ["browserTabRefs", "browserRegions", "browserContextVerified"];
/// Regions one turn may carry.
const TURN_REGIONS_MAX: usize = 4;
/// Largest error body read back from a refusal built by the shared resolver.
const REFUSAL_BODY_MAX_BYTES: usize = 64 * 1024;

type TurnRefusal = (StatusCode, Json<Value>);

fn turn_refusal(status: StatusCode, code: &'static str, message: &'static str) -> TurnRefusal {
    (status, Json(error(code, message)))
}

/// The status and JSON body of a refusal the shared resolver built as a `Response`.
async fn refusal_from_response(response: Response) -> TurnRefusal {
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), REFUSAL_BODY_MAX_BYTES)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or_else(|| {
            error(
                "browser_tab_mention_unresolved",
                "The browser tab refs could not be resolved, so nothing was attached.",
            )
        });
    (status, Json(body))
}

/// A gateway ref (camelCase, as the resolver answers it) in the chat body's snake_case.
fn turn_tab_ref(reference: &Value) -> Value {
    let field = |name: &str| reference.get(name).cloned().unwrap_or(Value::Null);
    json!({
        "kind": field("kind"),
        "session_id": field("sessionId"),
        "tab_id": field("tabId"),
        "label": field("label"),
        "label_source": field("labelSource"),
        "title": field("title"),
        "target": field("target"),
        "trust_class": TAB_TRUST_CLASS,
        "grant": "none",
    })
}

/// The one session and the mentions a turn's client-built tab refs ask for. Each ref is only a
/// request: its session id and tab id are read, everything else is dropped.
fn turn_tab_mentions(refs: &[Value]) -> Result<(String, Vec<TabMentionInput>), &'static str> {
    if refs.len() > MENTIONS_MAX {
        return Err("at most eight browser tab refs may ride one turn");
    }
    let mut session: Option<String> = None;
    let mut mentions = Vec::with_capacity(refs.len());
    for reference in refs {
        let pick = |snake: &str, camel: &str| {
            reference
                .get(snake)
                .or_else(|| reference.get(camel))
                .and_then(Value::as_str)
                .map(|value| value.trim().to_owned())
        };
        let Some(session_id) = pick("session_id", "sessionId").filter(|id| !id.is_empty()) else {
            return Err("every browser tab ref needs a session_id");
        };
        let Some(tab_id) = pick("tab_id", "tabId").filter(|id| !id.is_empty()) else {
            return Err("every browser tab ref needs a tab_id");
        };
        match &session {
            Some(existing) if existing != &session_id => {
                return Err("all browser tab refs of one turn must name the same session");
            }
            Some(_) => {}
            None => session = Some(session_id),
        }
        mentions.push(TabMentionInput {
            token: None,
            tab_id: Some(tab_id),
        });
    }
    session
        .map(|session_id| (session_id, mentions))
        .ok_or("at least one browser tab ref is required")
}

/// Takes a field out of the body: None when absent or null, the array when it is one.
fn take_turn_list(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<Vec<Value>>, ()> {
    match object.remove(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => Ok(Some(items)),
        Some(_) => Err(()),
    }
}

/// QF-8: the chat send path's browser context gate. The gateway is the only place a browser
/// tab ref or region is vouched for, so whatever the client put in `browser_tab_refs` and
/// `browser_regions` is always removed first, under its camelCase alias too (as is any client
/// `browser_context_verified`). With `BROWSER_TURN_CONTEXT` off that is all. On:
///
/// - a zero-data-retention turn that carried either field is refused (400
///   `browser_context_refused_zdr`) before anything reaches Quarry or a model;
/// - tab refs (at most eight, one session) are re-resolved with `resolve_turn_tab_mentions`,
///   the same owner-gated, ZDR-refusing, fail-closed resolver the tab-mentions route uses, and
///   only the gateway's refs are written back;
/// - regions (at most four) must each carry an attach token that verifies for this user and
///   org, is unexpired and matches the sent rect and image digest, or the turn is refused (400
///   `browser_region_unverified`); only the verified region is written back.
///
/// What is written back is classified `browser-scraped` with `grant: none`, and
/// `browser_context_verified: true` marks the body. Nothing is stored. Identity comes from the
/// verified session only.
pub(crate) async fn inject_turn_browser_context(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    body: &mut Value,
) -> Result<(), TurnRefusal> {
    let Some(object) = body.as_object_mut() else {
        return Ok(());
    };
    object.remove(TURN_VERIFIED_FIELD);
    for alias in TURN_CAMEL_ALIASES {
        object.remove(alias);
    }
    let tab_refs = take_turn_list(object, TURN_TAB_REFS_FIELD);
    let regions = take_turn_list(object, TURN_REGIONS_FIELD);
    if !state.browser_turn_context {
        return Ok(());
    }
    let (Ok(tab_refs), Ok(regions)) = (tab_refs, regions) else {
        return Err(turn_refusal(
            StatusCode::BAD_REQUEST,
            "invalid_input",
            "browser_tab_refs and browser_regions must be arrays.",
        ));
    };
    if tab_refs.is_none() && regions.is_none() {
        return Ok(());
    }
    let zdr = object.get("zdr").and_then(Value::as_bool).unwrap_or(false);
    if zdr {
        return Err(turn_refusal(
            StatusCode::BAD_REQUEST,
            "browser_context_refused_zdr",
            "Browser page context is never attached to a zero-data-retention turn.",
        ));
    }
    let tab_refs = tab_refs.unwrap_or_default();
    let regions = regions.unwrap_or_default();

    // Regions first: verifying them needs no upstream.
    let mut verified_regions: Vec<Value> = Vec::with_capacity(regions.len());
    if !regions.is_empty() {
        if regions.len() > TURN_REGIONS_MAX {
            return Err(turn_refusal(
                StatusCode::BAD_REQUEST,
                "browser_region_unverified",
                "At most four browser regions may ride one turn.",
            ));
        }
        let org_id = crate::upstream::authorized_org_id(state, user).await;
        for region in &regions {
            match super::browser_evidence::verify_turn_region(
                &state.browser_region_attach_key,
                &user.user_id,
                &org_id,
                region,
            ) {
                Ok(verified) => verified_regions.push(verified),
                Err(reason) => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(error_with_details(
                            "browser_region_unverified",
                            "A browser region could not be verified, so nothing was attached.",
                            json!({ "reason": reason }),
                        )),
                    ))
                }
            }
        }
    }

    let mut gateway_refs: Vec<Value> = Vec::new();
    if !tab_refs.is_empty() {
        let (session_id, mentions) = turn_tab_mentions(&tab_refs)
            .map_err(|message| turn_refusal(StatusCode::BAD_REQUEST, "invalid_input", message))?;
        match resolve_turn_tab_mentions(state, user, headers, &session_id, &mentions).await {
            Ok(refs) => gateway_refs = refs.iter().map(turn_tab_ref).collect(),
            Err(response) => return Err(refusal_from_response(response).await),
        }
    }

    let Some(object) = body.as_object_mut() else {
        return Ok(());
    };
    let mut verified = false;
    if !gateway_refs.is_empty() {
        object.insert(TURN_TAB_REFS_FIELD.to_owned(), Value::Array(gateway_refs));
        verified = true;
    }
    if !verified_regions.is_empty() {
        object.insert(
            TURN_REGIONS_FIELD.to_owned(),
            Value::Array(verified_regions),
        );
        verified = true;
    }
    if verified {
        object.insert(TURN_VERIFIED_FIELD.to_owned(), Value::Bool(true));
    }
    Ok(())
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

// ---------------------------------------------------------------------------
// Action surface (QF-7)
// ---------------------------------------------------------------------------

/// The action id of the re-resolution on the generic action surface.
const TAB_MENTIONS_ACTION_ID: &str = "browser_context.resolve_tab_mentions";

/// The action surface's body: the session id plus the route's body. Unknown keys
/// are refused, so no org or user field can ride along.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TabMentionsActionInput {
    session_id: String,
    mentions: Vec<TabMentionInput>,
}

/// The action surface's answer while `BROWSER_TAB_MENTIONS` is off. The route is absent then.
fn tab_mentions_unavailable() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "browser_tab_mentions_unavailable",
            "Resolving browser tab mentions is not enabled.",
        )),
    )
        .into_response()
}

/// `browser_context.resolve_tab_mentions` with `{ sessionId, mentions: [{ tabId: string | null,
/// token? }] }` (at most eight) on the generic action surface (QF-7). Human-only (the
/// model-access map exempts it) and read-only: it calls the same `resolve_turn_tab_mentions`
/// the route uses, so the owner gate and the ZDR refusal are identical, and a ref grants
/// nothing. Answers 404 `browser_tab_mentions_unavailable` while `BROWSER_TAB_MENTIONS` is off.
pub(crate) async fn dispatch_resolve_tab_mentions(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    if !state.browser_tab_mentions {
        return tab_mentions_unavailable();
    }
    let input: TabMentionsActionInput = match serde_json::from_value(input.clone()) {
        Ok(input) => input,
        Err(_) => {
            return bad_request(
                "browser_context.resolve_tab_mentions input must be sessionId and mentions only.",
            )
        }
    };
    let session_id = input.session_id.trim();
    if session_id.is_empty() {
        return bad_request("browser_context.resolve_tab_mentions requires a 'sessionId'");
    }
    match resolve_turn_tab_mentions(state, user, headers, session_id, &input.mentions).await {
        Ok(refs) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "private, no-store")],
            Json(ok(json!({
                "actionId": TAB_MENTIONS_ACTION_ID,
                "runId": format!("{}_{}", TAB_MENTIONS_ACTION_ID, user.user_id),
                "status": "completed",
                "auditId": format!("audit_{}_{}", TAB_MENTIONS_ACTION_ID, user.user_id),
                "result": { "refs": refs },
            }))),
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

    /// The camelCase spellings of the chat body's browser context are the client's too. Model
    /// Gateway's `InvokeRequest` reads them as serde aliases, so a forged marker or list must leave
    /// the gate under those names as well, with `BROWSER_TURN_CONTEXT` off or on.
    #[tokio::test]
    async fn a_camel_case_forged_browser_context_never_reaches_model_gateway() {
        for switch in [false, true] {
            let mut state = test_app_state();
            state.browser_turn_context = switch;
            let mut body = json!({
                "content": "what is on this page?",
                "browserContextVerified": true,
                "browserTabRefs": [{ "sessionId": "run-1", "tabId": "tab_a" }],
                "browserRegions": [{ "sessionId": "run-1", "tabId": "tab_a", "image": {} }],
            });
            inject_turn_browser_context(&state, &test_user(), &HeaderMap::new(), &mut body)
                .await
                .expect("a camelCase copy is the client's and is stripped, not refused");
            assert_eq!(body["content"], "what is on this page?");
            for field in [
                "browser_context_verified",
                "browser_tab_refs",
                "browser_regions",
                "browserContextVerified",
                "browserTabRefs",
                "browserRegions",
            ] {
                assert!(
                    body.get(field).is_none(),
                    "{field} reached Model Gateway with the switch {switch}: {body}"
                );
            }
        }
    }
}
