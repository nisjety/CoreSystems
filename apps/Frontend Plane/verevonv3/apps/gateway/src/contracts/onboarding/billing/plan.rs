use serde::Deserialize;
use serde_json::Value;

use crate::contracts::ActionActor;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetPlanRequest {
    pub(crate) actor: Option<ActionActor>,
    #[serde(alias = "org_id")]
    pub(crate) org_id: String,
    pub(crate) plan: String,
    pub(crate) reason: Option<String>,
    pub(crate) onboarding: Option<Value>,
}
