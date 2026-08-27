//! Index-side audio/video embeddings via the self-hosted `media-embedder`.
//!
//! Counterpart to `provider/visual.rs`: that one embeds page images through
//! Cohere Embed v4, this one embeds audio and video segments through the local
//! CLAP/SigLIP 2 sidecar. The retrieval-engine's `embed/media.rs` embeds the
//! text query into the same shared space, so stored media and text queries are
//! directly comparable.
//!
//! Self-hosted is forced rather than chosen: per
//! `docs/core-research/embedding-modality-and-rag-audit-2026-08-19.md` §3 there
//! is no Azure audio-similarity embedding model at all, and Azure's video
//! analyzer produces descriptions rather than dense vectors. So unlike the text
//! arm there is no cloud provider to fall back to, and unlike the visual arm
//! there is no retaining third party to guard against — which is why this
//! provider has no ZDR egress guard: nothing egresses.
//!
//! Wire contract (`services/media-embedder/app.py`):
//! ```text
//! POST {endpoint}/embed/audio   { "urls": [str, ...] } -> { "embeddings": [[f32]], "dim": int }
//! POST {endpoint}/embed/video   { "urls": [str, ...] } -> { "embeddings": [[f32]], "dim": int }
//! ```

use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// Which tower to embed with. Audio and video are separate spaces with separate
/// collections and dimensions, so the caller always names one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Audio,
    Video,
}

impl MediaKind {
    const fn path(self) -> &'static str {
        match self {
            Self::Audio => "/embed/audio",
            Self::Video => "/embed/video",
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }
}

#[derive(Clone)]
pub struct MediaEmbeddingProvider {
    http: Client,
    endpoint: String,
}

impl std::fmt::Debug for MediaEmbeddingProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaEmbeddingProvider")
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

#[derive(Serialize)]
struct UrlsRequest<'a> {
    urls: Vec<&'a str>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

/// One video's frames tiled into a single time-ordered image, for
/// caption-to-text. See `docs/core-research/video-temporal-retrieval-gap-2026-08-25.md`.
#[derive(Debug, Deserialize)]
pub struct Filmstrip {
    /// Base64 JPEG of the labelled tile sheet.
    pub image_base64: String,
    pub mime_type: String,
    /// How many frames actually made it into the sheet. Fewer than requested
    /// for very short or partially-decodable clips.
    pub frames: usize,
    /// Approximate timestamp of each tile, in order. Drawn onto the sheet too —
    /// carried here so a caller can cite a moment without re-parsing the image.
    #[serde(default)]
    pub seconds: Vec<f32>,
}

#[derive(Deserialize)]
struct FilmstripResponse {
    filmstrips: Vec<Filmstrip>,
}

/// Content hash for a generated video caption.
///
/// `knowledge_units.content_hash` is NOT NULL and every other writer fills it
/// with a hash of the row's text, so captions do the same. It also makes a
/// regenerated caption visibly different from an unchanged one, which is what a
/// future prompt-refresh pass would key on.
///
/// `DefaultHasher`, matching this crate's `make_idempotency_key`, rather than
/// index-engine's blake3 `content_hash`: nothing ever compares the two. Captions
/// live in their own id namespace (`:vidcap*`, negative `chunk_index`), so this
/// value only has to change when the caption changes, and adding a crypto-hash
/// dependency to buy nothing would be the worse trade.
pub fn caption_content_hash(caption: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    "video_caption".hash(&mut hasher);
    caption.hash(&mut hasher);
    format!("vidcap-{:016x}", hasher.finish())
}

impl MediaEmbeddingProvider {
    /// `Ok(None)` when no endpoint is configured — the audio/video arms are then
    /// dark, exactly like the visual arm without an Embed v4 endpoint.
    pub fn from_config(cfg: &Config) -> anyhow::Result<Option<Self>> {
        let endpoint = cfg.media_embedder_endpoint.trim().trim_end_matches('/');
        if endpoint.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            http: Client::builder()
                // Generous: decoding a minute of audio on CPU is slower than an
                // API round-trip, and the consumer's ack_wait is 120s.
                .timeout(Duration::from_secs(90))
                .build()
                .context("build media embed client")?,
            endpoint: endpoint.to_string(),
        }))
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Embed one or more media URLs. The sidecar fetches the bytes itself (same
    /// division of labour as ColQwen's reranker), so no bytes transit this
    /// process — only URLs go out and vectors come back.
    pub async fn embed_urls(&self, kind: MediaKind, urls: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if urls.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}{}", self.endpoint, kind.path());
        let resp = self
            .http
            .post(&url)
            .json(&UrlsRequest {
                urls: urls.to_vec(),
            })
            .send()
            .await
            .with_context(|| format!("{} embed call", kind.as_str()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("{} embed API {status}: {body}", kind.as_str());
        }
        let parsed: EmbedResponse = resp
            .json()
            .await
            .with_context(|| format!("parse {} embed response", kind.as_str()))?;
        Ok(parsed.embeddings)
    }

    /// Fetch one video's time-ordered filmstrip for caption-to-text.
    ///
    /// Deliberately one URL per call rather than a batch: the response carries a
    /// base64 JPEG per video, and batching would hold several megabytes of image
    /// in memory to save a round trip that is already cheap next to the vision
    /// call that follows.
    pub async fn filmstrip(&self, media_url: &str) -> anyhow::Result<Filmstrip> {
        let url = format!("{}/filmstrip/video", self.endpoint);
        let resp = self
            .http
            .post(&url)
            .json(&UrlsRequest {
                urls: vec![media_url],
            })
            .send()
            .await
            .context("filmstrip call")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("filmstrip API {status}: {body}");
        }
        let parsed: FilmstripResponse = resp.json().await.context("parse filmstrip response")?;
        parsed
            .filmstrips
            .into_iter()
            .next()
            .context("empty filmstrip response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(endpoint: &str) -> Config {
        serde_json::from_value(serde_json::json!({
            "database_url": "",
            "qdrant_url": "",
            "nats_url": "",
            "media_embedder_endpoint": endpoint,
        }))
        .expect("minimal config from defaults")
    }

    #[test]
    fn absent_endpoint_leaves_the_arms_dark() {
        assert!(MediaEmbeddingProvider::from_config(&cfg_with(""))
            .expect("ok")
            .is_none());
    }

    #[test]
    fn trailing_slash_is_normalised_so_paths_do_not_double_up() {
        let p = MediaEmbeddingProvider::from_config(&cfg_with("http://media-embedder:8095/"))
            .expect("ok")
            .expect("some");
        assert_eq!(p.endpoint(), "http://media-embedder:8095");
    }

    #[test]
    fn kinds_map_to_the_service_routes() {
        assert_eq!(MediaKind::Audio.path(), "/embed/audio");
        assert_eq!(MediaKind::Video.path(), "/embed/video");
    }

    #[tokio::test]
    async fn empty_input_short_circuits_without_a_call() {
        let p = MediaEmbeddingProvider::from_config(&cfg_with("http://127.0.0.1:1"))
            .expect("ok")
            .expect("some");
        // Port 1 would refuse instantly; an Ok here proves no request was made.
        let out = p.embed_urls(MediaKind::Audio, &[]).await.expect("no call");
        assert!(out.is_empty());
    }
}
