use serde_json::Value;

#[derive(Debug)]
pub(super) struct CrawlPayload {
    pub(super) kind: String,
    // Carried for parity with the upstream payload shape and for the derived
    // `Debug` impl; nothing in this crate reads it outside logging yet.
    #[allow(dead_code)]
    pub(super) source: Option<String>,
    pub(super) value: Option<Value>,
}
