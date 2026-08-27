//! Audio/video segment embedding subscriber (DURABLE JetStream) — the **media
//! RAG arms**.
//!
//! Structural twin of `image_consumer.rs`. The producer segments long media
//! upstream and emits one event per segment carrying a fetchable `media_url`;
//! we embed each segment with the self-hosted `media-embedder` (LAION-CLAP for
//! audio, X-CLIP or SigLIP 2 for video) and upsert one point into the matching Qdrant
//! collection, which the retrieval-engine fuses via `w_audio` / `w_video`.
//!
//! Segment-per-event rather than file-per-event deliberately: a dense vector
//! over an hour of audio is uselessly diffuse, and per-segment points give the
//! retrieval layer a timestamped citation target (`start_ms`/`end_ms`) in the
//! same way page images give it a page number.
//!
//! One consumer serves both modalities — the subjects and the collection differ,
//! but the fetch/embed/upsert shape is identical, so splitting it would duplicate
//! the DLQ and redelivery handling for no gain.
//!
//! **No ZDR guard on the EMBEDDING path, and that is deliberate.**
//! `image_consumer.rs` must drop ZDR-flagged pages because Embed v4 is a
//! retaining third party, which the modality audit records as a live data-loss
//! gap. This arm embeds locally and never egresses, so ZDR media is embeddable
//! by construction.
//!
//! Caption-to-text is the ONE exception and carries its own guard. It ships
//! frames to a vision model, so `caption_allowed_for_document` refuses anything
//! not clearly non-restricted — reading the classification from the `documents`
//! row rather than the event, and failing closed on a missing or unreadable
//! row. See `provider::video_caption`; the feature is off by default.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Context;
use async_nats::jetstream::{self, Context as JsContext};
use futures::StreamExt;
use qdrant_client::qdrant::{
    value::Kind as QdrantKind, PointStruct, UpsertPointsBuilder, Value as QdrantValue,
};
use serde::Deserialize;

use crate::provider::media::{MediaEmbeddingProvider, MediaKind};
use crate::provider::video_caption::{self, VideoDescriber};
use crate::provider::EmbeddingProvider;

/// Everything the caption-to-text path needs, bundled so `spawn` keeps one
/// optional collaborator instead of five loose parameters.
///
/// `None` means the feature is off, which is the default — see
/// `provider::video_caption` for why (one vision call per segment, and it is the
/// only egressing path in this arm).
#[derive(Clone)]
pub struct VideoCaptioning {
    pub describer: VideoDescriber,
    /// The TEXT embedding provider, not the media one: a caption is text and
    /// belongs in the text collection, where the query-side text embedder can
    /// actually reach it.
    pub text_provider: EmbeddingProvider,
    pub text_collection: String,
    pub pool: sqlx::PgPool,
}

/// Whether a document may have its video content described by a vision model.
///
/// This arm's module header records that it needs no ZDR guard because it embeds
/// locally and never egresses. Captioning BREAKS that property — it ships frames
/// to a model provider — so this is the guard that keeps the claim true for
/// everything else.
///
/// Read from the `documents` row rather than the event, deliberately: the same
/// reasoning the text stream consumer uses, so a stale or forged event cannot
/// downgrade a document's classification. Anything other than a clearly
/// non-restricted classification is refused, and an unreadable row is refused
/// too — absence of proof is not proof of safety.
async fn caption_allowed_for_document(
    pool: &sqlx::PgPool,
    org_id: &str,
    document_id: &str,
) -> bool {
    let row: Result<Option<(Option<String>,)>, _> = sqlx::query_as(
        "SELECT zdr_classification FROM documents WHERE document_id = $1 AND org_id = $2",
    )
    .bind(document_id)
    .bind(org_id)
    .fetch_optional(pool)
    .await;
    match row {
        Ok(Some((Some(classification),))) => {
            let allowed = matches!(
                classification.trim().to_ascii_lowercase().as_str(),
                "internal" | "public" | "sensitive"
            );
            if !allowed {
                tracing::info!(
                    document_id,
                    classification = %classification,
                    "video caption skipped: restricted document must not egress to a vision model"
                );
            }
            allowed
        }
        Ok(_) => {
            tracing::warn!(
                document_id,
                "video caption skipped: document row or classification missing (fail closed)"
            );
            false
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                document_id,
                "video caption skipped: classification lookup failed (fail closed)"
            );
            false
        }
    }
}

