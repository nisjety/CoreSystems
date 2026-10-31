use super::*;
use serde_json::json;

#[test]
fn trends_contract_requires_explicit_bounded_admin_research_and_strips_secrets() {
    let spec=route_spec("trends", &Method::POST).unwrap();
    assert!(spec.administrative);
    assert!(route_spec("trends", &Method::GET).is_none());
    let command=json!({"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":2,"terms":["SEO"],"geo":"NO","timeframe":"today 12-m"});
    assert!(validate_command(&spec,&serde_json::to_vec(&command).unwrap()).is_ok());
    for (key,value) in [("terms",json!(["a","a"])),("terms",json!([])),("geo",json!("http://localhost")),("timeframe",json!("now 1-H")),("token",json!("private"))] {
        let mut bad=command.clone();bad[key]=value;
        assert!(validate_command(&spec,&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let safe=projection::sanitize(&spec,&json!({"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","report":{"source":"pytrends_dev","status":"available","terms":["SEO"],"rows":[{"date":"2026-01-01","values":[0],"partial":true,"cookie":"private"}],"rawHTML":"private"},"organizationId":"private"})).unwrap();
    assert_eq!(safe["report"]["rows"][0]["values"][0],0);
    assert!(!safe.to_string().contains("private"));
}

#[test]
fn automatic_trends_is_a_scoped_read_and_only_projects_derived_evidence() {
    let spec=route_spec("trends/automatic", &Method::GET).unwrap();
    assert!(!spec.administrative);
    assert!(route_spec("trends/automatic", &Method::POST).is_none());
    let query="websiteId=web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&site=https%3A%2F%2Ftriodelab.no%2F";
    assert!(validate_query(&spec,query).is_ok());
    assert!(validate_query(&spec,&format!("{query}&organizationId=forged")).is_err());
    let safe=projection::sanitize(&spec,&json!({"source":"marketing_trends_auto","seedKey":"private","seed":{"source":"google_search_console","selection":"impressions_then_clicks","terms":["SEO"],"period":{"startDate":"2026-10-01","endDate":"2026-10-21","connectionId":"private"}},"report":{"status":"available","terms":["SEO"],"rows":[{"date":"2026-10-01","values":[0],"partial":false,"raw":"private"}]}})).unwrap();
    assert_eq!(safe["seed"]["terms"][0],"SEO");
    assert!(!safe.to_string().contains("private"));
}
