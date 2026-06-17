use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphPreviewQuery {
    #[serde(alias = "org_id")]
    pub(crate) org_id: String,
}
