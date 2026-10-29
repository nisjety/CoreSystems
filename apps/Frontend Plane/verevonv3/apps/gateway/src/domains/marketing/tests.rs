use super::*;
use axum::{body::to_bytes, http::Request};
use tower::ServiceExt;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn member(org: &str, role: &str) -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: "user-one".into(),
        user_email: "user@example.test".into(),
        user_name: "User".into(),
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
fn backend_scope_is_explicit_and_live_membership_is_required() {
    assert!(!scope_allowed("", &member("org-one", "owner"), false));
    assert!(!scope_allowed(
        "org-one",
        &member("org-two", "owner"),
        false
    ));
    assert!(scope_allowed(
        "org-one",
        &member("org-one", "viewer"),
        false
    ));
    assert!(!scope_allowed(
        "org-one",
        &member("org-one", "viewer"),
        true
    ));
    assert!(scope_allowed("org-one", &member("org-one", "member"), true));
    let user = AuthenticatedUser {
        authorized_membership: None,
        ..member("org-one", "owner")
    };
    assert!(!scope_allowed("org-one", &user, false));
}

#[test]
fn route_allowlist_never_turns_into_an_open_proxy() {
    for route in [
        "pulse",
        "search",
        "ga4",
        "clarity",
        "pagespeed",
        "websites",
        "work",
        "discovery",
        "analysis",
        "competitors",
    ] {
        assert!(route_spec(route, &Method::GET).is_some(), "{route}");
    }
    let id = "web_0123456789abcdef0123456789abcdef";
    for suffix in [
        "verify",
        "google/connect",
    ] {
        assert!(route_spec(&format!("websites/{id}/{suffix}"), &Method::POST).is_some());
    }
    for route in [
        "../integrations",
        "websites/invalid",
        "websites/web_0123456789abcdef0123456789abcdef/delete",
        "status",
        "assistant/drafts",
    ] {
        assert!(route_spec(route, &Method::GET).is_none());
    }
}

#[test]
fn collection_inventory_is_read_only_scoped_and_strips_private_fields() {
    let read=route_spec("collection",&Method::GET).unwrap();
    assert!(route_spec("collection",&Method::POST).is_none());
    let query="websiteId=web_0123456789abcdef0123456789abcdef&site=https%3A%2F%2Fexample.com%2F&quarter=Q4";
    assert!(validate_query(&read,query).is_ok());
    for extra in ["&organizationId=other","&providerUrl=https://evil.example/","&quarter=Q1"] {assert!(validate_query(&read,&format!("{query}{extra}")).is_err());}
    let clean=projection::sanitize(&read,&json!({"source":"visibility_collection_inventory","token":"private","datasets":[{"key":"gsc","records":null,"secret":"private","period":{"startDate":"2026-10-01","token":"private"}}],"summary":{"available":0,"cookie":"private"}})).unwrap();
    assert!(!clean.to_string().contains("private"));
    assert!(clean["datasets"][0]["records"].is_null());
    assert_eq!(clean["datasets"][0]["period"]["startDate"],"2026-10-01");
}

