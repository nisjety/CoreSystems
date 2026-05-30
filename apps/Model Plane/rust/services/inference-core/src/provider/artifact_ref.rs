//! ArtifactRef — opaque pointer to multimodal output stored outside the
//! request/response envelope.
//!
//! Multimodal providers (speech, vision, doc-intel) can produce binary or
//! large-text outputs that are too costly to stream inline through the
//! gateway. The provider trait surfaces them as `ArtifactRef` instead:
//! a URI plus content-type, size, and a content hash. The artifact bytes
//! live in an object store (S3, MinIO, blob, or in-memory for tests) the
//! caller is expected to know how to dereference.
//!
//! Under ZDR (`InferRequest.zdr = true`) providers must avoid producing
//! durable artifacts — either return the bytes inline (and trust the
//! gateway's ephemeral routing) or short-circuit with `ProviderError`.

#![allow(dead_code)] // artifact-store abstraction is scaffolding; wired up when multimodal outputs land

use std::collections::HashMap;
use std::sync::Mutex;

use super::ProviderError;

/// Opaque reference to an artifact stored outside the message envelope.
///
/// The `uri` scheme tells the consumer how to dereference: typical values
/// are `s3://bucket/key`, `blob://volume/path`, `inmem://artifact-id`. The
/// scheme is intentionally unconstrained so deployments can plug in their
/// own backends.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ArtifactRef {
    pub uri: String,
    pub content_type: String,
    pub size_bytes: u64,
    /// Hex-encoded SHA-256 of the artifact bytes; empty if not computed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha256: String,
    /// Optional ttl hint for ephemeral / ZDR artifacts. Backends are free
    /// to ignore; consumers should treat absence as "indefinite".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u32>,
}

impl ArtifactRef {
    /// Construct a ref without validation. Prefer [`Self::try_new`] for
    /// inputs from untrusted sources — `new` is intended for tests and
    /// for callers that have already validated the URI shape.
    #[must_use]
    pub fn new(uri: impl Into<String>, content_type: impl Into<String>, size_bytes: u64) -> Self {
        Self {
            uri: uri.into(),
            content_type: content_type.into(),
            size_bytes,
            sha256: String::new(),
            ttl_seconds: None,
        }
    }

    /// Validating constructor. Rejects:
    ///
    ///   * empty `uri` or `content_type`,
    ///   * `uri` without a `<scheme>://` prefix (so consumers always
    ///     have a way to dispatch to the right backend),
    ///   * embedded ASCII control characters in `uri` or `content_type`
    ///     (defends against header-injection-style smuggling when the
    ///     ref is later interpolated into URLs or HTTP headers).
    ///
    /// `size_bytes == 0` is permitted — empty artifacts are a valid
    /// outcome (e.g. a zero-page extraction).
    pub fn try_new(
        uri: impl Into<String>,
        content_type: impl Into<String>,
        size_bytes: u64,
    ) -> Result<Self, ProviderError> {
        let uri = uri.into();
        let content_type = content_type.into();

        if uri.is_empty() {
            return Err(ProviderError::InvalidResponse(
                "artifact uri is empty".to_owned(),
            ));
        }
        if content_type.is_empty() {
            return Err(ProviderError::InvalidResponse(
                "artifact content_type is empty".to_owned(),
            ));
        }
        // Require an explicit scheme so consumers can dispatch to a backend.
        let scheme_end = uri.find("://").ok_or_else(|| {
            ProviderError::InvalidResponse(format!("artifact uri missing scheme: {uri}"))
        })?;
        if scheme_end == 0 {
            return Err(ProviderError::InvalidResponse(format!(
                "artifact uri has empty scheme: {uri}"
            )));
        }
        if uri.chars().any(|c| c.is_ascii_control()) {
            return Err(ProviderError::InvalidResponse(
                "artifact uri contains control characters".to_owned(),
            ));
        }
        if content_type.chars().any(|c| c.is_ascii_control()) {
            return Err(ProviderError::InvalidResponse(
                "artifact content_type contains control characters".to_owned(),
            ));
        }

        Ok(Self {
            uri,
            content_type,
            size_bytes,
            sha256: String::new(),
            ttl_seconds: None,
        })
    }

    #[must_use]
    pub fn with_sha256(mut self, sha256: impl Into<String>) -> Self {
        self.sha256 = sha256.into();
        self
    }

    #[must_use]
    pub fn with_ttl(mut self, ttl_seconds: u32) -> Self {
        self.ttl_seconds = Some(ttl_seconds);
        self
    }
}

/// Backend that stores artifact bytes and returns a dereferenceable
/// `ArtifactRef`. Implementations are expected to compute or accept a
/// content hash for idempotency. ZDR-aware backends should consult the
/// `ttl_seconds` hint.
#[async_trait::async_trait]
pub trait ArtifactStore: Send + Sync {
    async fn put(
        &self,
        bytes: &[u8],
        content_type: &str,
        ttl_seconds: Option<u32>,
    ) -> Result<ArtifactRef, ProviderError>;

    async fn get(&self, artifact_ref: &ArtifactRef) -> Result<Vec<u8>, ProviderError>;
}

/// In-memory `ArtifactStore` for tests and dev. Not durable; cleared when
/// the process exits. URIs use the `inmem://` scheme.
#[derive(Default)]
pub struct InMemoryArtifactStore {
    items: Mutex<HashMap<String, (Vec<u8>, String)>>,
}

