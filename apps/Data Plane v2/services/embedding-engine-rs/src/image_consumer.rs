//! Page-image embedding subscriber (DURABLE JetStream) — the **visual RAG arm**.
//!
//! The Ingestion Plane renders each document page to an image, stores it in the
//! content-addressable store, and emits `dataplane.page_images.created` carrying
//! a fetchable `image_url`. We embed each page with **Cohere Embed v4** (the
//! `VisualEmbeddingProvider`: multimodal, single 1536-d vector) and upsert one
//! point into the `dataplane_page_images` Qdrant collection, which the
//! retrieval-engine fuses via `w_visual`. A `dataplane.page_images.deleted`
//! event purges every page of a document.
//!
//! Mirrors `wiki_consumer`: durable WorkQueue stream, ack on success / poison,
//! no-ack (JetStream redelivers up to `max_deliver`) on transient failure.
//!
//! ZDR: Embed v4 (Azure Foundry) is a *retaining* provider, so a page flagged
//! `zdr=true` has no compliant visual path yet — it is logged and acked (dropped)
//! rather than embedded, so it never egresses and never poison-loops.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Context;
use async_nats::jetstream::{self, Context as JsContext};
use base64::Engine;
use futures::StreamExt;
use qdrant_client::qdrant::{
    value::Kind as QdrantKind, PointStruct, UpsertPointsBuilder, Value as QdrantValue,
};
use serde::Deserialize;

use crate::provider::visual::{ImageEmbedInput, VisualEmbeddingProvider};

pub const SUBJECT_PAGE_IMAGE_CREATED: &str = "dataplane.page_images.created";
pub const SUBJECT_PAGE_IMAGE_DELETED: &str = "dataplane.page_images.deleted";
pub const PAGE_IMAGE_STREAM: &str = "DATAPLANE_PAGE_IMAGES";
pub const PAGE_IMAGE_CONSUMER: &str = "embedding-engine-page-images";
/// Terminal-failure sink for page images, mirroring the text consumers
/// (`dataplane.dlq.index-engine`, `dataplane.dlq.graph-index`). Without it a
/// page whose fetch or embed kept failing was simply dropped once
/// `MAX_DELIVER` was exhausted, so a rendered page could vanish from the
/// visual corpus with nothing recording that it had.
pub const PAGE_IMAGE_DLQ_SUBJECT: &str = "dataplane.dlq.embedding-engine-page-images";
/// Must match `max_deliver` on the durable consumer below.
const MAX_DELIVER: i64 = 5;

#[derive(Debug, Deserialize)]
struct PageImageCreatedEvent {
    document_id: String,
    org_id: String,
    page_no: i64,
    /// Fetchable URL for the rendered page image (MinIO presigned / internal
    /// artifact endpoint). The producer owns hosting; we GET the bytes.
    image_url: String,
    #[serde(default)]
    content_hash: String,
    /// Optional page title/caption. Stored in the Qdrant payload for
    /// display/citation; NOT fused into the vector — Embed v4 rejects
    /// image + text in one input (HTTP 422).
    #[serde(default)]
    title: Option<String>,
    /// Zero-Data-Retention flag; `true` ⇒ the page must not egress to Embed v4.
    #[serde(default)]
    zdr: bool,
}

