//! E3.2-g1: the organization browser policy that Settings > Browser reads and saves (WS7.H,
//! org-core `/orgs/:id/web-policy`).
//!
//! org-core owns the document, its revision (`If-Match`), the plan fold and every rule about which
//! switch may be on. This module is the Frontend Plane's door to it: the flag, the active-org
//! check, the owner-or-admin check for a save, a shape check, then a proxy that signs the same
//! delegation as the other org-core routes. It holds no state and evaluates no policy, and
//! org-core's typed status and body pass through. A save replaces the whole document at the
//! revision it was read at. A partial set would loosen the policy by omission, so the shape check
//! refuses every field it does not know.
//!
//! Order on every route: flag (404 `browser_policy_admin_disabled`), active org, the role for a
//! save, shape check, proxy. The flag is a rollout control, never an authorization input.

use axum::{http::StatusCode, Json};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::{proxy_json, proxy_json_with_if_match},
};

use super::shared::{actor_for, require_active_org, require_org_admin, GatewayJsonResponse};

const CODE_INVALID: &str = "browser_policy_invalid_request";
const MAX_ORG_ID_LENGTH: usize = 128;
const MAX_KEY_LENGTH: usize = 64;
const MAX_VALUE_LENGTH: usize = 32;
const MAX_CAPS: usize = 32;
const MAX_HOST_LENGTH: usize = 253;
const MAX_DOMAIN_ENTRIES: usize = 64;
const MAX_DOMAIN_BYTES: usize = 2048;

/// The only fields a save may carry: the whole org-core document (WS7.H).
const DOCUMENT_FIELDS: [&str; 4] = ["caps", "robots_max", "proxy_max", "domains"];

/// `BROWSER_POLICY_ADMIN_API`: the organization browser policy. Off, or unset, answers 404
/// `browser_policy_admin_disabled` before any other check. Read per request, as the agent
/// toolset switch is.
pub(crate) fn admin_api_enabled() -> bool {
    flag_is_on(std::env::var("BROWSER_POLICY_ADMIN_API").ok().as_deref())
}

fn flag_is_on(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

fn disabled() -> GatewayJsonResponse {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "browser_policy_admin_disabled",
            "Browser policy administration is not enabled.",
        )),
    )
}

fn invalid(field: &str, message: &str) -> GatewayJsonResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            CODE_INVALID,
            message,
            json!({ "field": field }),
        )),
    )
}

