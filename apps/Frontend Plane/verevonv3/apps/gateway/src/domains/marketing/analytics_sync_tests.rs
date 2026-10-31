use super::*;
use axum::body::to_bytes;
use serde_json::json;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

const QUERY: &str =
    "websiteId=web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&site=https%3A%2F%2Ftriodelab.no%2F&quarter=Q4";
const POST: &[u8] = br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"quarter":"Q4"}"#;
const PATCH: &[u8] = br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"quarter":"Q4","enabled":false}"#;

fn member(org: &str, role: &str) -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: "sync-user".into(),
        user_email: "sync@example.test".into(),
        user_name: "Sync user".into(),
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
fn analytics_sync_contract_is_exact_and_write_is_administrative() {
    let read = route_spec("analytics-sync", &Method::GET).expect("analytics metadata read");
    assert!(!read.administrative);
    assert!(validate_query(&read, QUERY).is_ok());
    for query in [
        format!("{QUERY}&organizationId=other"),
        format!("{QUERY}&quarter=Q1"),
        QUERY.replace("&quarter=Q4", ""),
        QUERY.replace("Q4", "q4"),
        QUERY.replace("triodelab.no", "localhost"),
        format!("{QUERY}&connectionId=private"),
    ] {
        assert!(validate_query(&read, &query).is_err(), "{query}");
    }
    for (method, command) in [(Method::POST, POST), (Method::PATCH, PATCH)] {
        let spec = route_spec("analytics-sync", &method).unwrap();
        assert!(spec.administrative);
        assert!(validate_command(&spec, command).is_ok());
        for (key, value) in [
            ("websiteRevision", json!(0)),
            ("websiteRevision", json!(0.5)),
            ("quarter", json!("Q5")),
            ("quarter", json!("")),
            ("site", json!("https://127.0.0.1/")),
            ("organizationId", json!("forged")),
            ("connectionId", json!("private")),
            ("bundle", json!({"token":"secret"})),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_slice(command).unwrap();
            invalid[key] = value;
            assert!(
                validate_command(&spec, &serde_json::to_vec(&invalid).unwrap()).is_err(),
                "{method} {key}"
            );
        }
    }
    assert!(
        validate_command(&route_spec("analytics-sync", &Method::POST).unwrap(), PATCH).is_err()
    );
    assert!(
        validate_command(&route_spec("analytics-sync", &Method::PATCH).unwrap(), POST).is_err()
    );
    assert!(validate_command(&read, br#"{"quarter":"Q1","quarter":"Q2"}"#).is_err());
    for route in [
        "analytics-sync/private",
        "analytics-sync/",
        "../analytics-sync",
    ] {
        assert!(route_spec(route, &Method::GET).is_none());
    }
    assert!(route_spec("analytics-sync", &Method::DELETE).is_none());
}

#[test]
fn analytics_sync_projection_contains_only_lifecycle_metadata() {
    let spec = route_spec("analytics-sync", &Method::GET).unwrap();
    let safe = projection::sanitize(&spec, &json!({
        "websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "site":"https://triodelab.no/", "websiteRevision":7,
        "source":"marketing_analytics_sync", "quarter":"Q4", "enabled":true, "status":"partial", "jobId":"sync_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "queuedAt":"2026-10-23T09:00:00Z", "startedAt":"2026-10-23T09:00:01Z", "completedAt":"2026-10-23T09:00:02Z", "nextSyncAt":"2026-10-23T15:00:02Z", "expiresAt":"2026-10-23T15:00:02Z",
        "steps":[{"key":"gsc", "status":"connected", "observedAt":"2026-10-23T09:00:02Z", "connectionId":"private", "raw":{"token":"private"}}, {"key":"ga4", "status":"unavailable", "reason":"not_connected"}],
        "bundle":{"pulse":{"totals":{"clicks":42}}, "token":"private"}, "fingerprint":"private", "organizationId":"private", "connectionId":"private", "attempts":3, "year":2026
    })).unwrap();
    assert_eq!(safe["expiresAt"], "2026-10-23T15:00:02Z");
    assert_eq!(
        safe["steps"][0],
        json!({"key":"gsc","status":"connected","observedAt":"2026-10-23T09:00:02Z"})
    );
    assert_eq!(safe["steps"][1]["reason"], "not_connected");
    assert!(!safe.to_string().contains("private"));
    assert!(safe.get("bundle").is_none());
    assert!(
        projection::sanitize(&spec, &json!({"steps":[{"status":{"token":"private"}}]})).is_none()
    );
}

#[tokio::test]
async fn analytics_sync_respects_viewer_read_admin_write_and_origin_boundaries() {
    let upstream = MockServer::start().await;
    for verb in ["GET", "POST", "PATCH"] {
        Mock::given(method(verb)).and(path("/api/v1/marketing/analytics-sync")).and(header("x-internal-api-key", "sync-server-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":{"source":"marketing_analytics_sync","status":"queued","steps":[],"bundle":{"token":"private"},"connectionId":"private"}}))).mount(&upstream).await;
    }
    let mut state = crate::tests::test_state(false);
    state.marketing_core_url = upstream.uri();
    state.marketing_core_organization_id = "org-one".into();
    state.marketing_core_service_key = "sync-server-secret".into();
    let mut headers = HeaderMap::new();
    headers.insert("origin", state.verevon_public_origin.parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert("authorization", "Bearer browser-private".parse().unwrap());
    for (method, command) in [(Method::POST, POST), (Method::PATCH, PATCH)] {
        for user in [
            member("org-one", "viewer"),
            member("org-one", "member"),
            member("org-two", "owner"),
        ] {
            assert_eq!(
                proxy(
                    &state,
                    &user,
                    &headers,
                    method.clone(),
                    "analytics-sync",
                    "",
                    Bytes::copy_from_slice(command)
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
                &member("org-one", "owner"),
                &foreign,
                method.clone(),
                "analytics-sync",
                "",
                Bytes::copy_from_slice(command)
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert!(upstream.received_requests().await.unwrap().is_empty());
    for (method, query, body, role) in [
        (Method::GET, QUERY, &b""[..], "viewer"),
        (Method::POST, "", POST, "owner"),
        (Method::PATCH, "", PATCH, "admin"),
    ] {
        let response = proxy(
            &state,
            &member("org-one", role),
            &headers,
            method,
            "analytics-sync",
            query,
            Bytes::copy_from_slice(body),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 8192).await.unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("private"));
    }
    let requests = upstream.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    for request in requests {
        assert!(!request.headers.contains_key("authorization"));
        assert!(!request.headers.contains_key("cookie"));
        assert!(!request.headers.contains_key("x-org-id"));
    }
}

#[tokio::test]
async fn outcome_reports_are_same_workspace_read_only_and_drop_person_data() {
    let query = format!("{QUERY}&environment=development");
    let spec = route_spec("outcomes", &Method::GET).unwrap();
    assert!(!spec.administrative);
    assert!(validate_query(&spec, &query).is_ok());
    assert!(validate_query(&spec, &format!("{query}&organizationId=forged")).is_err());
    assert!(route_spec("outcomes", &Method::POST).is_none());
    let upstream = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/v1/marketing/outcomes"))
        .and(header("x-internal-api-key", "outcomes-server-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":{
            "source":"leads_core", "status":"connected", "metrics":{"inquiries":1,"leads":1,"bookings":null,"email":"private"},
            "email":"private", "token":"private", "fields":{"message":"private"}
        }}))).mount(&upstream).await;
    let mut state=crate::tests::test_state(false);
    state.marketing_core_url=upstream.uri();
    state.marketing_core_organization_id="org-one".into();
    state.marketing_core_service_key="outcomes-server-key".into();
    let headers=HeaderMap::new();
    let denied=proxy(&state,&member("org-two","owner"),&headers,Method::GET,"outcomes",&query,Bytes::new()).await;
    assert_eq!(denied.status(),StatusCode::FORBIDDEN);
    assert!(upstream.received_requests().await.unwrap().is_empty());
    let response=proxy(&state,&member("org-one","viewer"),&headers,Method::GET,"outcomes",&query,Bytes::new()).await;
    assert_eq!(response.status(),StatusCode::OK);
    let body=to_bytes(response.into_body(),8192).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("private"));
    let value:serde_json::Value=serde_json::from_slice(&body).unwrap();
    assert_eq!(value["data"]["metrics"],json!({"inquiries":1,"leads":1,"bookings":null}));
}
