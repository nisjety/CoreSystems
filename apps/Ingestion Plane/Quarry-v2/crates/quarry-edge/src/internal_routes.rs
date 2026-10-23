//! Internal erasure surface for org purges (WS17.E3).
//!
//! `DELETE /v1/internal/orgs/:org/profiles` is called by quarry-control's GDPR
//! consumer when org-core publishes an organization erasure. The caller is a
//! service and the org travels in the path, so the route sits outside
//! `require_auth`, and the shared internal token gates it (the same check as
//! the change record route). It walks the org erasure families in `AppState`
//! (profiles first) and answers with counts only. It is idempotent: a second
//! call for the same org reports zeros.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Serialize;

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};

use crate::state::AppState;

/// Longest org id the route accepts. Org ids are short identifiers; the bound
/// keeps a hostile path from making a long SCAN pattern.
const MAX_ORG_ID_LEN: usize = 256;

/// Counts for one erasure family.
#[derive(Debug, Serialize)]
pub struct FamilyReport {
    pub family: &'static str,
    pub rows_deleted: u64,
    pub tombstones_deleted: u64,
    pub cache_keys_deleted: u64,
    pub keys_evicted: bool,
}

/// The whole purge, one entry per family in the order they ran.
#[derive(Debug, Serialize)]
pub struct PurgeOrgReport {
    pub families: Vec<FamilyReport>,
}

fn err_response(request_id: &str, err: QuarryError) -> (StatusCode, Json<Envelope<()>>) {
    (
        StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(Envelope::<()>::err(request_id, err)),
    )
}

/// `DELETE /v1/internal/orgs/:org/profiles`.
pub async fn purge_org_profiles(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<String>,
) -> Result<Json<Envelope<PurgeOrgReport>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::change_routes::verify_internal_token(&headers)
        .map_err(|e| err_response(&request_id, e))?;
    if org.trim().is_empty() || org.len() > MAX_ORG_ID_LEN || org.trim() != org {
        return Err(err_response(
            &request_id,
            QuarryError::new(ErrorCode::BadRequest, "org id is missing or malformed"),
        ));
    }
    let mut families = Vec::with_capacity(state.org_erasables.len());
    for family in state.org_erasables.iter() {
        let counts = family
            .purge_org(&org)
            .await
            .map_err(|e| err_response(&request_id, e))?;
        families.push(FamilyReport {
            family: family.family(),
            rows_deleted: counts.rows,
            tombstones_deleted: counts.tombstones,
            cache_keys_deleted: counts.cache_keys,
            keys_evicted: counts.keys_evicted,
        });
    }
    tracing::info!(
        families = families.len(),
        "org erasure: profile families purged"
    );
    Ok(Json(Envelope::ok(
        request_id,
        PurgeOrgReport { families },
    )))
}
