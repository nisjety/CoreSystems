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

use quarry_browser::session::{ProfileMetadata, ProfileScope, ProfileSummary, SessionSnapshot};
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::{ProfileKind, RequestKind};

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
}

impl From<ProfileSummary> for ProfileSummaryResponse {
    fn from(summary: ProfileSummary) -> Self {
        Self {
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
    pub snapshot: SessionSnapshot,
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
    let request_id = RequestKind::new().to_string();

    let profile_id = match req.profile_id.as_deref() {
        Some(s) => parse_id(s).map_err(|e| err_response(&request_id, e))?,
        None => quarry_core::ids::Id::new(),
    };

    state
        .profiles
        .save(&claims.org_id, &profile_id, &req.snapshot)
        .await
        .map_err(|e| err_response(&request_id, e))?;

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
    let request_id = RequestKind::new().to_string();

    if req.scope == ProfileScope::Ephemeral {
        return Err(err_response(
            &request_id,
            QuarryError::new(
                ErrorCode::BadRequest,
                "a created profile cannot use the ephemeral scope; omit the profile entirely for ephemeral browsing",
            ),
        ));
    }

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
        },
    )))
}

pub async fn update_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(profile_id): Path<String>,
    Json(req): Json<UpdateProfileRequest>,
) -> Result<Json<Envelope<ProfileSummaryResponse>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
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

    // A profile that predates this metadata (or was never explicitly
    // named) still exists via its snapshot — `load` confirms it's real
    // before we allow a rename, rather than silently creating a
    // metadata-only row for an id nobody asked for.
    let exists = state
        .profiles
        .load(&claims.org_id, &parsed)
        .await
        .map_err(|e| err_response(&request_id, e))?
        .is_some();
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
        },
    )))
}

pub async fn load_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(profile_id): Path<String>,
) -> Result<Json<Envelope<LoadProfileResponse>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();

    let parsed = parse_id(&profile_id).map_err(|e| err_response(&request_id, e))?;

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

    Ok(Json(Envelope::ok(
        request_id,
        LoadProfileResponse {
            profile_id: parsed.to_string(),
            snapshot,
        },
    )))
}

pub async fn delete_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(profile_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();

    let parsed = parse_id(&profile_id).map_err(|e| err_response(&request_id, e))?;

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
}

pub async fn restore_probe(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(profile_id): Path<String>,
    Json(req): Json<RestoreProbeRequest>,
) -> Result<Json<Envelope<RestoreProbeResponse>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
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
    let restorable =
        cookies_count + local_storage_count + session_storage_count + indexed_db_count > 0;

    Ok(Json(Envelope::ok(
        request_id,
        RestoreProbeResponse {
            profile_id: parsed.to_string(),
            name: metadata.name,
            scope: metadata.scope.as_str().to_owned(),
            url: req.url,
            restorable,
            cookies_count,
            local_storage_count,
            session_storage_count,
            indexed_db_count,
            has_user_agent: snapshot.user_agent.is_some(),
            has_viewport: snapshot.viewport.is_some(),
            locale: snapshot.locale,
            timezone: snapshot.timezone,
        },
    )))
}

pub async fn list_profiles(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
) -> Result<Json<Envelope<ListProfilesResponse>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();

    let summaries = state
        .profiles
        .list(&claims.org_id)
        .await
        .map_err(|e| err_response(&request_id, e))?;

    Ok(Json(Envelope::ok(
        request_id,
        ListProfilesResponse {
            profiles: summaries
                .into_iter()
                .map(ProfileSummaryResponse::from)
                .collect(),
        },
    )))
}