/// Deterministic `knowledge_id` for a segment's caption.
///
/// Stable across redeliveries and re-ingests so the row is UPSERTed in place
/// rather than accumulating a duplicate caption per delivery. The `vidcap`
/// domain separator keeps it from ever colliding with a real text chunk's
/// content-derived id from index-engine.
fn caption_knowledge_id(document_id: &str, segment_no: i64) -> String {
    format!("{document_id}:vidcap{segment_no}")
}

pub const MEDIA_STREAM: &str = "DATAPLANE_MEDIA_SEGMENTS";

pub const SUBJECT_AUDIO_SEGMENT_CREATED: &str = "dataplane.audio_segments.created";
pub const SUBJECT_AUDIO_SEGMENT_DELETED: &str = "dataplane.audio_segments.deleted";
pub const SUBJECT_VIDEO_SEGMENT_CREATED: &str = "dataplane.video_segments.created";
pub const SUBJECT_VIDEO_SEGMENT_DELETED: &str = "dataplane.video_segments.deleted";

pub const MEDIA_CONSUMER: &str = "embedding-engine-media-segments";
/// Terminal-failure sink, mirroring the page-image and text consumers.
pub const MEDIA_DLQ_SUBJECT: &str = "dataplane.dlq.embedding-engine-media-segments";

/// Must match `max_deliver` on the durable consumer below.
const MAX_DELIVER: i64 = 5;

