use serde::Deserialize;
use serde_json::Value;

use crate::contracts::ActionActor;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompleteOnboardingRequest {
    pub(crate) actor: Option<ActionActor>,
    #[serde(alias = "org_id")]
    pub(crate) org_id: Option<String>,
    pub(crate) plan: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) metadata: Option<Value>,
}
