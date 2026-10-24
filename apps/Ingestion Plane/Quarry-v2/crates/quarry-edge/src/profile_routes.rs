//! `/v1/profiles` REST endpoints.
//!
//! CRUD over `ProfileStore`. Snapshots are SessionSnapshot wire shape (cookies,
//! storage, viewport, locale, timezone, UA). The store is pluggable; the edge
//! ships an InMemory default and can swap to S3 in production.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use quarry_browser::session::{
    ProfileMetadata, ProfileScope, ProfileSummary, SaveMeta, SessionSnapshot,
};
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::ProfileKind;

use crate::profile_guard::{self, ProfileOp};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SaveProfileRequest {
    pub profile_id: Option<String>,
    pub snapshot: SessionSnapshot,
}

#[derive(Debug, Serialize)]
pub struct SaveProfileResponse {
    pub profile_id: String,
}

/// Wire shape for one profile row — used by both `list_profiles` and
/// `create_profile`/`update_profile`. Phase 3 continuation: replaces the
/// bare `Vec<String>` the SPA previously had to render with truncated
/// ids alone.
#[derive(Debug, Serialize)]
pub struct ProfileSummaryResponse {
    pub profile_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub scope: String,
    /// True when the caller owns the profile (WS17.P2; additive).
    pub owner_is_caller: bool,
}

