use serde::Deserialize;

use crate::contracts::ActionActor;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceDiscoveryRequest {
    pub(crate) actor: Option<ActionActor>,
    #[serde(alias = "org_id")]
    pub(crate) org_id: String,
    #[serde(alias = "connector_id")]
    pub(crate) connector_id: String,
    pub(crate) label: Option<String>,
    pub(crate) provider: String,
    pub(crate) sources: Vec<String>,
}
