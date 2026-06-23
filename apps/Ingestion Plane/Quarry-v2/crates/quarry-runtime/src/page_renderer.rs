//! `PageRenderer` — renders a web page to a PNG via a `BrowserDriver` session and
//! emits it into the visual-RAG arm (CAS write + `page_images.created`).
//!
//! Reuses: the P7 `BrowserDriver` (acquire→goto→screenshot→release — the only
//! handle that can actually screenshot; `PageRunner`'s `Arc<dyn Driver>` cannot),
//! the [`CasStore`] (content-addressable PNG write), and the [`page_image`] emit
//! contract. Lives in quarry-runtime so `PageRunner` can fire it at the ingest
//! hook, but holds its OWN `BrowserDriver` (injected from quarry-edge's
//! `AppState::agent_driver`).
//!
//! ZDR-before-CAS: [`render_and_emit`] is a no-op when ZDR is on — restricted
//! bytes are never rasterized, hashed, stored, or emitted.
//!
//! MVP web slice: one PNG (`page_no = 0`) per document. Multi-page is future work.

use std::sync::Arc;

use quarry_browser::BrowserDriver;
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds;
use quarry_core::lease::{BrowserLease, ProxyAffinity};
use quarry_core::zdr::ZdrMode;

use crate::cas_store::CasStore;
use crate::page_image::{emit_page_image_created, PageImageCreated};

/// Web-page → page-image producer.
#[derive(Clone)]
pub struct PageRenderer {
    browser: Arc<dyn BrowserDriver>,
    cas: Arc<CasStore>,
    js: async_nats::jetstream::Context,
    /// Base URL of the quarry-edge artifact-serve endpoint the consumer GETs,
    /// e.g. `http://quarry-edge:8082`. The emitted `image_url` is
    /// `{base}/v1/page-images/{document_id}/{page_no}`.
    edge_base_url: String,
}

impl PageRenderer {
    pub fn new(
        browser: Arc<dyn BrowserDriver>,
        cas: Arc<CasStore>,
        js: async_nats::jetstream::Context,
        edge_base_url: impl Into<String>,
    ) -> Self {
        Self {
            browser,
            cas,
            js,
            edge_base_url: edge_base_url.into(),
        }
    }

    /// Borrow the CAS client — used by the edge image-serve route to fetch a
    /// stored page PNG by its content-addressable key.
    pub fn cas(&self) -> &CasStore {
        &self.cas
    }

    /// Connect a `PageRenderer`: open a JetStream context to the Data Plane
    /// broker (where the embedding-engine consumer binds), build the CAS client,
    /// and hold the browser driver. Keeps `async_nats` + `CasStore` construction
    /// here so callers (quarry-edge `main`) don't need those deps directly.
    pub async fn connect(
        browser: Arc<dyn BrowserDriver>,
        dataplane_nats_url: &str,
        cas_bucket: impl Into<String>,
        cas_endpoint: Option<String>,
        edge_base_url: impl Into<String>,
    ) -> QuarryResult<Self> {
        let client = async_nats::connect(dataplane_nats_url).await.map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("dataplane nats connect {dataplane_nats_url}: {e}"),
            )
        })?;
        let js = async_nats::jetstream::new(client);
        let cas = std::sync::Arc::new(CasStore::new(cas_bucket, cas_endpoint).await);
        Ok(Self::new(browser, cas, js, edge_base_url))
    }

    /// Render `url` to a PNG, store it in the CAS, and publish
    /// `dataplane.page_images.created` for the embedding-engine consumer. No-op
    /// (`Ok`) when `zdr` is on (ZDR-before-CAS). Best-effort from the caller's
    /// side: an `Err` means "visual arm skipped for this doc", never fatal — the
    /// document is already ingested by the time this runs.
    pub async fn render_and_emit(
        &self,
        org_id: &str,
        document_id: &str,
        url: &str,
        title: Option<String>,
        zdr: ZdrMode,
    ) -> QuarryResult<()> {
        // ZDR-before-CAS: restricted content is never rasterized/stored/emitted.
        if !matches!(zdr, ZdrMode::Off) {
            tracing::debug!(document_id, "zdr on — skipping page-image render");
            return Ok(());
        }

        let lease = BrowserLease {
            lease_id: kinds::LeaseKind::new(),
            profile_id: kinds::ProfileKind::new(),
            session_affinity_key: document_id.to_string(),
            proxy_affinity: ProxyAffinity {
                pool: "default".into(),
                sticky_key: None,
            },
            ttl_s: 60,
            capabilities: vec![],
            artifact_bucket: "page-render".into(),
            persist_profile: false,
            viewport: None,
            org_id: org_id.to_string(),
        };

        // Acquire → navigate → full-page screenshot, releasing the session in all
        // paths (borrows of `&session` end before `release` takes it by value).
        let session = self.browser.acquire(&lease).await?;
        let shot = match self.browser.goto(&session, url).await {
            Ok(()) => self.browser.screenshot(&session, true).await,
            Err(e) => Err(e),
        };
        let _ = self.browser.release(session).await;
        let png = shot?.to_vec();

        let content_hash = blake3::hash(&png).to_hex().to_string();
        let page_no = 0i64;
        let _key = self
            .cas
            .put_page_png(org_id, document_id, page_no, &content_hash, png)
            .await?;

        // image_url carries org + content_hash so the (stateless) edge serve route
        // can reconstruct the exact CAS key; the consumer GETs it verbatim.
        let image_url = format!(
            "{}/v1/internal/page-images/{}/{}/{}/{}",
            self.edge_base_url.trim_end_matches('/'),
            org_id,
            document_id,
            page_no,
            content_hash
        );
        let evt = PageImageCreated {
            document_id: document_id.to_string(),
            org_id: org_id.to_string(),
            page_no,
            image_url,
            content_hash,
            title,
            zdr: false,
        };
        emit_page_image_created(&self.js, &evt).await?;
        tracing::info!(document_id, org_id, "page image rendered → CAS → emitted");
        Ok(())
    }
}
