use super::*;
use serde_json::json;

const QUERY: &str =
    "websiteId=web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&site=https%3A%2F%2Ftriodelab.no%2F";
const POST: &[u8] = br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7}"#;

#[test]
fn public_provider_timeout_provenance_survives_projection_without_raw_errors() {
    let read = route_spec("public-signals", &Method::GET).unwrap();
    let safe = projection::sanitize(&read, &json!({
        "source":"marketing_public_signals", "status":"complete",
        "report":{
            "crux":[{"formFactor":"PHONE", "status":"unavailable", "reason":"timeout", "rawError":"secret"}],
            "commonCrawl":{"status":"unavailable", "reason":"timeout", "index":"CC-MAIN-2026-43", "captures":[], "limit":20, "truncated":false, "rawResponse":"secret"}
        }
    })).unwrap();
    assert_eq!(safe["report"]["crux"][0]["reason"], "timeout");
    assert_eq!(safe["report"]["commonCrawl"]["reason"], "timeout");
    assert_eq!(safe["report"]["commonCrawl"]["index"], "CC-MAIN-2026-43");
    assert!(!safe.to_string().contains("secret"));
}

#[test]
fn public_signals_exact_admin_contract_and_deep_projection() {
    let read = route_spec("public-signals", &Method::GET).expect("public signals route");
    assert!(!read.administrative);
    assert!(validate_query(&read, QUERY).is_ok());
    for extra in [
        "&org=other",
        "&site=https://other.no/",
        "&quarter=Q4",
        "&apiKey=secret",
    ] {
        assert!(validate_query(&read, &format!("{QUERY}{extra}")).is_err());
    }
    let post = route_spec("public-signals", &Method::POST).unwrap();
    assert!(post.administrative);
    assert!(validate_command(&post, POST).is_ok());
    assert!(validate_command(&post, br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"apiKey":"secret"}"#).is_err());
    let patch = route_spec("public-signals", &Method::PATCH).unwrap();
    assert!(patch.administrative);
    assert!(validate_command(&patch, POST).is_err());
    assert!(validate_command(&patch, br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"enabled":false}"#).is_ok());
    assert!(route_spec("public-signals", &Method::DELETE).is_none());
    let safe = projection::sanitize(&read, &json!({
        "source":"marketing_public_signals","status":"complete","enabled":true,"organizationId":"secret",
        "report":{"site":"https://triodelab.no/","source":"public_web_signals","observedAt":"2026-10-23T00:00:00Z","apiKey":"secret",
          "crux":[{"formFactor":"PHONE","status":"connected","reportingOrigin":"https://www.triodelab.no","period":{"startDate":"2026-09-24","endDate":"2026-10-21","token":"secret"},"metrics":{"lcpMs":1234,"inpMs":100,"cls":0.1,"raw":"secret"}}],
          "commonCrawl":{"status":"connected","index":"CC-MAIN-2026-43","limit":20,"truncated":false,"captures":[{"url":"https://triodelab.no/","capturedAt":"2026-10-21T00:00:00Z","httpStatus":200,"filename":"secret"}]}}
    })).unwrap();
    assert_eq!(safe["report"]["crux"][0]["metrics"]["lcpMs"], 1234);
    assert_eq!(safe["report"]["crux"][0]["reportingOrigin"], "https://www.triodelab.no");
    assert_eq!(
        safe["report"]["commonCrawl"]["captures"][0]["httpStatus"],
        200
    );
    assert!(!safe.to_string().contains("secret"));
}

#[tokio::test]
async fn public_signals_scopes_admin_writes_and_drops_browser_credentials() {
    use axum::body::to_bytes;
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/marketing/public-signals"))
        .and(header("x-internal-api-key", "public-server-secret"))
        .respond_with(
            ResponseTemplate::new(202)
                .set_body_json(json!({"data":{"status":"queued","connectionId":"private"}})),
        )
        .expect(1)
        .mount(&upstream)
        .await;
    let mut state = crate::tests::test_state(false);
    state.marketing_core_url = upstream.uri();
    state.marketing_core_organization_id = "org-one".into();
    state.marketing_core_service_key = "public-server-secret".into();
    let user = |org: &str, role: &str| AuthenticatedUser {
        user_id: "public-user".into(),
        user_email: "user@example.test".into(),
        user_name: "Test".into(),
        user_image: None,
        email_verified: true,
        auth_role: Some(role.into()),
        active_org_id: Some(org.into()),
        authorized_membership: Some(crate::middleware::AuthorizedMembership {
            organization_id: org.into(),
            role: role.into(),
        }),
    };
    let mut headers = HeaderMap::new();
    headers.insert("origin", state.verevon_public_origin.parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert("authorization", "Bearer private-browser".parse().unwrap());
    for actor in [
        user("org-one", "viewer"),
        user("org-one", "member"),
        user("foreign", "owner"),
    ] {
        assert_eq!(
            proxy(
                &state,
                &actor,
                &headers,
                Method::POST,
                "public-signals",
                "",
                Bytes::from_static(POST)
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    let mut foreign = headers.clone();
    foreign.insert("origin", "https://foreign.example".parse().unwrap());
    assert_eq!(
        proxy(
            &state,
            &user("org-one", "admin"),
            &foreign,
            Method::POST,
            "public-signals",
            "",
            Bytes::from_static(POST)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert!(upstream.received_requests().await.unwrap().is_empty());
    let response = proxy(
        &state,
        &user("org-one", "admin"),
        &headers,
        Method::POST,
        "public-signals",
        "",
        Bytes::from_static(POST),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = to_bytes(response.into_body(), 8192).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("private"));
    for request in upstream.received_requests().await.unwrap() {
        assert!(!request.headers.contains_key("authorization"));
        assert!(!request.headers.contains_key("cookie"));
        assert!(!request.headers.contains_key("x-org-id"));
    }
}
