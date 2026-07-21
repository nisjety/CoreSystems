//! Wires `quarry_runtime::source_registrar::SourceRegistrar` onto the
//! edge's existing HMAC-signed control-plane forward path — the same one
//! `POST /v1/sources` (`resource_routes::create_source`) uses — so
//! `PageRunner`'s crawl-completion registration (`quarry_runtime::pipeline`)
//! actually reaches the durable `quarry_sources` store instead of being a
//! no-op. See `resource_routes::upsert_source_internal` for the forwarding
//! logic and `source_registrar::SourceRegistrar` (quarry-runtime) for why
//! this exists (2026-07-20 Aquatiq crawl-to-KB audit: crawl-originated
//! ingests never materialized a "tracked website" row).

use async_trait::async_trait;
use quarry_core::error::QuarryResult;
use quarry_runtime::source_registrar::SourceRegistrar;

use crate::resource_routes::upsert_source_internal;
use crate::state::AppState;

pub struct EdgeSourceRegistrar {
    pub state: AppState,
}

#[async_trait]
impl SourceRegistrar for EdgeSourceRegistrar {
    async fn register_source(&self, org_id: &str, name: &str, url: &str, kind: &str) -> QuarryResult<()> {
        upsert_source_internal(&self.state, org_id, name, url, kind).await
    }
}
