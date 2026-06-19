use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
struct ApiErrorEnvelope {
    error: ApiError,
}

#[derive(Debug, Serialize)]
struct ApiError {
    code: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
struct ApiSuccessEnvelope<T> {
    data: T,
}

pub(crate) fn unwrap_data(value: &Value) -> Value {
    value.get("data").cloned().unwrap_or_else(|| value.clone())
}

pub(crate) fn ok<T: Serialize>(data: T) -> Value {
    serde_json::to_value(ApiSuccessEnvelope { data }).unwrap_or_else(|_| json!({ "data": null }))
}

/// Success envelope carrying a `meta.source` hint. Used when an upstream core is
/// unavailable: the data is an honest empty payload and `meta.source` tells the
/// SPA the result is degraded (rather than fabricating data or erroring out).
pub(crate) fn ok_with_source<T: Serialize>(data: T, source: &str) -> Value {
    json!({
        "data": serde_json::to_value(data).unwrap_or(Value::Null),
        "meta": { "source": source },
    })
}

pub(crate) fn error(code: &'static str, message: impl Into<String>) -> Value {
    serde_json::to_value(ApiErrorEnvelope {
        error: ApiError {
            code,
            message: message.into(),
        },
    })
    .unwrap_or_else(|_| json!({ "error": { "code": code, "message": "Unknown error." } }))
}
