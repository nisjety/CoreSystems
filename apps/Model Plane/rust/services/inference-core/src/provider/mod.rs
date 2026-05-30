//! Provider routing traits and implementations.

pub mod anthropic;
pub mod artifact_ref;
pub mod doc_intel;
pub mod fallback;
pub mod language;
pub mod openai;
pub mod realtime;
pub mod speech;
pub mod translation;
pub mod video;
pub mod vision;

#[allow(unused_imports)]
// ArtifactStore is part of the intended provider surface; not yet consumed
pub use artifact_ref::{ArtifactRef, ArtifactStore};

use tokio::sync::mpsc;

/// A unified inference request used internally across providers.
#[derive(Debug, Clone)]
pub struct InferRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: i32,
    pub structured_output_schema: Option<String>,
    pub zdr: bool,
}

/// A single chat message.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
}

/// Unified inference response.
#[derive(Debug, Clone)]
pub struct InferResponse {
    pub request_id: String,
    pub content: String,
    pub model_used: String,
    pub stop_reason: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
}

/// A single streaming chunk.
#[derive(Debug, Clone)]
pub struct InferChunk {
    pub request_id: String,
    pub delta: String,
    pub done: bool,
    pub model_used: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
}

/// A unified embedding request used internally across providers.
#[derive(Debug, Clone)]
pub struct EmbedRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub text: String,
    pub model: String,
}

/// Unified embedding response.
#[derive(Debug, Clone)]
pub struct EmbedResponse {
    pub request_id: String,
    pub vector: Vec<f32>,
    pub model_used: String,
    pub provider_used: String,
}

/// Provider model/deployment metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    pub modality: String,
    pub streaming: bool,
}

/// Introspectable feature flags for a provider.
///
/// Added per `docs/capability-ownership-matrix.md` §G4 (shape adapted from
/// OpenAI Codex `ProviderCapabilities`, Apache-2.0). Lets the router and
/// capability-core policy gate modality/feature use *by querying the
/// provider* instead of hardcoding per-provider knowledge at the call site —
/// the prerequisite for clean multimodal routing (Phase 5) and routing
/// policies (Phase 2).
// Intended provider surface; constructed once routing/policy consumes it
// (Phase 2/5) — same "not yet consumed" convention as `ArtifactStore` above.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProviderCapabilities {
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_thinking: bool,
    pub supports_streaming: bool,
    pub supports_embeddings: bool,
    /// Modality groups served, e.g. `["chat", "vision", "speech"]`.
    pub modalities: Vec<String>,
    pub max_context_tokens: u32,
    pub max_output_tokens: u32,
}

impl Default for ProviderCapabilities {
    /// Conservative chat-only baseline. Providers override `capabilities()`
    /// to advertise more — defaulting low means an unconfigured provider is
    /// never *assumed* to support a modality it cannot serve.
    fn default() -> Self {
        Self {
            supports_tools: false,
            supports_vision: false,
            supports_thinking: false,
            supports_streaming: true,
            supports_embeddings: false,
            modalities: vec!["chat".to_owned()],
            max_context_tokens: 8_192,
            max_output_tokens: 4_096,
        }
    }
}

impl ProviderCapabilities {
    /// True if this provider advertises the named modality group.
    #[allow(dead_code)] // intended surface; consumed by router/policy (Phase 2/5)
    #[must_use]
    pub fn serves_modality(&self, modality: &str) -> bool {
        self.modalities.iter().any(|m| m == modality)
    }
}

/// Provider routing trait for inference backends.
#[async_trait::async_trait]
pub trait ProviderRouter: Send + Sync {
    /// Perform a unary inference call.
    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError>;

    /// Perform a streaming inference call, sending chunks to the returned receiver.
    async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError>;

    /// Create an embedding vector. Providers that do not support embeddings
    /// should keep the default unsupported response.
    async fn create_embedding(&self, req: &EmbedRequest) -> Result<EmbedResponse, ProviderError> {
        Err(ProviderError::UnsupportedModel(format!(
            "embedding:{}",
            req.model
        )))
    }

    /// Return provider models known at startup.
    fn list_models(&self) -> Vec<ModelInfo> {
        Vec::new()
    }

    /// Advertise this provider's feature/modality capabilities. Defaults to
    /// the conservative chat-only baseline; multimodal providers override.
    #[allow(dead_code)] // intended surface; consumed by router/policy (Phase 2/5)
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }
}

/// Errors from provider operations.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("http error: {0}")]
    Http(String),

    #[error("rate limited: retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("provider unavailable: {0}")]
    Unavailable(String),

    #[error("all providers exhausted after {attempts} total attempts")]
    AllExhausted { attempts: u32 },

    #[allow(dead_code)]
    #[error("unsupported model: {0}")]
    UnsupportedModel(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_capabilities_are_conservative_chat_only() {
        let caps = ProviderCapabilities::default();
        assert!(caps.serves_modality("chat"));
        assert!(!caps.serves_modality("vision"));
        assert!(!caps.supports_vision);
        assert!(!caps.supports_tools);
        // streaming is the one safe-on default (virtually all chat providers).
        assert!(caps.supports_streaming);
    }

    #[test]
    fn capabilities_serialize_to_json() {
        let caps = ProviderCapabilities {
            supports_tools: true,
            supports_vision: true,
            modalities: vec!["chat".to_owned(), "vision".to_owned()],
            ..Default::default()
        };
        let json = serde_json::to_string(&caps).expect("serialize");
        assert!(json.contains("\"supports_vision\":true"));
        assert!(caps.serves_modality("vision"));
    }
}
