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
