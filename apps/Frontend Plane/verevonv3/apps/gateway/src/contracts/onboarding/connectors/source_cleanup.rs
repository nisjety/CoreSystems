use serde::Deserialize;

use crate::contracts::ActionActor;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceCleanupRequest {
    pub(crate) actor: Option<ActionActor>,
    #[serde(alias = "org_id")]
    pub(crate) org_id: String,
    #[serde(alias = "source_id")]
    pub(crate) source_id: String,
}
