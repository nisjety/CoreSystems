use super::*;
use serde_json::json;

const QUERY: &str = "websiteId=web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&site=https%3A%2F%2Ftriodelab.no%2F&environment=production";
const POST: &[u8] = br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"environment":"production"}"#;

#[test]
fn web_vitals_contract_is_exact_read_only_with_explicit_admin_sync() {
    let read = route_spec("web-vitals", &Method::GET).expect("web vitals read");
    assert!(!read.administrative);
    assert!(validate_query(&read, QUERY).is_ok());
    for extra in ["&org=other", "&quarter=Q4", "&environment=development", "&apiKey=secret"] {
        assert!(validate_query(&read, &format!("{QUERY}{extra}")).is_err());
    }
    assert!(validate_query(&read, &QUERY.replace("production", "unknown")).is_err());
    assert!(validate_query(&read, &QUERY.replace("&environment=production", "")).is_err());
    for method in [Method::POST, Method::PATCH, Method::DELETE] { assert!(route_spec("web-vitals", &method).is_none()); }
    let sync = route_spec("web-vitals/sync", &Method::POST).expect("explicit sync");
    assert!(sync.administrative);
    assert!(validate_command(&sync, POST).is_ok());
    assert!(validate_command(&sync, br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"environment":"production","token":"private"}"#).is_err());
}

#[test]
fn web_vitals_projection_is_deep_bounded_and_drops_raw_private_fields() {
    let read = route_spec("web-vitals", &Method::GET).unwrap();
    let value = json!({"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"source":"first_party_web_vitals","environment":"production","status":"connected","reportingOrigin":"https://www.triodelab.no","window":{"startDate":"2026-10-17","endDate":"2026-10-23","token":"private"},"asOf":"2026-10-23T00:00:00Z","retentionDays":7,"sampleCount":1,"organizationId":"private","groups":[{"viewport":"narrow","raw":"private","metrics":{"lcp":{"count":1,"p75":1200,"rawSamples":"private"},"inp":{"count":0,"p75":null},"cls":{"count":0,"p75":null},"token":"private"}},{"viewport":"wide","metrics":{"lcp":{"count":0,"p75":null},"inp":{"count":0,"p75":null},"cls":{"count":0,"p75":null}}}]});
    let safe = projection::sanitize(&read, &value).unwrap();
    assert_eq!(safe["groups"][0]["metrics"]["lcp"]["p75"], 1200);
    assert_eq!(safe["groups"][0]["metrics"]["inp"]["p75"], serde_json::Value::Null);
    assert!(!safe.to_string().contains("private"));
    let mut invalid = value.clone();
    invalid["groups"] = json!([{}, {}, {}]);
    assert!(projection::sanitize(&read, &invalid).is_none());
}

#[tokio::test]
async fn web_vitals_scopes_reads_and_sync_and_never_forwards_browser_authority() {
    use axum::body::to_bytes;
    use wiremock::{matchers::{header, method, path}, Mock, MockServer, ResponseTemplate};
    let upstream = MockServer::start().await;
    for (verb, endpoint) in [("GET", "/api/v1/marketing/web-vitals"), ("POST", "/api/v1/marketing/web-vitals/sync")] {
        Mock::given(method(verb)).and(path(endpoint)).and(header("x-internal-api-key", "vitals-server-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":{"source":"first_party_web_vitals","status":"not_configured","token":"private","groups":[]}}))).expect(1).mount(&upstream).await;
    }
    let mut state = crate::tests::test_state(false);
    state.marketing_core_url = upstream.uri(); state.marketing_core_organization_id = "org-one".into(); state.marketing_core_service_key = "vitals-server-secret".into();
    let user = |org: &str, role: &str| AuthenticatedUser { user_id:"vitals-user".into(), user_email:"user@example.test".into(), user_name:"Test".into(), user_image:None, email_verified:true, auth_role:Some(role.into()), active_org_id:Some(org.into()), authorized_membership:Some(crate::middleware::AuthorizedMembership { organization_id:org.into(), role:role.into() }) };
    let mut headers = HeaderMap::new();
    headers.insert("origin", state.verevon_public_origin.parse().unwrap()); headers.insert("content-type", "application/json".parse().unwrap()); headers.insert("authorization", "Bearer private-browser".parse().unwrap()); headers.insert("x-org-id", "forged-other-org".parse().unwrap());
    assert_eq!(proxy(&state, &user("other", "owner"), &headers, Method::GET, "web-vitals", QUERY, Bytes::new()).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(proxy(&state, &user("org-one", "viewer"), &headers, Method::POST, "web-vitals/sync", "", Bytes::from_static(POST)).await.status(), StatusCode::FORBIDDEN);
    let mut foreign = headers.clone(); foreign.insert("origin", "https://other.example".parse().unwrap());
    assert_eq!(proxy(&state, &user("org-one", "admin"), &foreign, Method::POST, "web-vitals/sync", "", Bytes::from_static(POST)).await.status(), StatusCode::FORBIDDEN);
    let bad_query = format!("{QUERY}&organizationId=other");
    assert_eq!(proxy(&state, &user("org-one", "viewer"), &headers, Method::GET, "web-vitals", &bad_query, Bytes::new()).await.status(), StatusCode::BAD_REQUEST);
    for (actor, method, route, query, body) in [(user("org-one", "viewer"), Method::GET, "web-vitals", QUERY, Bytes::new()), (user("org-one", "admin"), Method::POST, "web-vitals/sync", "", Bytes::from_static(POST))] {
        let response = proxy(&state, &actor, &headers, method, route, query, body).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!String::from_utf8_lossy(&to_bytes(response.into_body(), 8192).await.unwrap()).contains("private"));
    }
    for request in upstream.received_requests().await.unwrap() {
        for name in ["authorization", "cookie", "x-org-id", "x-organization-id"] { assert!(!request.headers.contains_key(name)); }
    }
}
