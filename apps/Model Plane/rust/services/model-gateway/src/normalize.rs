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
    /// Server-injected Control decision for a newly-created scoped thread.
    /// Session Core is still the trust boundary and verifies its signature.
    pub space_context: Option<crate::session_flow::ThreadSpaceContext>,
    pub space_append_context: Option<crate::session_flow::ThreadSpaceContext>,
    pub structured_output_schema: Option<String>,
    pub zdr: bool,
    /// Caller-selected minimum privacy tier (Venice-style). `None` is
    /// `UNSPECIFIED` and keeps today's behavior byte-identical; `Some(tier)`
    /// is threaded onto every downstream InferRequest and enforced by
    /// inference-core's chain (fail-closed, never a silent downgrade).
    pub min_privacy_tier: Option<mp_contracts::model_plane::v1::PrivacyTier>,
    pub max_cost_usd: Option<f64>,
    pub max_tokens: Option<u32>,
}

/// Wire numeric (`model_plane.v1.PrivacyTier`) of the caller-selected minimum
/// tier, 0 (= UNSPECIFIED) when none was expressed. This is the exact value
/// inference-core validates/enforces downstream, so every transport threads
/// this one function's result rather than re-deriving its own encoding.
#[must_use]
pub fn min_privacy_tier_wire(req: &NormalizedRequest) -> i32 {
    req.min_privacy_tier.map_or(0, |tier| tier as i32)
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

/// Load the default model from `DEFAULT_MODEL` env var. Shared with the SSE
/// stream path so it resolves an unspecified model identically to the unary
/// path. When unset, returns an EMPTY string on purpose: inference-core's
/// fallback chain then resolves "Verevon Auto" to the configured provider's own
/// default (per-provider), so chat works against any single provider without an
/// operator pinning a model. (The previous literal "default" was not a real
/// model id and made every unpinned request fail provider lookup.)
pub(crate) fn load_default_model() -> String {
    std::env::var("DEFAULT_MODEL")
        .ok()
        .map(|m| m.trim().to_owned())
        .unwrap_or_default()
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

    // Validate model against allowlist — but skip when the model is unspecified
    // (empty): inference-core resolves it to a provider default downstream, so
    // there is nothing to validate against the allowlist yet.
    if let Some(allowed) = load_allowed_models() {
        if !model.is_empty() && !allowed.is_empty() && !allowed.contains(&model) {
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

    // Fail closed at the edge on an unknown tier numeric: a NEWER client naming
    // a tier this build does not know must be refused here rather than silently
    // treated as no constraint downstream.
    let min_privacy_tier = match req.min_privacy_tier {
        None | Some(0) => None,
        Some(value) => {
            let tier = mp_contracts::model_plane::v1::PrivacyTier::try_from(value).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("unknown privacy tier value: {value}"),
                    })),
                )
            })?;
            // 0 (UNSPECIFIED) imposes no constraint, mirroring inference-core:
            // GLOBAL=1 is a real floor and IS honored.
            (tier != mp_contracts::model_plane::v1::PrivacyTier::Unspecified).then_some(tier)
        }
    };

    Ok(NormalizedRequest {
        content,
        model,
        session_key: req.session_key.clone(),
        thread_id: req.thread_id.clone(),
        space_context: req.space_context.clone(),
        space_append_context: req.space_append_context.clone(),
        structured_output_schema: req.structured_output_schema.clone(),
        zdr: req.zdr,
        min_privacy_tier,
        max_cost_usd: req.max_cost_usd,
        max_tokens: req.max_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(content: &str, model: Option<&str>) -> InvokeRequest {
        InvokeRequest {
            regenerated: false,
            edited_resubmit: false,
            content: content.to_owned(),
            model: model.map(str::to_owned),
            session_key: None,
            thread_id: None,
            space_context: None,
            space_append_context: None,
            structured_output_schema: None,
            zdr: false,
            min_privacy_tier: None,
            browse_web: false,
            deep_research: false,
            max_cost_usd: None,
            max_tokens: None,
            profile: None,
            verbosity: None,
            features: Vec::new(),
            idempotency_key: None,
            attachments: Vec::new(),
            generate_image: false,
            plan_mode: false,
            tools: Vec::new(),
            org_name: None,
            user_name: None,
            agent_name: None,
            agent_system_prompt: None,
            org_instructions: None,
            space_instructions: None,
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
    fn unspecified_model_resolves_to_configured_default() {
        let req = make_request("hello", None);
        let result = normalize(&req).expect("should succeed");
        // Unspecified model → the gateway's configured default: `DEFAULT_MODEL`
        // env when set, else empty so inference-core resolves a per-provider
        // default downstream ("Verevon Auto"). Deterministic regardless of env.
        assert_eq!(result.model, load_default_model());
    }

    #[test]
    fn explicit_model_is_preserved() {
        let req = make_request("hello", Some("claude-sonnet-4-20250514"));
        let result = normalize(&req).expect("should succeed");
        assert_eq!(result.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn absent_and_unspecified_tiers_normalize_to_no_constraint() {
        // Absent field and explicit UNSPECIFIED (0) both mean "no constraint";
        // GLOBAL=1 is a real floor and must survive normalization.
        let none = normalize(&make_request("hi", None)).expect("ok");
        let unspecified = {
            let mut req = make_request("hi", None);
            req.min_privacy_tier = Some(0);
            normalize(&req).expect("ok")
        };
        let global = {
            let mut req = make_request("hi", None);
            req.min_privacy_tier = Some(1);
            normalize(&req).expect("ok")
        };
        let sovereign = {
            let mut req = make_request("hi", None);
            req.min_privacy_tier = Some(4);
            normalize(&req).expect("ok")
        };

        assert!(none.min_privacy_tier.is_none());
        assert!(unspecified.min_privacy_tier.is_none());
        assert_eq!(min_privacy_tier_wire(&none), 0);
        assert_eq!(min_privacy_tier_wire(&global), 1);
        assert_eq!(min_privacy_tier_wire(&sovereign), 4);
    }

    #[test]
    fn an_unknown_tier_numeric_fails_closed_at_the_edge() {
        for unknown in [-1, 5, 42, i32::MAX] {
            let mut req = make_request("hi", None);
            req.min_privacy_tier = Some(unknown);
            let error = normalize(&req).expect_err("unknown tier must be refused");
            let message = error.1 .0.to_string();
            assert!(
                message.contains("unknown privacy tier"),
                "got: {message}"
            );
        }
    }
}
