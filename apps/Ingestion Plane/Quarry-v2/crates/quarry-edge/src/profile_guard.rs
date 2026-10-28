//! Owner and ACL enforcement for the `/v1/profiles*` routes (WS17.P2).
//!
//! The model (D16-7). `owner_user_id` is stamped on INSERT only, with the caller's
//! `Claims::actor_id()`. A `user_private` profile, and a `run_scoped` one, belongs to
//! its owner alone. An `org_shared` profile is usable by every member of the org: any
//! member may list, read, probe, attach and save it, and only the owner may rename,
//! rescope or delete it. A caller who is not allowed gets 404, never 403, so a probe
//! cannot tell a foreign profile from a missing one.
//!
//! Legacy rows (owner NULL, written before WS17.P1) follow the precedent of
//! `0008_step_receipt_actor_scope.sql`: a private row fails closed (hidden from
//! everyone, not deleted); an org-shared row stays visible to the org, and its
//! owner-only operations stay open to the org as they were before WS17.P2 (depth).
//! Rescue is the operator SQL in `docs/RUNBOOKS.md`.
//!
//! The mode comes from `QUARRY_EDGE__PROFILE_OWNER_ENFORCE` (`AppState::profile_owner_enforce`).
//! `off` skips the decision. `observe` (the default, and what the build ships in) makes
//! the same decision, logs `profile.owner.would_deny` with a closed reason and the op,
//! and serves the request as before. `enforce` answers 404 on a denial. Turning
//! `enforce` on waits for the legacy-owner policy (D16 Q2, not among the approved 13):
//! the observe week shows which denials would reach real users.
//!
//! Logs carry the op and the reason class only: never a profile id, a name or an owner id.
//!
//! The owner of a profile a run creates is stamped at run start, for a person's run only
//! (`stamp_run_owner`, D16 Q2): insert-only, refused by a deletion tombstone newer than the
//! request, and never for a service principal (a model-driven run). A profile saved without
//! metadata is `user_private` in every store (`ProfileMetadata::stored_default`).
//!
//! The attach path is the start of a run that names a `profile_id` (`agent_routes::lifecycle`):
//! it calls `authorize` with `ProfileOp::Attach` (D2-045, `tests/profile_attach.rs`). The WS8.F
//! agent attachment (`POST /v1/agent/runs/{id}/attach`) names no profile, so it has no call.

use quarry_browser::session::{ProfileMetadata, ProfileScope, ProfileStore, ProfileSummary};
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::ProfileKind;
use quarry_core::QuarryResult;
use serde::Deserialize;

use crate::auth::Claims;
use crate::state::AppState;
use crate::zdr_refusal::{self, Refused};

/// Mode of the owner check. `Observe` is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OwnerEnforce {
    /// No check at all: the routes behave as before WS17.P2.
    Off,
    /// The check runs and logs `profile.owner.would_deny`; every request is served.
    #[default]
    Observe,
    /// A denied caller gets 404 `profile not found`.
    Enforce,
}

impl OwnerEnforce {
    /// The operator value of this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::Enforce => "enforce",
        }
    }

    /// Parses an operator value (`off`, `observe`, `enforce`; case and surrounding
    /// spaces ignored). `None` for anything else.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "observe" => Some(Self::Observe),
            "enforce" => Some(Self::Enforce),
            _ => None,
        }
    }
}

/// The operations the owner check distinguishes. `Import` is the `save_profile` write
/// (the capture write path); WS17.P0 names it the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileOp {
    /// Listing: rows the caller may not see are filtered out, not refused.
    List,
    /// Creating a named profile: the caller becomes the owner.
    Create,
    /// Writing a snapshot through `save_profile`.
    Import,
    /// Reading a profile's snapshot (`GET /v1/profiles/:id`).
    Read,
    /// Rename or rescope (`PATCH /v1/profiles/:id`).
    Update,
    /// Restore probe (`POST /v1/profiles/:id/restore-probe`).
    Probe,
    /// Deleting a profile.
    Delete,
    /// Attaching a profile to a run (the start of a run that names a `profile_id`).
    Attach,
}

