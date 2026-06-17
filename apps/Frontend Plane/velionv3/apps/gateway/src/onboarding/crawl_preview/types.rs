use serde_json::Value;

#[derive(Debug)]
pub(super) struct CrawlPayload {
    pub(super) kind: String,
    pub(super) source: Option<String>,
    pub(super) value: Option<Value>,
}
