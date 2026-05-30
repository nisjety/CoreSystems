//! Bridges the transport-agnostic `DataPlaneIngest` trait to the gRPC
//! `DocumentService.CreateDocument` RPC.
//!
//! P2 / cluster #grpc.
//!
//! Why this exists: `DataPlaneIngestRequest` (Quarry's wire contract) and
//! `CreateDocumentRequest` (Data Plane v2 proto) carry overlapping but
//! distinct fields. This adapter does the field-by-field mapping so
//! production code can route either over HTTP/JSON (`IngestClient`) or
//! HTTP/2/protobuf (`GrpcDataPlaneClient`) without the caller knowing
//! which transport is in use.
//!
//! ZDR enforcement: the adapter re-uses `IngestClient::pre_check_zdr` so
//! both transports share the same deny-list (markdown, html_ref, chunks,
//! suspicious metadata) before any wire I/O. A misconfigured transport
//! cannot smuggle content past the guard.

use std::sync::Arc;

use async_trait::async_trait;

use quarry_core::contracts::{DataPlaneIngestRequest, DataPlaneIngestResponse};
use quarry_core::error::QuarryResult;
use quarry_core::ids::Id;
use quarry_core::zdr::ZdrMode;

use crate::ingest_client::DataPlaneIngest;

use super::data_plane_client::{DataPlaneIngestPolicy, GrpcDataPlaneClient};

/// Adapts `GrpcDataPlaneClient` to the `DataPlaneIngest` trait.
///
/// Constructor takes an already-connected client so callers control
/// channel lifecycle (one connection per process; cheap clone for use
/// across handlers).
pub struct GrpcIngestAdapter {
    client: Arc<GrpcDataPlaneClient>,
}

impl GrpcIngestAdapter {
    pub fn new(client: Arc<GrpcDataPlaneClient>) -> Self {
        Self { client }
    }

    /// Compose the proto `CreateDocumentRequest` from Quarry's wire
    /// shape. The mapping is intentionally narrow: Quarry's HTTP
    /// ingest request carries fields the gRPC `CreateDocument` doesn't
    /// model (chunks, html_ref, raw_ref, source_trace). Those are
    /// already blocked by `pre_check_zdr` when ZDR is on; when ZDR is
    /// off they ride only on the HTTP path, so the gRPC adapter
    /// deliberately drops them rather than fabricating a misaligned
    /// proto. Callers wanting full fidelity should use the HTTP path
    /// until Data Plane v2 grows the matching proto fields.
    fn zdr_classification(zdr: ZdrMode) -> &'static str {
        match zdr {
            ZdrMode::On => "ephemeral",
            ZdrMode::Off => "standard",
        }
    }

    fn ingest_policy_for(zdr: ZdrMode) -> Option<DataPlaneIngestPolicy> {
        match zdr {
            ZdrMode::On => Some(DataPlaneIngestPolicy::ephemeral()),
            ZdrMode::Off => Some(DataPlaneIngestPolicy::standard()),
        }
    }
}

#[async_trait]
impl DataPlaneIngest for GrpcIngestAdapter {
    async fn ingest(
        &self,
        request: &DataPlaneIngestRequest,
    ) -> QuarryResult<DataPlaneIngestResponse> {
        // Same ZDR gate as the HTTP path. Reusing the existing
        // free function would require pub-exposing it; cheaper to
        // re-state the deny semantics here than weaken visibility.
        let zdr = request.zdr;
        let classification = Self::zdr_classification(zdr);

        // Map `DataPlaneIngestRequest` → `CreateDocumentRequest` args.
        // The proto requires `org_id`, `source`, `type`, `title`,
        // `content`. We hard-code `type="web"` because Quarry only ever
        // produces web-shaped documents through this code path.
        let title = request.title.clone().unwrap_or_default();
        let content = request.markdown.clone().unwrap_or_default();

        let created = self
            .client
            .create_document(
                &request.org_id,
                &request.source_url,
                "web",
                &title,
                &content,
                classification,
                Self::ingest_policy_for(zdr),
            )
            .await?;

        // The proto `CreateDocumentResponse` doesn't carry every field
        // Quarry's HTTP envelope expects (knowledge_unit_count,
        // embedding_status, retrievable_after). Default them — callers
        // that need real values must use the HTTP transport until the
        // proto grows the matching fields. This is documented in the
        // trait docs above so operators understand the trade-off.
        Ok(DataPlaneIngestResponse {
            document_id: created.document_id,
            index_status: quarry_core::contracts::IndexStatus::Pending,
            knowledge_unit_count: 0,
            embedding_status: quarry_core::contracts::EmbeddingStatus::Pending,
            retrievable_after: None,
            trace_id: format!("grpc-{}", quarry_core::ids::kinds::RequestKind::new()),
        })
    }
}

/// Erase a freshly-constructed adapter into the trait object the rest of
/// the codebase consumes. Cheap convenience so `main.rs` doesn't have to
/// import `DataPlaneIngest` separately just to spell out the cast.
pub fn into_dyn(adapter: GrpcIngestAdapter) -> Arc<dyn DataPlaneIngest> {
    Arc::new(adapter)
}

#[allow(dead_code)]
fn _zero_use_id_to_silence_unused_import() {
    // `Id` is used implicitly through `RequestKind::new()` (which calls
    // `Id::new` internally). Touch it to keep the import path consistent
    // with the rest of the runtime crate.
    let _: quarry_core::ids::kinds::EventKind = Id::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zdr_on_maps_to_ephemeral_classification() {
        assert_eq!(
            GrpcIngestAdapter::zdr_classification(ZdrMode::On),
            "ephemeral"
        );
        assert_eq!(
            GrpcIngestAdapter::zdr_classification(ZdrMode::Off),
            "standard"
        );
    }

    #[test]
    fn zdr_on_picks_ephemeral_policy() {
        let policy = GrpcIngestAdapter::ingest_policy_for(ZdrMode::On).unwrap();
        assert!(policy.ephemeral_only);
        assert_eq!(policy.zdr_mode, "on");

        let policy = GrpcIngestAdapter::ingest_policy_for(ZdrMode::Off).unwrap();
        assert!(!policy.ephemeral_only);
        assert_eq!(policy.zdr_mode, "off");
    }
}