impl ProfileOp {
    /// A short closed label for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Create => "create",
            Self::Import => "import",
            Self::Read => "read",
            Self::Update => "update",
            Self::Probe => "probe",
            Self::Delete => "delete",
            Self::Attach => "attach",
        }
    }

    /// Operations an org-shared profile reserves for its owner.
    fn owner_only_on_shared(self) -> bool {
        matches!(self, Self::Update | Self::Delete)
    }
}

/// The scope a profile import (`save_profile`) needs (WS17.P0). No token carries it in
/// production today, so an HTTP import is refused for everyone until one is issued.
pub const IMPORT_SCOPE: &str = "quarry:profiles:import";

/// The 403 message of a service principal. It names no caller data.
pub const SERVICE_PRINCIPAL_MESSAGE: &str =
    "profiles are not available to service principals; they never read or write browser state";

impl ProfileOp {
    /// The matrix action label a ZDR refusal of this op carries (`docs/ERROR_REASONS.md`).
    /// `None` for the two wind-down operations a ZDR organization keeps: list, and delete
    /// (erasing what it already holds).
    pub fn zdr_action(self) -> Option<&'static str> {
        match self {
            Self::List | Self::Delete => None,
            Self::Create => Some("create"),
            Self::Import => Some("save"),
            Self::Read => Some("read"),
            Self::Update => Some("update"),
            Self::Probe => Some("probe"),
            Self::Attach => Some("attach"),
        }
    }
}

/// A refusal decided from the principal alone, before the ownership check and the ZDR posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalRefusal {
    /// A service principal, on any profile operation.
    ServicePrincipal,
    /// An import from a token that lacks [`IMPORT_SCOPE`].
    MissingImportScope,
}

impl PrincipalRefusal {
    /// The closed log label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServicePrincipal => "service_principal",
            Self::MissingImportScope => "missing_import_scope",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::ServicePrincipal => SERVICE_PRINCIPAL_MESSAGE,
            Self::MissingImportScope => {
                "profile import requires the quarry:profiles:import scope"
            }
        }
    }
}

/// The principal check: a service principal never reaches a profile, and an import needs
/// [`IMPORT_SCOPE`]. Pure, so the table is unit-tested without a store.
pub fn principal_refusal(claims: &Claims, op: ProfileOp) -> Option<PrincipalRefusal> {
    if claims.is_service() {
        return Some(PrincipalRefusal::ServicePrincipal);
    }
    if op == ProfileOp::Import && !claims.has_scope(IMPORT_SCOPE) {
        return Some(PrincipalRefusal::MissingImportScope);
    }
    None
}

/// The first gate of every `/v1/profiles*` handler (WS17.P0), before any store read: the
/// principal check, then the ZDR posture. The ZDR half is WS4.H's helper: it observes
/// (logs `zdr.would_refuse`, serves) until `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` is
/// on, and then answers 403 `zdr_durable_mutation_forbidden`. List and delete are never
/// refused on the ZDR side.
pub fn precheck(
    state: &AppState,
    claims: &Claims,
    op: ProfileOp,
    request_id: &str,
) -> Result<(), Refused> {
    if let Some(refusal) = principal_refusal(claims, op) {
        tracing::info!(
            op = op.as_str(),
            reason = refusal.as_str(),
            "profile.precheck.refused"
        );
        return Err(zdr_refusal::respond(
            request_id,
            QuarryError::new(ErrorCode::Forbidden, refusal.message()),
        ));
    }
    match op.zdr_action() {
        Some(action) => {
            zdr_refusal::refuse_for_claims(state, claims, None, request_id, "profiles", action)
        }
        None => Ok(()),
    }
}

/// What the store says about one profile, for the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileFacts {
    /// Whether a snapshot row or a metadata row exists.
    pub exists: bool,
    /// The stored scope. A profile with no metadata row reads as `UserPrivate`
    /// (`ProfileMetadata::stored_default`, D16 Q2), so it is treated as owner-only.
    pub scope: ProfileScope,
    /// The owner's actor id; `None` on a legacy row.
    pub owner: Option<String>,
}

