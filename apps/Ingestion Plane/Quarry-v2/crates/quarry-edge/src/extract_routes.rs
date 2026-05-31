//! `/v1/extract` — multi-URL structured extraction (OSS-parity P1 2A).
//!
//! Firecrawl-`extract`-style: take a set of URLs + an optional JSON Schema and
//! return structured data per source. Agent-harness shape: **bounded fan-out**
//! (capped URL count), per-source isolation (one failure never sinks the
//! batch), and a typed per-source result envelope.
//!
//! Scope note: wildcard expansion (`domain/*`) is the two-step `/v1/map` →
//! `/v1/extract` flow for Velion — call `/v1/map` to discover URLs, then pass
//! them here. This route owns the extract fan-out + schema enforcement.
//!
//! Read-only (no durable writes), org-scoped for metering, ZDR-safe.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};
use url::Url;

use quarry_core::zdr::ZdrMode;
use quarry_runtime::ai_formats::AiFormatRunner;
use quarry_runtime::mp_client::ModelPlaneClient;

use crate::state::AppState;

const DEFAULT_MAX_URLS: usize = 10;
const MAX_MAX_URLS: usize = 25;
const MARKDOWN_CAP: usize = 20_000;

#[derive(Debug, Deserialize)]
pub struct ExtractRequest {
    pub urls: Vec<String>,
    /// JSON Schema for structured extraction. When omitted, the route returns
    /// cleaned markdown per source instead of structured data.
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
    /// Optional natural-language hint (advisory; structured extraction is
    /// schema-driven).
    #[serde(default)]
    #[allow(dead_code)] // scaffolding: folded into schema-guided prompt in follow-up
    pub prompt: Option<String>,
    #[serde(default)]
    pub max_urls: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ExtractItem {
    pub url: String,
    /// "ok" (structured), "fetched" (markdown only), or "error".
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExtractResponse {
    pub results: Vec<ExtractItem>,
    pub count: usize,
    pub requested: usize,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Validate + dedup + cap the target URL list (bounded fan-out). Drops
/// non-http(s) and unparseable URLs. Order-preserving.
pub(crate) fn expand_targets(urls: &[String], max: usize) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for raw in urls {
        let trimmed = raw.trim();
        match Url::parse(trimmed) {
            Ok(u) if matches!(u.scheme(), "http" | "https") => {
                if seen.insert(u.as_str().to_string()) {
                    out.push(u.as_str().to_string());
                    if out.len() >= max {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

async fn fetch_markdown(state: &AppState, url: &Url) -> Result<String, String> {
    match state.driver.fetch(url).await {
        Ok(resp) if (200..300).contains(&resp.status) => {
            let html = String::from_utf8_lossy(&resp.body);
            let mut md = quarry_transform::readability::html_to_readable_markdown(&html);
            if md.len() > MARKDOWN_CAP {
                md.truncate(MARKDOWN_CAP);
            }
            Ok(md)
        }
        Ok(resp) => Err(format!("upstream status {}", resp.status)),
        Err(e) => Err(format!("fetch failed: {e}")),
    }
}

pub async fn extract(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<ExtractRequest>,
) -> impl IntoResponse {
    let max = req.max_urls.map(|m| m as usize).unwrap_or(DEFAULT_MAX_URLS).clamp(1, MAX_MAX_URLS);
    let targets = expand_targets(&req.urls, max);
    if targets.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "no valid http(s) URLs in `urls`".into(),
                code: "BAD_REQUEST".into(),
                hint: None,
            }),
        )
            .into_response();
    }

    // Structured extraction needs Model Plane. Without it, fall back to
    // returning cleaned markdown (still useful), unless a schema was requested.
    let runner: Option<AiFormatRunner> = match state.model_plane_url.as_deref() {
        Some(url) if !url.is_empty() => match ModelPlaneClient::new(url) {
            Ok(c) => {
                let c = match state.model_plane_token.as_deref().filter(|t| !t.is_empty()) {
                    Some(tok) => c.with_bearer_token(tok),
                    None => c,
                };
                Some(AiFormatRunner::new(Arc::new(c)))
            }
            Err(_) => None,
        },
        _ => None,
    };

    if req.schema.is_some() && runner.is_none() {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(ErrorBody {
                error: "structured extraction requires a Model Plane".into(),
                code: "UNSUPPORTED".into(),
                hint: Some("set MODEL_PLANE_URL, or omit `schema` to receive markdown".into()),
            }),
        )
            .into_response();
    }

    let mut results = Vec::with_capacity(targets.len());
    for raw in &targets {
        let url = match Url::parse(raw) {
            Ok(u) => u,
            Err(_) => continue,
        };
        let item = match fetch_markdown(&state, &url).await {
            Err(e) => ExtractItem {
                url: raw.clone(),
                status: "error".into(),
                data: None,
                markdown: None,
                error: Some(e),
            },
            Ok(md) => match (&req.schema, &runner) {
                (Some(schema), Some(r)) => match r.json(&md, schema.clone(), ZdrMode::Off).await {
                    Ok(jr) => ExtractItem {
                        url: raw.clone(),
                        status: "ok".into(),
                        data: Some(jr.data),
                        markdown: None,
                        error: None,
                    },
                    Err(e) => ExtractItem {
                        url: raw.clone(),
                        status: "error".into(),
                        data: None,
                        markdown: None,
                        error: Some(e.message),
                    },
                },
                _ => ExtractItem {
                    url: raw.clone(),
                    status: "fetched".into(),
                    data: None,
                    markdown: Some(md),
                    error: None,
                },
            },
        };
        results.push(item);
    }

    // Usage metering — one unit per source attempted.
    let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
    state
        .usage
        .meter(quarry_runtime::UsageEvent::new(
            run_id.to_string(),
            claims.org_id.clone(),
            quarry_runtime::usage_metrics::SEARCH_QUERY,
            targets.len() as f64,
            serde_json::json!({
                "op": "extract",
                "user_id": claims.user_id,
                "urls": targets.len(),
                "structured": req.schema.is_some(),
            }),
        ))
        .await;

    let count = results.len();
    (
        StatusCode::OK,
        Json(ExtractResponse { results, count, requested: targets.len() }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_dedups_and_caps() {
        let urls = vec![
            "https://a.com/1".into(),
            "https://a.com/1".into(), // dup
            "ftp://a.com/x".into(),   // non-http
            "not a url".into(),       // invalid
            "https://a.com/2".into(),
            "https://a.com/3".into(),
        ];
        let got = expand_targets(&urls, 2);
        assert_eq!(got, vec!["https://a.com/1".to_string(), "https://a.com/2".to_string()]);
    }

    #[test]
    fn expand_empty_when_all_invalid() {
        let urls = vec!["nope".into(), "mailto:x@y.com".into()];
        assert!(expand_targets(&urls, 10).is_empty());
    }
}