#[test]
fn visibility_tools_keep_narrow_context_and_recursive_evidence_projections() {
    let id = "web_0123456789abcdef0123456789abcdef";
    let read = route_spec("analysis", &Method::GET).unwrap();
    let query = format!("websiteId={id}&site=https%3A%2F%2Fexample.com%2F&quarter=Q4");
    assert!(validate_query(&read, &query).is_ok());
    assert!(validate_query(&read, &format!("{query}&organizationId=other")).is_err());
    let clean = projection::sanitize(&read, &json!({"source":"visibility_analysis","token":"private","pages":[{"url":"https://example.com/","clicks":null,"html":"private"}],"groups":[{"key":"technical_review","entity":"page","items":[{"key":"https://example.com/","bearer":"private"}]}],"edges":[{"from":"https://example.com/","to":"https://example.com/a","status":"not_checked","cookie":"private"}]})).unwrap();
    assert!(!clean.to_string().contains("private"));
    assert!(clean["pages"][0]["clicks"].is_null());
    let write = route_spec("competitors", &Method::POST).unwrap();
    let valid = json!({"site":"https://example.com/","websiteId":id,"websiteRevision":1,"target":"https://competitor.no/","operation":"refresh"});
    assert!(validate_command(&write, &serde_json::to_vec(&valid).unwrap()).is_ok());
    for (field, value) in [("operation", json!("execute")), ("target", json!("https://localhost/")), ("token", json!("private"))] {
        let mut invalid = valid.clone(); invalid[field] = value;
        assert!(validate_command(&write, &serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    let clean = projection::sanitize(&write, &json!({"entries":[{"jobId":"private","target":"https://competitor.no/","own":{"title":"Public title","html":"private"},"competitor":{"title":"Public competitor","token":"private"}}]})).unwrap();
    assert!(!clean.to_string().contains("private"));
    assert_eq!(clean["entries"][0]["own"]["title"], "Public title");
}

#[test]
fn comparison_contract_accepts_only_adjacent_completed_equal_windows() {
    let spec =
        route_spec("search/compare", &Method::GET).expect("comparison is a shipped Pulse panel");
    let query = "site=https%3A%2F%2Fexample.com%2F&websiteId=web_0123456789abcdef0123456789abcdef&dimension=query&currentStart=2025-02-01&currentEnd=2025-02-28&previousStart=2025-01-04&previousEnd=2025-01-31&rowLimit=100";
    assert!(validate_query(&spec, query).is_ok());
    for invalid in [
        query.replace("rowLimit=100", "rowLimit=1000"),
        query.replace("previousEnd=2025-01-31", "previousEnd=2025-01-30"),
        query.replace("2025-02-28", "2025-02-30"),
        query.replace("dimension=query", "dimension=country"),
    ] {
        assert!(validate_query(&spec, &invalid).is_err());
    }
}

#[test]
fn strict_queries_and_commands_reject_tenant_and_provider_injection() {
    let spec = route_spec("pulse", &Method::GET).unwrap();
    let valid = "quarter=Q4&site=https%3A%2F%2Fexample.com%2F&websiteId=web_0123456789abcdef0123456789abcdef";
    assert!(validate_query(&spec, valid).is_ok());
    assert!(validate_query(&spec, &valid.replace("Q4", "FY")).is_ok());
    for query in [
        format!("{valid}&orgId=other"),
        format!("{valid}&quarter=Q1"),
        valid.replace("example.com", "localhost"),
        valid.replace("web_0123456789abcdef0123456789abcdef", "invalid"),
    ] {
        assert!(validate_query(&spec, &query).is_err());
    }
    let write = route_spec("websites", &Method::POST).unwrap();
    assert!(validate_command(&write, br#"{"site":"https://example.com/"}"#).is_ok());
    assert!(validate_command(
        &write,
        br#"{"site":"https://example.com/","organizationId":"other"}"#
    )
    .is_err());
    assert!(validate_command(
        &write,
        br#"{"site":"https://example.com/","site":"https://other.com/"}"#
    )
    .is_err());
}

#[tokio::test]
async fn approved_member_reads_rebuilt_projection_without_forwarding_browser_credentials() {
    let upstream = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/v1/marketing/pulse"))
        .and(header("x-internal-api-key", "marketing-server-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":{"site":"https://example.com/","source":"google_search_console","status":"unavailable","period":{"startDate":"2026-10-01","endDate":"2026-10-22"},"totals":null,"days":[],"token":"upstream-secret","reason":"not_configured"},"token":"top-secret"}))).mount(&upstream).await;
    let mut state = crate::tests::test_state(false);
    state.marketing_core_url = upstream.uri();
    state.marketing_core_organization_id = "org-one".into();
    state.marketing_core_service_key = "marketing-server-key".into();
    let response = proxy(&state, &member("org-one", "member"), &HeaderMap::new(), Method::GET, "pulse", "quarter=Q4&site=https%3A%2F%2Fexample.com%2F&websiteId=web_0123456789abcdef0123456789abcdef", Bytes::new()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = String::from_utf8(
        to_bytes(response.into_body(), 10000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(text.contains("google_search_console"));
    assert!(!text.contains("secret"));
    assert!(!text.contains("token"));
    let requests = upstream.received_requests().await.unwrap();
    assert!(!requests[0].headers.contains_key("cookie"));
    assert!(!requests[0].headers.contains_key("authorization"));
    assert!(!requests[0].headers.contains_key("x-org-id"));
}

#[tokio::test]
async fn different_tenant_and_cross_origin_writes_do_not_contact_marketing() {
    let upstream = MockServer::start().await;
    let mut state = crate::tests::test_state(false);
    state.marketing_core_url = upstream.uri();
    state.marketing_core_organization_id = "org-one".into();
    state.marketing_core_service_key = "marketing-server-key".into();
    let response = proxy(
        &state,
        &member("org-two", "owner"),
        &HeaderMap::new(),
        Method::GET,
        "websites",
        "",
        Bytes::new(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let mut headers = HeaderMap::new();
    headers.insert("origin", "https://evil.test".parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    let response = proxy(
        &state,
        &member("org-one", "owner"),
        &headers,
        Method::POST,
        "websites",
        "",
        Bytes::from_static(br#"{"site":"https://example.com/"}"#),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn main_marketing_routes_require_session() {
    for endpoint in ["/api/v1/marketing/websites", "/api/v1/visibility/websites", "/api/v1/visibility/analysis", "/api/v1/visibility/competitors"] {
        let state = crate::tests::test_state(false);
        let response = router(state.clone()).with_state(state).oneshot(
            Request::builder().uri(endpoint).body(Body::empty()).unwrap(),
        ).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{endpoint}");
    }
}

#[test]
fn every_shipped_client_command_has_a_strict_gateway_contract() {
    let id = "web_0123456789abcdef0123456789abcdef";
    let task = "task_0123456789abcdef0123456789abcdef";
    let context = json!({"site":"https://example.com/","websiteId":id,"websiteRevision":1});
    let commands = [
        (
            Method::POST,
            "pagespeed".to_owned(),
            json!({"site":"https://example.com/","websiteId":id,"strategy":"mobile"}),
        ),
        (Method::POST, "discovery".to_owned(), context.clone()),
        (Method::POST, "competitors".to_owned(), json!({"site":"https://example.com/","websiteId":id,"websiteRevision":1,"target":"https://competitor.no/","operation":"refresh"})),
        (
            Method::POST,
            "work".to_owned(),
            json!({"site":"https://example.com/","websiteId":id,"websiteRevision":1,"quarter":"Q4","query":"webdesign"}),
        ),
        (
            Method::POST,
            "work/technical".to_owned(),
            json!({"site":"https://example.com/","websiteId":id,"websiteRevision":1,"strategy":"desktop","checkId":"canonical","fetchTime":"2026-10-22T10:00:00Z"}),
        ),
        (
            Method::PATCH,
            format!("work/{task}"),
            json!({"site":"https://example.com/","websiteId":id,"websiteRevision":1,"revision":1,"status":"done"}),
        ),
        (
            Method::POST,
            format!("work/{task}/verify"),
            json!({"site":"https://example.com/","websiteId":id,"websiteRevision":1,"revision":1}),
        ),
        (
            Method::POST,
            "websites".to_owned(),
            json!({"site":"https://example.com/"}),
        ),
        (
            Method::PATCH,
            format!("websites/{id}"),
            json!({"site":"https://example.com/","revision":1}),
        ),
        (
            Method::PATCH,
            format!("websites/{id}/mappings"),
            json!({"ga4ReportingSite":"https://www.example.com/","revision":1}),
        ),
        (
            Method::PATCH,
            format!("websites/{id}/mappings"),
            json!({"clarityProjectId":"project123","revision":1}),
        ),
        (Method::POST, format!("websites/{id}/verify"), json!({})),
        (
            Method::POST,
            format!("websites/{id}/google/connect"),
            json!({}),
        ),
    ];
    for (method, route, command) in commands {
        let spec = route_spec(&route, &method).unwrap();
        assert_eq!(
            validate_command(&spec, &serde_json::to_vec(&command).unwrap()),
            Ok(command),
            "{method} {route}"
        );
    }
    assert!(validate_query(
        &route_spec(&format!("websites/{id}/google/status"), &Method::GET).unwrap(),
        "sessionId=session-one"
    )
    .is_ok());
    assert!(validate_query(
        &route_spec("discovery", &Method::GET).unwrap(),
        &format!("site=https%3A%2F%2Fexample.com%2F&websiteId={id}")
    )
    .is_ok());
}

#[tokio::test]
async fn router_validates_canonical_membership_and_ignores_forged_scope() {
    let auth = MockServer::start().await;
    let authority = MockServer::start().await;
    let upstream = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/auth/get-session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"user":{"id":"user-one","email":"user@example.test","emailVerified":true},"session":{"activeOrganizationId":"org-one"}}))).mount(&auth).await;
    Mock::given(method("GET")).and(path("/api/v1/me/session-context"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"userId":"user-one","orgId":"org-one","role":"member","onboardingStatus":"COMPLETED"}))).mount(&authority).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/marketing/websites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":{"websites":[]}})))
        .expect(1)
        .mount(&upstream)
        .await;
    let mut state = crate::tests::test_state(false);
    state.auth_core_url = auth.uri();
    state.user_core_url = authority.uri();
    state.marketing_core_url = upstream.uri();
    state.marketing_core_service_key = "marketing-server-key".into();
    state.marketing_core_organization_id = "org-one".into();
    let response = crate::build_router(state)
        .oneshot(
            Request::builder()
                .uri("/api/v1/visibility/websites")
                .header("cookie", "idknuten.sid=synthetic-session")
                .header("x-org-id", "org-other")
                .header("x-internal-api-key", "forged-browser-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let requests = upstream.received_requests().await.unwrap();
    assert_eq!(
        requests[0].headers.get("x-internal-api-key").unwrap(),
        "marketing-server-key"
    );
    assert!(!requests[0].headers.contains_key("x-org-id"));
    assert!(!requests[0].headers.contains_key("cookie"));
    assert_eq!(authority.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn upstream_errors_and_invalid_successes_never_leak_provider_data() {
    for status in [400, 401, 403, 404, 409, 429, 500, 503] {
        let upstream = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_string("provider-secret http://internal-server token=value"),
            )
            .mount(&upstream)
            .await;
        let mut state = crate::tests::test_state(false);
        state.marketing_core_url = upstream.uri();
        state.marketing_core_service_key = "marketing-server-key".into();
        state.marketing_core_organization_id = "org-one".into();
        let response = proxy(
            &state,
            &member("org-one", "owner"),
            &HeaderMap::new(),
            Method::GET,
            "websites",
            "",
            Bytes::new(),
        )
        .await;
        assert!(!response.status().is_success());
        let text = String::from_utf8(
            to_bytes(response.into_body(), 10000)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!text.contains("provider-secret"));
        assert!(!text.contains("internal-server"));
    }
}
