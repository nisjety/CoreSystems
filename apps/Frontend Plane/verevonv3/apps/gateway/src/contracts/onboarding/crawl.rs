use serde::Deserialize;

/// Onboarding crawl-preview request. The crawl runs under the verified session
/// org on quarry-edge, so `org_id` is not accepted from the body (a legacy
/// `orgId`/`brief` field is simply ignored).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CrawlPreviewRequest {
    pub(crate) url: String,
    #[serde(rename = "brief")]
    pub(crate) _brief: Option<String>,
    #[serde(alias = "max_pages")]
    pub(crate) max_pages: Option<u32>,
}
