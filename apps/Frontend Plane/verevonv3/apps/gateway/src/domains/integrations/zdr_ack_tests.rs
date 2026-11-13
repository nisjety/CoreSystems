//! ZDR-Z-19c: the connection ZDR assessment and acknowledgement proxy, driven through the real router
//! (session middleware included). Off, the four routes are not mounted. On, Integration Core receives
//! the caller's own bearer and no internal key, only the closed query values and the five known body
//! fields, nothing for a bad connection id, and its typed refusals come back unchanged.

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::{
    matchers::{header as wm_header, method as wm_method, path as wm_path},
    Mock, MockServer, ResponseTemplate,
};

use crate::config::AppState;

const COOKIE: &str = "better-auth.session_token=session-1";
const INGESTION_TOKEN: &str = "ingestion-token-for-user-1";

struct Upstreams {
    auth: MockServer,
    user_core: MockServer,
    integration_core: MockServer,
}

impl Upstreams {
    async fn start() -> Self {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/ingestion/token"))
            .and(wm_header("cookie", COOKIE))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": INGESTION_TOKEN, "expiresInSeconds": 300
            })))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Self {
            auth,
            user_core,
            integration_core: MockServer::start().await,
        }
    }

    fn state(&self, enabled: bool) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = self.auth.uri();
        state.user_core_url = self.user_core.uri();
        state.integration_core_url = self.integration_core.uri();
        state.zdr_waiver_ui_enabled = enabled;
        state
    }

    async fn upstream(&self, method: &str, path: &str, response: ResponseTemplate) {
        Mock::given(wm_method(method))
            .and(wm_path(path))
            .respond_with(response)
            .mount(&self.integration_core)
            .await;
    }

    async fn integration_requests(&self) -> Vec<wiremock::Request> {
        self.integration_core
            .received_requests()
            .await
            .expect("recording on")
    }
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

const ROUTES: [(&str, &str); 4] = [
    ("GET", "/api/v1/model-connections/conn-1/zdr"),
    (
        "POST",
        "/api/v1/model-connections/conn-1/zdr-acknowledgement",
    ),
    (
        "DELETE",
        "/api/v1/model-connections/conn-1/zdr-acknowledgement",
    ),
    ("GET", "/api/v1/model-connections/zdr-acknowledgements"),
];

#[tokio::test]
async fn with_the_switch_off_every_acknowledgement_route_is_404() {
    let upstreams = Upstreams::start().await;
    for (method, uri) in ROUTES {
        let body = (method == "POST").then(|| json!({ "digest": "d" }));
        let (status, _) = send(upstreams.state(false), method, uri, body).await;
        assert_eq!(status, 404, "{method} {uri}");
    }
    assert!(upstreams.integration_requests().await.is_empty());
}

#[tokio::test]
async fn with_the_switch_on_every_route_forwards_the_callers_own_bearer_and_no_internal_key() {
    let upstreams = Upstreams::start().await;
    for (method, path) in [
        ("GET", "/api/v1/model-connections/conn-1/zdr"),
        (
            "POST",
            "/api/v1/model-connections/conn-1/zdr-acknowledgement",
        ),
        (
            "DELETE",
            "/api/v1/model-connections/conn-1/zdr-acknowledgement",
        ),
        ("GET", "/api/v1/model-connections/zdr-acknowledgements"),
    ] {
        upstreams
            .upstream(
                method,
                path,
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "success": true, "data": { "ok": true } })),
            )
            .await;
    }
    for (method, uri) in ROUTES {
        let body = (method == "POST").then(|| json!({ "digest": "d" }));
        let (status, body) = send(upstreams.state(true), method, uri, body).await;
        assert_eq!(status, 200, "{method} {uri}: {body}");
        assert_eq!(body["data"]["ok"], true, "{method} {uri}");
    }
    let requests = upstreams.integration_requests().await;
    assert_eq!(requests.len(), 4);
    for request in requests {
        assert_eq!(
            request
                .headers
                .get("authorization")
                .and_then(|v| v.to_str().ok()),
            Some(format!("Bearer {INGESTION_TOKEN}").as_str()),
            "{}",
            request.url
        );
        for internal in ["x-internal-api-key", "x-service-api-key", "x-service-token"] {
            assert!(
                request.headers.get(internal).is_none(),
                "{} carried {internal}",
                request.url
            );
        }
    }
}