impl ProfileSummaryResponse {
    /// The wire row as seen by `actor`: `owner_is_caller` comes from the stored owner.
    fn for_caller(summary: ProfileSummary, actor: &str) -> Self {
        Self {
            owner_is_caller: summary.owner.as_deref() == Some(actor),
            profile_id: summary.profile_id.to_string(),
            name: summary.name,
            scope: summary.scope.as_str().to_owned(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ListProfilesResponse {
    pub profiles: Vec<ProfileSummaryResponse>,
}

/// `POST /v1/profiles/create` — explicitly create a named, scoped
/// profile with no browsing history yet (as opposed to `save_profile`,
/// which is Quarry's own internal snapshot-capture write path). Rejects
/// `scope: "ephemeral"` — an ephemeral *session* simply attaches no
/// profile at all, so a stored profile row with that scope would be a
/// contradiction in terms.
#[derive(Debug, Deserialize)]
pub struct CreateProfileRequest {
    #[serde(default)]
    pub name: Option<String>,
    pub scope: ProfileScope,
}

/// `PATCH /v1/profiles/:id` — rename and/or rescope an existing profile.
/// Fields are independently optional so a pure rename never has to
/// re-send the current scope (and vice versa); at least one must be
/// present.
#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub scope: Option<ProfileScope>,
}

#[derive(Debug, Serialize)]
pub struct LoadProfileResponse {
    pub profile_id: String,
    pub name: Option<String>,
    pub scope: String,
    /// True when the caller owns the profile (WS17.P2; additive).
    pub owner_is_caller: bool,
    /// The redacted snapshot (WS17.P0): shapes and counts, never a value.
    #[serde(flatten)]
    pub profile: ProfileView,
}

/// Largest body `POST /v1/profiles` (the import) accepts: a whole snapshot. Past it the
/// request answers 413 (WS17.P0). The rest of the `/v1/*` surface keeps the 1 MB cap.
pub const IMPORT_MAX_BYTES: usize = 4 * 1024 * 1024;

/// One cookie as the API shows it: every attribute of the stored cookie except its value.
/// `value` is always null; `value_len` is the stored value's length in bytes.
#[derive(Debug, Serialize)]
pub struct CookieView {
    pub name: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// True for a session cookie (no expiry).
    pub session: bool,
    pub expires: Option<DateTime<Utc>>,
    /// `strict`, `lax` or `none`; null when the snapshot predates capture v2 (WS17.C1).
    pub same_site: Option<&'static str>,
    pub value_len: usize,
    pub value: serde_json::Value,
}

/// The IndexedDB rows of one origin, by database name and row count. No key, no value.
#[derive(Debug, Serialize)]
pub struct OriginView {
    pub origin: String,
    pub idb_databases: Vec<String>,
    pub idb_rows: usize,
    /// Web storage entry counts of this origin (capture v2; 0 for a legacy snapshot).
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    /// The capture cut this origin short (a cap or the time budget).
    pub truncated: bool,
}

/// A profile's snapshot with every secret taken out: no cookie value, no storage key or
/// value, no IndexedDB key or value. Shapes and counts only.
#[derive(Debug, Serialize)]
pub struct ProfileView {
    pub cookies: Vec<CookieView>,
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    pub origins: Vec<OriginView>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    pub has_user_agent: bool,
    pub has_viewport: bool,
    /// A capture cut at least one origin short (capture v2, WS17.C1). False on a legacy snapshot.
    pub truncated: bool,
}

/// The redacted view of a snapshot. Every `GET /v1/profiles/:id` answers with this, never
/// with the snapshot itself.
pub fn redact(snapshot: &SessionSnapshot) -> ProfileView {
    let cookies = snapshot
        .cookies
        .iter()
        .map(|cookie| CookieView {
            name: cookie.name.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
            secure: cookie.secure,
            http_only: cookie.http_only,
            session: cookie.session || cookie.expires.is_none(),
            expires: cookie.expires,
            same_site: cookie.same_site.map(|same_site| match same_site {
                quarry_browser::session::SameSite::Strict => "strict",
                quarry_browser::session::SameSite::Lax => "lax",
                quarry_browser::session::SameSite::None => "none",
            }),
            value_len: cookie.value.len(),
            value: serde_json::Value::Null,
        })
        .collect();
    let mut by_origin: BTreeMap<&str, (BTreeSet<&str>, usize)> = BTreeMap::new();
    for entry in &snapshot.indexed_db {
        let slot = by_origin.entry(entry.origin.as_str()).or_default();
        slot.0.insert(entry.database.as_str());
        slot.1 += 1;
    }
    let mut origins: Vec<OriginView> = by_origin
        .into_iter()
        .map(|(origin, (databases, rows))| OriginView {
            origin: origin.to_owned(),
            idb_databases: databases.into_iter().map(str::to_owned).collect(),
            idb_rows: rows,
            local_storage_count: 0,
            session_storage_count: 0,
            truncated: false,
        })
        .collect();
    // Capture v2 snapshots (WS17.C1) carry every origin's web storage and IndexedDB.
    origins.extend(snapshot.origins.iter().map(|state| OriginView {
        origin: state.origin.clone(),
        idb_databases: state.indexed_db.iter().map(|db| db.name.clone()).collect(),
        idb_rows: state.idb_record_count(),
        local_storage_count: state.local_storage.len(),
        session_storage_count: state.session_storage.len(),
        truncated: state.truncated,
    }));
    let (local_storage_count, session_storage_count) = if snapshot.origins.is_empty() {
        (snapshot.local_storage.len(), snapshot.session_storage.len())
    } else {
        (
            snapshot.origins.iter().map(|o| o.local_storage.len()).sum(),
            snapshot.origins.iter().map(|o| o.session_storage.len()).sum(),
        )
    };
    ProfileView {
        cookies,
        local_storage_count,
        session_storage_count,
        origins,
        locale: snapshot.locale.clone(),
        timezone: snapshot.timezone.clone(),
        has_user_agent: snapshot.user_agent.is_some(),
        has_viewport: snapshot.viewport.is_some(),
        truncated: snapshot.origins.iter().any(|state| state.truncated),
    }
}

/// What a restore probe reports about a snapshot at one instant.
#[derive(Debug, PartialEq, Eq)]
pub struct RestoreSummary {
    pub restorable: bool,
    /// The nearest expiry among cookies that have not expired yet.
    pub earliest_expiry: Option<DateTime<Utc>>,
    /// Cookies whose expiry has passed.
    pub cookies_expired: usize,
    /// Distinct origins with IndexedDB rows (the only origin-attributed storage).
    pub origins_count: usize,
}

/// A profile is restorable when it holds a cookie that has not expired (a session cookie
/// always counts) or any storage at all. Expired cookies alone do not make it restorable.
pub fn restore_summary(snapshot: &SessionSnapshot, now: DateTime<Utc>) -> RestoreSummary {
    let mut live_cookies = 0usize;
    let mut cookies_expired = 0usize;
    let mut earliest_expiry: Option<DateTime<Utc>> = None;
    for cookie in &snapshot.cookies {
        match cookie.expires {
            Some(at) if at <= now => cookies_expired += 1,
            Some(at) => {
                live_cookies += 1;
                earliest_expiry = Some(earliest_expiry.map_or(at, |current| current.min(at)));
            }
            None => live_cookies += 1,
        }
    }
    let has_storage = !snapshot.local_storage.is_empty()
        || !snapshot.session_storage.is_empty()
        || !snapshot.indexed_db.is_empty()
        || snapshot.origins.iter().any(|state| !state.is_empty());
    let origins_count = snapshot
        .indexed_db
        .iter()
        .map(|entry| entry.origin.as_str())
        .chain(snapshot.origins.iter().map(|state| state.origin.as_str()))
        .collect::<BTreeSet<_>>()
        .len();
    RestoreSummary {
        restorable: live_cookies > 0 || has_storage,
        earliest_expiry,
        cookies_expired,
        origins_count,
    }
}

fn parse_id(s: &str) -> Result<ProfileKind, QuarryError> {
    s.parse::<ProfileKind>()
        .map_err(|e| QuarryError::new(ErrorCode::BadRequest, format!("invalid profile_id: {e}")))
}

fn err_response(request_id: &str, err: QuarryError) -> (StatusCode, Json<Envelope<()>>) {
    (
        StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(Envelope::<()>::err(request_id, err)),
    )
}

pub async fn save_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<SaveProfileRequest>,
) -> Result<Json<Envelope<SaveProfileResponse>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    profile_guard::precheck(&state, &claims, ProfileOp::Import, &request_id)?;

    let profile_id = match req.profile_id.as_deref() {
        Some(s) => parse_id(s).map_err(|e| err_response(&request_id, e))?,
        None => quarry_core::ids::Id::new(),
    };

    // WS17.P2: a body that names a profile needs write access to it. A body without an id
    // creates a new profile, owned by the caller.
    if req.profile_id.is_some() {
        profile_guard::authorize(&state, &claims, &profile_id, ProfileOp::Import)
            .await
            .map_err(|e| err_response(&request_id, e))?;
    }

    // Ordering stamp = request arrival (WS17.P1). An ignored save means a newer
    // capture or delete already holds the profile; the caller still gets the id,
    // as before, and the edge logs the class only.
    let outcome = state
        .profiles
        .save(
            &claims.org_id,
            &profile_id,
            &req.snapshot,
            &SaveMeta::now(Some(claims.actor_id().to_owned())),
        )
        .await
        .map_err(|e| err_response(&request_id, e))?;
    if !outcome.is_stored() {
        tracing::info!(reason = outcome.reason_class(), "profile save ignored");
    }

    Ok(Json(Envelope::ok(
        request_id,
        SaveProfileResponse {
            profile_id: profile_id.to_string(),
        },
    )))
}

pub async fn create_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<CreateProfileRequest>,
) -> Result<Json<Envelope<ProfileSummaryResponse>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    profile_guard::precheck(&state, &claims, ProfileOp::Create, &request_id)?;

    if req.scope == ProfileScope::Ephemeral {
        return Err(err_response(
            &request_id,
            QuarryError::new(
                ErrorCode::BadRequest,
                "a created profile cannot use the ephemeral scope; omit the profile entirely for ephemeral browsing",
            ),
        ));
    }

    // WS7.E: a created profile persists, so the operator and org layers decide whether it
    // may. The ZDR refusal of creation stays with profile_guard, so the decision is taken
    // for a non-ZDR run here.
    let decision = crate::web_policy_edge::decide(
        &state.policy,
        &claims,
        false,
        None,
    )
    .map_err(|error| err_response(&request_id, error))?;
    decision
        .policy()
        .require(quarry_core::web_policy::CapKey::ProfilePersistent, "profile.create")
        .map_err(|denial| {
            err_response(&request_id, crate::web_policy_edge::quarry_error(&denial))
        })?;

    let name = req
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let profile_id: ProfileKind = quarry_core::ids::Id::new();
    let metadata = ProfileMetadata {
        name: name.clone(),
        scope: req.scope,
        owner: Some(claims.actor_id().to_owned()),
    };

    state
        .profiles
        .save_metadata(&claims.org_id, &profile_id, &metadata)
        .await
        .map_err(|e| err_response(&request_id, e))?;

    Ok(Json(Envelope::ok(
        request_id,
        ProfileSummaryResponse {
            profile_id: profile_id.to_string(),
            name,
            scope: req.scope.as_str().to_owned(),
            owner_is_caller: true,
        },
    )))
}

