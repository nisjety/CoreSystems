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
    async fn register_source(
        &self,
        org_id: &str,
        name: &str,
        url: &str,
        kind: &str,
    ) -> QuarryResult<()> {
        // The crawl that triggered this registration passed the web policy of its caller. The
        // registrar has no token, so it re-checks under the baseline (the operator deny list) as a
        // human principal: an allow list binds autonomous callers only, and a row for a URL the
        // operator denies is never written. Best-effort by contract: the caller logs and swallows.
        let decision = crate::web_policy_edge::decide_for_principal(
            &self.state.policy,
            quarry_core::web_policy::Principal::Human,
            false,
            None,
        )?;
        crate::web_policy_edge::gate_url(&decision, url)?;
        upsert_source_internal(&self.state, org_id, name, url, kind).await
    }
}
