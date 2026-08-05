use serde::Deserialize;

use super::context::RecommendContext;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecommendPlanRequest {
    pub(crate) context: RecommendContext,
}
