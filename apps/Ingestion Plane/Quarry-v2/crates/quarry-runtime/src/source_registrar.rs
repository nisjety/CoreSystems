//! Best-effort materialization of a durable "tracked website" Source record
//! for crawl-originated ingests.
//!
//! Gap fixed (2026-07-20, Aquatiq crawl-to-KB audit): a website crawl that
//! ran to completion and ingested every page into the Data Plane still left
//! the Knowledge Base's "Tracked web sources" panel empty forever, because
//! nothing in the crawl pipeline ever wrote a `quarry_sources` row — only
//! the Ingestions page's manual "register a tracked source" flow did (via
//! `POST /v1/sources`, `quarry-control`'s `cycle23.go`). `PageRunner`
//! (`crate::pipeline`) now calls this transport-agnostic registrar
//! (mirroring `DataPlaneIngest`) once a page's Data Plane ingest succeeds,
//! so a `Source` row exists for every crawl target without a separate
//! manual step.
//!
//! Best-effort + fire-and-forget from the caller's perspective: a failed
//! registration is logged and never fails the ingest (see
//! `pipeline::PageRunner::run_inner`). The receiver upserts on
//! `(org_id, url)` so repeat calls — one per page of a multi-page crawl, or
//! repeat crawls of the same site — collapse into a single row instead of
//! duplicating it once per page.

use async_trait::async_trait;
use quarry_core::error::QuarryResult;

/// Registers (or refreshes) one durable "tracked website" row. `url` should
/// be the crawl target's root (`scheme://host`), not each individual page —
/// callers derive that from the page's final URL so a multi-page crawl of
/// one site produces exactly one row instead of one per page.
#[async_trait]
pub trait SourceRegistrar: Send + Sync {
    async fn register_source(
        &self,
        org_id: &str,
        name: &str,
        url: &str,
        kind: &str,
    ) -> QuarryResult<()>;
}
