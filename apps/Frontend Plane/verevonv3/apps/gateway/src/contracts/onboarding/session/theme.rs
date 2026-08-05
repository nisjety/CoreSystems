use serde::Deserialize;

use crate::contracts::ActionActor;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateThemeRequest {
    pub(crate) actor: Option<ActionActor>,
    pub(crate) mode: String,
    #[serde(alias = "primary_color")]
    pub(crate) primary_color: String,
}
