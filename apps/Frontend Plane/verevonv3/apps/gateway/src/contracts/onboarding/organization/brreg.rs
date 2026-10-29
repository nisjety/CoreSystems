use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrregSearchQuery {
    pub(crate) q: String,
    pub(crate) size: Option<usize>,
}
