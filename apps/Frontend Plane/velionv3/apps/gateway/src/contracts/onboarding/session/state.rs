use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contracts::ActionActor;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OnboardingStateWriteRequest {
    pub(crate) actor: Option<ActionActor>,
    pub(crate) step: String,
    pub(crate) state: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OnboardingStateResponse {
    pub(crate) step: String,
    pub(crate) state: Option<Value>,
}
