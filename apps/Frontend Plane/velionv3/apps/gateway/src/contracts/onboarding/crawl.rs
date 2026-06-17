use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CrawlPreviewRequest {
    pub(crate) url: String,
    #[serde(rename = "brief")]
    pub(crate) _brief: Option<String>,
    #[serde(alias = "max_pages")]
    pub(crate) max_pages: Option<u32>,
    #[serde(alias = "org_id")]
    pub(crate) org_id: Option<String>,
}
