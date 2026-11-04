//! `GET /api/v1/policy/effective?thread_id=`: the effective policy of one thread, each value with the layer that set it
//! (Wave 3 EP-9 T2).
//!
//! The browser's view of what governs a conversation. Each owner answers for its own layers and this route only
//! aggregates them: model-gateway for the approval posture, the run budget and the organization's agent switches as it
//! enforces them; org-core for the model access that applies to the caller and, for an admin, the browser-site mode;
//! and the organization's Zero Data Retention posture. Every owner has a two second budget, and an owner that does not
//! answer is `unavailable` for its keys, never a guessed value.
//!
//! A member reads values and origin labels. Revisions are admin detail, and so are the lists behind a count: a member
//! reads how many models the policy lists, an admin reads the list. The organization comes from the session only, and
//! the query may name nothing but the thread, so another organization's identifier is refused before anything is
//! forwarded. The route is switched on by `POLICY_EFFECTIVE_VIEW_ENABLED` (exactly `true` enables it); off, it is not
//! mounted at all, so it answers as any unknown route does.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use axum::{
    extract::{Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    domains::browser_sites,
    domains::chat::shared::{
        model_token, proxy_model_json_with_capability, required_capability_token,
    },
    envelope::error_with_details,
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
    upstream::{proxy_json, scope_org_id},
    zdr::{org_posture, request_zdr, OrgPosture},
};

/// The longest thread id the view forwards. A session thread id is a short opaque string.
const MAX_THREAD_ID_LEN: usize = 200;

/// How long one owner may take. The page waits on every owner together, so one slow owner would otherwise hold the
/// whole answer; an owner past this is `unavailable`.
const OWNER_TIMEOUT: Duration = Duration::from_secs(2);

/// The route, or no route at all while `POLICY_EFFECTIVE_VIEW_ENABLED` is off.
pub(crate) fn router(state: AppState) -> Router<AppState> {
    if !switch_enabled(std::env::var("POLICY_EFFECTIVE_VIEW_ENABLED").ok().as_deref()) {
        return Router::new();
    }
    Router::new()
        .route("/api/v1/policy/effective", get(get_effective_policy))
        // Per-org/user rate limiting, ordered as in `agent_policy.rs`: `require_session` runs first and inserts
        // `AuthenticatedUser`, so the limiter keys by the validated organization and user.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Exact, like the other switches: only the word `true` enables it.
fn switch_enabled(value: Option<&str>) -> bool {
    value == Some("true")
}

/// One value of the view, as the owners and this route shape it. `value` is `null` only when `unavailable` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Item {
    key: String,
    value: Value,
    origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision: Option<i64>,
    locked: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    unavailable: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Item {
    fn set(key: &str, value: Value, origin: &str, revision: Option<i64>, locked: bool) -> Self {
        Self {
            key: key.to_owned(),
            value,
            origin: Some(origin.to_owned()),
            revision,
            locked,
            unavailable: false,
        }
    }

    fn unavailable(key: &str) -> Self {
        Self {
            key: key.to_owned(),
            value: Value::Null,
            origin: None,
            revision: None,
            locked: false,
            unavailable: true,
        }
    }
}

/// Why an owner gave no answer. `ThreadNotFound` is model-gateway's refusal of a thread the caller does not own, and it
/// ends the whole view, since nothing about the thread may be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    ThreadNotFound,
    Unavailable,
}

