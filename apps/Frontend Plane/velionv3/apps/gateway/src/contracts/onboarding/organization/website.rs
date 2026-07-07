use serde::Deserialize;

/// Onboarding "ingest my website" request. Tenant identity and the initiating
/// user are taken from the verified session / data-plane JWT claims on the edge
/// — not from the body — so `org_id`/`actor` are intentionally not accepted
/// here. Extra client fields (a legacy `orgId`, `brief`, `actor`) are ignored.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebsiteIngestRequest {
    pub(crate) url: String,
    #[serde(alias = "max_pages")]
    pub(crate) max_pages: Option<u32>,
}
