//! Synchronous, narrowly-scoped text embedding for short, bounded inputs
//! (e.g. a ticket's category/intent/summary) — distinct from the async
//! document/chunk embedding pipeline in `stream`/`batch`. This endpoint does
//! not persist the input text or the resulting vector; the caller owns
//! storage and lifecycle of whatever it does with the vector.

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::config::Config;
use crate::provider::EmbeddingProvider;

/// A ticket summary has no business being anywhere near document-sized; this
/// bounds the endpoint to short-text inputs only.
const MAX_TEXT_CHARS: usize = 2_000;

#[derive(Deserialize)]
pub struct EmbedTextRequest {
    pub org_id: String,
    pub text: String,
    #[serde(default)]
    pub zdr: bool,
}

#[derive(Serialize)]
pub struct EmbedTextResponse {
    pub vector: Vec<f32>,
    pub provider: &'static str,
    pub model: String,
}

#[derive(Serialize)]
pub struct EmbedTextError {
    pub error: String,
}

fn provider() -> Result<&'static EmbeddingProvider, anyhow::Error> {
    static PROVIDER: OnceLock<anyhow::Result<EmbeddingProvider>> = OnceLock::new();
    match PROVIDER.get_or_init(|| Config::from_env().and_then(|cfg| EmbeddingProvider::from_config(&cfg)))
    {
        Ok(provider) => Ok(provider),
        Err(err) => Err(anyhow::anyhow!("{err}")),
    }
}

pub async fn embed_text(
    Json(request): Json<EmbedTextRequest>,
) -> Result<Json<EmbedTextResponse>, (StatusCode, Json<EmbedTextError>)> {
    if request.org_id.trim().is_empty() {
        return Err(bad_request("org_id is required"));
    }
    let text = request.text.trim();
    if text.is_empty() {
        return Err(bad_request("text is required"));
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(bad_request("text exceeds the bounded short-text limit"));
    }

    let provider = provider().map_err(|err| {
        tracing::error!(error = %err, "embed_text: embedding provider unavailable");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(EmbedTextError {
                error: "embedding provider unavailable".into(),
            }),
        )
    })?;

    let vectors = provider
        .embed_batch(request.org_id.trim(), &[text.to_string()], request.zdr)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "embed_text: embedding request failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(EmbedTextError {
                    error: "embedding request failed".into(),
                }),
            )
        })?;
    let vector = vectors.into_iter().next().ok_or_else(|| {
        (
            StatusCode::BAD_GATEWAY,
            Json(EmbedTextError {
                error: "embedding provider returned no vector".into(),
            }),
        )
    })?;

    Ok(Json(EmbedTextResponse {
        vector,
        provider: provider.provider_name(),
        model: provider.model_name().to_string(),
    }))
}

fn bad_request(message: &str) -> (StatusCode, Json<EmbedTextError>) {
    (
        StatusCode::BAD_REQUEST,
        Json(EmbedTextError {
            error: message.to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_empty_org_id() {
        let result = embed_text(Json(EmbedTextRequest {
            org_id: "  ".into(),
            text: "billing question".into(),
            zdr: false,
        }))
        .await;
        assert!(matches!(result, Err((StatusCode::BAD_REQUEST, _))));
    }

    #[tokio::test]
    async fn rejects_empty_text() {
        let result = embed_text(Json(EmbedTextRequest {
            org_id: "org-1".into(),
            text: "   ".into(),
            zdr: false,
        }))
        .await;
        assert!(matches!(result, Err((StatusCode::BAD_REQUEST, _))));
    }

    #[tokio::test]
    async fn rejects_document_sized_text() {
        let result = embed_text(Json(EmbedTextRequest {
            org_id: "org-1".into(),
            text: "x".repeat(MAX_TEXT_CHARS + 1),
            zdr: false,
        }))
        .await;
        assert!(matches!(result, Err((StatusCode::BAD_REQUEST, _))));
    }
}