/// `GET /api/v1/policy/effective?thread_id=` — the effective policy of the thread, for any member of the active
/// organization.
async fn get_effective_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let thread_id = match thread_id_from(&query) {
        Ok(thread_id) => thread_id,
        Err(response) => return response,
    };
    let Some(org_id) = active_org_id(&user) else {
        return refusal(
            StatusCode::FORBIDDEN,
            "forbidden",
            "The requested organization is not authorized for this session.",
        );
    };
    let admin = has_authorized_org_role(&user, &["owner", "admin"]);
    let model_access_on = state.model_entitlement_admin_enabled;
    // Org-core reads the browser-site policy for an owner or admin alone, so a member is not asked for it.
    let browser_sites_on = admin && browser_sites::enabled_from_env();

    let (model_plane, model_access, browser, zdr) = tokio::join!(
        within_budget(model_plane_items(&state, &user, &headers, &thread_id)),
        async {
            if model_access_on {
                Some(within_budget(model_access_items(&state, &user, &org_id, admin)).await)
            } else {
                None
            }
        },
        async {
            if browser_sites_on {
                Some(within_budget(browser_site_items(&state, &user, &org_id)).await)
            } else {
                None
            }
        },
        within_budget(zdr_item(&state, &user, &headers)),
    );

    let mut items = match model_plane {
        Ok(items) => items,
        Err(Failure::ThreadNotFound) => {
            return refusal(
                StatusCode::NOT_FOUND,
                "thread_not_found",
                "No thread with that id is visible to the caller.",
            )
        }
        Err(Failure::Unavailable) => vec![Item::unavailable("model_plane")],
    };
    if let Some(result) = model_access {
        items.extend(result.unwrap_or_else(|_| vec![Item::unavailable("model_access")]));
    }
    if let Some(result) = browser {
        items.extend(result.unwrap_or_else(|_| vec![Item::unavailable("browser_sites")]));
    }
    items.push(zdr.unwrap_or_else(|_| Item::unavailable("zdr")));
    (
        StatusCode::OK,
        Json(json!({ "data": { "items": for_viewer(items, admin) } })),
    )
        .into_response()
}

/// The thread the view is about. The query may name only the thread: the organization comes from the session, and a
/// space or any other identifier is refused before anything is forwarded.
fn thread_id_from(query: &HashMap<String, String>) -> Result<String, Response> {
    if query.contains_key("space_id") {
        return Err(refusal(
            StatusCode::BAD_REQUEST,
            "space_scope_unsupported",
            "The effective policy of a Space is not readable yet; name a thread.",
        ));
    }
    if query.keys().any(|key| key.as_str() != "thread_id") {
        return Err(refusal(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "The query may name only thread_id; the organization comes from the session.",
        ));
    }
    match query.get("thread_id").map(|id| id.trim()) {
        Some(id) if !id.is_empty() && id.len() <= MAX_THREAD_ID_LEN => Ok(id.to_owned()),
        _ => Err(refusal(
            StatusCode::BAD_REQUEST,
            "thread_id_required",
            "A thread_id of 1 to 200 characters is required.",
        )),
    }
}

/// The active organization, when the session holds a live membership in it. The organization is never read from the
/// request.
fn active_org_id(user: &AuthenticatedUser) -> Option<String> {
    let org_id = scope_org_id(user)?;
    let member = user
        .authorized_membership
        .as_ref()
        .is_some_and(|membership| membership.organization_id.trim() == org_id);
    member.then(|| org_id.to_owned())
}

fn refusal(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(error_with_details(code, message, json!({})))).into_response()
}

/// An owner that answers within [`OWNER_TIMEOUT`], or one that does not.
async fn within_budget<T>(
    owner: impl Future<Output = Result<T, Failure>>,
) -> Result<T, Failure> {
    tokio::time::timeout(OWNER_TIMEOUT, owner)
        .await
        .unwrap_or(Err(Failure::Unavailable))
}

/// What a viewer may read: the revision is admin detail, so a member's view carries none.
fn for_viewer(mut items: Vec<Item>, admin: bool) -> Vec<Item> {
    if !admin {
        for item in &mut items {
            item.revision = None;
        }
    }
    items
}

