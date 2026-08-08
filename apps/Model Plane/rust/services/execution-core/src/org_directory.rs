//! Auto-discovery of every organization a background worker must service.
//!
//! `approval_delivery_worker` is the first consumer: `ClaimApprovalDeliveries`
//! requires a credential scoped to the exact org being claimed (no "all orgs"
//! bypass exists for that call), so servicing every org means enumerating
//! them and minting one scoped token per org. That enumeration is this
//! module's whole job, backed by org-core's `GET /internal/orgs` —
//! the same `org:read:any` scope session-core/integration-corev2/billing-core
//! already hold for this class of read (see org-core's
//! `internal/http/service_auth.go`).

use std::time::Duration;

use serde::Deserialize;

const DEFAULT_ORG_CORE_URL: &str = "http://org-core:8080";
const PAGE_SIZE: u32 = 200;
/// Bounds total pages fetched regardless of what the server reports, so a
/// server-side `hasMore` bug can never turn this into an unbounded loop.
/// 50 * 200 = 10,000 orgs, a generous ceiling for the org count this system
/// is anywhere near today.
const MAX_PAGES: u32 = 50;

#[derive(Debug, Deserialize)]
struct OrgSummary {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListOrganizationsResponse {
    organizations: Vec<OrgSummary>,
    #[serde(default)]
    has_more: bool,
}

pub(crate) struct OrgDirectoryClient {
    base_url: String,
    service_id: String,
    service_token: String,
    http: reqwest::Client,
}

impl OrgDirectoryClient {
    /// Reads `ORG_CORE_URL` (defaults to the in-cluster address),
    /// `EXECUTION_CORE_SERVICE_ID` (shared with the session-core/shipping
    /// clients elsewhere in this crate), and
    /// `EXECUTION_ORG_CORE_SERVICE_TOKEN`. Returns `None` — not an
    /// error — when the token is unset, matching this crate's convention of
    /// idling quietly rather than failing startup when a background
    /// worker's dependency is unconfigured.
    #[must_use]
    pub(crate) fn from_env() -> Option<Self> {
        let base_url = std::env::var("ORG_CORE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_ORG_CORE_URL.to_owned());
        let service_id = std::env::var("EXECUTION_CORE_SERVICE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "execution-core".to_owned());
        let service_token = std::env::var("EXECUTION_ORG_CORE_SERVICE_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            service_id,
            service_token,
            http,
        })
    }

    #[cfg(test)]
    fn new_for_test(base_url: &str, service_id: &str, service_token: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            service_id: service_id.to_owned(),
            service_token: service_token.to_owned(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("test http client"),
        }
    }

    /// Every non-deleted organization's id, paged until org-core reports no
    /// more. Bounded by `MAX_PAGES` regardless of the server's own claim.
    pub(crate) async fn list_all_org_ids(&self) -> Result<Vec<String>, String> {
        let mut ids = Vec::new();
        let mut offset: u32 = 0;
        for _ in 0..MAX_PAGES {
            let response = self
                .http
                .get(format!("{}/internal/orgs", self.base_url))
                .query(&[("limit", PAGE_SIZE), ("offset", offset)])
                .header("x-service-id", &self.service_id)
                .header("x-service-token", &self.service_token)
                .send()
                .await
                .map_err(|error| format!("org-core /internal/orgs request failed: {error}"))?;
            let status = response.status();
            if !status.is_success() {
                return Err(format!("org-core /internal/orgs returned {status}"));
            }
            let page: ListOrganizationsResponse = response
                .json()
                .await
                .map_err(|error| format!("org-core /internal/orgs decode failed: {error}"))?;
            let page_len = page.organizations.len();
            ids.extend(page.organizations.into_iter().map(|org| org.id));
            if !page.has_more || page_len == 0 {
                return Ok(ids);
            }
            offset += PAGE_SIZE;
        }
        Err(format!(
            "org-core /internal/orgs did not terminate within {MAX_PAGES} pages"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn pages_until_has_more_is_false() {
        let org_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/orgs"))
            .and(query_param("limit", "200"))
            .and(query_param("offset", "0"))
            .and(header("x-service-id", "execution-core"))
            .and(header("x-service-token", "secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "organizations": [{"id": "org-1"}, {"id": "org-2"}],
                "count": 2,
                "hasMore": true
            })))
            .expect(1)
            .mount(&org_core)
            .await;
        Mock::given(method("GET"))
            .and(path("/internal/orgs"))
            .and(query_param("limit", "200"))
            .and(query_param("offset", "200"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "organizations": [{"id": "org-3"}],
                "count": 1,
                "hasMore": false
            })))
            .expect(1)
            .mount(&org_core)
            .await;

        let client = OrgDirectoryClient::new_for_test(&org_core.uri(), "execution-core", "secret");
        let ids = client.list_all_org_ids().await.unwrap();
        assert_eq!(ids, vec!["org-1", "org-2", "org-3"]);
    }

    #[tokio::test]
    async fn an_empty_first_page_stops_immediately() {
        let org_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/orgs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "organizations": [],
                "count": 0,
                "hasMore": false
            })))
            .expect(1)
            .mount(&org_core)
            .await;

        let client = OrgDirectoryClient::new_for_test(&org_core.uri(), "execution-core", "secret");
        assert_eq!(client.list_all_org_ids().await.unwrap(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn a_non_success_status_is_an_error() {
        let org_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/orgs"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&org_core)
            .await;

        let client = OrgDirectoryClient::new_for_test(&org_core.uri(), "execution-core", "secret");
        assert!(client.list_all_org_ids().await.is_err());
    }
}
