use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecommendationText {
    pub(crate) reason: String,
    pub(crate) summary: String,
    pub(crate) proof_points: Vec<String>,
    pub(crate) scope_signals: Vec<String>,
    pub(crate) opportunities: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslateRecommendationRequest {
    pub(crate) source_language: Option<String>,
    pub(crate) target_language: String,
    pub(crate) recommendation: RecommendationText,
}