pub async fn update_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(profile_id): Path<String>,
    Json(req): Json<UpdateProfileRequest>,
) -> Result<Json<Envelope<ProfileSummaryResponse>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    profile_guard::precheck(&state, &claims, ProfileOp::Update, &request_id)?;
    let parsed = parse_id(&profile_id).map_err(|e| err_response(&request_id, e))?;

    if req.name.is_none() && req.scope.is_none() {
        return Err(err_response(
            &request_id,
            QuarryError::new(ErrorCode::BadRequest, "provide name and/or scope to update"),
        ));
    }
    if req.scope == Some(ProfileScope::Ephemeral) {
        return Err(err_response(
            &request_id,
            QuarryError::new(
                ErrorCode::BadRequest,
                "a stored profile cannot be rescoped to ephemeral; delete it instead",
            ),
        ));
    }

    // WS17.P2: rename and rescope are owner-only, on an org-shared row too.
    profile_guard::authorize(&state, &claims, &parsed, ProfileOp::Update)
        .await
        .map_err(|e| err_response(&request_id, e))?;

    // A profile that predates this metadata (or was never explicitly
    // named) still exists via its snapshot — `exists` confirms it's real
    // before we allow a rename, rather than silently creating a
    // metadata-only row for an id nobody asked for.
    let exists = state
        .profiles
        .exists(&claims.org_id, &parsed)
        .await
        .map_err(|e| err_response(&request_id, e))?;
    if !exists {
        return Err(err_response(
            &request_id,
            QuarryError::new(
                ErrorCode::NotFound,
                format!("profile not found: {profile_id}"),
            ),
        ));
    }

    let current = state
        .profiles
        .load_metadata(&claims.org_id, &parsed)
        .await
        .map_err(|e| err_response(&request_id, e))?
        .unwrap_or_default();

    // The owner is carried through the rewrite unchanged: the store stamps it on INSERT only,
    // but a file-backed store rewrites the whole metadata object.
    let owner = current.owner.clone();
    let owner_is_caller = owner.as_deref() == Some(claims.actor_id());
    let name = match req.name.as_deref().map(str::trim) {
        // An explicit empty string clears the name.
        Some("") => None,
        Some(trimmed) => Some(trimmed.to_owned()),
        None => current.name,
    };
    // `req.scope` is already known non-ephemeral at this point (rejected
    // above). `current.scope` can still resolve to `Ephemeral` though — that
    // is `ProfileMetadata::default()`'s in-language value for "no metadata
    // was ever explicitly saved for this profile", returned via
    // `unwrap_or_default()` a few lines up. A profile in that state is, by
    // construction, real and persisted (it passed the `load` existence
    // check above) — never actually ephemeral — so a bare rename/rescope
    // that merges in that defaulted value must not durably write the
    // contradiction `scope: "ephemeral"` back to storage. Coerce it to
    // `UserPrivate`, mirroring the Postgres migration's own backfill
    // default for exactly this "row exists, no explicit scope was ever
    // recorded" case.
    let scope = match req.scope.unwrap_or(current.scope) {
        ProfileScope::Ephemeral => ProfileScope::UserPrivate,
        other => other,
    };
    let metadata = ProfileMetadata {
        name: name.clone(),
        scope,
        owner,
    };

    state
        .profiles
        .save_metadata(&claims.org_id, &parsed, &metadata)
        .await
        .map_err(|e| err_response(&request_id, e))?;

    Ok(Json(Envelope::ok(
        request_id,
        ProfileSummaryResponse {
            profile_id: parsed.to_string(),
            name,
            scope: scope.as_str().to_owned(),
            owner_is_caller,
        },
    )))
}

