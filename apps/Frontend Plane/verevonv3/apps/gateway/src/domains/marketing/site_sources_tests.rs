use super::*;
use serde_json::json;

const QUERY: &str = "websiteId=web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&site=https%3A%2F%2Ftriodelab.no%2F&provider=cookiebot";
const POST: &[u8] = br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"provider":"cookiebot"}"#;
#[test]
fn algolia_collection_and_term_tables_are_bounded_independent_contracts() {
    let read = route_spec("site-sources/collection", &Method::GET).expect("collection read");
    assert!(validate_query(&read, &QUERY.replace("cookiebot", "algolia")).is_ok());
    let queue = route_spec("site-sources/collection", &Method::POST).unwrap(); assert!(queue.administrative);
    assert!(validate_command(&queue, POST).is_err());
    let report = route_spec("site-sources", &Method::GET).unwrap();
    let safe = projection::sanitize(&report, &json!({"algolia":{"topSearches":{"status":"connected","limit":20,"rows":[{"query":"","count":2,"token":"drop"},{"query":"<script>x</script>","count":1,"averageResults":1.5}]},"noResultTerms":{"status":"unavailable","limit":20}}})).unwrap();
    assert_eq!(safe["algolia"]["topSearches"]["rows"][0]["query"], ""); assert!(!safe.to_string().contains("drop"));
    for term in [json!({"status":"connected","limit":20}),json!({"status":"unavailable","limit":20,"rows":[]}),json!({"status":"connected","limit":20,"rows":[{"query":"bad\nterm","count":1}]}),json!({"status":"connected","limit":20,"rows":[{"query":"","count":1},{"query":"","count":2}]}),json!({"status":"connected","limit":20,"rows":vec![json!({"query":"x","count":1});21]})] {
        assert!(projection::sanitize(&report,&json!({"algolia":{"topSearches":term}})).is_none());
    }
}

