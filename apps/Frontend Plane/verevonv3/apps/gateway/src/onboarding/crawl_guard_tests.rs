//! The onboarding crawl starts under the org's web domain rules (P2-bff-01 rel1, EP-9 O-23).
//!
//! The website ingest and the crawl preview start a crawl from the onboarding step. Under `WEB_DOMAIN_POLICY_BFF`, with
//! Model Gateway enforcing, their seed host is judged before anything is fetched, and the org's rules ride on the crawl
//! for Quarry to apply to every link it follows. Each test drives the real handler against a stub Quarry and a stub
//! Model Gateway. Auth-core is unroutable, so the calls go out without a token, which the stubs do not check.

use axum::{
    body::to_bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use crate::{
    config::AppState,
    contracts::{CrawlPreviewRequest, WebsiteIngestRequest},
    domains::web_domain_policy::{BLOCKED_CODE, CHECK_ROUTE, RULES_ROUTE, UNAVAILABLE_CODE},
    middleware::{AuthenticatedUser, AuthorizedMembership},
    onboarding::{actions::start_website_ingest, crawl_preview::crawl_preview},
};

const JOB_ID: &str = "job_crawl_guard_0001";
const BLOCKED: &str = "blocked.example";
const ALLOWED: &str = "good.example";

/// The job's event log: a completed run, then the end-of-drain marker Quarry sends.
const JOB_EVENTS: &str = r#"event: run_completed
data: {"seq":1,"type":"run_completed","payload":{"pages_failed":0,"pages_visited":0}}

data: done

"#;

/// The domain rules the org holds: `BLOCKED` is denied, and `ALLOWED` is the allowlist.
fn org_rules() -> Value {
    json!({ "mode": "allowlist", "deny": [BLOCKED], "allow": [ALLOWED] })
}

/// Model Gateway's host check under enforcement: `BLOCKED` is denied and every other host is allowed.
struct EnforcingCheck;

impl Respond for EnforcingCheck {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or_default();
        let verdicts: serde_json::Map<String, Value> = body["hosts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|host| {
                let verdict = if host == BLOCKED { "deny" } else { "allow" };
                (host.to_owned(), json!(verdict))
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({ "mode": "enforce", "verdicts": verdicts }))
    }
}

/// Model Gateway under enforcement: the host check denies `BLOCKED`, and the org's rules are `org_rules()`.
async fn enforcing_gateway() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(CHECK_ROUTE))
        .respond_with(EnforcingCheck)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(RULES_ROUTE))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "mode": "enforce",
            "rules": { "mode": "allowlist", "deny": [BLOCKED], "allow": [ALLOWED], "revision": 4 },
        })))
        .mount(&server)
        .await;
    server
}

/// Model Gateway whose host check cannot be answered, so the org's rules cannot be read.
async fn unreadable_gateway() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(CHECK_ROUTE))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    server
}

/// Quarry-edge: the crawl handoff takes the job, the seed scrape streams nothing, and the job's event log ends with a
/// completed run, so a preview that starts ends on its own.
async fn quarry_edge() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/crawl"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "job_id": JOB_ID } })),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/scrape/stream"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("")
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/jobs/{JOB_ID}/events")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(JOB_EVENTS)
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;
    server
}

/// The gateway's test state, with Quarry and Model Gateway at the stubs and the switch as given.
fn state(quarry: &MockServer, gateway: &MockServer, switch: bool) -> AppState {
    let mut state = crate::tests::test_state(false);
    state.quarry_edge_url = quarry.uri();
    state.model_gateway_url = gateway.uri();
    state.space_features.web_domain_policy_bff = switch;
    state
}

/// A signed-in member of the org the rules belong to.
fn user() -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: "user-crawl-guard".to_owned(),
        user_email: "user@crawl-guard.invalid".to_owned(),
        user_name: "User".to_owned(),
        user_image: None,
        email_verified: true,
        auth_role: Some("member".to_owned()),
        active_org_id: Some("org-crawl-guard".to_owned()),
        authorized_membership: Some(AuthorizedMembership {
            organization_id: "org-crawl-guard".to_owned(),
            role: "member".to_owned(),
        }),
    }
}

/// The requests `server` received on `route`, in order.
async fn received(server: &MockServer, route: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|request| request.url.path() == route)
        .collect()
}

/// The JSON bodies Quarry received on `POST /v1/crawl`, in order.
async fn crawl_bodies(quarry: &MockServer) -> Vec<Value> {
    received(quarry, "/v1/crawl")
        .await
        .iter()
        .map(|request| serde_json::from_slice(&request.body).expect("a JSON crawl body"))
        .collect()
}

/// How many requests of any kind `server` received.
async fn calls(server: &MockServer) -> usize {
    server.received_requests().await.unwrap_or_default().len()
}

/// The JSON body of a response.
async fn body_of(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("the response body");
    serde_json::from_slice(&bytes).expect("a JSON body")
}

/// Runs the crawl preview for `url` and returns its SSE frames as `(event name, data)`, in stream order.
async fn preview(state: AppState, url: &str) -> Vec<(String, Value)> {
    let response = crawl_preview(
        State(state),
        Extension(user()),
        HeaderMap::new(),
        Json(CrawlPreviewRequest {
            url: url.to_owned(),
            _brief: None,
            max_pages: Some(4),
        }),
    )
    .await
    .into_response();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("the preview body");
    let text = String::from_utf8_lossy(&bytes);
    text.split("\n\n")
        .filter_map(|frame| {
            let name = frame.lines().find_map(|line| line.strip_prefix("event:"))?;
            let data = frame.lines().find_map(|line| line.strip_prefix("data:"))?;
            Some((
                name.trim().to_owned(),
                serde_json::from_str::<Value>(data.trim()).ok()?,
            ))
        })
        .collect()
}

