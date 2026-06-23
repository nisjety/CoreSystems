//! Page-image event contract — the PRODUCER side of the visual-RAG arm.
//!
//! These structs + subjects are the EXACT wire contract the Data Plane v2
//! embedding-engine page-image consumer deserializes
//! (`embedding-engine-rs/src/image_consumer.rs`: `PageImageCreatedEvent` /
//! `PageImageDeletedEvent`). Any drift here is a SILENT poison-drop on the
//! consumer (it acks malformed events with no DLQ), so the field names, types,
//! and subject strings are pinned and guarded by the byte-compatibility tests
//! below.
//!
//! Rules baked in:
//!   - emit `image_url` (a fetchable URL the consumer HTTP-GETs), NOT a CAS key;
//!   - `page_no` is a JSON integer (i64);
//!   - publish on JetStream to the SAME broker the consumer binds (its stream
//!     `DATAPLANE_PAGE_IMAGES` is auto-created there) — publish the RAW subject,
//!     never route through `NatsEventBus` (which prefixes `quarry.` → wrong stream).

use serde::Serialize;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

/// Verbatim subjects — MUST byte-match the consumer (`image_consumer.rs:32-33`).
pub const SUBJECT_PAGE_IMAGE_CREATED: &str = "dataplane.page_images.created";
pub const SUBJECT_PAGE_IMAGE_DELETED: &str = "dataplane.page_images.deleted";

/// `dataplane.page_images.created` payload. Mirrors the consumer's
/// `PageImageCreatedEvent`: `document_id` / `org_id` / `page_no` / `image_url`
/// are REQUIRED; `content_hash` / `title` / `zdr` are optional on the consumer
/// (`#[serde(default)]`), so empties are omitted rather than sent as null.
#[derive(Debug, Clone, Serialize)]
pub struct PageImageCreated {
    pub document_id: String,
    pub org_id: String,
    pub page_no: i64,
    pub image_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub zdr: bool,
}

/// `dataplane.page_images.deleted` payload — the consumer purges that document's
/// vectors; the Ingestion side purges its CAS objects.
#[derive(Debug, Clone, Serialize)]
pub struct PageImageDeleted {
    pub document_id: String,
}

/// Publish a page-image-created event on JetStream. The producer must only ever
/// emit `zdr == false` events (a ZDR page is skipped before render/CAS/emit);
/// this fn does not itself gate ZDR.
pub async fn emit_page_image_created(
    js: &async_nats::jetstream::Context,
    evt: &PageImageCreated,
) -> QuarryResult<()> {
    publish_json(js, SUBJECT_PAGE_IMAGE_CREATED, evt).await
}

/// Publish a page-image-deleted event (drives the consumer's Qdrant purge and
/// the CAS-erasure counterpart).
pub async fn emit_page_image_deleted(
    js: &async_nats::jetstream::Context,
    evt: &PageImageDeleted,
) -> QuarryResult<()> {
    publish_json(js, SUBJECT_PAGE_IMAGE_DELETED, evt).await
}

async fn publish_json<T: Serialize>(
    js: &async_nats::jetstream::Context,
    subject: &'static str,
    evt: &T,
) -> QuarryResult<()> {
    let payload = serde_json::to_vec(evt)
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("serialize {subject}: {e}")))?;
    // `publish()` resolves once the broker accepts the message; the returned
    // ack future resolves once it is persisted to the stream — await both so a
    // stream/storage rejection surfaces as an error instead of a silent loss.
    let ack = js
        .publish(subject.to_string(), bytes::Bytes::from(payload))
        .await
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("publish {subject}: {e}")))?;
    ack.await
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("ack {subject}: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// Byte-for-byte mirror of the consumer's `PageImageCreatedEvent`
    /// (`embedding-engine-rs/src/image_consumer.rs`) — identical field names,
    /// types, and `#[serde(default)]` attributes. If the producer's serialized
    /// JSON fails to deserialize into THIS, the live consumer would poison-drop.
    #[derive(Debug, Deserialize)]
    struct ConsumerMirror {
        #[allow(dead_code)]
        document_id: String,
        #[allow(dead_code)]
        org_id: String,
        page_no: i64,
        image_url: String,
        #[serde(default)]
        content_hash: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        zdr: bool,
    }

    fn sample() -> PageImageCreated {
        PageImageCreated {
            document_id: "doc-1".into(),
            org_id: "org-1".into(),
            page_no: 0,
            image_url: "http://quarry-edge:8082/v1/page-images/doc-1/0".into(),
            content_hash: "b3deadbeef".into(),
            title: Some("Acme Pricing".into()),
            zdr: false,
        }
    }

    #[test]
    fn created_serializes_to_consumer_shape() {
        let v = serde_json::to_value(sample()).unwrap();
        assert!(v["page_no"].is_i64(), "page_no MUST be a JSON integer");
        assert_eq!(
            v["image_url"].as_str(),
            Some("http://quarry-edge:8082/v1/page-images/doc-1/0")
        );
        assert!(
            v.get("image_cas_key").is_none(),
            "must NOT emit image_cas_key (the consumer has no such field)"
        );
        assert!(v["zdr"].is_boolean(), "zdr MUST be a bool");
        assert_eq!(v["content_hash"], "b3deadbeef");
    }

    #[test]
    fn created_round_trips_through_consumer_mirror() {
        let bytes = serde_json::to_vec(&sample()).unwrap();
        let parsed: ConsumerMirror =
            serde_json::from_slice(&bytes).expect("consumer must parse the producer's JSON");
        assert_eq!(parsed.page_no, 0);
        assert_eq!(
            parsed.image_url,
            "http://quarry-edge:8082/v1/page-images/doc-1/0"
        );
        assert_eq!(parsed.content_hash, "b3deadbeef");
        assert_eq!(parsed.title.as_deref(), Some("Acme Pricing"));
        assert!(!parsed.zdr);
    }

    #[test]
    fn minimal_required_only_parses_with_defaults() {
        // The consumer's ground truth: {document_id, org_id, page_no, image_url}
        // alone must parse (content_hash/title/zdr default).
        let minimal = PageImageCreated {
            document_id: "d".into(),
            org_id: "o".into(),
            page_no: 3,
            image_url: "http://x/y".into(),
            content_hash: String::new(), // empty → omitted
            title: None,                 // None → omitted
            zdr: false,
        };
        let v = serde_json::to_value(&minimal).unwrap();
        assert!(
            v.get("content_hash").is_none(),
            "empty content_hash must be omitted (consumer defaults it)"
        );
        assert!(v.get("title").is_none(), "None title must be omitted");
        let parsed: ConsumerMirror = serde_json::from_value(v).expect("minimal payload must parse");
        assert_eq!(parsed.page_no, 3);
        assert_eq!(parsed.content_hash, "", "consumer defaults content_hash to empty");
        assert!(parsed.title.is_none());
        assert!(!parsed.zdr, "consumer defaults zdr to false");
    }

    #[test]
    fn deleted_serializes_document_id_only() {
        let v = serde_json::to_value(PageImageDeleted {
            document_id: "doc-9".into(),
        })
        .unwrap();
        assert_eq!(v["document_id"], "doc-9");
        assert_eq!(
            v.as_object().unwrap().len(),
            1,
            "deleted payload is document_id only"
        );
    }
}