impl ProfileFacts {
    /// Whether `actor` owns the profile (a legacy row is owned by nobody).
    pub fn owned_by(&self, actor: &str) -> bool {
        self.owner.as_deref() == Some(actor)
    }
}

/// Why a decision denies. The reason is the only detail a log carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    /// The row has an owner and the caller is not it (and the op is not open to the org).
    NotOwner,
    /// The row has no owner (written before WS17.P1) and it is not org-shared.
    LegacyOwnerless,
}

impl DenyReason {
    /// The closed log label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotOwner => "not_owner",
            Self::LegacyOwnerless => "legacy_ownerless",
        }
    }
}

/// The decision. `None` allows; `Some(reason)` denies. Pure, so the table is unit-tested
/// without a store. Callers apply [`allowed`] to turn it into an outcome.
pub fn decide(facts: &ProfileFacts, actor: &str, op: ProfileOp) -> Option<DenyReason> {
    // A create has nothing to protect, and a missing row is the route's own 404.
    if op == ProfileOp::Create || !facts.exists {
        return None;
    }
    let org_shared = facts.scope == ProfileScope::OrgShared;
    match facts.owner.as_deref() {
        Some(owner) if owner == actor => None,
        Some(_) if org_shared && !op.owner_only_on_shared() => None,
        Some(_) => Some(DenyReason::NotOwner),
        // Legacy org-shared rows stay visible, and their owner-only operations stay
        // open to the org, as before WS17.P2 (the owner is unknown; see depth).
        None if org_shared => None,
        None => Some(DenyReason::LegacyOwnerless),
    }
}

/// Applies the mode to one decision. Returns `true` when the request may proceed.
/// `observe` logs `profile.owner.would_deny` and proceeds; `enforce` logs
/// `profile.owner.denied` and refuses.
pub fn allowed(mode: OwnerEnforce, op: ProfileOp, denial: Option<DenyReason>) -> bool {
    let Some(reason) = denial else {
        return true;
    };
    match mode {
        OwnerEnforce::Off => true,
        OwnerEnforce::Observe => {
            tracing::info!(
                op = op.as_str(),
                reason = reason.as_str(),
                "profile.owner.would_deny"
            );
            crate::metrics::record_profile_owner_decision(
                "would_deny",
                op.as_str(),
                reason.as_str(),
                1,
            );
            true
        }
        OwnerEnforce::Enforce => {
            tracing::info!(
                op = op.as_str(),
                reason = reason.as_str(),
                "profile.owner.denied"
            );
            crate::metrics::record_profile_owner_decision(
                "denied",
                op.as_str(),
                reason.as_str(),
                1,
            );
            false
        }
    }
}

/// The facts for one profile: an existence probe, then the metadata row, which carries
/// the owner and the scope. A snapshot-only row reports its owner through the metadata
/// load as well (both stores stamp the owner on the row's INSERT).
pub async fn load_facts(
    store: &dyn ProfileStore,
    org_id: &str,
    profile_id: &ProfileKind,
) -> QuarryResult<ProfileFacts> {
    let exists = store.exists(org_id, profile_id).await?;
    let metadata = store
        .load_metadata(org_id, profile_id)
        .await?
        .unwrap_or_else(ProfileMetadata::stored_default);
    Ok(ProfileFacts {
        exists,
        scope: metadata.scope,
        owner: metadata.owner,
    })
}

/// The route check for one profile. Returns the facts (the route reports
/// `owner_is_caller` from them), or the 404 a denied caller gets under `enforce`.
/// Under `off` no decision is made, but the facts are still read so the response can
/// report `owner_is_caller`.
pub async fn authorize(
    state: &AppState,
    claims: &Claims,
    profile_id: &ProfileKind,
    op: ProfileOp,
) -> Result<ProfileFacts, QuarryError> {
    let mode = state.profile_owner_enforce;
    let facts = load_facts(state.profiles.as_ref(), &claims.org_id, profile_id).await?;
    let denial = match mode {
        OwnerEnforce::Off => None,
        _ => decide(&facts, claims.actor_id(), op),
    };
    if allowed(mode, op, denial) {
        Ok(facts)
    } else {
        Err(QuarryError::new(
            ErrorCode::NotFound,
            format!("profile not found: {profile_id}"),
        ))
    }
}