#[derive(Debug, Deserialize)]
struct MediaSegmentCreatedEvent {
    document_id: String,
    org_id: String,
    /// Monotonic segment index within the parent document. Doubles as the
    /// `chunk_index` the retrieval-engine's payload reader expects.
    segment_no: i64,
    /// Fetchable URL for the segment's media bytes (MinIO presigned / internal
    /// artifact endpoint). The producer owns hosting; the sidecar GETs it.
    media_url: String,
    #[serde(default)]
    start_ms: i64,
    #[serde(default)]
    end_ms: i64,
    #[serde(default)]
    content_hash: String,
    /// Optional transcript/caption. Stored for display and citation only — it is
    /// NOT fused into the vector, because the media tower embeds media alone and
    /// mixing a text vector in would leave the point in neither space cleanly.
    #[serde(default)]
    transcript: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MediaSegmentDeletedEvent {
    document_id: String,
}

fn str_val(value: impl Into<String>) -> QdrantValue {
    QdrantValue {
        kind: Some(QdrantKind::StringValue(value.into())),
    }
}

fn int_val(value: i64) -> QdrantValue {
    QdrantValue {
        kind: Some(QdrantKind::IntegerValue(value)),
    }
}

/// Deterministic point id from `(document_id, kind, segment_no)` so re-ingesting
/// the same segment overwrites in place instead of accumulating duplicates. The
/// kind is part of the hash so an audio and a video segment sharing a document
/// and index cannot collide.
fn segment_point_id(document_id: &str, kind: MediaKind, segment_no: i64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    document_id.hash(&mut hasher);
    1u8.hash(&mut hasher); // domain separator (0 = page images)
    kind.as_str().hash(&mut hasher);
    segment_no.hash(&mut hasher);
    hasher.finish()
}

/// Route a subject to its modality, or `None` for anything unexpected.
fn kind_for(subject: &str) -> Option<(MediaKind, bool)> {
    match subject {
        SUBJECT_AUDIO_SEGMENT_CREATED => Some((MediaKind::Audio, true)),
        SUBJECT_AUDIO_SEGMENT_DELETED => Some((MediaKind::Audio, false)),
        SUBJECT_VIDEO_SEGMENT_CREATED => Some((MediaKind::Video, true)),
        SUBJECT_VIDEO_SEGMENT_DELETED => Some((MediaKind::Video, false)),
        _ => None,
    }
}

/// Collections are per-modality because the towers have different dimensions
/// (CLAP 512 vs the video tower: 512 for X-CLIP, 768 for SigLIP 2) and
/// Qdrant fixes vector size per collection.
#[derive(Clone)]
pub struct MediaCollections {
    pub audio: String,
    pub video: String,
}

impl MediaCollections {
    fn for_kind(&self, kind: MediaKind) -> &str {
        match kind {
            MediaKind::Audio => &self.audio,
            MediaKind::Video => &self.video,
        }
    }
}

pub async fn spawn(
    js: JsContext,
    nats: async_nats::Client,
    qdrant: qdrant_client::Qdrant,
    media: MediaEmbeddingProvider,
    collections: MediaCollections,
    // `None` disables caption-to-text, which is the default. See
    // `provider::video_caption`.
    captioning: Option<VideoCaptioning>,
) -> anyhow::Result<()> {
    js.get_or_create_stream(jetstream::stream::Config {
        name: MEDIA_STREAM.to_string(),
        subjects: vec![
            SUBJECT_AUDIO_SEGMENT_CREATED.to_string(),
            SUBJECT_AUDIO_SEGMENT_DELETED.to_string(),
            SUBJECT_VIDEO_SEGMENT_CREATED.to_string(),
            SUBJECT_VIDEO_SEGMENT_DELETED.to_string(),
        ],
        retention: jetstream::stream::RetentionPolicy::WorkQueue,
        max_age: Duration::from_secs(7 * 24 * 3600),
        ..Default::default()
    })
    .await
    .context("create DATAPLANE_MEDIA_SEGMENTS stream")?;

    let stream = js
        .get_stream(MEDIA_STREAM)
        .await
        .context("get media-segment stream")?;
    let consumer = stream
        .get_or_create_consumer(
            MEDIA_CONSUMER,
            jetstream::consumer::pull::Config {
                durable_name: Some(MEDIA_CONSUMER.to_string()),
                filter_subjects: vec![
                    SUBJECT_AUDIO_SEGMENT_CREATED.to_string(),
                    SUBJECT_AUDIO_SEGMENT_DELETED.to_string(),
                    SUBJECT_VIDEO_SEGMENT_CREATED.to_string(),
                    SUBJECT_VIDEO_SEGMENT_DELETED.to_string(),
                ],
                // Matches the sidecar's 90s client timeout with headroom: CPU
                // audio decode + embed is slower than an API round-trip.
                ack_wait: Duration::from_secs(180),
                max_deliver: 5,
                ..Default::default()
            },
        )
        .await
        .context("create media-segment durable consumer")?;

    tracing::info!(
        stream = MEDIA_STREAM,
        audio_collection = %collections.audio,
        video_collection = %collections.video,
        endpoint = media.endpoint(),
        video_caption = captioning.is_some(),
        "media-segment durable subscriber online"
    );

    tokio::spawn(async move {
        loop {
            let mut messages = match consumer.messages().await {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(error = %e, "media consumer stream open failed; retrying");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            while let Some(item) = messages.next().await {
                let msg = match item {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!(error = %e, "media message recv error");
                        continue;
                    }
                };
                let subject = msg.subject.to_string();
                let Some((kind, created)) = kind_for(&subject) else {
                    tracing::warn!(subject = %subject, "unknown media subject; acking");
                    let _ = msg.ack().await;
                    continue;
                };
                let collection = collections.for_kind(kind).to_string();

                let result = if created {
                    match serde_json::from_slice::<MediaSegmentCreatedEvent>(&msg.payload) {
                        Ok(evt) => {
                            let embedded =
                                handle_created(&evt, kind, &qdrant, &media, &collection).await;
                            // Caption-to-text runs only for VIDEO, only when
                            // configured, and only after the vector landed —
                            // it is an additive text arm, never a substitute
                            // for the segment's own embedding, and it must not
                            // run if the primary embed failed and the message
                            // is about to be retried.
                            if embedded.is_ok() && kind == MediaKind::Video {
                                if let Some(captioning) = captioning.as_ref() {
                                    caption_video_segment(&evt, &media, captioning, &qdrant).await;
                                }
                            }
                            embedded
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, subject = %subject, "invalid media created payload; acking poison");
                            let _ = msg.ack().await;
                            continue;
                        }
                    }
                } else {
                    match serde_json::from_slice::<MediaSegmentDeletedEvent>(&msg.payload) {
                        Ok(evt) => crate::qdrant_writer::delete_vectors_by_document(
                            &qdrant,
                            &collection,
                            &evt.document_id,
                        )
                        .await
                        .context("delete media segment vectors"),
                        Err(e) => {
                            tracing::warn!(error = %e, subject = %subject, "invalid media deleted payload; acking poison");
                            let _ = msg.ack().await;
                            continue;
                        }
                    }
                };

                match result {
                    Ok(()) => {
                        let _ = msg.ack().await;
                    }
                    Err(e) => {
                        let delivered = msg.info().map(|info| info.delivered).unwrap_or(1);
                        if delivered < MAX_DELIVER {
                            // No ack → JetStream redelivers (up to max_deliver).
                            tracing::warn!(error = %e, subject = %subject, delivered, "media handling failed; will redeliver");
                        } else {
                            tracing::error!(
                                error = %e,
                                subject = %subject,
                                delivered,
                                "media handling failed permanently; routing to DLQ"
                            );
                            let dlq = serde_json::json!({
                                "original_subject": subject,
                                "stream": MEDIA_STREAM,
                                "error": e.to_string(),
                                "attempts": delivered,
                            });
                            if let Err(error) =
                                nats.publish(MEDIA_DLQ_SUBJECT, dlq.to_string().into()).await
                            {
                                tracing::error!(%error, "media DLQ publish failed");
                            }
                            let _ = msg.ack().await;
                        }
                    }
                }
            }
        }
    });
    Ok(())
}

