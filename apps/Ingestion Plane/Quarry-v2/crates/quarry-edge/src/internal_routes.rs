//! Internal erasure surface for org purges (WS17.E3).
//!
//! `DELETE /v1/internal/orgs/:org/profiles` is called by quarry-control's GDPR
//! consumer when org-core publishes an organization erasure. The caller is a
//! service and the org travels in the path, so the route sits outside
//! `require_auth`, and the shared internal token gates it (the same check as
//! the change record route). It walks the org erasure families in `AppState`
//! (profiles first) and answers with counts only. It is idempotent: a second
//! call for the same org reports zeros.
//!
//! W2-U41a: `?scope=org` walks every registered family (step receipts,
//! checkpoints, frontier, change baselines, job history, artifacts, when
//! `QUARRY_ORG_ERASURE_FAMILIES` registers them). Without `scope`, or with
//! `scope=profiles`, the route walks the profile family only, as before, so a
//! caller that does not know the new families keeps the old behaviour.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

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
    /// Tombstones the profile tombstone guard wrote (D2-077). Left out of the
    /// response when zero, so the default response is unchanged.
    #[serde(skip_serializing_if = "is_zero")]
    pub guard_tombstones_written: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// `?scope=` of the purge route.
#[derive(Debug, Default, Deserialize)]
pub struct PurgeQuery {
    /// `profiles` (default) or `org` (every registered family).
    #[serde(default)]
    pub scope: Option<String>,
}

/// Which families one call walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurgeScope {
    Profiles,
    Org,
}

impl PurgeScope {
    pub fn parse(raw: Option<&str>) -> Option<Self> {
        match raw.map(str::trim) {
            None | Some("") | Some("profiles") => Some(Self::Profiles),
            Some("org") => Some(Self::Org),
            _ => None,
        }
    }

    /// The family is walked in this scope.
    pub fn includes(self, family: &str) -> bool {
        match self {
            Self::Profiles => family == quarry_runtime::org_erasure::PROFILES_FAMILY,
            Self::Org => true,
        }
    }
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

/// `DELETE /v1/internal/orgs/:org/profiles[?scope=profiles|org]`.
pub async fn purge_org_profiles(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<String>,
    Query(query): Query<PurgeQuery>,
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
    let Some(scope) = PurgeScope::parse(query.scope.as_deref()) else {
        return Err(err_response(
            &request_id,
            QuarryError::new(ErrorCode::BadRequest, "scope must be profiles or org"),
        ));
    };
    let mut families = Vec::with_capacity(state.org_erasables.len());
    for family in state
        .org_erasables
        .iter()
        .filter(|f| scope.includes(f.family()))
    {
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
            guard_tombstones_written: counts.guard_tombstones,
        });
    }
    tracing::info!(
        families = families.len(),
        scope = ?scope,
        "org erasure: families purged"
    );
    Ok(Json(Envelope::ok(
        request_id,
        PurgeOrgReport { families },
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_defaults_to_profiles_and_refuses_unknown_values() {
        assert_eq!(PurgeScope::parse(None), Some(PurgeScope::Profiles));
        assert_eq!(PurgeScope::parse(Some("")), Some(PurgeScope::Profiles));
        assert_eq!(PurgeScope::parse(Some("profiles")), Some(PurgeScope::Profiles));
        assert_eq!(PurgeScope::parse(Some("org")), Some(PurgeScope::Org));
        assert_eq!(PurgeScope::parse(Some("all")), None);
        assert!(PurgeScope::Profiles.includes("profiles"));
        assert!(!PurgeScope::Profiles.includes("step_receipts"));
        assert!(!PurgeScope::Profiles.includes("artifacts"));
        assert!(PurgeScope::Org.includes("step_receipts"));
    }

    #[test]
    fn a_zero_guard_count_leaves_the_response_shape_unchanged() {
        let report = FamilyReport {
            family: "profiles",
            rows_deleted: 1,
            tombstones_deleted: 0,
            cache_keys_deleted: 0,
            keys_evicted: true,
            guard_tombstones_written: 0,
        };
        let json = serde_json::to_value(&report).unwrap();
        assert!(json.get("guard_tombstones_written").is_none());
        assert_eq!(json["rows_deleted"], 1);
    }
}
