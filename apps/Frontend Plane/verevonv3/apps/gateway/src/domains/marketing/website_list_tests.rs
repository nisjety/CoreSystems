use super::*;
use serde_json::json;

// The website list keeps the primary mark and where the primary came from, and drops what the registry
// never sends the browser.
#[test]
fn the_website_list_keeps_the_primary_mark_and_its_source_and_drops_everything_else() {
    let spec = route_spec("websites", &Method::GET).unwrap();
    let safe = projection::sanitize(
        &spec,
        &json!({
            "primarySource": "org-core",
            "websites": [{
                "id": "web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "site": "https://triodelab.no/", "revision": 8,
                "createdAt": "2026-10-24T00:00:00Z", "updatedAt": "2026-10-26T00:00:00Z", "primary": true,
                "ownership": {"status": "google_owner", "checkedAt": "2026-10-26T00:00:00Z", "token": "private"},
                "integrations": {
                    "searchConsole": {"status": "connected", "property": "sc-domain:triodelab.no", "accessLevel": "siteOwner", "connectionId": "private"},
                    "ga4": {"status": "connected", "propertyId": "560102923", "streamId": "16177443323", "measurementId": "G-ZEWPHF6X3K", "reportingSite": "https://www.triodelab.no/", "credential": "private"},
                    "clarity": {"status": "not_connected"},
                    "pageSpeed": {"status": "available", "raw": "private"}
                },
                "dns": {"record": "private"}
            }]
        }),
    )
    .expect("a registry answer in the contract");

    assert_eq!(safe["primarySource"], json!("org-core"));
    let site = &safe["websites"][0];
    assert_eq!(site["primary"], json!(true));
    assert_eq!(site["site"], json!("https://triodelab.no/"));
    let text = safe.to_string();
    for private in ["connectionId", "credential", "\"raw\"", "\"dns\"", "\"token\""] {
        assert!(!text.contains(private), "{private} reached the browser: {text}");
    }
}