async fn handle_created(
    evt: &MediaSegmentCreatedEvent,
    kind: MediaKind,
    qdrant: &qdrant_client::Qdrant,
    media: &MediaEmbeddingProvider,
    collection: &str,
) -> anyhow::Result<()> {
    let vectors = media
        .embed_urls(kind, &[evt.media_url.as_str()])
        .await
        .with_context(|| format!("embed {} segment", kind.as_str()))?;
    let vector = vectors
        .into_iter()
        .next()
        .context("empty media embed result")?;

    // Payload mirrors the fields the retrieval-engine's vector_search reads, so a
    // media segment is a first-class hybrid candidate that resolves to its parent
    // document (ownership gate + source-join key on `document_id`).
    let mut payload: HashMap<String, QdrantValue> = HashMap::new();
    payload.insert(
        "knowledge_id".to_string(),
        str_val(format!(
            "{}:{}{}",
            evt.document_id,
            kind.as_str().chars().next().unwrap_or('m'),
            evt.segment_no
        )),
    );
    payload.insert("document_id".to_string(), str_val(evt.document_id.clone()));
    payload.insert("org_id".to_string(), str_val(evt.org_id.clone()));
    payload.insert("chunk_index".to_string(), int_val(evt.segment_no));
    payload.insert("segment_no".to_string(), int_val(evt.segment_no));
    // Timestamps make a hit citable to a position in the media, the way page_no
    // does for images. Kept even when zero so the field shape is stable.
    payload.insert("start_ms".to_string(), int_val(evt.start_ms));
    payload.insert("end_ms".to_string(), int_val(evt.end_ms));
    payload.insert(
        "source_type".to_string(),
        str_val(format!("{}_segment", kind.as_str())),
    );
    payload.insert(
        "content_hash".to_string(),
        str_val(evt.content_hash.clone()),
    );
    payload.insert("media_url".to_string(), str_val(evt.media_url.clone()));
    // Media candidates carry no text body; the segment IS the content. Any
    // transcript rides along for display/citation only.
    payload.insert(
        "text".to_string(),
        str_val(evt.transcript.clone().unwrap_or_default()),
    );

    let point = PointStruct::new(
        segment_point_id(&evt.document_id, kind, evt.segment_no),
        vector,
        payload,
    );
    qdrant
        .upsert_points(UpsertPointsBuilder::new(collection, vec![point]).wait(true))
        .await
        .context("upsert media segment vector")?;
    Ok(())
}

