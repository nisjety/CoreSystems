//! The org's web domain rules, answered to the BFF (EP-9, verification O-23, D-f10-3).
//!
//! The BFF's web search and crawl are not model calls, so the gateway's own tool loop never reaches them. The BFF cannot
//! hold an org-core scope of its own, so it asks this service, which already holds the checker the web tools use
//! ([`crate::site_policy_client::shared_checker`]) and the switch (`WEB_DOMAIN_POLICY_MODE`). Two routes:
//!
//! * `POST /v1/web/domain-check` takes up to 32 distinct hosts and answers `{ mode, verdicts: { host: allow | deny } }`.
//!   A host the rules refuse, a host with no canonical form and a host org-core cannot answer for are all `deny`.
//! * `POST /v1/web/domain-rules` answers the org's rules (`{ mode, rules: { mode, deny, allow, revision } }`) for a crawl
//!   to carry to every link it follows. A read that fails under `enforce` is a typed 503, so a crawl that cannot read the
//!   rules does not start.
//!
//! The org comes from the verified identity, never from the request. Under `off` nothing is read: the check answers
//! `allow` for every host and the rules answer carries none, and the BFF changes nothing. Under `shadow` the check reads
//! and logs what it would refuse and says `shadow`, so the caller refuses nothing. A waived token is refused as a route
//! mismatch, because a waiver binds one chat route on one connection. A zero-retention token is served: both routes
//! carry hosts and verdicts only, never page content, so there is nothing for the posture to protect.

use axum::{extract::State, http::StatusCode, Extension, Json};
use mp_contracts::site_policy::WebDomainPolicyMode;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::Claims,
    site_policy_client::{
        domain_verdicts, mode_label, policy_mode, shared_checker, SiteChecker, SiteOwner,
        MAX_DOMAIN_CHECK_HOSTS,
    },
    state::AppState,
};

type HttpJsonError = (StatusCode, Json<Value>);

/// The code of a domain-rules read that failed under `enforce`.
pub const RULES_UNREADABLE_CODE: &str = "org_site_rules_unreadable";

/// The code of a domain-check body that names too many hosts or none a string.
pub const BAD_HOSTS_CODE: &str = "invalid_hosts";

/// The body of a domain check.
#[derive(Debug, Deserialize)]
pub struct DomainCheckRequest {
    /// The hosts to check, at most [`MAX_DOMAIN_CHECK_HOSTS`] distinct ones.
    #[serde(default)]
    pub hosts: Vec<String>,
}

fn typed_error(status: StatusCode, code: &str, message: &str) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
}

/// The caller's posture on a route that carries hosts only. A waived token is a route mismatch.
fn posture_gate(claims: &Claims) -> Result<(), HttpJsonError> {
    crate::zdr_posture::zdr_or_refuse(claims, &crate::zdr_posture::Route::plain(false)).map(|_| ())
}

/// `POST /v1/web/domain-check`.
pub async fn domain_check(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<DomainCheckRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    posture_gate(&claims)?;
    domain_check_in(
        &state,
        shared_checker(),
        policy_mode(),
        &claims.org_id,
        req.hosts,
    )
    .await
    .map(Json)
}

/// The check under an explicit mode, so a test names the mode without the process switch.
pub(crate) async fn domain_check_in(
    state: &AppState,
    checker: &SiteChecker,
    mode: WebDomainPolicyMode,
    org_id: &str,
    hosts: Vec<String>,
) -> Result<Value, HttpJsonError> {
    let mut distinct: Vec<String> = Vec::new();
    for host in hosts {
        let host = host.trim().to_owned();
        if host.is_empty() || host.len() > 253 {
            return Err(typed_error(
                StatusCode::BAD_REQUEST,
                BAD_HOSTS_CODE,
                "every host must be a non-empty name of at most 253 characters",
            ));
        }
        if !distinct.contains(&host) {
            distinct.push(host);
        }
    }
    if distinct.len() > MAX_DOMAIN_CHECK_HOSTS {
        return Err(typed_error(
            StatusCode::BAD_REQUEST,
            BAD_HOSTS_CODE,
            "at most 32 distinct hosts may be checked in one call",
        ));
    }
    let verdicts = domain_verdicts(mode, checker, &SiteOwner::of(state), org_id, &distinct).await;
    Ok(json!({ "mode": mode_label(mode), "verdicts": verdicts }))
}