pub async fn load_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(profile_id): Path<String>,
) -> Result<Json<Envelope<LoadProfileResponse>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    profile_guard::precheck(&state, &claims, ProfileOp::Read, &request_id)?;

    let parsed = parse_id(&profile_id).map_err(|e| err_response(&request_id, e))?;

    let facts = profile_guard::authorize(&state, &claims, &parsed, ProfileOp::Read)
        .await
        .map_err(|e| err_response(&request_id, e))?;

    let snapshot = state
        .profiles
        .load(&claims.org_id, &parsed)
        .await
        .map_err(|e| err_response(&request_id, e))?
        .ok_or_else(|| {
            err_response(
                &request_id,
                QuarryError::new(
                    ErrorCode::NotFound,
                    format!("profile not found: {profile_id}"),
                ),
            )
        })?;

    let metadata = state
        .profiles
        .load_metadata(&claims.org_id, &parsed)
        .await
        .map_err(|e| err_response(&request_id, e))?
        .unwrap_or_default();

    Ok(Json(Envelope::ok(
        request_id,
        LoadProfileResponse {
            profile_id: parsed.to_string(),
            name: metadata.name,
            scope: metadata.scope.as_str().to_owned(),
            owner_is_caller: facts.owned_by(claims.actor_id()),
            profile: redact(&snapshot),
        },
    )))
}