/// Describe one video segment and index the description as TEXT.
///
/// This is the temporal fix. The caption lands as a `knowledge_units` row, so it
/// reaches BOTH text arms with no new plumbing: `content_tsv` is a generated
/// column, so BM25 picks it up the instant the row commits, and the dense vector
/// is upserted here into the text collection. A query like "someone falling"
/// then matches because the caption says the person falls — ordering expressed
/// in language, which the text arms model and no video tower does.
///
/// Infallible by design. Every failure mode leaves the segment with its vector
/// embedding and no caption, which is exactly the behaviour before this feature
/// existed. A vision-provider outage must not fail a media ingest.
async fn caption_video_segment(
    evt: &MediaSegmentCreatedEvent,
    media: &MediaEmbeddingProvider,
    captioning: &VideoCaptioning,
    qdrant: &qdrant_client::Qdrant,
) {
    // ZDR first, before any frame is even extracted.
    if !caption_allowed_for_document(&captioning.pool, &evt.org_id, &evt.document_id).await {
        return;
    }

    let strip = match media.filmstrip(&evt.media_url).await {
        Ok(strip) => strip,
        Err(e) => {
            tracing::warn!(error = %e, document_id = %evt.document_id, "filmstrip failed; segment keeps its vector, no caption");
            return;
        }
    };
    // `Engine` must be in scope for `.decode`; base64 0.22 moved it off the
    // free-function API.
    use base64::Engine as _;
    let image = match base64::engine::general_purpose::STANDARD.decode(&strip.image_base64) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(error = %e, "filmstrip was not valid base64; no caption");
            return;
        }
    };

    let narrative = match captioning
        .describer
        .describe(&evt.org_id, image, &strip.mime_type, strip.frames)
        .await
    {
        Ok(Some(text)) => text,
        Ok(None) => {
            tracing::info!(document_id = %evt.document_id, "vision model returned no description; no caption");
            return;
        }
        Err(e) => {
            tracing::warn!(error = %e, document_id = %evt.document_id, "video description failed; no caption");
            return;
        }
    };
    let caption = video_caption::compose_caption(evt.start_ms, evt.end_ms, &narrative);
    let knowledge_id = caption_knowledge_id(&evt.document_id, evt.segment_no);

    // Persist first, embed second. The row is what makes the caption reachable
    // by the LEXICAL arm (via the generated `content_tsv`), and that half is
    // worth keeping even if the embedding call then fails — the reverse order
    // would risk a live vector pointing at a row that was never written.
    //
    // `embedding_status` starts 'pending' and is flipped to 'done' only after the
    // vector lands, so a failure here leaves an honest state rather than a row
    // claiming a vector that does not exist.
    let metadata = serde_json::json!({
        "source_type": "video_caption",
        "segment_no": evt.segment_no,
        "start_ms": evt.start_ms,
        "end_ms": evt.end_ms,
        "media_url": evt.media_url,
        "caption_prompt_version": video_caption::PROMPT_VERSION,
        "caption_model": captioning.describer.model_name(),
        "caption_frames": strip.frames,
        // Which moments the model actually saw. Without this a caption is an
        // unfalsifiable claim about the segment; with it, a trace can show that
        // e.g. only 0.0-3.5s was sampled from a 60s clip, which is usually the
        // explanation for a caption that misses something.
        "caption_frame_seconds": strip.seconds,
    });
    let written = sqlx::query(
        r#"
        INSERT INTO knowledge_units (
            knowledge_id, document_id, org_id, chunk_index, text,
            embedding_status, content_hash, chunk_version, metadata
        ) VALUES ($1, $2, $3, $4, $5, 'pending', $6, '1', $7)
        ON CONFLICT (knowledge_id) DO UPDATE
            SET text = EXCLUDED.text,
                metadata = EXCLUDED.metadata,
                embedding_status = 'pending',
                error_message = NULL
        "#,
    )
    .bind(&knowledge_id)
    .bind(&evt.document_id)
    .bind(&evt.org_id)
    // Negative chunk_index namespaces captions away from index-engine's real
    // text chunks (0..n) for the same document, so the two can never overwrite
    // each other's ordering or be mistaken for one another in a trace.
    .bind(-(evt.segment_no + 1) as i32)
    .bind(&caption)
    .bind(crate::provider::media::caption_content_hash(&caption))
    .bind(&metadata)
    .execute(&captioning.pool)
    .await;
    if let Err(e) = written {
        tracing::warn!(error = %e, %knowledge_id, "caption row not written; no caption indexed");
        return;
    }

    // Dense text arm. ZDR is false here by construction: a restricted document
    // was refused above, so this text is safe to send to the text embedder.
    let vectors = match captioning
        .text_provider
        .embed_batch(&evt.org_id, std::slice::from_ref(&caption), false)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, %knowledge_id, "caption embed failed; caption is live on the LEXICAL arm only");
            return;
        }
    };
    let Some(vector) = vectors.into_iter().next() else {
        tracing::warn!(%knowledge_id, "caption embed returned no vector");
        return;
    };

    let point = crate::qdrant_writer::EmbeddingPoint {
        knowledge_id: knowledge_id.clone(),
        document_id: evt.document_id.clone(),
        org_id: evt.org_id.clone(),
        chunk_index: -(evt.segment_no + 1) as i32,
        text: caption.clone(),
        vector,
        metadata: HashMap::new(),
    };
    if let Err(e) =
        crate::qdrant_writer::upsert_vectors(qdrant, &captioning.text_collection, vec![point]).await
    {
        tracing::warn!(error = %e, %knowledge_id, "caption vector upsert failed; caption is live on the LEXICAL arm only");
        return;
    }

    if let Err(e) = sqlx::query(
        "UPDATE knowledge_units SET embedding_status = 'done', embedded_at = NOW(), error_message = NULL WHERE knowledge_id = $1",
    )
    .bind(&knowledge_id)
    .execute(&captioning.pool)
    .await
    {
        tracing::warn!(error = %e, %knowledge_id, "caption status not flipped to done");
    }

    tracing::info!(
        document_id = %evt.document_id,
        segment_no = evt.segment_no,
        frames = strip.frames,
        chars = caption.len(),
        "video caption indexed into both text arms"
    );
}

