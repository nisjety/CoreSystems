//! Caption-to-text: describe a video in words so its MOTION becomes retrievable.
//!
//! ## Why this exists
//!
//! No video-text embedding tower distinguishes a video from its own reverse.
//! Measured locally: SigLIP 2 frame pooling, X-CLIP's `video_embeds`, and
//! X-CLIP's own multiframe pooler all return cosine 1.000000 on an exact frame
//! reversal. Published: on RTime-Binary, CLIP scores 49.1%, UMT 49.8% and
//! InternVideo2-1B 50.0% — chance, on a binary task, for a video-native dual
//! encoder trained on 50M pairs. Swapping towers does not fix it.
//!
//! So this routes around vision entirely. A vision model describes the video's
//! filmstrip in ordered prose — "a person walks to the door, then falls" — and
//! that prose is indexed as ordinary TEXT. Word order is something the text
//! arms genuinely model, so "someone falling" and "someone standing up" become
//! different queries against different text, which is exactly what the vector
//! towers could not express.
//!
//! Full analysis, candidate table and the validation gate:
//! `docs/core-research/video-temporal-retrieval-gap-2026-08-25.md`.
//!
//! ## Shape
//!
//! One vision call per video, not one per frame. `media-embedder`'s
//! `/filmstrip/video` tiles the sampled frames into a single labelled,
//! time-stamped sheet, so the model sees the progression instead of describing
//! frames in isolation with no idea what came before — and it costs 1 call
//! rather than N.
//!
//! ## ZDR
//!
//! This is the ONLY egressing path in the media arm. `media_consumer`'s header
//! records that the arm needs no ZDR guard because it embeds locally and never
//! egresses; captioning breaks that property, so the caller MUST check the
//! owning document's classification before asking. See
//! `media_consumer::caption_allowed_for_document`.
//!
//! OFF by default (`VIDEO_CAPTION_ENABLED`): it spends one vision call per video
//! segment, which is a real per-tenant cost.

use std::time::Duration;

use anyhow::Context as _;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use super::inference_auth::{InferenceTokenClient, RetentionPosture};
use super::model_plane;
use crate::config::Config;

/// Bumped when the prompt below changes in a way that alters the caption.
/// Recorded on the produced knowledge unit so an audit can tell which prompt
/// wrote a given caption, and a refresh pass can find superseded ones.
pub const PROMPT_VERSION: &str = "vidcap-v1";

/// The instruction. Two things are load-bearing:
///
/// * It states that the tiles are TIME-ORDERED and labelled, because otherwise a
///   vision model describes a grid of unrelated pictures.
/// * It asks for the ORDER of events explicitly. Order is the entire point — a
///   description that lists what appears without saying what happens first would
///   reproduce the bag-of-frames failure in text form.
fn build_prompt(frames: usize) -> String {
    format!(
        "This image is a filmstrip: {frames} frames sampled in time order from a \
         single video, laid out left-to-right then top-to-bottom, each labelled \
         with its index and timestamp. Describe what HAPPENS across the video as \
         a short factual narrative, in the order it occurs — name the actions, \
         movements and changes of state, and use ordering words such as \"then\" \
         and \"after\". Do not describe the layout, the labels, or the frames as \
         separate pictures. Do not speculate about anything not visible. Reply \
         with two to four sentences and nothing else."
    )
}

/// The text actually indexed for a captioned segment.
///
/// The timestamp line is prepended so a lexical hit can be cited to a position
/// in the media, matching what `start_ms`/`end_ms` already do for the vector
/// payload. It also keeps a caption self-describing if it is ever read on its
/// own in a trace.
pub fn compose_caption(start_ms: i64, end_ms: i64, narrative: &str) -> String {
    if end_ms > start_ms {
        format!(
            "[video {:.1}s–{:.1}s] {}",
            start_ms as f64 / 1000.0,
            end_ms as f64 / 1000.0,
            narrative.trim()
        )
    } else {
        format!("[video] {}", narrative.trim())
    }
}

#[derive(Clone)]
pub struct VideoDescriber {
    client: model_plane::v1::inference_core_client::InferenceCoreClient<Channel>,
    token_client: InferenceTokenClient,
    model: String,
    provider_hint: String,
    max_tokens: i32,
    timeout: Duration,
}

