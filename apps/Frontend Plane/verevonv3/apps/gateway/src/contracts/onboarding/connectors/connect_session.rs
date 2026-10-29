use serde::Deserialize;
use serde_json::Value;

use crate::contracts::ActionActor;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectSessionRequest {
    pub(crate) actor: Option<ActionActor>,
    #[serde(alias = "org_id")]
    pub(crate) org_id: String,
    pub(crate) provider: String,
    #[serde(alias = "selected_sources")]
    pub(crate) selected_sources: Vec<String>,
    pub(crate) bundles: Option<Vec<String>>,
    #[serde(alias = "provider_context")]
    pub(crate) provider_context: Option<Value>,
    pub(crate) shop: Option<String>,
}