#[derive(Debug, Deserialize)]
struct PageImageDeletedEvent {
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

/// Deterministic point id from `(document_id, page_no)` so a re-render of the
/// same page overwrites in place instead of accumulating duplicates.
fn page_point_id(document_id: &str, page_no: i64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    document_id.hash(&mut hasher);
    0u8.hash(&mut hasher); // domain separator
    page_no.hash(&mut hasher);
    hasher.finish()
}

/// Embed v4 (Azure Foundry) meters the base64 image payload against an
/// ~8000-token request budget (≈ 32 KB of base64). Full-page screenshots of
/// content-rich pages exceed it, so the consumer downscales + JPEG-recompresses
/// until the encoded data URI fits. Kept below the observed ceiling with margin.
const EMBED_B64_BUDGET: usize = 28_000;
/// Cap the longest side before recompressing oversized images. Bounds the token
/// cost of very large/tall full-page captures while preserving enough detail.
const EMBED_MAX_DIM: u32 = 1280;

/// Build the `data:` URI Embed v4 receives. Small images are passed through
/// untouched (lossless PNG); oversized ones are decoded, fit within
/// `EMBED_MAX_DIM`, and JPEG-encoded at descending quality until the base64
/// payload fits `EMBED_B64_BUDGET`. Returns an error only if even the most
/// aggressive recompression cannot fit the budget.
fn prepare_image_data_url(bytes: &[u8]) -> anyhow::Result<String> {
    // base64 expands 3 bytes → 4 chars (with padding).
    let b64_len = |n: usize| n.div_ceil(3) * 4;

    if b64_len(bytes.len()) <= EMBED_B64_BUDGET {
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        return Ok(format!("data:image/png;base64,{encoded}"));
    }

    let img = image::load_from_memory(bytes).context("decode oversized page image")?;
    let longest = img.width().max(img.height());

    // Page screenshots are text-dense (high-frequency), so JPEG quality alone
    // doesn't shrink them enough — we descend through BOTH longest-side and
    // quality until the encoded payload fits the Embed v4 token budget.
    for &max_dim in &[EMBED_MAX_DIM, 1024, 896, 768, 640, 512] {
        // Skip sizes that wouldn't actually shrink the current image.
        if max_dim < EMBED_MAX_DIM && max_dim >= longest {
            continue;
        }
        let scaled = if longest > max_dim {
            img.resize(max_dim, max_dim, image::imageops::FilterType::Triangle)
        } else {
            img.clone()
        };
        let rgb = scaled.to_rgb8();
        for quality in [80u8, 65, 50, 40, 30] {
            let mut buf: Vec<u8> = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality)
                .encode_image(&rgb)
                .context("jpeg re-encode page image")?;
            if b64_len(buf.len()) <= EMBED_B64_BUDGET {
                let encoded = base64::engine::general_purpose::STANDARD.encode(&buf);
                return Ok(format!("data:image/jpeg;base64,{encoded}"));
            }
        }
    }

    anyhow::bail!(
        "page image exceeds Embed v4 budget even after max downscale+recompress (orig {} bytes)",
        bytes.len()
    )
}

