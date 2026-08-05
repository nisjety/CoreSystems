use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewNode {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) group: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewEdge {
    pub(crate) a: String,
    pub(crate) b: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphPreviewResponse {
    pub(crate) nodes: Vec<PreviewNode>,
    pub(crate) edges: Vec<PreviewEdge>,
    pub(crate) counts: GraphCounts,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphCounts {
    pub(crate) nodes: usize,
    pub(crate) edges: usize,
    pub(crate) groups: usize,
}
