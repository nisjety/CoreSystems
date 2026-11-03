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
//!
//! W2-U49 (X-19 condition c): `DELETE /v1/internal/orgs/:org/users/:user/receipts`
//! is the user-level hop. quarry-control calls it for a `user` or
//! `user_anonymize` erasure (one event per org the user belonged to, from the
//! user-core saga) when `QUARRY_CONTROL_GDPR_USER_RECEIPTS` is on. It deletes
//! the step receipts and browser timeline events whose `org_id` and `actor_id`
//! match, answers in the same counts-only shape (one `step_receipts` family) and
//! is idempotent. Same internal token, outside `require_auth`.
//!
//! W2-Q-U41b: one family's failure never stops the others. Every selected
//! family runs; when one failed, the route then answers with that error so
//! the caller retries the whole (idempotent) purge. The profile family's
//! crypto-shred receipt is `key_shred`, left out of the response when the hop
//! is off.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_runtime::org_erasure::OrgErasable;

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
    /// W2-Q-U41b: the crypto-shred receipt of the profile family
    /// (`destroyed`, `already_gone` or `not_needed`). Left out when the hop is
    /// off, so the default response is unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_shred: Option<&'static str>,
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

/// Family name the user-level route reports (the org family of the same tables).
pub const USER_RECEIPTS_FAMILY: &str = "step_receipts";

/// The same bound as an org id, for the user id in the path.
const MAX_USER_ID_LEN: usize = 256;

fn path_id_ok(id: &str, max: usize) -> bool {
    !id.trim().is_empty() && id.len() <= max && id.trim() == id
}

/// W2-Q-U41b: walk the families of `scope` in order. A failed family never
/// stops the ones after it (every family is idempotent, so the caller's retry
/// redoes the whole purge); the first error is returned beside the reports of
/// the families that completed.
pub(crate) async fn run_families(
    erasables: &[Arc<dyn OrgErasable>],
    scope: PurgeScope,
    org: &str,
) -> (Vec<FamilyReport>, Option<QuarryError>) {
    let mut families = Vec::with_capacity(erasables.len());
    let mut failure: Option<QuarryError> = None;
    for family in erasables.iter().filter(|f| scope.includes(f.family())) {
        match family.purge_org(org).await {
            Ok(counts) => families.push(FamilyReport {
                family: family.family(),
                rows_deleted: counts.rows,
                tombstones_deleted: counts.tombstones,
                cache_keys_deleted: counts.cache_keys,
                keys_evicted: counts.keys_evicted,
                guard_tombstones_written: counts.guard_tombstones,
                key_shred: counts.key_shred.map(|o| o.as_str()),
            }),
            Err(e) => {
                tracing::warn!(
                    family = family.family(),
                    "org erasure: family failed; continuing with the others"
                );
                if failure.is_none() {
                    failure = Some(e);
                }
            }
        }
    }
    (families, failure)
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
    if !path_id_ok(&org, MAX_ORG_ID_LEN) {
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
    let (families, failure) = run_families(&state.org_erasables, scope, &org).await;
    if let Some(err) = failure {
        tracing::warn!(
            families_done = families.len(),
            scope = ?scope,
            "org erasure: a family failed after the others ran; the erasure stays retryable"
        );
        return Err(err_response(&request_id, err));
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

/// `DELETE /v1/internal/orgs/:org/users/:user/receipts` (W2-U49).
///
/// With a durable frontier pool (the same `DATABASE_URL` the durable receipt
/// store uses) the delete runs in Postgres; otherwise it goes to the receipt
/// store in `AppState`, which is the in-memory store in development and answers
/// 501 for a store that cannot erase.
pub async fn purge_user_receipts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, user)): Path<(String, String)>,
) -> Result<Json<Envelope<PurgeOrgReport>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::change_routes::verify_internal_token(&headers)
        .map_err(|e| err_response(&request_id, e))?;
    if !path_id_ok(&org, MAX_ORG_ID_LEN) {
        return Err(err_response(
            &request_id,
            QuarryError::new(ErrorCode::BadRequest, "org id is missing or malformed"),
        ));
    }
    if !path_id_ok(&user, MAX_USER_ID_LEN) {
        return Err(err_response(
            &request_id,
            QuarryError::new(ErrorCode::BadRequest, "user id is missing or malformed"),
        ));
    }
    let counts = purge_user_receipts_in(&state, &org, &user)
        .await
        .map_err(|e| err_response(&request_id, e))?;
    tracing::info!(
        receipts = counts.receipts,
        timeline_events = counts.timeline_events,
        "user erasure: step receipts purged"
    );
    Ok(Json(Envelope::ok(
        request_id,
        user_purge_report(counts),
    )))
}