/// Runs the website ingest for `url`, with the default page count when `max_pages` is `None`.
async fn website(state: AppState, url: &str, max_pages: Option<u32>) -> axum::response::Response {
    start_website_ingest(
        State(state),
        Extension(user()),
        HeaderMap::new(),
        Json(WebsiteIngestRequest {
            url: url.to_owned(),
            max_pages,
        }),
    )
    .await
    .into_response()
}

#[tokio::test]
async fn the_website_ingest_refuses_a_denied_seed_before_quarry_starts_a_crawl() {
    let quarry = quarry_edge().await;
    let gateway = enforcing_gateway().await;
    let response = website(
        state(&quarry, &gateway, true),
        &format!("https://{BLOCKED}/start"),
        Some(5),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(body_of(response).await["error"]["code"], BLOCKED_CODE);
    assert!(
        received(&quarry, "/v1/crawl").await.is_empty(),
        "a denied seed starts no crawl"
    );
}

#[tokio::test]
async fn the_website_crawl_carries_the_org_rules_for_quarry_to_apply_to_every_link() {
    let quarry = quarry_edge().await;
    let gateway = enforcing_gateway().await;
    let url = format!("https://{ALLOWED}/");
    let response = website(state(&quarry, &gateway, true), &url, Some(50)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        crawl_bodies(&quarry).await,
        vec![json!({
            "url": url,
            "max_pages": 20,
            "max_depth": 1,
            "ingest": true,
            "domain_rules": org_rules(),
        })]
    );
}

#[tokio::test]
async fn the_website_ingest_is_what_it_was_with_the_switch_off() {
    let quarry = quarry_edge().await;
    let gateway = enforcing_gateway().await;
    let url = format!("https://{BLOCKED}/start");
    let response = website(state(&quarry, &gateway, false), &url, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        crawl_bodies(&quarry).await,
        vec![json!({
            "url": url,
            "max_pages": 8,
            "max_depth": 1,
            "ingest": true,
        })]
    );
    assert_eq!(calls(&gateway).await, 0, "off asks Model Gateway nothing");
}

#[tokio::test]
async fn the_crawl_preview_refuses_a_denied_seed_before_it_scrapes_or_crawls() {
    let quarry = quarry_edge().await;
    let gateway = enforcing_gateway().await;
    let url = format!("https://{BLOCKED}/start");
    let frames = preview(state(&quarry, &gateway, true), &url).await;
    let names: Vec<&str> = frames.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["warning", "done"]);
    assert_eq!(frames[0].1["code"], BLOCKED_CODE);
    assert_eq!(frames[1].1["status"], "failed");
    assert!(
        received(&quarry, "/v1/scrape/stream").await.is_empty(),
        "the denied seed is not fetched"
    );
    assert!(
        received(&quarry, "/v1/crawl").await.is_empty(),
        "no crawl job starts"
    );
}

#[tokio::test]
async fn the_crawl_preview_starts_nothing_when_the_rules_cannot_be_read() {
    let quarry = quarry_edge().await;
    let gateway = unreadable_gateway().await;
    let url = format!("https://{ALLOWED}/");
    let frames = preview(state(&quarry, &gateway, true), &url).await;
    let names: Vec<&str> = frames.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["warning", "done"]);
    assert_eq!(frames[0].1["code"], UNAVAILABLE_CODE);
    assert_eq!(frames[1].1["status"], "failed");
    assert!(received(&quarry, "/v1/scrape/stream").await.is_empty());
    assert!(received(&quarry, "/v1/crawl").await.is_empty());
}

#[tokio::test]
async fn the_crawl_preview_carries_the_rules_and_still_scrapes_an_allowed_seed() {
    let quarry = quarry_edge().await;
    let gateway = enforcing_gateway().await;
    let url = format!("https://{ALLOWED}/");
    let frames = preview(state(&quarry, &gateway, true), &url).await;
    assert_eq!(frames[0].0, "started");
    assert_eq!(
        crawl_bodies(&quarry).await,
        vec![json!({
            "url": url,
            "max_pages": 4,
            "max_depth": 1,
            "domain_rules": org_rules(),
        })]
    );
    assert_eq!(received(&quarry, "/v1/scrape/stream").await.len(), 1);
}

#[tokio::test]
async fn the_crawl_preview_is_what_it_was_with_the_switch_off() {
    let quarry = quarry_edge().await;
    let gateway = enforcing_gateway().await;
    let url = format!("https://{BLOCKED}/start");
    let frames = preview(state(&quarry, &gateway, false), &url).await;
    assert_eq!(frames[0].0, "started");
    assert_eq!(
        crawl_bodies(&quarry).await,
        vec![json!({
            "url": url,
            "max_pages": 4,
            "max_depth": 1,
        })]
    );
    assert_eq!(received(&quarry, "/v1/scrape/stream").await.len(), 1);
    assert_eq!(calls(&gateway).await, 0, "off asks Model Gateway nothing");
}