pub async fn spawn(
    js: JsContext,
    nats: async_nats::Client,
    qdrant: qdrant_client::Qdrant,
    visual: VisualEmbeddingProvider,
    collection: String,
) -> anyhow::Result<()> {
    js.get_or_create_stream(jetstream::stream::Config {
        name: PAGE_IMAGE_STREAM.to_string(),
        subjects: vec![
            SUBJECT_PAGE_IMAGE_CREATED.to_string(),
            SUBJECT_PAGE_IMAGE_DELETED.to_string(),
        ],
        retention: jetstream::stream::RetentionPolicy::WorkQueue,
        max_age: Duration::from_secs(7 * 24 * 3600),
        ..Default::default()
    })
    .await
    .context("create DATAPLANE_PAGE_IMAGES stream")?;

    let stream = js
        .get_stream(PAGE_IMAGE_STREAM)
        .await
        .context("get page-image stream")?;
    let consumer = stream
        .get_or_create_consumer(
            PAGE_IMAGE_CONSUMER,
            jetstream::consumer::pull::Config {
                durable_name: Some(PAGE_IMAGE_CONSUMER.to_string()),
                filter_subjects: vec![
                    SUBJECT_PAGE_IMAGE_CREATED.to_string(),
                    SUBJECT_PAGE_IMAGE_DELETED.to_string(),
                ],
                ack_wait: Duration::from_secs(120),
                max_deliver: 5,
                ..Default::default()
            },
        )
        .await
        .context("create page-image durable consumer")?;

    // Dedicated HTTP client for fetching rendered page bytes (separate from the
    // embedding client inside `VisualEmbeddingProvider`).
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("build page-image fetch client")?;

    tracing::info!(
        stream = PAGE_IMAGE_STREAM,
        collection = %collection,
        "page-image durable subscriber online"
    );

    tokio::spawn(async move {
        loop {
            let mut messages = match consumer.messages().await {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(error = %e, "page-image consumer stream open failed; retrying");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            while let Some(item) = messages.next().await {
                let msg = match item {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!(error = %e, "page-image message recv error");
                        continue;
                    }
                };
                let subject = msg.subject.to_string();
                let result = if subject == SUBJECT_PAGE_IMAGE_CREATED {
                    match serde_json::from_slice::<PageImageCreatedEvent>(&msg.payload) {
                        Ok(evt) => handle_created(&evt, &qdrant, &visual, &http, &collection).await,
                        Err(e) => {
                            tracing::warn!(error = %e, "invalid page_images.created payload; acking poison");
                            let _ = msg.ack().await;
                            continue;
                        }
                    }
                } else if subject == SUBJECT_PAGE_IMAGE_DELETED {
                    match serde_json::from_slice::<PageImageDeletedEvent>(&msg.payload) {
                        Ok(evt) => crate::qdrant_writer::delete_vectors_by_document(
                            &qdrant,
                            &collection,
                            &evt.document_id,
                        )
                        .await
                        .context("delete page-image vectors"),
                        Err(e) => {
                            tracing::warn!(error = %e, "invalid page_images.deleted payload; acking poison");
                            let _ = msg.ack().await;
                            continue;
                        }
                    }
                } else {
                    tracing::warn!(subject = %subject, "unknown page-image subject; acking");
                    let _ = msg.ack().await;
                    continue;
                };

                match result {
                    Ok(()) => {
                        let _ = msg.ack().await;
                    }
                    Err(e) => {
                        let delivered = msg.info().map(|info| info.delivered).unwrap_or(1);
                        if delivered < MAX_DELIVER {
                            // No ack → JetStream redelivers (up to max_deliver).
                            tracing::warn!(error = %e, subject = %subject, delivered, "page-image handling failed; will redeliver");
                        } else {
                            // Final attempt: route to the DLQ and ack, so the
                            // failure is recorded instead of silently dropped
                            // when JetStream stops redelivering.
                            tracing::error!(
                                error = %e,
                                subject = %subject,
                                delivered,
                                "page-image handling failed permanently; routing to DLQ"
                            );
                            let dlq = serde_json::json!({
                                "original_subject": subject,
                                "stream": PAGE_IMAGE_STREAM,
                                "error": e.to_string(),
                                "attempts": delivered,
                            });
                            if let Err(error) =
                                nats.publish(PAGE_IMAGE_DLQ_SUBJECT, dlq.to_string().into()).await
                            {
                                tracing::error!(%error, "page-image DLQ publish failed");
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
    evt: &PageImageCreatedEvent,
    qdrant: &qdrant_client::Qdrant,
    visual: &VisualEmbeddingProvider,
    http: &reqwest::Client,
    collection: &str,
) -> anyhow::Result<()> {
    // ZDR egress guard: a restricted page has no compliant visual embedding path
    // (Embed v4 retains). Drop + ack (Ok) so it never egresses and never loops.
    if evt.zdr {
        tracing::warn!(
            document_id = %evt.document_id,
            page_no = evt.page_no,
            "ZDR page image skipped (no compliant visual embedding path)"
        );
        return Ok(());
    }

    // Fetch the rendered page bytes from the producer-hosted URL.
    let resp = http
        .get(&evt.image_url)
        .send()
        .await
        .context("fetch page image")?
        .error_for_status()
        .context("page image fetch status")?;
    let bytes = resp.bytes().await.context("read page image bytes")?;
    // Adapt the payload to Embed v4's request budget: small images pass through
    // losslessly; larger ones are downscaled + JPEG-recompressed until they fit.
    let data_url = prepare_image_data_url(&bytes)?;

    let input = ImageEmbedInput {
        image_data_url: data_url,
        // Embed v4 (Foundry) rejects image + text in a single input (HTTP 422
        // "cannot have both text and image inputs"), so we embed the image
        // alone. The page title still rides in the Qdrant payload below for
        // display/citation — it is simply not fused into the vector.
        text: None,
    };
    let vectors = visual
        .embed_images(std::slice::from_ref(&input), false)
        .await
        .context("embed page image")?;
    let vector = vectors
        .into_iter()
        .next()
        .context("empty visual embed result")?;

    // Payload mirrors the fields the retrieval-engine's vector_search reads, so a
    // page-image point is a first-class hybrid candidate that resolves to its
    // parent document (ownership gate + source-join key on `document_id`).
    let mut payload: HashMap<String, QdrantValue> = HashMap::new();
    payload.insert(
        "knowledge_id".to_string(),
        str_val(format!("{}:p{}", evt.document_id, evt.page_no)),
    );
    payload.insert("document_id".to_string(), str_val(evt.document_id.clone()));
    payload.insert("org_id".to_string(), str_val(evt.org_id.clone()));
    payload.insert("chunk_index".to_string(), int_val(evt.page_no));
    payload.insert("page_no".to_string(), int_val(evt.page_no));
    payload.insert("source_type".to_string(), str_val("page_image"));
    payload.insert(
        "content_hash".to_string(),
        str_val(evt.content_hash.clone()),
    );
    payload.insert("image_url".to_string(), str_val(evt.image_url.clone()));
    // Visual candidates carry no text body; the page image IS the content. The
    // optional fused title rides along for display/citation.
    payload.insert(
        "text".to_string(),
        str_val(evt.title.clone().unwrap_or_default()),
    );

    let point = PointStruct::new(
        page_point_id(&evt.document_id, evt.page_no),
        vector,
        payload,
    );
    qdrant
        .upsert_points(UpsertPointsBuilder::new(collection, vec![point]).wait(true))
        .await
        .context("qdrant upsert page-image point")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_point_id_is_deterministic_and_page_scoped() {
        let base = page_point_id("doc-1", 0);
        assert_eq!(base, page_point_id("doc-1", 0), "deterministic");
        assert_ne!(base, page_point_id("doc-1", 1), "page-scoped");
        assert_ne!(base, page_point_id("doc-2", 0), "document-scoped");
    }

    #[test]
    fn small_image_passes_through_as_lossless_png() {
        // A tiny solid image is well under budget → returned untouched as PNG.
        let mut png: Vec<u8> = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            8,
            8,
            image::Rgb([200, 210, 220]),
        ))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode test png");
        assert!(
            png.len().div_ceil(3) * 4 <= EMBED_B64_BUDGET,
            "precondition: small image under budget"
        );
        let url = prepare_image_data_url(&png).expect("prepare small png");
        assert!(
            url.starts_with("data:image/png;base64,"),
            "small image must stay lossless PNG, got prefix: {}",
            &url[..url.find(',').unwrap_or(40).min(40)]
        );
    }

    #[test]
    fn created_event_parses_with_defaults() {
        let evt: PageImageCreatedEvent = serde_json::from_value(serde_json::json!({
            "document_id": "doc-1",
            "org_id": "org-1",
            "page_no": 3,
            "image_url": "https://minio/internal/doc-1/p3.png"
        }))
        .expect("parse created event");
        assert_eq!(evt.document_id, "doc-1");
        assert_eq!(evt.page_no, 3);
        assert!(!evt.zdr, "zdr defaults false");
        assert!(evt.title.is_none());
        assert!(evt.content_hash.is_empty());
    }
}