async fn purge_user_receipts_in(
    state: &AppState,
    org: &str,
    user: &str,
) -> Result<quarry_runtime::step_receipts::ActorReceiptPurge, QuarryError> {
    #[cfg(feature = "postgres-queue")]
    {
        if let Some(pool) = state.queue_pool.as_ref() {
            return quarry_runtime::step_receipts::purge_actor_receipts_pg(pool, org, user).await;
        }
    }
    state.receipts.purge_for_actor(org, user).await
}

/// The counts-only response: one family, the same shape as the org route.
fn user_purge_report(counts: quarry_runtime::step_receipts::ActorReceiptPurge) -> PurgeOrgReport {
    PurgeOrgReport {
        families: vec![FamilyReport {
            family: USER_RECEIPTS_FAMILY,
            rows_deleted: counts.total(),
            tombstones_deleted: 0,
            cache_keys_deleted: 0,
            keys_evicted: false,
            guard_tombstones_written: 0,
            key_shred: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_ids_refuse_empty_padded_and_overlong_values() {
        assert!(path_id_ok("org_a", MAX_ORG_ID_LEN));
        assert!(!path_id_ok("", MAX_ORG_ID_LEN));
        assert!(!path_id_ok(" org_a", MAX_ORG_ID_LEN));
        assert!(!path_id_ok("org_a ", MAX_ORG_ID_LEN));
        assert!(!path_id_ok(&"u".repeat(MAX_USER_ID_LEN + 1), MAX_USER_ID_LEN));
    }

    #[test]
    fn the_user_report_is_one_counts_only_family() {
        let report = user_purge_report(quarry_runtime::step_receipts::ActorReceiptPurge {
            receipts: 2,
            timeline_events: 1,
        });
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["families"].as_array().unwrap().len(), 1);
        assert_eq!(json["families"][0]["family"], "step_receipts");
        assert_eq!(json["families"][0]["rows_deleted"], 3);
        assert!(json["families"][0].get("guard_tombstones_written").is_none());
    }

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
            key_shred: None,
        };
        let json = serde_json::to_value(&report).unwrap();
        assert!(json.get("guard_tombstones_written").is_none());
        assert!(json.get("key_shred").is_none());
        assert_eq!(json["rows_deleted"], 1);
    }

    /// W2-Q-U41b: a failed family never stops the others, and its error comes
    /// back so the caller retries.
    struct FakeFamily {
        name: &'static str,
        fail: bool,
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl OrgErasable for FakeFamily {
        fn family(&self) -> &'static str {
            self.name
        }

        async fn purge_org(
            &self,
            _org_id: &str,
        ) -> quarry_core::error::QuarryResult<quarry_runtime::org_erasure::PurgeCounts> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail {
                return Err(QuarryError::new(
                    ErrorCode::RuntimeNotReady,
                    "org erasure profiles: key shred failed (unavailable); retry the erasure",
                ));
            }
            Ok(quarry_runtime::org_erasure::PurgeCounts {
                rows: 2,
                ..Default::default()
            })
        }
    }

    fn fake(name: &'static str, fail: bool) -> Arc<FakeFamily> {
        Arc::new(FakeFamily {
            name,
            fail,
            calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    #[tokio::test]
    async fn a_failed_family_never_blocks_the_others() {
        let profiles = fake("profiles", true);
        let receipts = fake("step_receipts", false);
        let erasables: Vec<Arc<dyn OrgErasable>> = vec![
            profiles.clone() as Arc<dyn OrgErasable>,
            receipts.clone() as Arc<dyn OrgErasable>,
        ];

        let (reports, failure) = run_families(&erasables, PurgeScope::Org, "org_a").await;
        assert_eq!(profiles.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(receipts.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].family, "step_receipts");
        assert_eq!(reports[0].rows_deleted, 2);
        let err = failure.expect("the profile failure comes back");
        assert_eq!(err.code, ErrorCode::RuntimeNotReady);

        // Profiles scope walks the profile family only, as before.
        let (reports, failure) = run_families(&erasables, PurgeScope::Profiles, "org_a").await;
        assert!(reports.is_empty());
        assert!(failure.is_some());
        assert_eq!(receipts.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn the_key_shred_receipt_is_reported_when_present() {
        let report = FamilyReport {
            family: "profiles",
            rows_deleted: 0,
            tombstones_deleted: 0,
            cache_keys_deleted: 0,
            keys_evicted: true,
            guard_tombstones_written: 0,
            key_shred: Some("already_gone"),
        };
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["key_shred"], "already_gone");
    }
}
