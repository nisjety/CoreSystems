//! 3.4: the BFF routes this phase added behind a switch, driven through the real router (session
//! middleware included) with wiremock Auth Core, user-core, org-core and every upstream they relay
//! to (model-gateway, documents-api, integration-core).
//!
//! * Switch off: every route answers 404 with its own disabled code and nothing reaches an
//!   upstream. The switch is the first refusal each module documents ("each step is a refusal
//!   before the next one runs"), so a malformed id or body is not looked at either.
//! * CHAT_MEDIA_TRANSCRIPTS_ENABLED on, ZDR org: the transcript route refuses before the recording
//!   leaves the gateway, with a typed refusal from the ZDR catalogue (project rule: ZDR fails closed
//!   with typed refusals from the catalogue).

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path, path_regex},
    Mock, MockServer, ResponseTemplate,
};

use crate::config::AppState;

const COOKIE: &str = "better-auth.session_token=session-1";

struct Upstreams {
    auth: MockServer,
    user_core: MockServer,
    org_core: MockServer,
    model_gateway: MockServer,
    documents: MockServer,
    integration: MockServer,
}

impl Upstreams {
    async fn start(org_zdr: bool) -> Self {
        let auth = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/[a-z-]+/token$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "plain-token", "expiresInSeconds": 300,
            })))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "owner",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let org_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {
                "id": "org-1",
                "metadata": { "interactiveRetention": { "zdr": org_zdr } },
                "retention_posture": { "effective_zdr": org_zdr },
            }})))
            .mount(&org_core)
            .await;
        let model_gateway = MockServer::start().await;
        let documents = MockServer::start().await;
        let integration = MockServer::start().await;
        for server in [&model_gateway, &documents, &integration] {
            Mock::given(wiremock::matchers::any())
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {} })))
                .mount(server)
                .await;
        }
        Self {
            auth,
            user_core,
            org_core,
            model_gateway,
            documents,
            integration,
        }
    }

    /// Every phase switch of these routes off, as `test_state` leaves them.
    fn state(&self) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = self.auth.uri();
        state.user_core_url = self.user_core.uri();
        state.org_core_url = self.org_core.uri();
        state.model_gateway_url = self.model_gateway.uri();
        state.documents_api_url = self.documents.uri();
        state.integration_core_url = self.integration.uri();
        state
    }

    /// The relays' upstreams saw nothing.
    async fn relayed(&self) -> usize {
        let mut count = 0;
        for server in [&self.model_gateway, &self.documents, &self.integration] {
            count += server
                .received_requests()
                .await
                .expect("recording on")
                .len();
        }
        count
    }
}