/// `POST /v1/web/domain-rules`. The body is ignored: the org is the verified identity's.
pub async fn domain_rules(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpJsonError> {
    posture_gate(&claims)?;
    domain_rules_in(&state, shared_checker(), policy_mode(), &claims.org_id)
        .await
        .map(Json)
}

/// The rules answer under an explicit mode.
pub(crate) async fn domain_rules_in(
    state: &AppState,
    checker: &SiteChecker,
    mode: WebDomainPolicyMode,
    org_id: &str,
) -> Result<Value, HttpJsonError> {
    // Under `off` and `shadow` a crawl is not bound by the rules, so nothing is read and no rule is carried.
    if mode != WebDomainPolicyMode::Enforce {
        return Ok(json!({ "mode": mode_label(mode), "rules": Value::Null }));
    }
    match checker.rules(&SiteOwner::of(state), org_id).await {
        Ok(rules) => Ok(json!({ "mode": mode_label(mode), "rules": rules })),
        Err(error) => {
            tracing::warn!(%org_id, %error, "org web domain rules cannot be read");
            Err(typed_error(
                StatusCode::SERVICE_UNAVAILABLE,
                RULES_UNREADABLE_CODE,
                "the organisation's web domain rules cannot be read right now",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;
    use mp_contracts::agentic_subscription::PROVIDER_ID;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::site_policy_client::{mode_label, test_seam, SiteChecker};

    /// A user token as Auth Core mints it, decoded by the same serde path the middleware uses.
    fn claims(org: &str, zdr: bool, waiver: Option<Value>) -> Claims {
        let mut payload = json!({
            "sub": "user-1",
            "iss": "https://auth.test/model",
            "aud": "model-gateway",
            "exp": 4_102_444_800_i64,
            "nbf": 1_700_000_000_i64,
            "org_id": org,
            "user_id": "user-1",
            "principal_type": "user",
            "zdr": zdr,
        });
        if let Some(waiver) = waiver {
            // A waiver is minted for an org whose standing posture is ZDR.
            payload["org_zdr"] = json!(true);
            payload["zdr_waiver"] = waiver;
        }
        serde_json::from_value(payload).expect("a minted token's payload is valid Claims")
    }

    fn state_for(org_core: &MockServer) -> AppState {
        let mut state = AppState::new();
        state.org_core_base_url = org_core.uri();
        state.org_core_service_token = "test-token".to_owned();
        state
    }

    async fn resolver(org: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/internal/orgs/{org}/browser-sites/resolve")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "organization_id": org,
                "revision": 3,
                "decisions": [
                    {"host": "allowed.example", "effective": "ask", "cap": "act", "source": "default"},
                    {"host": "blocked.example", "effective": "deny", "cap": "none", "source": "org_block"}
                ]
            })))
            .mount(&server)
            .await;
        server
    }

    async fn requests(server: &MockServer) -> usize {
        server.received_requests().await.unwrap_or_default().len()
    }

    fn hosts(list: &[&str]) -> Vec<String> {
        list.iter().map(|host| (*host).to_owned()).collect()
    }

    #[tokio::test]
    async fn enforce_denies_a_blocked_host_and_allows_the_rest() {
        let org = resolver("org-dc-1").await;
        let state = state_for(&org);
        let answer = domain_check_in(
            &state,
            &SiteChecker::default(),
            WebDomainPolicyMode::Enforce,
            "org-dc-1",
            hosts(&["allowed.example", "blocked.example", "10.0.0.1", "nodots"]),
        )
        .await
        .expect("answered");
        assert_eq!(answer["mode"], "enforce");
        assert_eq!(answer["verdicts"]["allowed.example"], "allow");
        assert_eq!(answer["verdicts"]["blocked.example"], "deny");
        assert_eq!(
            answer["verdicts"]["10.0.0.1"], "deny",
            "a host the rules cannot name is refused"
        );
        assert_eq!(answer["verdicts"]["nodots"], "deny");
    }

    #[tokio::test]
    async fn shadow_reads_the_rules_and_says_shadow() {
        let org = resolver("org-dc-2").await;
        let state = state_for(&org);
        let answer = domain_check_in(
            &state,
            &SiteChecker::default(),
            WebDomainPolicyMode::Shadow,
            "org-dc-2",
            hosts(&["blocked.example"]),
        )
        .await
        .expect("answered");
        assert_eq!(
            answer["mode"], "shadow",
            "the caller is told the verdicts do not bind"
        );
        assert_eq!(answer["verdicts"]["blocked.example"], "deny");
        assert_eq!(requests(&org).await, 1);
    }

    #[tokio::test]
    async fn off_reads_nothing_and_allows_every_host() {
        let org = resolver("org-dc-3").await;
        let state = state_for(&org);
        let answer = domain_check_in(
            &state,
            &SiteChecker::default(),
            WebDomainPolicyMode::Off,
            "org-dc-3",
            hosts(&["blocked.example"]),
        )
        .await
        .expect("answered");
        assert_eq!(answer["mode"], "off");
        assert_eq!(answer["verdicts"]["blocked.example"], "allow");
        assert_eq!(requests(&org).await, 0, "off makes no call to org-core");
    }

    #[tokio::test]
    async fn an_unreadable_org_core_denies_every_host() {
        let org = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&org)
            .await;
        let state = state_for(&org);
        let answer = domain_check_in(
            &state,
            &SiteChecker::default(),
            WebDomainPolicyMode::Enforce,
            "org-dc-4",
            hosts(&["allowed.example"]),
        )
        .await
        .expect("answered");
        assert_eq!(
            answer["verdicts"]["allowed.example"], "deny",
            "an outage narrows and never widens"
        );
    }

    #[tokio::test]
    async fn more_than_thirty_two_distinct_hosts_is_refused_and_duplicates_count_once() {
        let org = resolver("org-dc-5").await;
        let state = state_for(&org);
        let many: Vec<String> = (0..33).map(|n| format!("site{n:02}.example.com")).collect();
        let (status, Json(body)) = domain_check_in(
            &state,
            &SiteChecker::default(),
            WebDomainPolicyMode::Enforce,
            "org-dc-5",
            many,
        )
        .await
        .expect_err("33 distinct hosts are refused");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], BAD_HOSTS_CODE);
        assert_eq!(requests(&org).await, 0, "a refused list reads nothing");

        let repeated = vec!["allowed.example".to_owned(); 40];
        let answer = domain_check_in(
            &state,
            &SiteChecker::default(),
            WebDomainPolicyMode::Enforce,
            "org-dc-5",
            repeated,
        )
        .await
        .expect("one distinct host is within the limit");
        assert_eq!(
            answer["verdicts"].as_object().map(serde_json::Map::len),
            Some(1)
        );
    }

    #[tokio::test]
    async fn a_blank_host_is_refused() {
        let state = AppState::new();
        let (status, _) = domain_check_in(
            &state,
            &SiteChecker::default(),
            WebDomainPolicyMode::Enforce,
            "org-dc-6",
            hosts(&["  "]),
        )
        .await
        .expect_err("a blank host is refused");
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn the_org_is_the_verified_identitys_and_a_waived_token_is_a_route_mismatch() {
        let org = resolver("org-dc-7").await;
        let state = state_for(&org);
        let _mode = test_seam::set(WebDomainPolicyMode::Enforce);
        let ok = domain_check(
            State(state.clone()),
            Extension(claims("org-dc-7", false, None)),
            Json(DomainCheckRequest {
                hosts: hosts(&["blocked.example"]),
            }),
        )
        .await
        .expect("a persistent token is served");
        assert_eq!(ok.0["verdicts"]["blocked.example"], "deny");

        let waived = claims(
            "org-dc-7",
            false,
            Some(json!({"version": 1, "provider": PROVIDER_ID, "connection": "conn-1"})),
        );
        let refused = domain_check(
            State(state.clone()),
            Extension(waived.clone()),
            Json(DomainCheckRequest {
                hosts: hosts(&["blocked.example"]),
            }),
        )
        .await
        .expect_err("a waived token is refused");
        let body = to_bytes(refused.into_response().into_body(), usize::MAX)
            .await
            .expect("body");
        let body: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(body["error"]["code"], "zdr_waiver_route_mismatch");
        let rules = domain_rules(State(state), Extension(waived))
            .await
            .expect_err("refused too");
        assert_eq!(rules.0.as_u16() / 100, 4);
    }

    #[tokio::test]
    async fn a_zero_retention_token_is_served_with_hosts_only() {
        let org = resolver("org-dc-8").await;
        let state = state_for(&org);
        let _mode = test_seam::set(WebDomainPolicyMode::Enforce);
        let answer = domain_check(
            State(state),
            Extension(claims("org-dc-8", true, None)),
            Json(DomainCheckRequest {
                hosts: hosts(&["allowed.example"]),
            }),
        )
        .await
        .expect("a ZDR token is served: the call carries hosts and verdicts, no content");
        assert_eq!(answer.0["verdicts"]["allowed.example"], "allow");
        let received = org.received_requests().await.unwrap_or_default();
        assert_eq!(received.len(), 1);
        assert_eq!(
            received[0].url.query(),
            Some("host=allowed.example"),
            "only the canonical host is sent"
        );
    }

    async fn rules_server(org: &str, status: u16, body: Value) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/internal/orgs/{org}/browser-sites/rules")))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn enforce_answers_the_orgs_rules_as_deny_and_allow_hosts() {
        let org = rules_server(
            "org-dr-1",
            200,
            json!({"organization_id": "org-dr-1", "revision": 7, "mode": "allowlist", "rules": [
                {"host": "Blocked.Example", "rule": "block"},
                {"host": "docs.example", "rule": "read"},
                {"host": "app.example", "rule": "act", "note": "x"}
            ]}),
        )
        .await;
        let state = state_for(&org);
        let answer = domain_rules_in(
            &state,
            &SiteChecker::default(),
            WebDomainPolicyMode::Enforce,
            "org-dr-1",
        )
        .await
        .expect("read");
        assert_eq!(answer["mode"], "enforce");
        assert_eq!(answer["rules"]["mode"], "allowlist");
        assert_eq!(answer["rules"]["deny"], json!(["blocked.example"]));
        assert_eq!(
            answer["rules"]["allow"],
            json!(["app.example", "docs.example"])
        );
        assert_eq!(answer["rules"]["revision"], 7);
    }

    #[tokio::test]
    async fn an_open_org_with_no_rules_reads_as_open_and_an_unknown_mode_narrows() {
        let open = rules_server(
            "org-dr-2",
            200,
            json!({"organization_id": "org-dr-2", "revision": 0, "mode": "open", "rules": []}),
        )
        .await;
        let answer = domain_rules_in(
            &state_for(&open),
            &SiteChecker::default(),
            WebDomainPolicyMode::Enforce,
            "org-dr-2",
        )
        .await
        .expect("read");
        assert_eq!(answer["rules"]["mode"], "open");
        let odd = rules_server(
            "org-dr-3",
            200,
            json!({"organization_id": "org-dr-3", "revision": 2, "mode": "something-new", "rules": []}),
        )
        .await;
        let answer = domain_rules_in(
            &state_for(&odd),
            &SiteChecker::default(),
            WebDomainPolicyMode::Enforce,
            "org-dr-3",
        )
        .await
        .expect("read");
        assert_eq!(
            answer["rules"]["mode"], "allowlist",
            "an unknown mode narrows access"
        );
    }

    #[tokio::test]
    async fn rules_that_cannot_be_read_are_a_typed_503_under_enforce() {
        for (org_id, status, body) in [
            ("org-dr-4", 500, json!({"error": "down"})),
            (
                "org-dr-5",
                200,
                json!({"organization_id": "org-other", "revision": 1, "mode": "open", "rules": []}),
            ),
            (
                "org-dr-6",
                200,
                json!({"organization_id": "org-dr-6", "revision": 1, "mode": "open", "rules": [{"host": "bad host", "rule": "block"}]}),
            ),
            (
                "org-dr-7",
                200,
                json!({"organization_id": "org-dr-7", "revision": 1, "mode": "open", "rules": [{"host": "a.example", "rule": "mystery"}]}),
            ),
        ] {
            let org = rules_server(org_id, status, body).await;
            let (code, Json(body)) = domain_rules_in(
                &state_for(&org),
                &SiteChecker::default(),
                WebDomainPolicyMode::Enforce,
                org_id,
            )
            .await
            .expect_err("unreadable rules are refused");
            assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{org_id}");
            assert_eq!(body["error"]["code"], RULES_UNREADABLE_CODE, "{org_id}");
        }
        // org-core not configured at all.
        let (code, _) = domain_rules_in(
            &AppState::new(),
            &SiteChecker::default(),
            WebDomainPolicyMode::Enforce,
            "org-dr-8",
        )
        .await
        .expect_err("an unconfigured org-core is unreadable");
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn off_and_shadow_read_no_rules() {
        let org = rules_server("org-dr-9", 200, json!({"mode": "open", "rules": []})).await;
        for mode in [WebDomainPolicyMode::Off, WebDomainPolicyMode::Shadow] {
            let answer =
                domain_rules_in(&state_for(&org), &SiteChecker::default(), mode, "org-dr-9")
                    .await
                    .expect("answered");
            assert_eq!(answer["rules"], Value::Null);
            assert_eq!(answer["mode"], mode_label(mode));
        }
        assert_eq!(requests(&org).await, 0);
    }
}
