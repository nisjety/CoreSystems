use super::*;
use axum::body::to_bytes;
use serde_json::json;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

const QUERY: &str =
    "websiteId=web_11111111111111111111111111111111&site=https%3A%2F%2Ftriodelab.no%2F";
const COMMAND: &[u8] = br#"{"websiteId":"web_11111111111111111111111111111111","site":"https://triodelab.no/","websiteRevision":7,"scheduleRevision":0,"enabled":true,"cadence":"weekly"}"#;

fn member(org: &str, role: &str) -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: "monitor-user".into(),
        user_email: "monitor@example.test".into(),
        user_name: "Monitor user".into(),
        user_image: None,
        email_verified: true,
        auth_role: Some(role.into()),
        active_org_id: Some(org.into()),
        authorized_membership: Some(crate::middleware::AuthorizedMembership {
            organization_id: org.into(),
            role: role.into(),
        }),
    }
}

#[test]
fn monitoring_contract_cannot_inject_tenants_urls_jobs_or_cadences() {
    let read = route_spec("monitoring", &Method::GET).expect("real monitoring read route");
    assert!(validate_query(&read, QUERY).is_ok());
    for query in [
        format!("{QUERY}&orgId=other"),
        format!("{QUERY}&websiteId=web_22222222222222222222222222222222"),
        format!("{QUERY}&nextRunAt=now"),
    ] {
        assert!(validate_query(&read, &query).is_err());
    }
    let write = route_spec("monitoring", &Method::PATCH).expect("explicit schedule configuration");
    assert!(validate_command(&write, COMMAND).is_ok());
    for (key, value) in [
        ("enabled", json!("true")),
        ("cadence", json!("minutely")),
        ("scheduleRevision", json!(-1)),
        ("scheduleRevision", json!(0.5)),
        ("websiteRevision", json!(0)),
    ] {
        let mut invalid: serde_json::Value = serde_json::from_slice(COMMAND).unwrap();
        invalid[key] = value;
        assert!(validate_command(&write, &serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    for key in ["organizationId", "nextRunAt", "auditId", "connectionId"] {
        let mut invalid: serde_json::Value = serde_json::from_slice(COMMAND).unwrap();
        invalid[key] = json!("forged");
        assert!(validate_command(&write, &serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    assert!(route_spec("monitoring", &Method::POST).is_none());
}

#[test]
fn monitoring_projection_rebuilds_history_without_private_state() {
    let spec = route_spec("monitoring", &Method::GET).unwrap();
    let safe=projection::sanitize(&spec,&json!({"source":"marketing_scheduler","configured":true,"enabled":true,"cadence":"weekly","revision":1,"nextRunAt":"2026-10-30T09:00:00Z","history":[{"auditId":"audit_11111111111111111111111111111111","attempt":1,"checkedPages":10,"findingCount":0,"status":"complete","privateKey":"private"}],"dispatchKey":"private","token":"private"})).unwrap();
    assert_eq!(safe["history"][0]["checkedPages"], 10);
    assert_eq!(safe["enabled"], true);
    assert!(!safe.to_string().contains("private"));
}

#[tokio::test]
async fn schedule_write_keeps_live_membership_origin_and_server_secret_boundaries() {
    let upstream = MockServer::start().await;
    Mock::given(method("PATCH")).and(path("/api/v1/marketing/monitoring"))
        .and(header("x-internal-api-key","monitor-server-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":{"source":"marketing_scheduler","configured":true,"enabled":true,"revision":1,"cadence":"weekly","history":[]}})))
        .mount(&upstream).await;
    let mut state = crate::tests::test_state(false);
    state.marketing_core_url = upstream.uri();
    state.marketing_core_organization_id = "org-one".into();
    state.marketing_core_service_key = "monitor-server-secret".into();
    let mut headers = HeaderMap::new();
    headers.insert("origin", state.verevon_public_origin.parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert("authorization", "Bearer browser-secret".parse().unwrap());
    for user in [member("org-two", "owner"), member("org-one", "viewer")] {
        assert_eq!(
            proxy(
                &state,
                &user,
                &headers,
                Method::PATCH,
                "monitoring",
                "",
                Bytes::from_static(COMMAND)
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    let mut foreign = headers.clone();
    foreign.insert("origin", "https://other.example".parse().unwrap());
    assert_eq!(
        proxy(
            &state,
            &member("org-one", "member"),
            &foreign,
            Method::PATCH,
            "monitoring",
            "",
            Bytes::from_static(COMMAND)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert!(upstream.received_requests().await.unwrap().is_empty());
    let response = proxy(
        &state,
        &member("org-one", "member"),
        &headers,
        Method::PATCH,
        "monitoring",
        "",
        Bytes::from_static(COMMAND),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 8192).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("secret"));
    let requests = upstream.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].headers.contains_key("authorization"));
    assert!(!requests[0].headers.contains_key("x-org-id"));
}