#[tokio::test]
async fn collection_queue_is_org_admin_same_origin_and_retired_dns_never_forwards() {
    use wiremock::{matchers::{header, method, path}, Mock, MockServer, ResponseTemplate};
    let upstream=MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/v1/marketing/site-sources/collection")).and(header("x-internal-api-key","server-only")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":{"provider":"algolia","status":"idle","token":"private"}}))).expect(1).mount(&upstream).await;
    Mock::given(method("POST")).and(path("/api/v1/marketing/site-sources/collection")).and(header("x-internal-api-key","server-only")).respond_with(ResponseTemplate::new(202).set_body_json(json!({"data":{"provider":"algolia","status":"queued","token":"private"}}))).expect(1).mount(&upstream).await;
    let mut state=crate::tests::test_state(false);state.marketing_core_url=upstream.uri();state.marketing_core_organization_id="org-one".into();state.marketing_core_service_key="server-only".into();
    let user=|org:&str,role:&str|AuthenticatedUser{user_id:"test".into(),user_email:"test@example.test".into(),user_name:"Test".into(),user_image:None,email_verified:true,auth_role:Some(role.into()),active_org_id:Some(org.into()),authorized_membership:Some(crate::middleware::AuthorizedMembership{organization_id:org.into(),role:role.into()})};
    let mut headers=HeaderMap::new();headers.insert("origin",state.verevon_public_origin.parse().unwrap());headers.insert("content-type","application/json".parse().unwrap());headers.insert("x-org-id","forged".parse().unwrap());headers.insert("authorization","Bearer private".parse().unwrap());
    let command=String::from_utf8(POST.to_vec()).unwrap().replace("cookiebot","algolia");let query=QUERY.replace("cookiebot","algolia");
    for actor in [user("org-one","member"),user("other","owner")] {assert_eq!(proxy(&state,&actor,&headers,Method::POST,"site-sources/collection","",Bytes::from(command.clone())).await.status(),StatusCode::FORBIDDEN);}
    let mut foreign=headers.clone();foreign.insert("origin","https://evil.example".parse().unwrap());assert_eq!(proxy(&state,&user("org-one","admin"),&foreign,Method::POST,"site-sources/collection","",Bytes::from(command.clone())).await.status(),StatusCode::FORBIDDEN);
    for suffix in ["challenge","verify"] {assert_eq!(proxy(&state,&user("org-one","admin"),&headers,Method::POST,&format!("websites/web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/ownership/{suffix}"),"",Bytes::new()).await.status(),StatusCode::GONE);}
    assert_eq!(proxy(&state,&user("org-one","viewer"),&headers,Method::GET,"site-sources/collection",&query,Bytes::new()).await.status(),StatusCode::OK);
    let receipt=proxy(&state,&user("org-one","admin"),&headers,Method::POST,"site-sources/collection","",Bytes::from(command)).await;assert_eq!(receipt.status(),StatusCode::ACCEPTED);
    for request in upstream.received_requests().await.unwrap(){for name in ["authorization","cookie","x-org-id","x-organization-id"]{assert!(!request.headers.contains_key(name));}}
}
#[test]
fn algolia_mapping_contract_and_projection_are_site_scoped_not_credential_inputs() {
    let read=route_spec("site-sources/algolia-mapping",&Method::GET).expect("mapping read");
    assert!(validate_query(&read,"websiteId=web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&site=https%3A%2F%2Ftriodelab.no%2F").is_ok());
    let put=route_spec("site-sources/algolia-mapping",&Method::PUT).unwrap();assert!(put.administrative);
    assert!(validate_command(&put,br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"connectionId":"conn_test","applicationId":"APP1","indexName":"public_index"}"#).is_ok());
    assert!(validate_command(&put,br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"connectionId":"conn_test","applicationId":"APP1","indexName":"public_index","apiKey":"secret"}"#).is_err());
    let delete=route_spec("site-sources/algolia-mapping",&Method::DELETE).unwrap();assert!(delete.administrative);
    assert!(validate_command(&delete,br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7}"#).is_ok());
    let value=projection::sanitize(&read,&json!({"status":"mapped","connectionId":"conn_test","applicationId":"APP1","indexName":"public_index","token":"private","generation":"private"})).unwrap();
    assert!(!value.to_string().contains("private"));assert_eq!(value["applicationId"],"APP1");
}

#[test]
fn site_sources_contract_is_exact_and_only_sync_has_admin_write_authority() {
    let read = route_spec("site-sources", &Method::GET).expect("site source read");
    assert!(!read.administrative);
    assert!(validate_query(&read, QUERY).is_ok());
    assert!(validate_query(&read, &QUERY.replace("cookiebot", "algolia")).is_ok());
    for extra in ["&org=other", "&quarter=Q4", "&provider=algolia", "&apiKey=secret"] {
        assert!(validate_query(&read, &format!("{QUERY}{extra}")).is_err());
    }
    assert!(validate_query(&read, &QUERY.replace("cookiebot", "google")).is_err());
    for method in [Method::POST, Method::PATCH, Method::DELETE] { assert!(route_spec("site-sources", &method).is_none()); }
    let sync = route_spec("site-sources/sync", &Method::POST).expect("explicit sync");
    assert!(sync.administrative);
    assert!(validate_command(&sync, POST).is_ok());
    assert!(validate_command(&sync, br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"provider":"cookiebot","apiKey":"private"}"#).is_err());
}

#[test]
fn site_sources_projection_drops_credentials_and_raw_query_inventory_fields_at_every_level() {
    let spec = route_spec("site-sources", &Method::GET).unwrap();
    let cookie = projection::sanitize(&spec, &json!({"provider":"cookiebot","token":"private","cookiebot":{"reportingDomain":"www.triodelab.no","apiKey":"private","consents":{"status":"connected","reportedDays":7,"raw":"private","totals":{"optIn":2,"optOut":1,"preferences":1,"statistics":2,"marketing":0,"visitorIp":"private"}},"inventory":{"status":"connected","scannedAt":"2026-10-20T00:00:00Z","total":1,"categories":{"necessary":1,"preferences":0,"statistics":0,"marketing":0,"unclassified":0,"raw":"private"},"thirdParty":0,"secure":1,"httpOnly":0,"cookieValues":"private"}}})).unwrap();
    assert_eq!(cookie["cookiebot"]["consents"]["totals"]["optIn"],2);
    assert_eq!(cookie["cookiebot"]["inventory"]["categories"]["necessary"],1);
    assert!(!cookie.to_string().contains("private"));
    let algolia = projection::sanitize(&spec, &json!({"provider":"algolia","algolia":{"indexName":"triodelab_public","region":"de","applicationId":"private","analytics":{"status":"connected","reportedDays":7,"searches":10,"noResultSearches":2,"noResultRate":0.2,"queries":["private"]}}})).unwrap();
    assert_eq!(algolia["algolia"]["analytics"]["noResultRate"],0.2);
    assert!(!algolia.to_string().contains("private"));
    assert!(projection::sanitize(&spec,&json!({"provider":"algolia","algolia":{"analytics":{"queries":[{}],"searches":{"private":1}}}})).is_none());
}

#[tokio::test]
async fn site_sources_read_and_sync_scope_authority_and_drop_browser_headers() {
    use wiremock::{matchers::{header, method, path}, Mock, MockServer, ResponseTemplate};
    let upstream = MockServer::start().await;
    for (verb, endpoint) in [("GET", "/api/v1/marketing/site-sources"), ("POST", "/api/v1/marketing/site-sources/sync")] {
        Mock::given(method(verb)).and(path(endpoint)).and(header("x-internal-api-key", "source-server-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":{"provider":"cookiebot","status":"not_configured","token":"private"}}))).expect(1).mount(&upstream).await;
    }
    let mut state = crate::tests::test_state(false);
    state.marketing_core_url = upstream.uri(); state.marketing_core_organization_id = "org-one".into(); state.marketing_core_service_key = "source-server-secret".into();
    let user = |org: &str, role: &str| AuthenticatedUser { user_id:"source-user".into(), user_email:"user@example.test".into(), user_name:"Test".into(), user_image:None, email_verified:true, auth_role:Some(role.into()), active_org_id:Some(org.into()), authorized_membership:Some(crate::middleware::AuthorizedMembership { organization_id:org.into(), role:role.into() }) };
    let mut headers = HeaderMap::new();
    headers.insert("origin", state.verevon_public_origin.parse().unwrap()); headers.insert("content-type", "application/json".parse().unwrap()); headers.insert("authorization", "Bearer private-browser".parse().unwrap()); headers.insert("x-org-id", "forged-other-org".parse().unwrap());
    assert_eq!(proxy(&state, &user("other", "owner"), &headers, Method::GET, "site-sources", QUERY, Bytes::new()).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(proxy(&state, &user("org-one", "viewer"), &headers, Method::POST, "site-sources/sync", "", Bytes::from_static(POST)).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(proxy(&state, &user("org-one", "member"), &headers, Method::POST, "site-sources/sync", "", Bytes::from_static(POST)).await.status(), StatusCode::FORBIDDEN);
    let mut foreign = headers.clone(); foreign.insert("origin", "https://other.example".parse().unwrap());
    assert_eq!(proxy(&state, &user("org-one", "admin"), &foreign, Method::POST, "site-sources/sync", "", Bytes::from_static(POST)).await.status(), StatusCode::FORBIDDEN);
    let bad_query = format!("{QUERY}&organizationId=other");
    assert_eq!(proxy(&state, &user("org-one", "viewer"), &headers, Method::GET, "site-sources", &bad_query, Bytes::new()).await.status(), StatusCode::BAD_REQUEST);
    for (actor, method, route, query, body) in [(user("org-one", "viewer"), Method::GET, "site-sources", QUERY, Bytes::new()), (user("org-one", "admin"), Method::POST, "site-sources/sync", "", Bytes::from_static(POST))] {
        assert_eq!(proxy(&state, &actor, &headers, method, route, query, body).await.status(), StatusCode::OK);
    }
    for request in upstream.received_requests().await.unwrap() {
        for name in ["authorization", "cookie", "x-org-id", "x-organization-id"] { assert!(!request.headers.contains_key(name)); }
    }
}