pub async fn delete_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(profile_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    profile_guard::precheck(&state, &claims, ProfileOp::Delete, &request_id)?;

    let parsed = parse_id(&profile_id).map_err(|e| err_response(&request_id, e))?;

    profile_guard::authorize(&state, &claims, &parsed, ProfileOp::Delete)
        .await
        .map_err(|e| err_response(&request_id, e))?;

    state
        .profiles
        .delete(&claims.org_id, &parsed)
        .await
        .map_err(|e| err_response(&request_id, e))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Cycle 20 / cluster #13 — restore-probe endpoint.
///
/// Validates that a saved profile can still drive a browser session to
/// the auth-gated URL the caller asked about. The handler:
///
/// 1. Loads the snapshot from the tenant-scoped store.
/// 2. Inspects the cookies + storage so the client can render a
///    "we have N cookies, M storage entries, K IDB rows" summary.
/// 3. Reports whether the snapshot has the shape needed to attempt a
///    restore (non-empty cookies OR storage OR IDB).
///
/// The probe deliberately does NOT spin up a browser to navigate the
/// URL — that's a high-cost operation and is being scaffolded for a
/// follow-up cycle. For now the probe surfaces enough information that
/// the UI can flag "profile expired, please re-login" without waiting
/// for a real navigation.
#[derive(Debug, serde::Deserialize)]
pub struct RestoreProbeRequest {
    /// URL the caller intends to restore against. We don't navigate yet
    /// but we echo it back so the client correlates the probe to its
    /// intended workflow.
    pub url: String,
}

#[derive(Debug, serde::Serialize)]
pub struct RestoreProbeResponse {
    pub profile_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub scope: String,
    pub url: String,
    pub restorable: bool,
    pub cookies_count: usize,
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    pub indexed_db_count: usize,
    pub has_user_agent: bool,
    pub has_viewport: bool,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    /// The nearest expiry among cookies that have not expired yet (WS17.P0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earliest_expiry: Option<DateTime<Utc>>,
    /// Cookies whose expiry has passed. They do not make the profile restorable (WS17.P0).
    pub cookies_expired: usize,
    /// Distinct origins with IndexedDB rows (WS17.P0).
    pub origins_count: usize,
    /// True when the caller owns the profile (WS17.P2; additive).
    pub owner_is_caller: bool,
}

pub async fn restore_probe(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(profile_id): Path<String>,
    Json(req): Json<RestoreProbeRequest>,
) -> Result<Json<Envelope<RestoreProbeResponse>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    profile_guard::precheck(&state, &claims, ProfileOp::Probe, &request_id)?;
    let parsed = parse_id(&profile_id).map_err(|e| err_response(&request_id, e))?;
    if req.url.trim().is_empty() {
        return Err(err_response(
            &request_id,
            QuarryError::new(ErrorCode::BadRequest, "url must not be empty"),
        ));
    }
    // Parse the URL so callers can't probe with garbage shapes — same
    // SSRF posture as /v1/scrape.
    url::Url::parse(&req.url).map_err(|e| {
        err_response(
            &request_id,
            QuarryError::new(ErrorCode::BadRequest, format!("invalid url: {e}")),
        )
    })?;
    let facts = profile_guard::authorize(&state, &claims, &parsed, ProfileOp::Probe)
        .await
        .map_err(|e| err_response(&request_id, e))?;
    let snapshot = state
        .profiles
        .load(&claims.org_id, &parsed)
        .await
        .map_err(|e| err_response(&request_id, e))?
        .ok_or_else(|| {
            err_response(
                &request_id,
                QuarryError::new(
                    ErrorCode::NotFound,
                    format!("profile not found: {profile_id}"),
                ),
            )
        })?;

    let metadata = state
        .profiles
        .load_metadata(&claims.org_id, &parsed)
        .await
        .map_err(|e| err_response(&request_id, e))?
        .unwrap_or_default();

    let cookies_count = snapshot.cookies.len();
    let local_storage_count = snapshot.local_storage.len();
    let session_storage_count = snapshot.session_storage.len();
    let indexed_db_count = snapshot.indexed_db.len();
    let summary = restore_summary(&snapshot, Utc::now());

    Ok(Json(Envelope::ok(
        request_id,
        RestoreProbeResponse {
            profile_id: parsed.to_string(),
            name: metadata.name,
            scope: metadata.scope.as_str().to_owned(),
            url: req.url,
            restorable: summary.restorable,
            cookies_count,
            local_storage_count,
            session_storage_count,
            indexed_db_count,
            has_user_agent: snapshot.user_agent.is_some(),
            has_viewport: snapshot.viewport.is_some(),
            locale: snapshot.locale,
            timezone: snapshot.timezone,
            earliest_expiry: summary.earliest_expiry,
            cookies_expired: summary.cookies_expired,
            origins_count: summary.origins_count,
            owner_is_caller: facts.owned_by(claims.actor_id()),
        },
    )))
}

