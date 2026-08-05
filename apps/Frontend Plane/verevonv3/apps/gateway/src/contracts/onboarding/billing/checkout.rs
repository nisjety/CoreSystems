use serde::Deserialize;

use crate::contracts::ActionActor;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartCheckoutRequest {
    pub(crate) actor: Option<ActionActor>,
    #[serde(alias = "org_id")]
    pub(crate) org_id: String,
    pub(crate) plan: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfirmCheckoutRequest {
    pub(crate) actor: Option<ActionActor>,
    #[serde(alias = "org_id")]
    pub(crate) org_id: String,
    pub(crate) plan: String,
    #[serde(alias = "payment_id")]
    pub(crate) payment_id: Option<String>,
    #[serde(alias = "client_secret")]
    pub(crate) client_secret: Option<String>,
}