/// One GET to org-core on the caller's behalf. Anything but a 200 is an owner that did not answer.
async fn org_core_json(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    url: &str,
) -> Result<Value, Failure> {
    let actor = actor_for(user);
    match proxy_json(state, Method::GET, url, None, Some(org_id), Some(&actor), None).await {
        (StatusCode::OK, Json(body)) => Ok(body),
        _ => Err(Failure::Unavailable),
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

/// model-gateway's view of the thread, with the two credentials the gateway checks: the session and the capability
/// bearer its organization policy is read with.
async fn model_plane_items(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    thread_id: &str,
) -> Result<Vec<Item>, Failure> {
    let token = model_token(state, user, headers).await;
    let capability = required_capability_token(state, user, headers)
        .await
        .map_err(|_| Failure::Unavailable)?;
    let url = format!(
        "{}/v1/policy/effective?thread_id={}",
        state.model_gateway_url,
        urlencoding::encode(thread_id)
    );
    let (status, Json(body)) = proxy_model_json_with_capability(
        state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(capability.as_str()),
        user,
    )
    .await;
    model_plane_answer(status, &body)
}

#[derive(Deserialize)]
struct ModelPlaneBody {
    data: ItemsBody,
}

#[derive(Deserialize)]
struct ItemsBody {
    items: Vec<Item>,
}

/// The model-gateway answer read into items. A thread it refuses is named as such; any other answer it does not give
/// is unavailable.
fn model_plane_answer(status: StatusCode, body: &Value) -> Result<Vec<Item>, Failure> {
    match status {
        StatusCode::OK => serde_json::from_value::<ModelPlaneBody>(body.clone())
            .map(|answer| answer.data.items)
            .map_err(|_| Failure::Unavailable),
        StatusCode::NOT_FOUND if body["error"]["code"].as_str() == Some("thread_not_found") => {
            Err(Failure::ThreadNotFound)
        }
        _ => Err(Failure::Unavailable),
    }
}

/// The model access that applies to the caller in this organization, from org-core's explanation of their own
/// entitlement.
async fn model_access_items(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    admin: bool,
) -> Result<Vec<Item>, Failure> {
    let url = format!(
        "{}/api/v1/organizations/{}/model-entitlement/explain",
        state.org_core_url,
        urlencoding::encode(org_id)
    );
    let body = org_core_json(state, user, org_id, &url).await?;
    Ok(model_access_from(&body, admin))
}

/// The model-access items from org-core's explanation: the mode, the privacy-tier floor and the effort ceiling, and the
/// allow and deny lists. A layer the organization did not set reads as the platform's own value.
fn model_access_from(body: &Value, admin: bool) -> Vec<Item> {
    let set = body
        .get("state")
        .and_then(Value::as_str)
        .is_some_and(|layer| layer != "none");
    let revision = body.get("revision").and_then(Value::as_i64).unwrap_or(0);
    let policy = body.get("policy").filter(|policy| policy.is_object());
    let allow_mode = policy_text(policy, "allow_mode");
    let privacy = policy_text(policy, "min_privacy_tier");
    let effort = policy_text(policy, "max_effort");
    let allow = policy_list(policy, "allow");
    let deny = policy_list(policy, "deny");

    let mode_set = set && !allow_mode.is_empty();
    let privacy_set = set && !privacy.is_empty() && privacy != "unspecified";
    let effort_set = set && !effort.is_empty();
    let allow_set = set && !allow.is_empty();
    let deny_set = set && !deny.is_empty();
    let listed = allow_mode == "listed";
    let allow_shown = if admin {
        Value::Array(allow.clone())
    } else {
        json!(allow.len())
    };
    let deny_shown = if admin {
        Value::Array(deny.clone())
    } else {
        json!(deny.len())
    };
    vec![
        org_or_platform(
            "model_access.allow_mode",
            json!(if allow_mode.is_empty() { "all" } else { allow_mode.as_str() }),
            mode_set,
            revision,
            mode_set && listed,
        ),
        org_or_platform(
            "model_access.min_privacy_tier",
            json!(if privacy.is_empty() { "unspecified" } else { privacy.as_str() }),
            privacy_set,
            revision,
            privacy_set,
        ),
        org_or_platform(
            "model_access.max_effort",
            json!(if effort.is_empty() { "unlimited" } else { effort.as_str() }),
            effort_set,
            revision,
            effort_set,
        ),
        org_or_platform(
            "model_access.allow",
            allow_shown,
            allow_set,
            revision,
            allow_set && listed,
        ),
        org_or_platform("model_access.deny", deny_shown, deny_set, revision, deny_set),
    ]
}

fn policy_text(policy: Option<&Value>, field: &str) -> String {
    policy
        .and_then(|policy| policy.get(field))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned()
}

fn policy_list(policy: Option<&Value>, field: &str) -> Vec<Value> {
    policy
        .and_then(|policy| policy.get(field))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// A value an organization may set: the organization's at its revision when it set one, otherwise the platform's.
fn org_or_platform(
    key: &str,
    value: Value,
    set: bool,
    revision: i64,
    restricts: bool,
) -> Item {
    if set {
        Item::set(key, value, "org", Some(revision), restricts)
    } else {
        Item::set(key, value, "platform", None, false)
    }
}

/// The organization's browser-site mode and rules, from org-core's policy read. Only an owner or admin may read it
/// there, so this runs for an admin alone.
async fn browser_site_items(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> Result<Vec<Item>, Failure> {
    let url = format!(
        "{}/api/v1/organizations/{}/browser-sites",
        state.org_core_url,
        urlencoding::encode(org_id)
    );
    let body = org_core_json(state, user, org_id, &url).await?;
    Ok(browser_site_items_from(&body))
}

/// The browser-site items from org-core's policy: the mode, and the rules a site is held to. A policy never written
/// reads as the platform's open mode with no rules.
fn browser_site_items_from(body: &Value) -> Vec<Item> {
    let revision = body.get("revision").and_then(Value::as_i64).unwrap_or(0);
    let set = revision > 0;
    let mode = body
        .get("mode")
        .and_then(Value::as_str)
        .filter(|mode| !mode.is_empty())
        .unwrap_or("open");
    let rules = body
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rules_set = set && !rules.is_empty();
    vec![
        org_or_platform(
            "browser_sites.mode",
            json!(mode),
            set,
            revision,
            set && mode == "allowlist",
        ),
        org_or_platform(
            "browser_sites.rules",
            Value::Array(rules),
            rules_set,
            revision,
            rules_set,
        ),
    ]
}

/// The organization's Zero Data Retention posture, or the request's own ZDR where the organization has none. An unknown
/// posture is unavailable, never off.
async fn zdr_item(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<Item, Failure> {
    zdr_item_from(org_posture(state, user).await, request_zdr(headers, None))
}

fn zdr_item_from(posture: OrgPosture, requested: bool) -> Result<Item, Failure> {
    match (posture, requested) {
        (OrgPosture::Unknown, _) => Err(Failure::Unavailable),
        (OrgPosture::Zdr, _) => Ok(Item::set("zdr", json!(true), "org", None, true)),
        (OrgPosture::NotZdr, true) => Ok(Item::set("zdr", json!(true), "request", None, false)),
        (OrgPosture::NotZdr, false) => Ok(Item::set("zdr", json!(false), "platform", None, false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn find<'a>(items: &'a [Item], key: &str) -> &'a Item {
        items
            .iter()
            .find(|item| item.key == key)
            .unwrap_or_else(|| panic!("{key} is listed"))
    }

    #[test]
    fn the_switch_is_exact_and_off_by_default() {
        assert!(switch_enabled(Some("true")));
        for off in [None, Some(""), Some("TRUE"), Some("1"), Some("false"), Some(" true")] {
            assert!(!switch_enabled(off), "{off:?} must stay off");
        }
    }

    #[test]
    fn the_thread_is_the_only_query_key_and_a_space_or_another_org_is_refused() {
        let thread = thread_id_from(&query(&[("thread_id", " t-1 ")])).ok();
        assert_eq!(thread.as_deref(), Some("t-1"));

        let too_long = "t".repeat(MAX_THREAD_ID_LEN + 1);
        for refused in [
            query(&[]),
            query(&[("thread_id", "   ")]),
            query(&[("thread_id", too_long.as_str())]),
            query(&[("thread_id", "t-1"), ("org_id", "org-2")]),
            query(&[("thread_id", "t-1"), ("user_id", "user-2")]),
            query(&[("space_id", "space-1")]),
        ] {
            let response = thread_id_from(&refused).expect_err("the query is refused");
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{refused:?}");
        }
    }

    #[test]
    fn a_model_plane_refusal_for_a_thread_is_named_and_any_other_answer_is_unavailable() {
        let refused = json!({"error": {"code": "thread_not_found", "message": "x", "details": {}}});
        assert_eq!(
            model_plane_answer(StatusCode::NOT_FOUND, &refused),
            Err(Failure::ThreadNotFound)
        );
        assert_eq!(
            model_plane_answer(StatusCode::NOT_FOUND, &json!({})),
            Err(Failure::Unavailable)
        );
        assert_eq!(
            model_plane_answer(StatusCode::INTERNAL_SERVER_ERROR, &json!({})),
            Err(Failure::Unavailable)
        );
        assert_eq!(
            model_plane_answer(StatusCode::OK, &json!({"data": {"items": "nope"}})),
            Err(Failure::Unavailable)
        );
    }

    #[test]
    fn a_model_plane_answer_is_read_into_its_items_with_the_origin_kept() {
        let body = json!({"data": {"items": [
            {"key": "approval.posture", "value": "auto", "origin": "platform", "locked": false},
            {"key": "web.mode", "value": null, "origin": null, "locked": false, "unavailable": true},
        ]}});
        let items = model_plane_answer(StatusCode::OK, &body).expect("the items are read");
        assert_eq!(
            items,
            vec![
                Item::set("approval.posture", json!("auto"), "platform", None, false),
                Item::unavailable("web.mode"),
            ]
        );
    }

    #[test]
    fn a_member_reads_no_revision_and_the_count_where_an_admin_reads_the_list() {
        let body = json!({
            "state": "org",
            "revision": 7,
            "policy": {
                "allow_mode": "listed",
                "allow": ["model-a", "model-b"],
                "deny": [],
                "min_privacy_tier": "eu_resident",
                "max_effort": "",
            },
        });
        let member = for_viewer(model_access_from(&body, false), false);
        let admin = for_viewer(model_access_from(&body, true), true);

        let allow_member = find(&member, "model_access.allow");
        assert_eq!(allow_member.value, json!(2));
        assert_eq!(allow_member.revision, None);
        assert_eq!(find(&admin, "model_access.allow").value, json!(["model-a", "model-b"]));
        assert_eq!(find(&admin, "model_access.allow").revision, Some(7));

        let privacy = find(&member, "model_access.min_privacy_tier");
        assert_eq!(privacy.value, json!("eu_resident"));
        assert_eq!(privacy.origin.as_deref(), Some("org"));
        assert!(privacy.locked);
        assert_eq!(find(&member, "model_access.max_effort").value, json!("unlimited"));
    }

    #[test]
    fn an_organization_with_no_policy_reads_the_platform_defaults() {
        let items = model_access_from(&json!({"state": "none", "revision": 0, "policy": null}), true);
        let mode = find(&items, "model_access.allow_mode");
        assert_eq!(mode.value, json!("all"));
        assert_eq!(mode.origin.as_deref(), Some("platform"));
        assert!(!mode.locked);
        assert!(items.iter().all(|item| item.origin.as_deref() == Some("platform")));
    }

    #[test]
    fn a_browser_site_policy_never_written_reads_as_the_open_platform_default() {
        let items = browser_site_items_from(&json!({"revision": 0, "mode": "open", "rules": []}));
        assert!(items
            .iter()
            .all(|item| item.origin.as_deref() == Some("platform") && !item.locked));
    }

    #[test]
    fn an_allowlist_set_by_the_organization_is_locked_and_names_its_revision() {
        let items = browser_site_items_from(&json!({
            "revision": 3,
            "mode": "allowlist",
            "rules": [{"host": "example.com", "rule": "read"}],
        }));
        let mode = find(&items, "browser_sites.mode");
        assert_eq!(mode.origin.as_deref(), Some("org"));
        assert_eq!(mode.revision, Some(3));
        assert!(mode.locked);
        assert!(find(&items, "browser_sites.rules").locked);
    }

    #[test]
    fn an_unknown_zdr_posture_is_unavailable_and_never_off() {
        assert_eq!(
            zdr_item_from(OrgPosture::Unknown, false),
            Err(Failure::Unavailable)
        );
        assert_eq!(
            zdr_item_from(OrgPosture::Zdr, false).expect("answered").origin.as_deref(),
            Some("org")
        );
        assert_eq!(
            zdr_item_from(OrgPosture::NotZdr, true).expect("answered").origin.as_deref(),
            Some("request")
        );
        assert_eq!(
            zdr_item_from(OrgPosture::NotZdr, false).expect("answered").value,
            json!(false)
        );
    }

    #[tokio::test]
    async fn an_owner_that_does_not_answer_in_time_is_unavailable() {
        let never = std::future::pending::<Result<Vec<Item>, Failure>>();
        assert_eq!(within_budget(never).await, Err(Failure::Unavailable));
    }
}