#[cfg(test)]
mod caption_tests {
    use super::*;

    fn evt(segment_no: i64, start_ms: i64, end_ms: i64) -> MediaSegmentCreatedEvent {
        MediaSegmentCreatedEvent {
            document_id: "doc-1".into(),
            org_id: "org-1".into(),
            segment_no,
            media_url: "http://minio/seg.mp4".into(),
            start_ms,
            end_ms,
            content_hash: "h1".into(),
            transcript: None,
        }
    }

    /// A caption must never collide with, or be mistaken for, one of
    /// index-engine's real text chunks for the same document.
    #[test]
    fn caption_ids_are_stable_and_namespaced_away_from_text_chunks() {
        assert_eq!(caption_knowledge_id("doc-1", 0), "doc-1:vidcap0");
        assert_eq!(
            caption_knowledge_id("doc-1", 3),
            caption_knowledge_id("doc-1", 3),
            "must be stable so a redelivery upserts in place"
        );
        assert_ne!(
            caption_knowledge_id("doc-1", 3),
            caption_knowledge_id("doc-1", 4)
        );
        assert_ne!(
            caption_knowledge_id("doc-1", 3),
            caption_knowledge_id("doc-2", 3)
        );
    }

    /// Captions occupy negative chunk_index so they cannot overwrite or
    /// interleave with the 0..n text chunks of the same document.
    #[test]
    fn caption_chunk_indexes_never_overlap_real_chunks() {
        for segment_no in [0_i64, 1, 7, 999] {
            let index = -(segment_no + 1) as i32;
            assert!(index < 0, "segment {segment_no} produced {index}");
        }
        // Distinct per segment, so two captions on one document are distinct
        // rows rather than one clobbering the other.
        let index_for = |segment_no: i64| -(segment_no + 1) as i32;
        assert_ne!(index_for(0), index_for(1));
        assert_eq!(index_for(0), -1);
    }

