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
    /// Additional onboarding signals the SPA sends beyond the typed fields above
    /// (e.g. `dataPlane` graph evidence, `goal`, `industry`, `orgForm`,
    /// `branding`, `websitePages`). Captured verbatim and flattened back out so
    /// they reach the Model Plane recommendation prompt. The typed fields drive
    /// the local-fallback heuristic; this carries the rest through for the AI.
    #[serde(flatten, default)]
    pub(crate) extra: serde_json::Map<String, serde_json::Value>,
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
    #[serde(default)]
    pub(crate) sources: Vec<String>,
    #[serde(alias = "source_count")]
    pub(crate) source_count: Option<u32>,
}
