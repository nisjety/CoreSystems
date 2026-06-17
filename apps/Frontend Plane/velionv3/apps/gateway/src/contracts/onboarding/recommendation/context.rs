use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecommendContext {
    pub(crate) organization: Option<RecommendOrganization>,
    pub(crate) website: Option<RecommendWebsite>,
    pub(crate) websites: Option<Vec<RecommendWebsite>>,
    pub(crate) connectors: Option<Vec<RecommendConnector>>,
    #[serde(alias = "source_count")]
    pub(crate) source_count: Option<u32>,
    pub(crate) locale: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecommendOrganization {
    pub(crate) name: Option<String>,
    pub(crate) size: Option<String>,
    #[serde(alias = "employee_count")]
    pub(crate) employee_count: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecommendWebsite {
    pub(crate) url: Option<String>,
    #[serde(alias = "agent_brief")]
    pub(crate) agent_brief: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecommendConnector {
    pub(crate) id: String,
    pub(crate) label: String,
}
