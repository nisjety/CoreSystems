use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActionActor {
    #[serde(default)]
    pub(crate) user_id: String,
    #[serde(default)]
    pub(crate) user_email: String,
    #[serde(default)]
    pub(crate) user_name: String,
    #[serde(default)]
    pub(crate) user_role: String,
}
