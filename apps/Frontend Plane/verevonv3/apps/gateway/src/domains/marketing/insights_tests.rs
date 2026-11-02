use super::*;
use serde_json::Value;

const QUERY: &str =
    "websiteId=web_11111111111111111111111111111111&site=https%3A%2F%2Ftriodelab.no%2F";

#[test]
fn insights_and_weekly_reports_are_closed_read_only_routes() {
    for route in ["audit-insights", "reports/weekly"] {
        let read = route_spec(route, &Method::GET).expect("authenticated read projection");
        assert!(validate_query(&read, QUERY).is_ok());
        for query in [
            format!("{QUERY}&organizationId=other"),
            format!("{QUERY}&auditId=audit_22222222222222222222222222222222"),
            format!("{QUERY}&site=https%3A%2F%2Fother.example%2F"),
        ] {
            assert!(validate_query(&read, &query).is_err());
        }
        for method in [Method::POST, Method::PATCH, Method::DELETE] {
            assert!(route_spec(route, &method).is_none());
        }
    }
}

#[test]
fn weekly_selector_is_optional_strict_past_or_current_utc_monday() {
    let read = route_spec("reports/weekly", &Method::GET).unwrap();
    assert!(validate_query(&read, &format!("{QUERY}&weekStart=2026-10-19")).is_ok());
    for day in ["2026-10-20", "2026-02-30", "2026-1-5", "2999-01-04", ""] {
        assert!(validate_query(&read, &format!("{QUERY}&weekStart={day}")).is_err());
    }
    assert!(validate_query(
        &read,
        &format!("{QUERY}&weekStart=2026-10-19&weekStart=2026-10-12")
    )
    .is_err());
    let insights = route_spec("audit-insights", &Method::GET).unwrap();
    assert!(validate_query(&insights, &format!("{QUERY}&weekStart=2026-10-19")).is_err());
}

#[test]
fn insights_and_reports_strip_private_ledger_and_work_evidence() {
    let read = route_spec("audit-insights", &Method::GET).unwrap();
    let safe=projection::sanitize(&read,&json!({"source":"marketing_audit_history","summary":{"unknown":1,"token":"private"},"items":[{"status":"unknown","proofAuditId":"audit_22222222222222222222222222222222","pageUrl":"https://triodelab.no/","private":"private","relatedUrls":["https://triodelab.no/other"]}],"alerts":[{"kind":"coverage_gap","reason":"timeout","authorization":"private"}],"processed":["private"],"token":"private"})).unwrap();
    assert_eq!(safe["items"][0]["status"], "unknown");
    assert_eq!(safe["summary"]["unknown"], 1);
    assert!(!safe.to_string().contains("private"));
    let read = route_spec("reports/weekly", &Method::GET).unwrap();
    let safe=projection::sanitize(&read,&json!({"source":"marketing_audit_history","work":{"open":1,"total":1,"tasks":[{"evidence":"private"}],"token":"private"},"receipts":[{"auditId":"audit_22222222222222222222222222222222","checkedPages":10,"html":"private"}],"weeks":"private"})).unwrap();
    assert_eq!(safe["work"]["open"], 1);
    assert!(!safe.to_string().contains("private"));
    assert_eq!(
        projection::sanitize(&read, &json!({"work":null})).unwrap()["work"],
        Value::Null
    );
}
