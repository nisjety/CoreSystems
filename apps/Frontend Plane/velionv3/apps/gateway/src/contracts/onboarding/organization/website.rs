use serde::Deserialize;

use crate::contracts::ActionActor;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebsiteIngestRequest {
    pub(crate) actor: Option<ActionActor>,
    #[serde(alias = "org_id")]
    pub(crate) org_id: String,
    pub(crate) url: String,
    pub(crate) brief: Option<String>,
    #[serde(alias = "max_pages")]
    pub(crate) max_pages: Option<u32>,
}
