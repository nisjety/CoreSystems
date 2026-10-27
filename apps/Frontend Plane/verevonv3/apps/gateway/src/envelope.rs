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
struct ApiErrorWithDetailsEnvelope {
    error: ApiErrorWithDetails,
}

#[derive(Debug, Serialize)]
struct ApiErrorWithDetails {
    code: &'static str,
    message: String,
    details: Value,
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

/// Success envelope carrying a keyset cursor in `meta.next_cursor`. `null`
/// means the page was the last one, so the client stops asking for more.
pub(crate) fn ok_with_cursor<T: Serialize>(data: T, next_cursor: &str) -> Value {
    let next_cursor = if next_cursor.is_empty() {
        Value::Null
    } else {
        json!(next_cursor)
    };
    json!({
        "data": serde_json::to_value(data).unwrap_or(Value::Null),
        "meta": { "next_cursor": next_cursor },
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

/// Typed error envelope with a `details` object: `{ error: { code, message, details } }`.
/// `details` must already be safe for the browser (no upstream URLs, tokens or internal ids).
#[allow(dead_code, reason = "first consumer is the next Space slice (WS01-S5, WS03-S4)")]
pub(crate) fn error_with_details(
    code: &'static str,
    message: impl Into<String>,
    details: Value,
) -> Value {
    serde_json::to_value(ApiErrorWithDetailsEnvelope {
        error: ApiErrorWithDetails {
            code,
            message: message.into(),
            details,
        },
    })
    .unwrap_or_else(|_| json!({ "error": { "code": code, "message": "Unknown error.", "details": {} } }))
}

/// Public response for transport failures. `reqwest::Error` display strings can
/// contain configured upstream URLs, query identifiers, and internal topology;
/// callers receive only this bounded category.
pub(crate) fn upstream_unavailable() -> Value {
    error(
        "upstream_unavailable",
        "The upstream service is unavailable.",
    )
}

#[cfg(test)]
mod tests {
    use super::{ok_with_cursor, upstream_unavailable};
    use serde_json::{json, Value};

    #[test]
    fn cursor_envelope_reports_null_on_the_last_page() {
        let last = ok_with_cursor(json!({ "sessions": [] }), "");
        assert_eq!(last["meta"]["next_cursor"], Value::Null);
        let more = ok_with_cursor(json!({ "sessions": [] }), "abc-_123");
        assert_eq!(more["meta"]["next_cursor"], "abc-_123");
        assert_eq!(more["data"]["sessions"], json!([]));
    }

    #[test]
    fn upstream_transport_failures_use_a_bounded_public_message() {
        let body = upstream_unavailable();
        assert_eq!(body["error"]["code"], "upstream_unavailable");
        assert_eq!(
            body["error"]["message"],
            "The upstream service is unavailable."
        );
        assert!(!body.to_string().contains("http://"));
        assert!(!body.to_string().contains("https://"));
    }

    #[test]
    fn error_with_details_carries_the_given_details_object() {
        let body = super::error_with_details(
            "invalid_post",
            "The post is not valid.",
            json!({ "field": "content", "max_chars": 8000 }),
        );
        assert_eq!(body["error"]["code"], "invalid_post");
        assert_eq!(body["error"]["message"], "The post is not valid.");
        assert_eq!(
            body["error"]["details"],
            json!({ "field": "content", "max_chars": 8000 })
        );
        assert_eq!(body.as_object().map(|o| o.len()), Some(1));
        assert_eq!(body["error"].as_object().map(|o| o.len()), Some(3));
    }
}