/// Filters a listing for the caller. `enforce` drops the rows the caller may not see;
/// `observe` keeps every row and logs one count per reason; `off` keeps all rows.
pub fn filter_listing(
    mode: OwnerEnforce,
    actor: &str,
    rows: Vec<ProfileSummary>,
) -> Vec<ProfileSummary> {
    if mode == OwnerEnforce::Off {
        return rows;
    }
    let mut not_owner = 0usize;
    let mut legacy_ownerless = 0usize;
    let mut kept = Vec::with_capacity(rows.len());
    for row in rows {
        let facts = ProfileFacts {
            exists: true,
            scope: row.scope,
            owner: row.owner.clone(),
        };
        match decide(&facts, actor, ProfileOp::List) {
            None => kept.push(row),
            Some(reason) => {
                match reason {
                    DenyReason::NotOwner => not_owner += 1,
                    DenyReason::LegacyOwnerless => legacy_ownerless += 1,
                }
                if mode == OwnerEnforce::Observe {
                    kept.push(row);
                }
            }
        }
    }
    if not_owner + legacy_ownerless > 0 {
        let outcome = if mode == OwnerEnforce::Enforce {
            "denied"
        } else {
            "would_deny"
        };
        for (reason, count) in [
            (DenyReason::NotOwner, not_owner),
            (DenyReason::LegacyOwnerless, legacy_ownerless),
        ] {
            if count > 0 {
                crate::metrics::record_profile_owner_decision(
                    outcome,
                    "list",
                    reason.as_str(),
                    count as u64,
                );
            }
        }
        if mode == OwnerEnforce::Enforce {
            tracing::info!(
                op = "list",
                not_owner,
                legacy_ownerless,
                "profile.owner.denied"
            );
        } else {
            tracing::info!(
                op = "list",
                not_owner,
                legacy_ownerless,
                "profile.owner.would_deny"
            );
        }
    }
    kept
}

/// Why a run start stamps no owner on its profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampSkip {
    /// The run keeps no persistent profile.
    NotPersistent,
    /// A resumed run keeps the profile (and the owner) its checkpoint holds.
    Resumed,
    /// The owner check is `off`: the routes behave as before WS17.P2.
    Off,
    /// A model-driven (service principal) run. It names no person: `Claims::actor_id()` is the
    /// shared service id, and stamping it would make every model run in the org the owner of
    /// those cookies, hidden from the person (D16 Q2, challenger). Until an acting-user claim
    /// exists the profile it saves stays ownerless, and so hidden under `enforce`.
    ServicePrincipal,
}

/// Decides whether a run start stamps the owner (D16 Q2, option C). Pure, so the table is
/// unit-tested without a store. `None` means stamp.
pub fn stamp_skip(
    mode: OwnerEnforce,
    claims: &Claims,
    persist_profile: bool,
    resumed: bool,
) -> Option<StampSkip> {
    if resumed {
        Some(StampSkip::Resumed)
    } else if !persist_profile {
        Some(StampSkip::NotPersistent)
    } else if mode == OwnerEnforce::Off {
        Some(StampSkip::Off)
    } else if claims.is_service() || claims.actor_id().trim().is_empty() {
        Some(StampSkip::ServicePrincipal)
    } else {
        None
    }
}

/// The scope a run's profile row is created with. A run's `profile_scope` of `ephemeral` (the
/// request default) is not a scope a stored row may carry, so it becomes `user_private`, as in
/// every store (D16 Q2).
pub fn stamp_scope(requested: quarry_runtime::step_receipts::BrowserProfileScope) -> ProfileScope {
    use quarry_runtime::step_receipts::BrowserProfileScope as Requested;
    match requested {
        Requested::Ephemeral | Requested::UserPrivate => ProfileScope::UserPrivate,
        Requested::OrgShared => ProfileScope::OrgShared,
        Requested::RunScoped => ProfileScope::RunScoped,
    }
}

