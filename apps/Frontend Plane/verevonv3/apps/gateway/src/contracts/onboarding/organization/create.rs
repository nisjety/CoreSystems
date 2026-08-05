use serde::Deserialize;
use serde_json::Value;

use crate::contracts::ActionActor;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateOrganizationRequest {
    pub(crate) actor: Option<ActionActor>,
    pub(crate) name: String,
    pub(crate) plan: Option<String>,
    #[serde(alias = "org_number")]
    pub(crate) org_number: Option<String>,
    #[serde(alias = "brreg_data")]
    pub(crate) brreg_data: Option<Value>,
    pub(crate) metadata: Option<Value>,
}
