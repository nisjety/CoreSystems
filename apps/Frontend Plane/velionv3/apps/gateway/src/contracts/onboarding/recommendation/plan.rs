use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanRecommendation {
    pub(crate) plan_id: &'static str,
    pub(crate) reason: String,
    pub(crate) summary: String,
    pub(crate) proof_points: Vec<String>,
    pub(crate) scope_signals: Vec<String>,
    pub(crate) opportunities: Vec<String>,
    pub(crate) generated_at: String,
    pub(crate) source: &'static str,
}
