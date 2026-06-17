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

pub(crate) fn error(code: &'static str, message: impl Into<String>) -> Value {
    serde_json::to_value(ApiErrorEnvelope {
        error: ApiError {
            code,
            message: message.into(),
        },
    })
    .unwrap_or_else(|_| json!({ "error": { "code": code, "message": "Unknown error." } }))
}