async fn send(state: AppState, verb: &str, uri: &str, body: Option<&str>) -> (u16, Value) {
    let mut request = Request::builder()
        .method(verb)
        .uri(uri)
        .header("cookie", COOKIE);
    let body = match body {
        Some(body) => {
            request = request.header("content-type", "application/json");
            Body::from(body.to_owned())
        }
        None => Body::empty(),
    };
    let response = crate::build_router(state)
        .oneshot(request.body(body).expect("request"))
        .await
        .expect("response");
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

const TRANSCRIPT_BODY: &str = r#"{"audio_base64":"AAAA","mime":"audio/webm","title":"t"}"#;

/// (verb, uri, body, disabled code): well-formed requests to every route of the four switches.
fn well_formed_routes() -> Vec<(
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
)> {
    vec![
        (
            "POST",
            "/api/v1/chat/threads/thread-1/snapshots",
            None,
            "chat_snapshots_disabled",
        ),
        (
            "GET",
            "/api/v1/chat/threads/thread-1/snapshots",
            None,
            "chat_snapshots_disabled",
        ),
        (
            "DELETE",
            "/api/v1/chat/snapshots/snap-1",
            None,
            "chat_snapshots_disabled",
        ),
        (
            "POST",
            "/api/v1/chat/snapshots/snap-1/grants",
            Some(r#"{"grantee_user_id":"user-2"}"#),
            "chat_snapshots_disabled",
        ),
        (
            "DELETE",
            "/api/v1/chat/snapshots/snap-1/grants",
            Some(r#"{"grantee_user_id":"user-2"}"#),
            "chat_snapshots_disabled",
        ),
        (
            "GET",
            "/api/v1/chat/snapshots/snap-1/grants",
            None,
            "chat_snapshots_disabled",
        ),
        (
            "GET",
            "/api/v1/chat/shared-snapshots/snap-1",
            None,
            "chat_snapshots_disabled",
        ),
        (
            "GET",
            "/api/v1/chat/runs/run-1/processes/p-1/output?space_ref=space-1",
            None,
            "process_output_disabled",
        ),
        (
            "POST",
            "/api/v1/chat/transcripts",
            Some(TRANSCRIPT_BODY),
            "media_transcripts_disabled",
        ),
        (
            "GET",
            "/api/v1/credential-bindings",
            None,
            "credential_bindings_disabled",
        ),
        (
            "POST",
            "/api/v1/credential-bindings",
            Some(r#"{"kind":"header","host":"api.example.com","secret":"s"}"#),
            "credential_bindings_disabled",
        ),
        (
            "DELETE",
            "/api/v1/credential-bindings/cb_1",
            None,
            "credential_bindings_disabled",
        ),
        (
            "POST",
            "/api/v1/connectors/custom-rest",
            Some(r#"{"name":"n","credential_binding_id":"cb_1"}"#),
            "credential_bindings_disabled",
        ),
    ]
}

#[tokio::test]
async fn with_every_switch_off_each_added_route_is_404_with_its_code_and_nothing_upstream() {
    for (verb, uri, body, code) in well_formed_routes() {
        let upstreams = Upstreams::start(false).await;
        let (status, answered) = send(upstreams.state(), verb, uri, body).await;
        assert_eq!(status, 404, "{verb} {uri}: {answered}");
        assert_eq!(answered["error"]["code"], code, "{verb} {uri}: {answered}");
        assert_eq!(
            upstreams.relayed().await,
            0,
            "{verb} {uri}: nothing relayed"
        );
    }
}

/// The switch is step 1 of every module, so with it off the route does not look at the request:
/// a malformed id or body gets the same disabled answer as a well-formed one.
#[tokio::test]
async fn with_every_switch_off_a_malformed_request_is_the_same_disabled_404() {
    for (verb, uri, body, code) in [
        (
            "POST",
            "/api/v1/chat/threads/bad%20id/snapshots",
            None,
            "chat_snapshots_disabled",
        ),
        (
            "DELETE",
            "/api/v1/chat/snapshots/bad%20id",
            None,
            "chat_snapshots_disabled",
        ),
        (
            "POST",
            "/api/v1/chat/snapshots/snap-1/grants",
            Some(r#"{"grantee_user_id":"user-2","org_id":"org-2"}"#),
            "chat_snapshots_disabled",
        ),
        (
            "GET",
            "/api/v1/chat/shared-snapshots/bad%20id",
            None,
            "chat_snapshots_disabled",
        ),
        (
            "GET",
            "/api/v1/chat/runs/bad%20id/processes/p-1/output",
            None,
            "process_output_disabled",
        ),
        (
            "POST",
            "/api/v1/chat/transcripts",
            Some("not json"),
            "media_transcripts_disabled",
        ),
        (
            "DELETE",
            "/api/v1/credential-bindings/not-a-binding",
            None,
            "credential_bindings_disabled",
        ),
    ] {
        let upstreams = Upstreams::start(false).await;
        let (status, answered) = send(upstreams.state(), verb, uri, body).await;
        assert_eq!(status, 404, "{verb} {uri}: {answered}");
        assert_eq!(answered["error"]["code"], code, "{verb} {uri}: {answered}");
        assert_eq!(
            upstreams.relayed().await,
            0,
            "{verb} {uri}: nothing relayed"
        );
    }
}

/// A ZDR org's transcript request is refused before the recording leaves the gateway, and the
/// refusal is one of the catalogue's typed codes, with the catalogue's retryable flag and action.
#[tokio::test]
async fn a_zdr_transcript_is_refused_with_a_catalogue_code_before_anything_is_sent() {
    let catalogue: Value = serde_json::from_str(include_str!(
        "../../../../../src/shared/api/contracts/zdr-refusals.json"
    ))
    .expect("catalogue json");
    let codes: Vec<&Value> = catalogue["codes"]
        .as_array()
        .expect("codes")
        .iter()
        .collect();

    for (org_zdr, header) in [(true, None), (false, Some("true"))] {
        let upstreams = Upstreams::start(org_zdr).await;
        let mut state = upstreams.state();
        state.space_features.chat_media_transcripts = true;
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/v1/chat/transcripts")
            .header("cookie", COOKIE)
            .header("content-type", "application/json");
        if let Some(value) = header {
            request = request.header("x-zdr", value);
        }
        let response = crate::build_router(state)
            .oneshot(request.body(Body::from(TRANSCRIPT_BODY)).expect("request"))
            .await
            .expect("response");
        let status = response.status().as_u16();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let answered: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert_eq!(status, 412, "org_zdr={org_zdr}: {answered}");
        assert_eq!(
            upstreams.relayed().await,
            0,
            "org_zdr={org_zdr}: nothing sent"
        );
        let code = answered["error"]["code"].as_str().unwrap_or_default();
        let entry = codes
            .iter()
            .find(|entry| entry["code"] == code)
            .unwrap_or_else(|| {
                panic!("org_zdr={org_zdr}: {code} is not a ZDR catalogue code: {answered}")
            });
        assert_eq!(answered["error"]["retryable"], entry["retryable"], "{code}");
        assert_eq!(answered["error"]["action"], entry["action"], "{code}");
    }
}

/// An organisation whose posture cannot be read is an outage, not a posture: the transcript is refused before anything is
/// sent, with the catalogue's retryable `zdr_decision_unavailable` (503), not a claim that the organisation is ZDR.
#[tokio::test]
async fn a_transcript_whose_org_posture_cannot_be_read_is_refused_as_decision_unavailable() {
    let upstreams = Upstreams::start(false).await;
    let org_core_down = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/organizations/org-1"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&org_core_down)
        .await;
    let mut state = upstreams.state();
    state.org_core_url = org_core_down.uri();
    state.space_features.chat_media_transcripts = true;

    let (status, answered) = send(
        state,
        "POST",
        "/api/v1/chat/transcripts",
        Some(TRANSCRIPT_BODY),
    )
    .await;
    assert_eq!(status, 503, "{answered}");
    assert_eq!(
        answered["error"]["code"], "zdr_decision_unavailable",
        "{answered}"
    );
    assert_eq!(answered["error"]["retryable"], true, "{answered}");
    assert_eq!(answered["error"]["action"], "retry", "{answered}");
    assert_eq!(upstreams.relayed().await, 0, "nothing sent");
}