#[tokio::test]
async fn out_of_set_query_values_and_unknown_body_fields_are_dropped() {
    let upstreams = Upstreams::start().await;
    upstreams
        .upstream(
            "GET",
            "/api/v1/model-connections/conn-1/zdr",
            ResponseTemplate::new(200).set_body_json(json!({ "success": true, "data": {} })),
        )
        .await;
    upstreams
        .upstream(
            "POST",
            "/api/v1/model-connections/conn-1/zdr-acknowledgement",
            ResponseTemplate::new(200).set_body_json(json!({ "success": true, "data": {} })),
        )
        .await;
    upstreams
        .upstream(
            "GET",
            "/api/v1/model-connections/zdr-acknowledgements",
            ResponseTemplate::new(200).set_body_json(json!({ "success": true, "data": [] })),
        )
        .await;

    let state = upstreams.state(true);
    send(
        state.clone(),
        "GET",
        "/api/v1/model-connections/conn-1/zdr?locale=en&variant=org_zdr_on&extra=1",
        None,
    )
    .await;
    send(
        state.clone(),
        "GET",
        "/api/v1/model-connections/conn-1/zdr?locale=de&variant=everything",
        None,
    )
    .await;
    send(
        state.clone(),
        "POST",
        "/api/v1/model-connections/conn-1/zdr-acknowledgement",
        Some(json!({
            "digest": "a".repeat(64),
            "textVersion": "3",
            "locale": "no",
            "variant": "first_use",
            "acknowledgedVia": "connection_settings",
            "userId": "someone-else",
            "connection_id": "conn-2",
        })),
    )
    .await;
    send(
        state.clone(),
        "GET",
        "/api/v1/model-connections/zdr-acknowledgements?limit=500",
        None,
    )
    .await;
    send(
        state,
        "GET",
        "/api/v1/model-connections/zdr-acknowledgements?limit=50&org=other",
        None,
    )
    .await;

    let requests = upstreams.integration_requests().await;
    let queries: Vec<Option<String>> = requests
        .iter()
        .filter(|request| request.method.as_str() == "GET")
        .map(|request| request.url.query().map(str::to_owned))
        .collect();
    assert_eq!(
        queries,
        vec![
            Some("locale=en&variant=org_zdr_on".to_owned()),
            None,
            None,
            Some("limit=50".to_owned()),
        ]
    );
    let posted = requests
        .iter()
        .find(|request| request.method.as_str() == "POST")
        .expect("the acknowledgement was forwarded");
    let forwarded: Value = serde_json::from_slice(&posted.body).expect("json");
    assert_eq!(
        forwarded,
        json!({
            "digest": "a".repeat(64),
            "textVersion": "3",
            "locale": "no",
            "variant": "first_use",
            "acknowledgedVia": "connection_settings",
        })
    );
}

#[tokio::test]
async fn a_bad_connection_id_is_400_without_an_upstream_call() {
    let upstreams = Upstreams::start().await;
    let long = "c".repeat(129);
    for id in ["conn%201", "conn%2F1", long.as_str(), "%20"] {
        for (method, suffix) in [
            ("GET", "zdr"),
            ("POST", "zdr-acknowledgement"),
            ("DELETE", "zdr-acknowledgement"),
        ] {
            let uri = format!("/api/v1/model-connections/{id}/{suffix}");
            let body = (method == "POST").then(|| json!({ "digest": "d" }));
            let (status, body) = send(upstreams.state(true), method, &uri, body).await;
            assert_eq!(status, 400, "{method} {uri}");
            assert_eq!(body["error"]["code"], "invalid_request", "{method} {uri}");
        }
    }
    assert!(upstreams.integration_requests().await.is_empty());
}

#[tokio::test]
async fn integration_cores_typed_refusals_come_back_unchanged() {
    for (status, code, method, path) in [
        (
            403,
            "not_connection_owner",
            "POST",
            "/api/v1/model-connections/conn-1/zdr-acknowledgement",
        ),
        (
            409,
            "zdr_assessment_changed",
            "POST",
            "/api/v1/model-connections/conn-1/zdr-acknowledgement",
        ),
        (
            503,
            "zdr_org_state_unavailable",
            "GET",
            "/api/v1/model-connections/conn-1/zdr",
        ),
    ] {
        let upstreams = Upstreams::start().await;
        let refusal = json!({
            "success": false,
            "error": { "code": code, "message": "refused", "details": {} },
        });
        upstreams
            .upstream(
                method,
                path,
                ResponseTemplate::new(status).set_body_json(refusal.clone()),
            )
            .await;
        let body = (method == "POST").then(|| json!({ "digest": "d" }));
        let (answered, answer) = send(upstreams.state(true), method, path, body).await;
        assert_eq!(answered, status, "{code}");
        assert_eq!(answer, refusal, "{code}");
    }
}
