//! Request normalization and validation for model-gateway.
//!
//! Trims whitespace, validates content length and model names against an allowlist.

use axum::{http::StatusCode, Json};
use serde_json::json;

use crate::http_routes::InvokeRequest;

/// Maximum allowed content size in bytes (100 KB).
const MAX_CONTENT_BYTES: usize = 100 * 1024;

/// Normalized request after validation.
#[derive(Debug)]
pub struct NormalizedRequest {
    pub content: String,
    pub model: String,
    #[allow(dead_code)]
    pub session_key: Option<String>,
    #[allow(dead_code)]
    pub thread_id: Option<String>,
    pub structured_output_schema: Option<String>,
    pub zdr: bool,
    pub max_cost_usd: Option<f64>,
    pub max_tokens: Option<u32>,
}

/// Load the model allowlist from `ALLOWED_MODELS` env var (comma-separated).
/// Returns `None` if the env var is not set (all models allowed).
fn load_allowed_models() -> Option<Vec<String>> {
    std::env::var("ALLOWED_MODELS").ok().map(|v| {
        v.split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    })
}

/// Load the default model from `DEFAULT_MODEL` env var.
fn load_default_model() -> String {
    std::env::var("DEFAULT_MODEL").unwrap_or_else(|_| "default".to_owned())
}

/// Normalize and validate an incoming invoke request.
///
/// Returns a `NormalizedRequest` on success, or an error response tuple on failure.
///
/// # Errors
/// Returns [`StatusCode::BAD_REQUEST`] when the request fails validation
/// (empty content, unknown modality, invalid parameters, etc.).
pub fn normalize(
    req: &InvokeRequest,
) -> Result<NormalizedRequest, (StatusCode, Json<serde_json::Value>)> {
    let content = req.content.trim().to_owned();

    // Validate content is not empty
    if content.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content must not be empty" })),
        ));
    }

    // Validate content length
    if content.len() > MAX_CONTENT_BYTES {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!(
                    "content exceeds maximum size of {} bytes (got {} bytes)",
                    MAX_CONTENT_BYTES,
                    content.len()
                )
            })),
        ));
    }

    // Resolve model name
    let default_model = load_default_model();
    let model = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map_or(default_model, str::to_owned);

    // Validate model against allowlist
    if let Some(allowed) = load_allowed_models() {
        if !allowed.is_empty() && !allowed.contains(&model) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!(
                        "model '{}' is not in the allowed list: {:?}",
                        model, allowed
                    )
                })),
            ));
        }
    }

    if let Some(max_cost) = req.max_cost_usd {
        if max_cost <= 0.0 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "max_cost_usd must be positive" })),
            ));
        }
    }
    if let Some(max_tokens) = req.max_tokens {
        if max_tokens == 0 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "max_tokens must be positive" })),
            ));
        }
    }

    Ok(NormalizedRequest {
        content,
        model,
        session_key: req.session_key.clone(),
        thread_id: req.thread_id.clone(),
        structured_output_schema: req.structured_output_schema.clone(),
        zdr: req.zdr,
        max_cost_usd: req.max_cost_usd,
        max_tokens: req.max_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(content: &str, model: Option<&str>) -> InvokeRequest {
        InvokeRequest {
            content: content.to_owned(),
            model: model.map(str::to_owned),
            session_key: None,
            thread_id: None,
            structured_output_schema: None,
            zdr: false,
            max_cost_usd: None,
            max_tokens: None,
            profile: None,
            features: Vec::new(),
            idempotency_key: None,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn trims_whitespace() {
        let req = make_request("  hello world  ", None);
        let result = normalize(&req).expect("should succeed");
        assert_eq!(result.content, "hello world");
    }

    #[test]
    fn rejects_empty_content() {
        let req = make_request("   ", None);
        assert!(normalize(&req).is_err());
    }

    #[test]
    fn rejects_oversized_content() {
        let content = "x".repeat(MAX_CONTENT_BYTES + 1);
        let req = make_request(&content, None);
        assert!(normalize(&req).is_err());
    }

    #[test]
    fn uses_default_model_when_none() {
        let req = make_request("hello", None);
        let result = normalize(&req).expect("should succeed");
        // Default model comes from env or hardcoded "default"
        assert!(!result.model.is_empty());
    }
}