impl InMemoryArtifactStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl ArtifactStore for InMemoryArtifactStore {
    async fn put(
        &self,
        bytes: &[u8],
        content_type: &str,
        ttl_seconds: Option<u32>,
    ) -> Result<ArtifactRef, ProviderError> {
        let id = mp_ids::new_ulid();
        let uri = format!("inmem://{id}");
        let sha = blake3::hash(bytes).to_hex().to_string();
        self.items
            .lock()
            .map_err(|e| ProviderError::Unavailable(format!("artifact store poisoned: {e}")))?
            .insert(uri.clone(), (bytes.to_vec(), content_type.to_owned()));
        let mut r = ArtifactRef::try_new(uri, content_type, bytes.len() as u64)?.with_sha256(sha);
        if let Some(ttl) = ttl_seconds {
            r = r.with_ttl(ttl);
        }
        Ok(r)
    }

    async fn get(&self, artifact_ref: &ArtifactRef) -> Result<Vec<u8>, ProviderError> {
        self.items
            .lock()
            .map_err(|e| ProviderError::Unavailable(format!("artifact store poisoned: {e}")))?
            .get(&artifact_ref.uri)
            .map(|(b, _)| b.clone())
            .ok_or_else(|| {
                ProviderError::Unavailable(format!("artifact not found: {}", artifact_ref.uri))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_ref_builders_work() {
        let r = ArtifactRef::new("s3://b/k", "audio/mpeg", 1024)
            .with_sha256("abc")
            .with_ttl(60);
        assert_eq!(r.uri, "s3://b/k");
        assert_eq!(r.content_type, "audio/mpeg");
        assert_eq!(r.size_bytes, 1024);
        assert_eq!(r.sha256, "abc");
        assert_eq!(r.ttl_seconds, Some(60));
    }

    #[test]
    fn artifact_ref_serializes_without_optional_fields() {
        let r = ArtifactRef::new("inmem://a", "image/png", 42);
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("sha256"));
        assert!(!json.contains("ttl_seconds"));
    }

    #[tokio::test]
    async fn in_memory_store_round_trips_bytes() {
        let store = InMemoryArtifactStore::new();
        let r = store.put(b"hello", "text/plain", None).await.unwrap();
        assert!(r.uri.starts_with("inmem://"));
        assert_eq!(r.size_bytes, 5);
        assert!(!r.sha256.is_empty());

        let got = store.get(&r).await.unwrap();
        assert_eq!(got, b"hello");
    }

    #[tokio::test]
    async fn in_memory_store_propagates_ttl_hint() {
        let store = InMemoryArtifactStore::new();
        let r = store
            .put(b"x", "application/octet-stream", Some(30))
            .await
            .unwrap();
        assert_eq!(r.ttl_seconds, Some(30));
    }

    #[tokio::test]
    async fn in_memory_store_returns_error_for_unknown_uri() {
        let store = InMemoryArtifactStore::new();
        let unknown = ArtifactRef::new("inmem://does-not-exist", "x", 0);
        assert!(store.get(&unknown).await.is_err());
    }

    #[test]
    fn try_new_rejects_empty_uri() {
        let err = ArtifactRef::try_new("", "image/png", 1).unwrap_err();
        assert!(matches!(err, ProviderError::InvalidResponse(_)));
    }

    #[test]
    fn try_new_rejects_empty_content_type() {
        let err = ArtifactRef::try_new("s3://b/k", "", 1).unwrap_err();
        assert!(matches!(err, ProviderError::InvalidResponse(_)));
    }

    #[test]
    fn try_new_rejects_uri_without_scheme() {
        let err = ArtifactRef::try_new("/local/path/file.bin", "x", 1).unwrap_err();
        assert!(matches!(err, ProviderError::InvalidResponse(_)));
    }

    #[test]
    fn try_new_rejects_uri_with_empty_scheme() {
        let err = ArtifactRef::try_new("://no-scheme/x", "x", 1).unwrap_err();
        assert!(matches!(err, ProviderError::InvalidResponse(_)));
    }

    #[test]
    fn try_new_rejects_control_chars_in_uri() {
        let err = ArtifactRef::try_new("s3://b/k\nInjected: yes", "x", 1).unwrap_err();
        assert!(matches!(err, ProviderError::InvalidResponse(_)));
    }

    #[test]
    fn try_new_rejects_control_chars_in_content_type() {
        let err = ArtifactRef::try_new("s3://b/k", "image/png\r\n", 1).unwrap_err();
        assert!(matches!(err, ProviderError::InvalidResponse(_)));
    }

    #[test]
    fn try_new_accepts_zero_size() {
        // Empty artifacts (e.g. zero-page extraction) are still valid refs.
        let r = ArtifactRef::try_new("inmem://abc", "application/json", 0).unwrap();
        assert_eq!(r.size_bytes, 0);
    }

    #[test]
    fn try_new_accepts_well_formed_uri() {
        assert!(ArtifactRef::try_new("s3://bucket/key", "audio/mpeg", 1024).is_ok());
        assert!(ArtifactRef::try_new("inmem://abc-123", "application/json", 8).is_ok());
        assert!(ArtifactRef::try_new("blob://vol/path/to/file", "image/png", 256).is_ok());
    }
}