pub async fn list_profiles(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
) -> Result<Json<Envelope<ListProfilesResponse>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    profile_guard::precheck(&state, &claims, ProfileOp::List, &request_id)?;

    let summaries = state
        .profiles
        .list(&claims.org_id)
        .await
        .map_err(|e| err_response(&request_id, e))?;
    let actor = claims.actor_id();
    let visible = profile_guard::filter_listing(state.profile_owner_enforce, actor, summaries);

    Ok(Json(Envelope::ok(
        request_id,
        ListProfilesResponse {
            profiles: visible
                .into_iter()
                .map(|summary| ProfileSummaryResponse::for_caller(summary, actor))
                .collect(),
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use quarry_browser::session::{Cookie, IndexedDbEntry, Viewport};

    const CANARY: &str = "CANARY-d16-p0-unit";

    fn cookie(name: &str, value: &str, expires: Option<DateTime<Utc>>) -> Cookie {
        Cookie {
            name: name.to_owned(),
            value: value.to_owned(),
            domain: "example.com".to_owned(),
            path: "/".to_owned(),
            secure: true,
            http_only: true,
            expires,
            ..Default::default()
        }
    }

    #[test]
    fn redact_never_emits_a_cookie_value() {
        let snapshot = SessionSnapshot {
            cookies: vec![cookie("sid", CANARY, None)],
            local_storage: vec![("token".to_owned(), CANARY.to_owned())],
            session_storage: vec![("nav".to_owned(), CANARY.to_owned())],
            indexed_db: vec![IndexedDbEntry {
                origin: "https://app.example".to_owned(),
                database: "db".to_owned(),
                store: "records".to_owned(),
                key: "k".to_owned(),
                value: serde_json::json!({ "secret": CANARY }),
            }],
            user_agent: Some("Quarry/1.0 (test)".to_owned()),
            viewport: Some(Viewport {
                width: 800,
                height: 600,
                device_scale_factor: 1.0,
                is_mobile: false,
            }),
            locale: Some("en-US".to_owned()),
            timezone: None,
            ..Default::default()
        };
        let view = redact(&snapshot);
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains(CANARY), "a secret leaked: {json}");
        assert!(json.contains("\"value\":null"), "{json}");
        assert!(json.contains(&format!("\"value_len\":{}", CANARY.len())), "{json}");
        assert_eq!(view.local_storage_count, 1);
        assert_eq!(view.session_storage_count, 1);
        assert_eq!(view.origins.len(), 1);
        assert_eq!(view.origins[0].idb_rows, 1);
        assert_eq!(view.origins[0].idb_databases, vec!["db".to_owned()]);
        assert!(view.has_user_agent && view.has_viewport && !view.truncated);
    }

    #[test]
    fn restorable_is_false_when_all_cookies_expired() {
        let now = Utc::now();
        let expired_only = SessionSnapshot {
            cookies: vec![cookie("sid", "a", Some(now - Duration::hours(1)))],
            ..Default::default()
        };
        let summary = restore_summary(&expired_only, now);
        assert!(!summary.restorable);
        assert_eq!(summary.cookies_expired, 1);
        assert_eq!(summary.earliest_expiry, None);

        // One live cookie makes it restorable, and earliest_expiry is the nearest live one.
        let later = now + Duration::hours(2);
        let mixed = SessionSnapshot {
            cookies: vec![
                cookie("old", "a", Some(now - Duration::hours(1))),
                cookie("live", "b", Some(later)),
                cookie("session", "c", None),
            ],
            ..Default::default()
        };
        let summary = restore_summary(&mixed, now);
        assert!(summary.restorable);
        assert_eq!(summary.cookies_expired, 1);
        assert_eq!(summary.earliest_expiry, Some(later));
    }

    #[test]
    fn storage_alone_makes_a_profile_restorable() {
        let now = Utc::now();
        let snapshot = SessionSnapshot {
            local_storage: vec![("theme".to_owned(), "dark".to_owned())],
            ..Default::default()
        };
        assert!(restore_summary(&snapshot, now).restorable);
    }
}