/// The org id is placed in a URL path, so only the characters an org id uses are let through.
fn org_id_is_path_safe(org_id: &str) -> bool {
    !org_id.is_empty()
        && org_id.len() <= MAX_ORG_ID_LENGTH
        && org_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn check_org_id(org_id: &str) -> Result<(), GatewayJsonResponse> {
    if org_id_is_path_safe(org_id) {
        Ok(())
    } else {
        Err(invalid("orgId", "orgId is not a valid organization id."))
    }
}

fn org_policy_url(state: &AppState, org_id: &str) -> String {
    format!("{}/orgs/{org_id}/web-policy", state.org_core_url)
}

/// GET: the organization's browser policy for any active member. org-core answers the stored
/// document to an owner or admin only; `can_manage` in the answer says which view the caller got.
pub(crate) async fn get_org_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> GatewayJsonResponse {
    if !admin_api_enabled() {
        return disabled();
    }
    if let Err(response) = require_active_org(user, org_id) {
        return response;
    }
    if let Err(response) = check_org_id(org_id) {
        return response;
    }
    let can_manage = require_org_admin(state, user, org_id).await.is_ok();
    let (status, Json(mut body)) = proxy_json(
        state,
        Method::GET,
        &org_policy_url(state, org_id),
        None,
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if status.is_success() {
        if let Some(data) = body.get_mut("data").and_then(Value::as_object_mut) {
            data.insert("can_manage".to_owned(), json!(can_manage));
        }
    }
    (status, Json(body))
}

/// PUT: replaces the organization's browser policy with `document`, when the stored revision is
/// still `if_match`. Owner or admin only. org-core checks the revision, the plan entitlements and
/// every rule about a value, and its 409, 402 and 422 answers pass through.
pub(crate) async fn set_org_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    if_match: i64,
    document: &Value,
) -> GatewayJsonResponse {
    if !admin_api_enabled() {
        return disabled();
    }
    if let Err(response) = require_org_admin(state, user, org_id).await {
        return response;
    }
    if let Err(response) = check_org_id(org_id) {
        return response;
    }
    if if_match < 0 {
        return invalid("ifMatch", "ifMatch must be the revision you read, 0 or more.");
    }
    let forwarded = match validate_document(document) {
        Ok(forwarded) => forwarded,
        Err(response) => return response,
    };
    let (status, body) = proxy_json_with_if_match(
        state,
        Method::PUT,
        &org_policy_url(state, org_id),
        Some(forwarded),
        Some(org_id),
        Some(&actor_for(user)),
        &if_match.to_string(),
    )
    .await;
    (status, body)
}

/// Shape check of a save. Returns the document unchanged when it carries only the fields org-core
/// reads, with capability keys and values, host lists and their sizes in range; the rules about
/// each value stay with org-core.
fn validate_document(document: &Value) -> Result<Value, GatewayJsonResponse> {
    let Some(object) = document.as_object() else {
        return Err(invalid("document", "document must be a JSON object."));
    };
    if object
        .keys()
        .any(|key| !DOCUMENT_FIELDS.contains(&key.as_str()))
    {
        return Err(invalid("document", "document has an unsupported field."));
    }
    if let Some(caps) = object.get("caps") {
        let Some(map) = caps.as_object() else {
            return Err(invalid("caps", "caps must be an object."));
        };
        if map.len() > MAX_CAPS || !map.iter().all(|(key, value)| is_cap_key(key) && is_short_text(value)) {
            return Err(invalid(
                "caps",
                "caps must hold at most 32 lowercase keys with short string values.",
            ));
        }
    }
    for field in ["robots_max", "proxy_max"] {
        if let Some(value) = object.get(field) {
            if !is_short_text(value) {
                return Err(invalid(field, "must be a short string."));
            }
        }
    }
    if let Some(domains) = object.get("domains") {
        validate_domains(domains)?;
    }
    Ok(document.clone())
}

fn validate_domains(domains: &Value) -> Result<(), GatewayJsonResponse> {
    let Some(map) = domains.as_object() else {
        return Err(invalid("domains", "domains must be an object."));
    };
    if map.keys().any(|key| key != "allow" && key != "deny") {
        return Err(invalid("domains", "domains holds only allow and deny."));
    }
    let mut entries = 0usize;
    let mut bytes = 0usize;
    for list in ["allow", "deny"] {
        let Some(value) = map.get(list) else {
            continue;
        };
        let Some(items) = value.as_array() else {
            return Err(invalid("domains", "allow and deny must be arrays of host names."));
        };
        for item in items {
            let Some(host) = item
                .as_str()
                .filter(|host| !host.is_empty() && host.len() <= MAX_HOST_LENGTH)
            else {
                return Err(invalid("domains", "each host name must be a short string."));
            };
            entries += 1;
            bytes += host.len();
        }
    }
    if entries > MAX_DOMAIN_ENTRIES || bytes > MAX_DOMAIN_BYTES {
        return Err(invalid(
            "domains",
            "at most 64 host names across both lists, 2 KiB in total.",
        ));
    }
    Ok(())
}

/// A capability key: lower-case letters, digits, dots and underscores, as the catalogue names them.
fn is_cap_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_LENGTH
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_')
}

fn is_short_text(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| !text.is_empty() && text.len() <= MAX_VALUE_LENGTH)
}

