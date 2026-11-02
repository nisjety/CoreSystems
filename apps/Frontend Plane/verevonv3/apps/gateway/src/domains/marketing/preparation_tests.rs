use super::{
    contract::{route_spec, validate_command, validate_query},
    projection::sanitize,
};
use axum::http::Method;
use serde_json::json;

#[test]
fn preparation_and_sources_have_closed_scoped_routes() {
    let query = "websiteId=web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&site=https%3A%2F%2Ftriodelab.no%2F";
    for route in ["preparation", "sources"] {
        let spec = route_spec(route, &Method::GET).expect("supported read");
        assert!(validate_query(&spec, query).is_ok());
        assert!(validate_query(&spec, &format!("{query}&orgId=other")).is_err());
        assert!(validate_query(
            &spec,
            &format!("{query}&websiteId=web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        )
        .is_err());
    }
    let command = route_spec("preparation", &Method::POST).expect("supported preparation");
    assert!(command.administrative);
    assert!(validate_command(&command,br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7}"#).is_ok());
    assert!(validate_command(&command,br#"{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":7,"token":"secret"}"#).is_err());
    assert!(route_spec("sources", &Method::POST).is_none());
    assert!(route_spec("preparation", &Method::PATCH).is_none());
}

#[test]
fn preparation_projection_discards_private_fields_recursively() {
    let spec = route_spec("preparation", &Method::GET).unwrap();
    let value=sanitize(&spec,&json!({"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source":"marketing_preparation","status":"queued","token":"secret","steps":[{"key":"discovery","status":"queued","internal":{"token":"secret"}}]})).unwrap();
    assert_eq!(
        value["steps"][0],
        json!({"key":"discovery","status":"queued"})
    );
    assert!(!value.to_string().contains("secret"));
    let spec = route_spec("sources", &Method::GET).unwrap();
    let value=sanitize(&spec,&json!({"items":[{"key":"bing_webmaster","implemented":false,"status":"not_implemented","connectionId":"private","refresh_token":"secret"}]})).unwrap();
    assert_eq!(
        value["items"][0],
        json!({"key":"bing_webmaster","implemented":false,"status":"not_implemented"})
    );
}