/// Stamps the caller as the owner of the profile a run is starting on (WS17.P2, D16 Q2): a
/// person's run only, insert-only (an existing row, owned or not, is never touched), and
/// refused by a tombstone newer than `requested_at` (the start of the request), as `save` is.
/// Never fails the run start: a store error is counted and logged as a class, and the row
/// the run saves later has no owner (hidden under `enforce`, found by the legacy count).
pub async fn stamp_run_owner(
    state: &AppState,
    claims: &Claims,
    profile_id: &ProfileKind,
    scope: ProfileScope,
    requested_at: chrono::DateTime<chrono::Utc>,
    persist_profile: bool,
    resumed: bool,
) {
    match stamp_skip(state.profile_owner_enforce, claims, persist_profile, resumed) {
        Some(StampSkip::ServicePrincipal) => {
            crate::metrics::record_profile_owner_stamp("skipped_service");
            return;
        }
        Some(_) => return,
        None => {}
    }
    let metadata = ProfileMetadata {
        name: None,
        scope,
        owner: Some(claims.actor_id().to_owned()),
    };
    match state
        .profiles
        .stamp_owner(&claims.org_id, profile_id, &metadata, requested_at)
        .await
    {
        Ok(outcome) => {
            tracing::info!(outcome = outcome.reason_class(), "profile.owner.stamp");
            crate::metrics::record_profile_owner_stamp(outcome.reason_class());
        }
        Err(_) => {
            // The error text can carry a row or an id: only the class leaves this function.
            tracing::warn!(outcome = "failed", "profile.owner.stamp");
            crate::metrics::record_profile_owner_stamp("failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "user_owner";
    const OTHER: &str = "user_other";

    fn facts(scope: ProfileScope, owner: Option<&str>) -> ProfileFacts {
        ProfileFacts {
            exists: true,
            scope,
            owner: owner.map(str::to_owned),
        }
    }

    fn summary(scope: ProfileScope, owner: Option<&str>) -> ProfileSummary {
        ProfileSummary {
            profile_id: quarry_core::ids::Id::new(),
            name: None,
            scope,
            owner: owner.map(str::to_owned),
        }
    }

    const ALL_OPS: [ProfileOp; 8] = [
        ProfileOp::List,
        ProfileOp::Create,
        ProfileOp::Import,
        ProfileOp::Read,
        ProfileOp::Update,
        ProfileOp::Probe,
        ProfileOp::Delete,
        ProfileOp::Attach,
    ];

    fn principal(service: bool, scopes: &[&str]) -> Claims {
        let actor = if service { "execution-core" } else { OWNER };
        Claims {
            web_policy: None,
            sub: actor.to_owned(),
            iss: "test".to_owned(),
            exp: i64::MAX,
            org_id: "org_guard".to_owned(),
            user_id: if service { String::new() } else { actor.to_owned() },
            principal_type: Some(if service { "service" } else { "user" }.to_owned()),
            service_id: service.then(|| actor.to_owned()),
            nbf: None,
            aud: None,
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
            zdr: None,
        }
    }

    #[test]
    fn service_principal_is_refused_for_every_op() {
        // Even a service token that holds the import scope gets 403 on every profile op.
        let service = principal(true, &[IMPORT_SCOPE]);
        for op in ALL_OPS {
            assert_eq!(
                principal_refusal(&service, op),
                Some(PrincipalRefusal::ServicePrincipal),
                "{op:?}"
            );
        }
    }

    #[test]
    fn import_requires_scope() {
        let user = principal(false, &[]);
        assert_eq!(
            principal_refusal(&user, ProfileOp::Import),
            Some(PrincipalRefusal::MissingImportScope)
        );
        // Every other op needs no scope from a user token.
        for op in ALL_OPS {
            if op != ProfileOp::Import {
                assert_eq!(principal_refusal(&user, op), None, "{op:?}");
            }
        }
        let importer = principal(false, &[IMPORT_SCOPE]);
        assert_eq!(principal_refusal(&importer, ProfileOp::Import), None);
    }

    #[test]
    fn zdr_wind_down_is_list_and_delete_only() {
        // The op set a ZDR organization keeps: list and delete. Every other op is refused
        // when the flag is on (the flag half is in tests/profile_guard.rs).
        for op in ALL_OPS {
            let wind_down = matches!(op, ProfileOp::List | ProfileOp::Delete);
            assert_eq!(op.zdr_action().is_none(), wind_down, "{op:?}");
        }
    }

    #[test]
    fn owner_can_use_user_private() {
        let f = facts(ProfileScope::UserPrivate, Some(OWNER));
        for op in ALL_OPS {
            assert_eq!(decide(&f, OWNER, op), None, "owner denied on {op:?}");
        }
    }

    #[test]
    fn non_owner_gets_404_on_every_route() {
        // Every op a non-owner can reach on a private row is denied, and under enforce
        // each denial becomes the route's 404 (Create has no row to protect).
        let f = facts(ProfileScope::UserPrivate, Some(OWNER));
        for op in ALL_OPS {
            if op == ProfileOp::Create {
                continue;
            }
            let denial = decide(&f, OTHER, op);
            assert_eq!(denial, Some(DenyReason::NotOwner), "{op:?}");
            assert!(!allowed(OwnerEnforce::Enforce, op, denial), "{op:?}");
        }
    }

    #[test]
    fn non_owner_list_hides_user_private_but_shows_org_shared() {
        let rows = vec![
            summary(ProfileScope::UserPrivate, Some(OWNER)),
            summary(ProfileScope::OrgShared, Some(OWNER)),
            summary(ProfileScope::RunScoped, Some(OWNER)),
            summary(ProfileScope::UserPrivate, None),
        ];
        let kept = filter_listing(OwnerEnforce::Enforce, OTHER, rows);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].scope, ProfileScope::OrgShared);
    }

    #[test]
    fn org_shared_member_can_attach_but_not_rename_or_delete() {
        let f = facts(ProfileScope::OrgShared, Some(OWNER));
        for op in [ProfileOp::Attach, ProfileOp::Read, ProfileOp::Probe, ProfileOp::Import] {
            assert_eq!(decide(&f, OTHER, op), None, "member denied on {op:?}");
        }
        assert_eq!(decide(&f, OTHER, ProfileOp::Update), Some(DenyReason::NotOwner));
        assert_eq!(decide(&f, OTHER, ProfileOp::Delete), Some(DenyReason::NotOwner));
        // The owner keeps every op.
        assert_eq!(decide(&f, OWNER, ProfileOp::Update), None);
        assert_eq!(decide(&f, OWNER, ProfileOp::Delete), None);
    }

    #[test]
    fn legacy_ownerless_user_private_is_hidden_in_enforce_and_allowed_in_observe() {
        let f = facts(ProfileScope::UserPrivate, None);
        let denial = decide(&f, OWNER, ProfileOp::Read);
        assert_eq!(denial, Some(DenyReason::LegacyOwnerless));
        assert!(!allowed(OwnerEnforce::Enforce, ProfileOp::Read, denial));
        assert!(allowed(OwnerEnforce::Observe, ProfileOp::Read, denial));
        assert!(allowed(OwnerEnforce::Off, ProfileOp::Read, denial));

        let rows = vec![summary(ProfileScope::UserPrivate, None)];
        assert!(filter_listing(OwnerEnforce::Enforce, OWNER, rows.clone()).is_empty());
        assert_eq!(filter_listing(OwnerEnforce::Observe, OWNER, rows).len(), 1);
    }

    #[test]
    fn legacy_org_shared_stays_visible_to_the_org() {
        let f = facts(ProfileScope::OrgShared, None);
        for op in ALL_OPS {
            if op == ProfileOp::Create {
                continue;
            }
            assert_eq!(decide(&f, OTHER, op), None, "{op:?}");
        }
    }

    #[test]
    fn attach_of_unknown_profile_id_creates_it_owned_by_caller() {
        // No row yet: nothing to protect, so the first save or attach goes through.
        let missing = ProfileFacts {
            exists: false,
            scope: ProfileScope::Ephemeral,
            owner: None,
        };
        assert_eq!(decide(&missing, OWNER, ProfileOp::Attach), None);
        assert_eq!(decide(&missing, OWNER, ProfileOp::Import), None);
    }

    #[test]
    fn second_user_cannot_claim_existing_profile_id_by_saving() {
        let private = facts(ProfileScope::UserPrivate, Some(OWNER));
        assert_eq!(
            decide(&private, OTHER, ProfileOp::Import),
            Some(DenyReason::NotOwner)
        );
        // An org-shared profile takes writes from its members (the run that attached it
        // persists it); the owner-only ops are the ones that stay with the owner.
        let shared = facts(ProfileScope::OrgShared, Some(OWNER));
        assert_eq!(decide(&shared, OTHER, ProfileOp::Import), None);
    }

    #[test]
    fn off_never_denies_and_observe_only_logs() {
        assert!(allowed(OwnerEnforce::Off, ProfileOp::Delete, Some(DenyReason::NotOwner)));
        assert!(allowed(
            OwnerEnforce::Observe,
            ProfileOp::Delete,
            Some(DenyReason::NotOwner)
        ));
        assert!(allowed(OwnerEnforce::Enforce, ProfileOp::Delete, None));
    }

    #[test]
    fn a_stored_row_without_metadata_is_owner_only_not_open() {
        // D16 Q2: a row with no metadata reads as user_private, never as a scope that is open.
        let stored = ProfileMetadata::stored_default();
        assert_eq!(stored.scope, ProfileScope::UserPrivate);
        let f = ProfileFacts {
            exists: true,
            scope: stored.scope,
            owner: Some(OWNER.to_owned()),
        };
        assert_eq!(decide(&f, OTHER, ProfileOp::Read), Some(DenyReason::NotOwner));
        assert_eq!(decide(&f, OWNER, ProfileOp::Read), None);
    }

    #[test]
    fn the_stamp_is_for_a_persistent_fresh_run_of_a_person_only() {
        let person = principal(false, &[]);
        let service = principal(true, &[]);
        let observe = OwnerEnforce::Observe;
        assert_eq!(stamp_skip(observe, &person, true, false), None);
        assert_eq!(stamp_skip(OwnerEnforce::Enforce, &person, true, false), None);
        assert_eq!(
            stamp_skip(observe, &service, true, false),
            Some(StampSkip::ServicePrincipal)
        );
        assert_eq!(
            stamp_skip(observe, &person, false, false),
            Some(StampSkip::NotPersistent)
        );
        assert_eq!(
            stamp_skip(observe, &person, true, true),
            Some(StampSkip::Resumed)
        );
        assert_eq!(
            stamp_skip(OwnerEnforce::Off, &person, true, false),
            Some(StampSkip::Off)
        );
        // A token with no actor id names no person.
        let mut nameless = principal(false, &[]);
        nameless.user_id = String::new();
        assert_eq!(
            stamp_skip(observe, &nameless, true, false),
            Some(StampSkip::ServicePrincipal)
        );
    }

    #[test]
    fn a_runs_ephemeral_scope_is_stamped_user_private() {
        use quarry_runtime::step_receipts::BrowserProfileScope as Requested;
        assert_eq!(stamp_scope(Requested::Ephemeral), ProfileScope::UserPrivate);
        assert_eq!(stamp_scope(Requested::UserPrivate), ProfileScope::UserPrivate);
        assert_eq!(stamp_scope(Requested::OrgShared), ProfileScope::OrgShared);
        assert_eq!(stamp_scope(Requested::RunScoped), ProfileScope::RunScoped);
    }

    #[test]
    fn mode_parses_the_three_operator_values() {
        assert_eq!(OwnerEnforce::default(), OwnerEnforce::Observe);
        assert_eq!(OwnerEnforce::parse(" Enforce "), Some(OwnerEnforce::Enforce));
        assert_eq!(OwnerEnforce::parse("observe"), Some(OwnerEnforce::Observe));
        assert_eq!(OwnerEnforce::parse("off"), Some(OwnerEnforce::Off));
        assert_eq!(OwnerEnforce::parse("on"), None);
    }
}