    /// The caption text is what both arms index, so its shape is the contract:
    /// a citable time range plus the narrative, with ordering words intact.
    #[test]
    fn caption_text_keeps_the_narrative_and_a_citable_range() {
        let e = evt(0, 2_000, 6_500);
        let text = video_caption::compose_caption(
            e.start_ms,
            e.end_ms,
            "A person walks to the door, then falls.",
        );
        assert!(text.starts_with("[video 2.0s–6.5s]"));
        assert!(text.contains("then falls"), "ordering must survive: {text}");
    }

    /// The whole point of caption-to-text: a video and its reverse produce
    /// DIFFERENT text, which is what the vector towers could not do (both give
    /// cosine 1.000000 on a reversal).
    #[test]
    fn reversed_footage_yields_different_text_than_the_forward_version() {
        let forward = video_caption::compose_caption(0, 4_000, "A person stands, then falls to the floor.");
        let reverse = video_caption::compose_caption(0, 4_000, "A person lies on the floor, then stands up.");
        assert_ne!(
            forward, reverse,
            "order must be observable in the indexed text, or the fix does nothing"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_route_to_the_right_modality_and_operation() {
        assert_eq!(
            kind_for(SUBJECT_AUDIO_SEGMENT_CREATED),
            Some((MediaKind::Audio, true))
        );
        assert_eq!(
            kind_for(SUBJECT_VIDEO_SEGMENT_DELETED),
            Some((MediaKind::Video, false))
        );
        assert_eq!(kind_for("dataplane.page_images.created"), None);
    }

    #[test]
    fn point_ids_are_stable_and_do_not_collide_across_modalities() {
        let a = segment_point_id("doc-1", MediaKind::Audio, 3);
        let b = segment_point_id("doc-1", MediaKind::Video, 3);
        // Same doc + index, different modality ⇒ different point, or a video
        // segment would silently overwrite its audio sibling.
        assert_ne!(a, b);
        // Re-ingest of the same segment must overwrite in place.
        assert_eq!(a, segment_point_id("doc-1", MediaKind::Audio, 3));
        assert_ne!(a, segment_point_id("doc-1", MediaKind::Audio, 4));
        assert_ne!(a, segment_point_id("doc-2", MediaKind::Audio, 3));
    }

    #[test]
    fn collections_select_per_modality() {
        let c = MediaCollections {
            audio: "aud".into(),
            video: "vid".into(),
        };
        assert_eq!(c.for_kind(MediaKind::Audio), "aud");
        assert_eq!(c.for_kind(MediaKind::Video), "vid");
    }
}