impl VideoDescriber {
    /// `Ok(None)` when the feature is off — the normal state, not an error.
    pub fn from_config(cfg: &Config) -> anyhow::Result<Option<Self>> {
        if !cfg.video_caption_enabled {
            return Ok(None);
        }
        if cfg.video_caption_model.trim().is_empty() {
            anyhow::bail!("VIDEO_CAPTION_MODEL is required when VIDEO_CAPTION_ENABLED=true");
        }
        let timeout = Duration::from_millis(cfg.video_caption_timeout_ms.max(1));
        // Same principal and retention posture the service already uses for
        // model-plane embedding, so enabling this grants no new authority.
        // Strict outside standalone startup for the same reason as
        // `contextualize`: an operator who asked for captions and supplied no
        // credential should learn that at boot.
        let posture = RetentionPosture::parse(&cfg.model_plane_inference_retention_posture)?;
        let token_client = if super::standalone_startup() {
            InferenceTokenClient::new_allow_unconfigured(
                &cfg.model_plane_inference_token_url,
                &cfg.model_plane_inference_token_issuer,
                &cfg.model_plane_inference_service_id,
                &cfg.model_plane_inference_service_api_key,
                posture,
            )?
        } else {
            InferenceTokenClient::new(
                &cfg.model_plane_inference_token_url,
                &cfg.model_plane_inference_token_issuer,
                &cfg.model_plane_inference_service_id,
                &cfg.model_plane_inference_service_api_key,
                posture,
            )?
        };
        let channel = Endpoint::from_shared(cfg.model_plane_ai_core_grpc_url.clone())
            .with_context(|| {
                format!(
                    "invalid MODEL_PLANE_AI_CORE_GRPC_URL `{}`",
                    cfg.model_plane_ai_core_grpc_url
                )
            })?
            .connect_timeout(timeout)
            .timeout(timeout)
            .connect_lazy();
        Ok(Some(Self {
            client: model_plane::v1::inference_core_client::InferenceCoreClient::new(channel),
            token_client,
            model: cfg.video_caption_model.trim().to_string(),
            provider_hint: cfg.video_caption_provider_hint.trim().to_string(),
            max_tokens: cfg.video_caption_max_tokens,
            timeout,
        }))
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// Describe one filmstrip. `Ok(None)` when the model returned nothing
    /// usable — a normal outcome meaning "no caption for this segment", not a
    /// failure worth propagating.
    ///
    /// `image` is raw JPEG bytes; `AnalyzeImage` takes them inline, so no
    /// publicly reachable URL has to exist for the frames.
    pub async fn describe(
        &self,
        org_id: &str,
        image: Vec<u8>,
        mime_type: &str,
        frames: usize,
    ) -> anyhow::Result<Option<String>> {
        let bearer = self
            .token_client
            .mint(org_id)
            .await
            .context("mint inference bearer for video caption")?;

        let request = model_plane::v1::AnalyzeImageRequest {
            request_id: Uuid::new_v4().to_string(),
            org_id: org_id.to_string(),
            // Inline bytes, not a URL: the filmstrip is generated on demand and
            // never persisted anywhere fetchable.
            image_url: String::new(),
            image_data: image,
            mime_type: mime_type.to_string(),
            prompt: build_prompt(frames),
            model: self.model.clone(),
            provider_hint: self.provider_hint.clone(),
            max_tokens: self.max_tokens,
        };

        let mut req = tonic::Request::new(request);
        req.set_timeout(self.timeout);
        let bearer_value: MetadataValue<_> = format!("Bearer {}", bearer.as_str())
            .parse()
            .context("inference bearer is not a valid header value")?;
        req.metadata_mut().insert("authorization", bearer_value);

        let response = self
            .client
            .clone()
            .analyze_image(req)
            .await
            .map_err(|status| {
                // Status only, never the body: a provider error can echo the
                // prompt, and for other calls that would mean echoing content.
                anyhow::anyhow!(
                    "model-plane analyze_image failed: {} ({})",
                    status.code(),
                    status.message()
                )
            })?
            .into_inner();

        let description = response.description.trim();
        if description.is_empty() {
            return Ok(None);
        }
        Ok(Some(description.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_config() -> Config {
        envy::from_iter::<_, Config>(vec![
            (
                "DATABASE_URL".to_string(),
                "postgres://test.invalid/test".to_string(),
            ),
            (
                "NATS_URL".to_string(),
                "nats://test.invalid:4222".to_string(),
            ),
            (
                "QDRANT_URL".to_string(),
                "http://test.invalid:6334".to_string(),
            ),
        ])
        .expect("minimal config")
    }

    /// The prompt has to establish that the tiles are one ordered sequence and
    /// ask for ordering. Without both, the model describes a grid of unrelated
    /// pictures and the caption reproduces the bag-of-frames failure in text.
    #[test]
    fn the_prompt_states_time_order_and_asks_for_it_back() {
        let prompt = build_prompt(8);
        assert!(prompt.contains("8 frames"));
        assert!(prompt.contains("time order"));
        assert!(prompt.contains("in the order it occurs"));
        assert!(prompt.contains("then"), "must ask for ordering words");
        assert!(
            prompt.contains("Do not describe the layout"),
            "must suppress descriptions of the filmstrip itself"
        );
    }

    #[test]
    fn a_caption_carries_a_citable_timestamp_range() {
        let text = compose_caption(1_500, 4_000, "  A person walks, then falls.  ");
        assert_eq!(text, "[video 1.5s–4.0s] A person walks, then falls.");
    }

    /// Zero/absent timings are the common case for a whole-file segment, and
    /// must not render as a nonsense "0.0s–0.0s" range.
    #[test]
    fn a_segment_without_timings_omits_the_range() {
        assert_eq!(compose_caption(0, 0, "A door closes."), "[video] A door closes.");
        assert_eq!(
            compose_caption(5_000, 1_000, "Out of order."),
            "[video] Out of order.",
            "an end before its start is not a citable range"
        );
    }

    #[test]
    fn a_disabled_config_yields_no_describer_rather_than_an_error() {
        let cfg = minimal_config();
        assert!(
            !cfg.video_caption_enabled,
            "caption-to-text must default to OFF: one vision call per segment is a real cost"
        );
        assert!(VideoDescriber::from_config(&cfg)
            .expect("disabled is not an error")
            .is_none());
    }

    #[test]
    fn enabling_without_a_model_fails_closed() {
        let cfg = Config {
            video_caption_enabled: true,
            video_caption_model: "   ".to_string(),
            ..minimal_config()
        };
        let err = match VideoDescriber::from_config(&cfg) {
            Ok(_) => panic!("an enabled describer with no model must fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("VIDEO_CAPTION_MODEL"),
            "unexpected error: {err}"
        );
    }
}
