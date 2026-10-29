use super::*;
use axum::http::Method;
use serde_json::json;

#[test]
fn audit_routes_require_closed_site_context_and_server_finding_identity() {
    let read = route_spec("audits", &Method::GET).expect("audit panel must have a real route");
    let base = "websiteId=web_11111111111111111111111111111111&site=https%3A%2F%2Ftriodelab.no%2F";
    assert!(validate_query(&read, base).is_ok());
    assert!(validate_query(
        &read,
        &format!("{base}&auditId=audit_22222222222222222222222222222222")
    )
    .is_ok());
    assert!(validate_query(&read, &format!("{base}&organizationId=other")).is_err());
    assert!(validate_query(&read, &format!("{base}&auditId=arbitrary")).is_err());
    let save = route_spec("work/audit", &Method::POST).expect("server-derived audit work");
    assert!(validate_command(&save,br#"{"websiteId":"web_11111111111111111111111111111111","site":"https://triodelab.no/","websiteRevision":7,"auditId":"audit_22222222222222222222222222222222","findingId":"finding_33333333333333333333333333333333"}"#).is_ok());
    assert!(validate_command(&save,br#"{"websiteId":"web_11111111111111111111111111111111","site":"https://triodelab.no/","websiteRevision":7,"auditId":"audit_22222222222222222222222222222222","findingId":"finding_33333333333333333333333333333333","score":100}"#).is_err());
}

#[test]
fn audit_projection_strips_raw_pages_and_credentials_at_every_depth() {
    let read = route_spec("audits", &Method::GET).unwrap();
    let data=projection::sanitize(&read,&json!({"site":"https://triodelab.no/","pages":[{"url":"https://triodelab.no/","html":"private","checks":[{"ruleId":"http_error","status":"passed","token":"private"}]}],"findings":[],"history":[],"token":"private"})).unwrap();
    let encoded = data.to_string();
    assert!(encoded.contains("http_error"));
    assert!(!encoded.contains("private"));
    assert!(!encoded.contains("html"));
}
