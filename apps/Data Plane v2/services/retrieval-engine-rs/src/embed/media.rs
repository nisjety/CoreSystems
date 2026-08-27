//! Audio/video-arm **query** embeddings via the self-hosted `media-embedder`.
//!
//! The index side (embedding-engine) embeds audio and video *segments* into the
//! `dataplane_audio_segments` / `dataplane_video_segments` collections. To search
//! them we embed the TEXT query into the *same* shared space — LAION-CLAP and
//! SigLIP 2 are both cross-modal, so a text-query vector is directly
//! comparable to the stored media vectors, exactly as Embed v4 works for the
//! visual arm (`embed/visual.rs`).
//!
//! Which models, and why self-hosted, is settled in
//! `docs/core-research/embedding-modality-and-rag-audit-2026-08-19.md` §3: Azure
//! has no audio-similarity embedding product at all, and its video analyzer is
//! extraction/description rather than a dense embedder. There is no cloud option
//! to fall back to here.
//!
//! **ZDR:** unlike the visual arm, this one does not fail closed. `visual.rs`
//! must refuse ZDR queries because Embed v4 is a third-party endpoint that
//! retains; a self-hosted embedder never egresses, so ZDR content is embeddable
//! by construction. That closes — for these two modalities — the same class of
//! gap the audit records for images, where ZDR-flagged pages are silently
//! dropped for lack of a compliant path.
//!
//! Wire contract (mirrors `services/media-embedder/app.py`):
//! ```text
//! POST {endpoint}/embed/text
//! { "texts": ["<query>"], "space": "audio" }
//! → { "embeddings": [[ … ]], "dim": 512 }
//! ```

use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// Which shared space to project the query into. The towers are independent —
/// a CLAP text vector is meaningless against SigLIP 2 video vectors — so the
/// caller must name the arm it is searching rather than getting a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSpace {
    Audio,
    Video,
}

impl MediaSpace {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }
}

#[derive(Clone)]
pub struct MediaQueryEmbedder {
    http: Client,
    endpoint: String,
}

impl std::fmt::Debug for MediaQueryEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaQueryEmbedder")
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

#[derive(Serialize)]
struct TextEmbedRequest<'a> {
    texts: Vec<&'a str>,
    space: &'a str,
}

#[derive(Deserialize)]
struct TextEmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

impl MediaQueryEmbedder {
    /// Build from config. `None` when no endpoint is configured — the audio and
    /// video arms are then simply dark, the same way the ColQwen reranker and the
    /// visual embedder degrade rather than erroring. No key is required: the
    /// service is in-cluster and unauthenticated, like the other dpv2-private
    /// inference sidecars.
    #[must_use]
    pub fn from_config(cfg: &Config) -> Option<Self> {
        let endpoint = cfg.media_embedder_endpoint.trim().trim_end_matches('/');
        if endpoint.is_empty() {
            return None;
        }
        Some(Self {
            http: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .ok()?,
            endpoint: endpoint.to_string(),
        })
    }

    /// Embed a text query into the named media space.
    pub async fn embed_query(&self, text: &str, space: MediaSpace) -> anyhow::Result<Vec<f32>> {
        let url = format!("{}/embed/text", self.endpoint);
        let body = TextEmbedRequest {
            texts: vec![text],
            space: space.as_str(),
        };
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("media query embed call")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("media query embed API {status}: {body_text}");
        }
        let parsed: TextEmbedResponse = resp
            .json()
            .await
            .context("parse media query embed response")?;
        parsed
            .embeddings
            .into_iter()
            .next()
            .context("empty media query embed response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(endpoint: &str) -> Config {
        serde_json::from_value(serde_json::json!({
            "database_url": "",
            "qdrant_url": "",
            "media_embedder_endpoint": endpoint,
        }))
        .expect("minimal config from defaults")
    }

    #[test]
    fn absent_endpoint_leaves_the_arms_dark() {
        assert!(MediaQueryEmbedder::from_config(&cfg_with("")).is_none());
        assert!(MediaQueryEmbedder::from_config(&cfg_with("   ")).is_none());
    }

    #[test]
    fn configured_endpoint_builds_and_normalises_trailing_slash() {
        let e = MediaQueryEmbedder::from_config(&cfg_with("http://media-embedder:8095/"))
            .expect("embedder");
        assert_eq!(e.endpoint, "http://media-embedder:8095");
    }

    #[test]
    fn space_maps_to_the_wire_value_the_service_expects() {
        assert_eq!(MediaSpace::Audio.as_str(), "audio");
        assert_eq!(MediaSpace::Video.as_str(), "video");
    }
}