#[cfg(test)]
mod tests {
    //! E3.2-g1 tests: the gateway proxy against a wiremock org-core. Every refusal is checked
    //! before org-core is reached, so a refused request leaves org-core with zero calls.
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use wiremock::matchers::{header, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const POLICY_PATH: &str = "/orgs/org-1/web-policy";

    fn user(org: &str, role: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some(role.to_owned()),
            active_org_id: Some(org.to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: org.to_owned(),
                role: role.to_owned(),
            }),
        }
    }

    fn state_for(org_core: &MockServer) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.org_core_url = org_core.uri();
        state
    }

    fn view() -> Value {
        json!({"data": {
            "policy": null,
            "effective": {"caps": {"browser.act.script": "disabled"}},
            "revision": 3,
            "rev": "abc123def456",
            "entitlements": {"web.paid_search": false},
            "updated_by": null,
            "updated_at": null,
        }})
    }

    fn document() -> Value {
        json!({
            "caps": {"browser.act.input": "disabled"},
            "domains": {"allow": ["example.com"], "deny": ["blocked.example.org"]},
        })
    }

    async fn org_core_calls(org_core: &MockServer) -> Vec<wiremock::Request> {
        org_core.received_requests().await.unwrap_or_default()
    }

    #[test]
    fn the_flag_reads_the_same_grammar_as_the_other_switches() {
        for on in ["1", "true", "yes", "on", " on "] {
            assert!(flag_is_on(Some(on)), "{on}");
        }
        for off in ["0", "false", "off", "", "2", "enabled"] {
            assert!(!flag_is_on(Some(off)), "{off}");
        }
        assert!(!flag_is_on(None));
    }

    #[tokio::test]
    async fn flag_off_answers_404_before_any_check_and_reaches_no_upstream() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        std::env::remove_var("BROWSER_POLICY_ADMIN_API");
        let org_core = MockServer::start().await;
        let state = state_for(&org_core);
        // A plain member on a foreign org: only the flag check can answer 404 here.
        let (status, Json(body)) = get_org_policy(&state, &user("org-2", "member"), "org-1").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "browser_policy_admin_disabled");
        let (status, _) = set_org_policy(&state, &user("org-1", "admin"), "org-1", 3, &document()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(org_core_calls(&org_core).await.is_empty());
    }

    #[tokio::test]
    async fn a_member_reads_the_view_and_is_told_they_cannot_manage_it() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        std::env::set_var("BROWSER_POLICY_ADMIN_API", "1");
        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(POLICY_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(view()))
            .mount(&org_core)
            .await;
        let state = state_for(&org_core);
        let (status, Json(body)) = get_org_policy(&state, &user("org-1", "member"), "org-1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["revision"], 3);
        assert_eq!(body["data"]["can_manage"], false);
        std::env::remove_var("BROWSER_POLICY_ADMIN_API");
    }

    #[tokio::test]
    async fn an_admin_reads_the_stored_document_with_can_manage_set() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        std::env::set_var("BROWSER_POLICY_ADMIN_API", "1");
        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(POLICY_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(view()))
            .mount(&org_core)
            .await;
        let state = state_for(&org_core);
        let (status, Json(body)) = get_org_policy(&state, &user("org-1", "admin"), "org-1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["can_manage"], true);
        assert_eq!(body["data"]["rev"], "abc123def456");
        std::env::remove_var("BROWSER_POLICY_ADMIN_API");
    }

    #[tokio::test]
    async fn a_member_cannot_save_and_org_core_is_not_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        std::env::set_var("BROWSER_POLICY_ADMIN_API", "1");
        let org_core = MockServer::start().await;
        let state = state_for(&org_core);
        let (status, Json(body)) =
            set_org_policy(&state, &user("org-1", "member"), "org-1", 3, &document()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "forbidden");
        assert!(org_core_calls(&org_core).await.is_empty());
        std::env::remove_var("BROWSER_POLICY_ADMIN_API");
    }

    #[tokio::test]
    async fn an_admin_save_sends_the_revision_as_if_match_and_only_the_document() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        std::env::set_var("BROWSER_POLICY_ADMIN_API", "1");
        let org_core = MockServer::start().await;
        Mock::given(wm_method("PUT"))
            .and(wm_path(POLICY_PATH))
            .and(header("if-match", "3"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": {"revision": 4, "rev": "fff000aaa111"}})),
            )
            .mount(&org_core)
            .await;
        let state = state_for(&org_core);
        let (status, Json(body)) =
            set_org_policy(&state, &user("org-1", "admin"), "org-1", 3, &document()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["revision"], 4);
        let calls = org_core_calls(&org_core).await;
        assert_eq!(calls.len(), 1);
        let sent: Value = serde_json::from_slice(&calls[0].body).unwrap();
        assert_eq!(sent, document());
        assert_eq!(
            calls[0].headers.get("x-org-id").and_then(|v| v.to_str().ok()),
            Some("org-1")
        );
        std::env::remove_var("BROWSER_POLICY_ADMIN_API");
    }

    #[tokio::test]
    async fn a_stale_revision_is_409_and_passes_through_unchanged() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        std::env::set_var("BROWSER_POLICY_ADMIN_API", "1");
        let org_core = MockServer::start().await;
        let conflict = json!({"error": {
            "code": "revision_conflict",
            "message": "the web policy changed since it was read",
            "details": {"current": 4},
        }});
        Mock::given(wm_method("PUT"))
            .and(wm_path(POLICY_PATH))
            .respond_with(ResponseTemplate::new(409).set_body_json(conflict.clone()))
            .mount(&org_core)
            .await;
        let state = state_for(&org_core);
        let (status, Json(body)) =
            set_org_policy(&state, &user("org-1", "owner"), "org-1", 3, &document()).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body, conflict);
        std::env::remove_var("BROWSER_POLICY_ADMIN_API");
    }

    #[tokio::test]
    async fn a_document_with_a_field_org_core_does_not_read_is_refused_before_org_core() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        std::env::set_var("BROWSER_POLICY_ADMIN_API", "1");
        let org_core = MockServer::start().await;
        let state = state_for(&org_core);
        let admin = user("org-1", "admin");
        let too_many_hosts: Vec<String> = (0..70).map(|i| format!("host{i}.example.com")).collect();
        let cases: Vec<(Value, &str)> = vec![
            (json!({"org_id": "org-2"}), "document"),
            (json!("not an object"), "document"),
            (json!({"caps": ["browser.act.input"]}), "caps"),
            (json!({"caps": {"Browser.Act": "disabled"}}), "caps"),
            (json!({"caps": {"browser.act.input": ""}}), "caps"),
            (json!({"robots_max": 3}), "robots_max"),
            (json!({"domains": {"allow": "example.com"}}), "domains"),
            (json!({"domains": {"allow": [1]}}), "domains"),
            (json!({"domains": {"wildcard": ["x.example"]}}), "domains"),
            (json!({"domains": {"allow": too_many_hosts}}), "domains"),
        ];
        for (document, field) in cases {
            let (status, Json(body)) = set_org_policy(&state, &admin, "org-1", 3, &document).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{document}");
            assert_eq!(body["error"]["code"], CODE_INVALID, "{document}");
            assert_eq!(body["error"]["details"]["field"], field, "{document}");
        }
        assert!(org_core_calls(&org_core).await.is_empty(), "no refused save reached org-core");
        std::env::remove_var("BROWSER_POLICY_ADMIN_API");
    }

    #[tokio::test]
    async fn a_negative_revision_and_a_foreign_or_malformed_org_are_refused() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        std::env::set_var("BROWSER_POLICY_ADMIN_API", "1");
        let org_core = MockServer::start().await;
        let state = state_for(&org_core);
        let (status, Json(body)) =
            set_org_policy(&state, &user("org-1", "admin"), "org-1", -1, &document()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["details"]["field"], "ifMatch");
        // The admin is an admin of org-1, not of org-2: the membership check refuses first.
        let (status, _) = get_org_policy(&state, &user("org-1", "admin"), "org-2").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = set_org_policy(&state, &user("org-1", "admin"), "org-2", 3, &document()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, Json(body)) = get_org_policy(&state, &user("../x", "owner"), "../x").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["details"]["field"], "orgId");
        assert!(org_core_calls(&org_core).await.is_empty());
        std::env::remove_var("BROWSER_POLICY_ADMIN_API");
    }
}
