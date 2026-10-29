//! `/v1/<resource>` list routes.
//!
//! Cycle 22 / cluster #4 part 1.
//!
//! Six resources land in this cycle:
//!
//! - `GET /v1/artifacts` — served locally from `state.artifacts.list`.
//! - `GET /v1/sources` — forwarded to Control Plane (durable resource).
//! - `GET /v1/snapshots` — forwarded to Control Plane.
//! - `GET /v1/{crawl,search,extract,research,agent,batch}/jobs` —
//!   one shared handler that dispatches on the path segment.
//!
//! All routes require auth (the router applies `require_auth` to every
//! `/v1/*` path); the verified `org_id` from the JWT is the only org
//! filter the backend sees — query-string `org_id` is ignored.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    Extension, Json,
};
use serde::Deserialize;

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::{ArtifactKind, RequestKind};
use quarry_core::pagination::{ListFilter, Page};
use quarry_core::resources::{
    ArtifactSummary, BenchmarkSummary, JobResourceKind, JobSummary, RequestQueueSummary, Snapshot,
    Source, TeamActivityEntry, TeamConcurrency, TeamCreditUsage, TeamQueueStatus, TeamTokenUsage,
};

use crate::state::AppState;

/// Query-string shape for every list endpoint. We deserialize via
/// axum's `Query` extractor; the resulting struct is converted into a
/// canonical `ListFilter` via `From<ListQuery>`.
///
/// Why not deserialize directly into `ListFilter`? Because query
/// strings don't natively round-trip `DateTime<Utc>` from RFC3339
/// without a custom helper — easier to take strings here and parse.
#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub created_before: Option<String>,
    #[serde(default)]
    pub created_after: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
}

impl ListQuery {
    fn into_filter(self) -> Result<ListFilter, QuarryError> {
        let created_before = self
            .created_before
            .as_deref()
            .map(parse_rfc3339)
            .transpose()?;
        let created_after = self
            .created_after
            .as_deref()
            .map(parse_rfc3339)
            .transpose()?;
        let sort = match self.sort.as_deref() {
            Some("asc") => quarry_core::pagination::SortDirection::Asc,
            Some("desc") => quarry_core::pagination::SortDirection::Desc,
            Some("oldest") => quarry_core::pagination::SortDirection::Oldest,
            Some("newest") | None => quarry_core::pagination::SortDirection::Newest,
            Some(other) => {
                return Err(QuarryError::new(
                    ErrorCode::BadRequest,
                    format!("invalid sort: {other}"),
                ))
            }
        };
        Ok(ListFilter {
            status: self.status,
            created_before,
            created_after,
            limit: self.limit,
            cursor: self.cursor,
            sort,
        })
    }
}

fn parse_rfc3339(s: &str) -> Result<chrono::DateTime<chrono::Utc>, QuarryError> {
    s.parse::<chrono::DateTime<chrono::Utc>>().map_err(|e| {
        QuarryError::new(
            ErrorCode::BadRequest,
            format!("invalid RFC3339 timestamp `{s}`: {e}"),
        )
    })
}

fn err_response(request_id: &str, err: QuarryError) -> (StatusCode, Json<Envelope<()>>) {
    (
        StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(Envelope::<()>::err(request_id, err)),
    )
}

// =============================================================================
// /v1/artifacts — served locally from ArtifactStore
// =============================================================================

pub async fn list_artifacts(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Envelope<Page<ArtifactSummary>>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let filter = q.into_filter().map_err(|e| err_response(&request_id, e))?;
    let page = state
        .artifacts
        .list(&claims.org_id, &filter)
        .await
        .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, page)))
}

/// `GET /v1/artifacts/{id}` — the only way to materialize the bytes behind a
/// `FormatRef` returned by `/v1/scrape` (the scrape response carries references,
/// never inline page text).
///
/// Tenant-bound: the store matches the artifact's stored org against the
/// verified claim, so an artifact id from another tenant reads as `NotFound`
/// rather than serving its bytes.
pub async fn get_artifact(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
) -> Response {
    let request_id = RequestKind::new().to_string();
    let artifact_id = match id.parse::<ArtifactKind>() {
        Ok(value) => value,
        Err(err) => return err_response(&request_id, err).into_response(),
    };
    // The KEY, not just the bytes: it records what the producer stored, which is
    // the only authoritative source for a content type. Without it this route
    // served everything as an opaque download, so a screenshot the runtime had
    // captured could not be rendered anywhere — `screenshot_ref` reached the UI
    // with nothing able to display it.
    let (bytes, key) = match state
        .artifacts
        .get_with_key(&claims.org_id, &artifact_id)
        .await
    {
        Ok(value) => value,
        Err(err) => return err_response(&request_id, err).into_response(),
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, artifact_content_type(&key))
        .header(header::CACHE_CONTROL, "private, max-age=30")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| {
            err_response(
                &request_id,
                QuarryError::new(ErrorCode::Internal, "artifact response build failed"),
            )
            .into_response()
        })
}

/// Content type for an artifact, from the object key the producer stored it
/// under.
///
/// Kept alongside `nosniff` deliberately: the pairing is what makes an accurate
/// type safe. The kind is a server-side fact chosen at `put` time and encoded
/// into the key, so declaring it tells the browser exactly what was stored while
/// `nosniff` forbids it from guessing anything else. An empty or unrecognised key
/// falls back to the opaque default this function used to return unconditionally.
fn artifact_content_type(key: &str) -> HeaderValue {
    HeaderValue::from_static(quarry_core::artifact::content_type_for_key(key))
}

/// Phase-2 visual RAG — serve a page-image PNG from the CAS for the
/// embedding-engine consumer, which GETs the producer-emitted `image_url` with
/// NO auth. Lives on the INTERNAL (no-JWT) router; the content-hash in the path
/// is the capability on the trusted inter-plane bus. Always returns `image/png`
/// (the consumer filters on an `image/*` content-type).
pub async fn get_page_image(
    State(state): State<AppState>,
    Path((org, doc, page, hash)): Path<(String, String, i64, String)>,
) -> Response {
    let request_id = RequestKind::new().to_string();
    let Some(renderer) = state.page_renderer.as_ref() else {
        return err_response(
            &request_id,
            QuarryError::new(ErrorCode::Internal, "page-image producer not configured"),
        )
        .into_response();
    };
    let key = quarry_runtime::cas_store::CasStore::page_object_key(&org, &doc, page, &hash);
    let bytes = match renderer.cas().get_object(&key).await {
        Ok(b) => b,
        Err(err) => return err_response(&request_id, err).into_response(),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))
        .header(header::CACHE_CONTROL, "private, max-age=300")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| {
            err_response(
                &request_id,
                QuarryError::new(ErrorCode::Internal, "page-image response build failed"),
            )
            .into_response()
        })
}

// =============================================================================
// /v1/sources, /v1/snapshots, /v1/{kind}/jobs — Control-Plane forwards
// =============================================================================

/// Forward the typed `ListFilter` to a sibling control-plane GET. The
/// control plane owns the durable Postgres tables for these resources
/// (gap-quarry cluster #4 owner = "Go control"); the edge stays the
/// public-facing surface so callers don't need to hop between
/// services.
///
/// When `control_base_url` is empty (dev / test), the forward returns
/// a typed empty page so consumers see the right shape rather than a
/// 502. Production wires the real URL.
async fn forward_list<T>(
    state: &AppState,
    _request_id: &str,
    org_id: &str,
    path: &str,
    filter: &ListFilter,
) -> Result<Page<T>, QuarryError>
where
    T: serde::de::DeserializeOwned,
{
    if state.control_base_url.is_empty() {
        tracing::debug!(path, "control_base_url unset; returning empty page");
        return Ok(Page::<T> {
            items: Vec::new(),
            next_cursor: None,
            total_estimated: Some(0),
        });
    }

    let url = format!("{}{}", state.control_base_url.trim_end_matches('/'), path);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {e}")))?;
    let mut req = client.get(&url).query(&[("org_id", org_id)]);
    if let Some(s) = &filter.status {
        req = req.query(&[("status", s)]);
    }
    if let Some(t) = filter.created_before {
        req = req.query(&[("created_before", t.to_rfc3339())]);
    }
    if let Some(t) = filter.created_after {
        req = req.query(&[("created_after", t.to_rfc3339())]);
    }
    req = req.query(&[("limit", filter.effective_limit().to_string())]);
    if let Some(c) = &filter.cursor {
        req = req.query(&[("cursor", c)]);
    }
    req = req.query(&[(
        "sort",
        if filter.sort.is_descending() {
            "desc"
        } else {
            "asc"
        },
    )]);
    // D2 / cluster #14 — sign the outgoing request when an internal
    // signer is configured. GET requests have an empty body so the
    // signer hashes `b""`. We can't easily recover the final query
    // string from a `RequestBuilder` post-hoc, so we reconstruct the
    // path-with-query manually from the same inputs.
    if let Some(signer) = state.internal_signer.as_ref() {
        let mut path_q = String::from(path);
        let mut first = true;
        let mut push_q = |name: &str, value: &str| {
            path_q.push(if first { '?' } else { '&' });
            first = false;
            // URL-encode minimally — query helpers in reqwest do the
            // same. We avoid pulling in `urlencoding` for this one
            // spot and rely on values being well-formed.
            path_q.push_str(name);
            path_q.push('=');
            path_q.push_str(value);
        };
        push_q("org_id", org_id);
        if let Some(s) = &filter.status {
            push_q("status", s);
        }
        if let Some(t) = filter.created_before {
            push_q("created_before", &t.to_rfc3339());
        }
        if let Some(t) = filter.created_after {
            push_q("created_after", &t.to_rfc3339());
        }
        push_q("limit", &filter.effective_limit().to_string());
        if let Some(c) = &filter.cursor {
            push_q("cursor", c);
        }
        push_q(
            "sort",
            if filter.sort.is_descending() {
                "desc"
            } else {
                "asc"
            },
        );
        let signed = crate::internal_auth::apply_to_request(signer, "GET", &path_q, b"");
        req = req
            .header(crate::internal_auth::HEADER_SIG, signed.signature)
            .header(crate::internal_auth::HEADER_TS, signed.timestamp)
            .header(crate::internal_auth::HEADER_NONCE, signed.nonce);
    }

    let resp = req.send().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            format!("control-plane GET {path} failed: {e}"),
        )
    })?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(QuarryError::new(
            ErrorCode::DriverFailed,
            format!("control-plane GET {path} returned {status}: {body}"),
        ));
    }
    // Tolerate BOTH shapes control serves on its list endpoints: a raw `Page`
    // (`{items,next_cursor,total_estimated}`, e.g. listSourcesHandler) AND an
    // httpx.WriteJSON envelope (`{data: <Page|array>}`, e.g. listSchedulesHandler
    // which wraps a bare `[]scheduleWire`). Decoding strictly as `Page<T>` 500'd
    // on the latter; unwrap `data` when present and accept a bare array as items.
    let raw: serde_json::Value = resp.json().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::Internal,
            format!("control-plane GET {path} read: {e}"),
        )
    })?;
    let inner = raw.get("data").cloned().unwrap_or(raw);
    if inner.is_array() {
        let items: Vec<T> = serde_json::from_value(inner).map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("control-plane GET {path} parse items: {e}"),
            )
        })?;
        Ok(Page {
            items,
            next_cursor: None,
            total_estimated: None,
        })
    } else {
        serde_json::from_value::<Page<T>>(inner).map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("control-plane GET {path} parse: {e}"),
            )
        })
    }
}

pub async fn list_sources(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Envelope<Page<Source>>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let filter = q.into_filter().map_err(|e| err_response(&request_id, e))?;
    let page = forward_list::<Source>(&state, &request_id, &claims.org_id, "/v1/sources", &filter)
        .await
        .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, page)))
}

// =============================================================================
// /v1/sources — create + delete (Cycle 23 / cluster #4 — durable CRUD).
//
// The list path forwards a typed `ListFilter`; create/delete forward a JSON
// body (POST) or empty body (DELETE) to the same Control-Plane owner. org_id
// always comes from the verified JWT claim and rides as the `?org_id` query
// param — NEVER from the client body — so a caller can't register or delete a
// source under another tenant (control's handler reads the query param only).
// =============================================================================

/// `POST /v1/sources` request body. Mirrors quarry-control's `sourceCreateBody`
/// (`services/quarry-control/internal/resources/cycle23.go`). `org_id` is
/// intentionally ABSENT — control derives org from the `?org_id` query param
/// the edge stamps from the verified JWT, so the body can never smuggle a
/// foreign tenant.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct CreateSourceRequest {
    pub name: String,
    pub url: String,
    /// `"crawl" | "scrape" | "search"`.
    pub kind: String,
    /// When true, control also registers a recurring `change_monitor`
    /// schedule for this source so the orchestrator reconcile materializes a
    /// Temporal schedule.
    #[serde(default)]
    pub monitor: bool,
    /// `"hourly" | "daily" | "weekly"` — honoured by control only when
    /// `monitor` is set (defaults to `daily` there).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// Free-form per-source config (max_pages, include_patterns, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

const VALID_SOURCE_KINDS: [&str; 3] = ["crawl", "scrape", "search"];

/// Forward a mutating JSON request (POST body / DELETE empty) to the
/// Control-Plane owner, HMAC-signing the canonical `(method, path?org_id,
/// body_hash, ts, nonce)`. Returns the parsed JSON response (control's create
/// returns the created `store.Source`; DELETE returns 204 / no body).
///
/// We forward the create response as an untyped `serde_json::Value` rather
/// than the strongly-typed [`Source`]: control serializes `created_at` /
/// `updated_at` as Unix-millis integers (`store.Source` json tags), which do
/// not round-trip into [`Source`]'s `DateTime<Utc>` fields. The surface
/// callers (gateway → SPA) read string fields only, so an honest passthrough
/// is both correct and avoids a lossy re-encode.
async fn forward_mutation(
    state: &AppState,
    method: reqwest::Method,
    path: &str,
    org_id: &str,
    body_bytes: &[u8],
) -> Result<serde_json::Value, QuarryError> {
    if state.control_base_url.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "control plane URL not configured; source CRUD routes are inert",
        ));
    }
    let url = format!("{}{}", state.control_base_url.trim_end_matches('/'), path);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {e}")))?;

    let mut req = client
        .request(method.clone(), &url)
        .query(&[("org_id", org_id)]);
    if !body_bytes.is_empty() {
        req = req
            .header("content-type", "application/json")
            .body(body_bytes.to_vec());
    }

    // D3 / cluster #14 — Idempotency-Key on every mutating call so a network
    // retry doesn't double-execute (control dedupes (org_id, key, route)).
    let idem_key: RequestKind = quarry_core::ids::Id::new();
    req = req.header("Idempotency-Key", idem_key.to_string());

    // D2 / cluster #14 — sign the canonical string. POST signs the JSON body
    // bytes; DELETE has an empty body (`b""`). The path-with-query MUST match
    // exactly what control verifies — `?org_id=<org>` only (the
    // Idempotency-Key header is not part of the canonical string).
    if let Some(signer) = state.internal_signer.as_ref() {
        let path_q = format!("{path}?org_id={org_id}");
        let signed =
            crate::internal_auth::apply_to_request(signer, method.as_str(), &path_q, body_bytes);
        req = req
            .header(crate::internal_auth::HEADER_SIG, signed.signature)
            .header(crate::internal_auth::HEADER_TS, signed.timestamp)
            .header(crate::internal_auth::HEADER_NONCE, signed.nonce);
    }

    let resp = req.send().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            format!("control-plane {method} {path} failed: {e}"),
        )
    })?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let code = match status.as_u16() {
            404 => ErrorCode::NotFound,
            400 => ErrorCode::BadRequest,
            401 | 403 => ErrorCode::Forbidden,
            409 => ErrorCode::Conflict,
            _ => ErrorCode::DriverFailed,
        };
        return Err(QuarryError::new(
            code,
            format!("control-plane {method} {path} returned {status}: {body}"),
        ));
    }
    // 204 No Content (DELETE) carries an empty body — surface JSON null.
    if status == StatusCode::NO_CONTENT {
        return Ok(serde_json::Value::Null);
    }
    let text = resp.text().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::Internal,
            format!("control-plane {method} {path} read: {e}"),
        )
    })?;
    if text.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| {
        QuarryError::new(
            ErrorCode::Internal,
            format!("control-plane {method} {path} parse: {e}"),
        )
    })
}

pub async fn create_source(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<CreateSourceRequest>,
) -> Result<Json<Envelope<serde_json::Value>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();

    let name = req.name.trim();
    let url = req.url.trim();
    if name.is_empty() || url.is_empty() {
        return Err(err_response(
            &request_id,
            QuarryError::new(ErrorCode::BadRequest, "name and url are required"),
        ));
    }
    let kind = req.kind.trim();
    if !VALID_SOURCE_KINDS.contains(&kind) {
        return Err(err_response(
            &request_id,
            QuarryError::new(
                ErrorCode::BadRequest,
                "kind must be one of crawl|scrape|search",
            ),
        ));
    }
    // Re-build the control-facing body from validated, trimmed fields. We
    // never forward a client-supplied org_id — it isn't part of the request
    // shape and control reads org from the `?org_id` query param exclusively.
    let body = CreateSourceRequest {
        name: name.to_string(),
        url: url.to_string(),
        kind: kind.to_string(),
        monitor: req.monitor,
        preset: req
            .preset
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        config: req.config.clone(),
    };
    let body_bytes = serde_json::to_vec(&body).map_err(|e| {
        err_response(
            &request_id,
            QuarryError::new(ErrorCode::Internal, format!("encode body: {e}")),
        )
    })?;
    let created = forward_mutation(
        &state,
        reqwest::Method::POST,
        "/v1/sources",
        &claims.org_id,
        &body_bytes,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, created)))
}

/// Internal, non-HTTP-handler counterpart to [`create_source`] used by
/// `PageRunner`'s [`quarry_runtime::source_registrar::SourceRegistrar`] impl
/// (`crate::source_registrar::EdgeSourceRegistrar`) to materialize a durable
/// "tracked website" row on crawl-completion. Reuses the exact same
/// HMAC-signed [`forward_mutation`] path the public `POST /v1/sources`
/// handler uses — the difference is this is invoked in-process from the
/// crawl pipeline (no inbound request / JWT claims to extract `org_id`
/// from), so `org_id` is passed directly instead of read from `claims`.
///
/// `monitor` is always `false` here: crawl-completion registration should
/// not silently spin up a recurring change-monitor schedule the user never
/// asked for. Best-effort by contract — callers (the registrar impl) log
/// and swallow errors so a materialization hiccup never fails the ingest
/// that already succeeded.
pub(crate) async fn upsert_source_internal(
    state: &AppState,
    org_id: &str,
    name: &str,
    url: &str,
    kind: &str,
) -> Result<(), QuarryError> {
    let body = CreateSourceRequest {
        name: name.to_string(),
        url: url.to_string(),
        kind: kind.to_string(),
        monitor: false,
        preset: None,
        config: None,
    };
    let body_bytes = serde_json::to_vec(&body)
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("encode body: {e}")))?;
    forward_mutation(
        state,
        reqwest::Method::POST,
        "/v1/sources",
        org_id,
        &body_bytes,
    )
    .await?;
    Ok(())
}

pub async fn delete_source(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let path = format!("/v1/sources/{id}");
    forward_mutation(&state, reqwest::Method::DELETE, &path, &claims.org_id, b"")
        .await
        .map_err(|e| err_response(&request_id, e))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_snapshots(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Envelope<Page<Snapshot>>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let filter = q.into_filter().map_err(|e| err_response(&request_id, e))?;
    let page = forward_list::<Snapshot>(
        &state,
        &request_id,
        &claims.org_id,
        "/v1/snapshots",
        &filter,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, page)))
}

/// Forward a typed GET that returns a single object (not a `Page<T>`).
/// Used for `/v1/team/*` endpoints where each org has exactly one
/// snapshot, not a paginated list. Returns `None` when
/// `control_base_url` is empty (dev / test) so handlers can synth a
/// default-zero response instead of bubbling a 502.
async fn forward_one<T>(
    state: &AppState,
    _request_id: &str,
    org_id: &str,
    path: &str,
    query: &[(&str, String)],
) -> Result<Option<T>, QuarryError>
where
    T: serde::de::DeserializeOwned,
{
    if state.control_base_url.is_empty() {
        return Ok(None);
    }
    let url = format!("{}{}", state.control_base_url.trim_end_matches('/'), path);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {e}")))?;
    let mut req = client.get(&url).query(&[("org_id", org_id)]);
    for (k, v) in query {
        req = req.query(&[(*k, v)]);
    }
    if let Some(signer) = state.internal_signer.as_ref() {
        let mut path_q = format!("{path}?org_id={org_id}");
        for (k, v) in query {
            path_q.push('&');
            path_q.push_str(k);
            path_q.push('=');
            path_q.push_str(v);
        }
        let signed = crate::internal_auth::apply_to_request(signer, "GET", &path_q, b"");
        req = req
            .header(crate::internal_auth::HEADER_SIG, signed.signature)
            .header(crate::internal_auth::HEADER_TS, signed.timestamp)
            .header(crate::internal_auth::HEADER_NONCE, signed.nonce);
    }
    let resp = req.send().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            format!("control-plane GET {path} failed: {e}"),
        )
    })?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(QuarryError::new(
            ErrorCode::DriverFailed,
            format!("control-plane GET {path} returned {status}: {body}"),
        ));
    }
    let parsed: T = resp.json().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::Internal,
            format!("control-plane GET {path} parse: {e}"),
        )
    })?;
    Ok(Some(parsed))
}

/// Single handler for every `/v1/{kind}/jobs` route. Dispatch is
/// path-segment based — `JobResourceKind::from_path_segment` returns
/// `None` for unknown kinds → 404.
pub async fn list_jobs(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(kind): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Envelope<Page<JobSummary>>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let resource_kind = JobResourceKind::from_path_segment(&kind).ok_or_else(|| {
        err_response(
            &request_id,
            QuarryError::new(ErrorCode::NotFound, format!("unknown job kind: {kind}")),
        )
    })?;
    let filter = q.into_filter().map_err(|e| err_response(&request_id, e))?;
    let path = format!("/v1/{}/jobs", resource_kind.as_str());
    let page = forward_list::<JobSummary>(&state, &request_id, &claims.org_id, &path, &filter)
        .await
        .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, page)))
}

// =============================================================================
// Cycle 23 / cluster #4 part 2 — request-queues, benchmarks, team/*, schedules
// =============================================================================

pub async fn list_request_queues(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Envelope<Page<RequestQueueSummary>>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let filter = q.into_filter().map_err(|e| err_response(&request_id, e))?;
    let page = forward_list::<RequestQueueSummary>(
        &state,
        &request_id,
        &claims.org_id,
        "/v1/request-queues",
        &filter,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, page)))
}

pub async fn list_benchmarks(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Envelope<Page<BenchmarkSummary>>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let filter = q.into_filter().map_err(|e| err_response(&request_id, e))?;
    let page = forward_list::<BenchmarkSummary>(
        &state,
        &request_id,
        &claims.org_id,
        "/v1/benchmarks",
        &filter,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, page)))
}

pub async fn list_schedules(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Envelope<Page<serde_json::Value>>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let filter = q.into_filter().map_err(|e| err_response(&request_id, e))?;
    // control's /v1/schedules serves the orchestrator-facing `scheduleWire`
    // projection (id / target_kind / target_ref / enabled / created_at-as-unix),
    // NOT the generic `ScheduleSummary` resource shape. Pass items through as raw
    // JSON so the gateway's change-monitor filter (which keys on `target_kind`)
    // sees the real fields instead of failing a strict decode (was a 500).
    let page = forward_list::<serde_json::Value>(
        &state,
        &request_id,
        &claims.org_id,
        "/v1/schedules",
        &filter,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, page)))
}

/// `/v1/team/*` query string. Today only `period` is honoured — the
/// rest of the cluster #4 column will grow more knobs over time.
#[derive(Debug, Default, Deserialize)]
pub struct TeamQuery {
    /// Window: `"today" | "7d" | "30d"` or a specific `YYYY-MM-DD`.
    /// Default `"7d"`. Backends interpret unknown values as `7d`.
    #[serde(default)]
    pub period: Option<String>,
}

fn team_period(q: &TeamQuery) -> Vec<(&'static str, String)> {
    vec![(
        "period",
        q.period.clone().unwrap_or_else(|| "7d".to_string()),
    )]
}

pub async fn team_credit_usage(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<TeamQuery>,
) -> Result<Json<Envelope<TeamCreditUsage>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let period = team_period(&q);
    let body = forward_one::<TeamCreditUsage>(
        &state,
        &request_id,
        &claims.org_id,
        "/v1/team/credit-usage",
        &period,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?
    .unwrap_or_else(|| TeamCreditUsage {
        org_id: claims.org_id.clone(),
        period: period[0].1.clone(),
        credits_used: 0.0,
        credits_limit: None,
        utilization_percent: 0.0,
    });
    Ok(Json(Envelope::ok(request_id, body)))
}

pub async fn team_token_usage(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<TeamQuery>,
) -> Result<Json<Envelope<TeamTokenUsage>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let period = team_period(&q);
    let body = forward_one::<TeamTokenUsage>(
        &state,
        &request_id,
        &claims.org_id,
        "/v1/team/token-usage",
        &period,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?
    .unwrap_or_else(|| TeamTokenUsage {
        org_id: claims.org_id.clone(),
        period: period[0].1.clone(),
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cost_micro_usd: None,
    });
    Ok(Json(Envelope::ok(request_id, body)))
}

pub async fn team_concurrency(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
) -> Result<Json<Envelope<TeamConcurrency>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let body = forward_one::<TeamConcurrency>(
        &state,
        &request_id,
        &claims.org_id,
        "/v1/team/concurrency",
        &[],
    )
    .await
    .map_err(|e| err_response(&request_id, e))?
    .unwrap_or_else(|| TeamConcurrency {
        org_id: claims.org_id.clone(),
        current: 0,
        ceiling: 0,
        by_host: vec![],
    });
    Ok(Json(Envelope::ok(request_id, body)))
}

pub async fn team_queue_status(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
) -> Result<Json<Envelope<TeamQueueStatus>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let body = forward_one::<TeamQueueStatus>(
        &state,
        &request_id,
        &claims.org_id,
        "/v1/team/queue-status",
        &[],
    )
    .await
    .map_err(|e| err_response(&request_id, e))?
    .unwrap_or_else(|| TeamQueueStatus {
        org_id: claims.org_id.clone(),
        queued_total: 0,
        in_flight_total: 0,
        by_queue: vec![],
    });
    Ok(Json(Envelope::ok(request_id, body)))
}

pub async fn team_activity(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Envelope<Page<TeamActivityEntry>>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let filter = q.into_filter().map_err(|e| err_response(&request_id, e))?;
    let page = forward_list::<TeamActivityEntry>(
        &state,
        &request_id,
        &claims.org_id,
        "/v1/team/activity",
        &filter,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, page)))
}

// =============================================================================
// C30.1 / cluster #7 — /v1/runs/:id/events durable job-history read.
//
// When the edge has a PostgresEventHistory wired into AppState (gated
// behind --features postgres-queue + DATABASE_URL configured), this
// route serves the canonical JobHistoryEvent list for the run.
// Otherwise we forward to control plane via the existing forward
// helper so the wire shape is uniform regardless of backend.
// =============================================================================

#[derive(Debug, Default, serde::Deserialize)]
pub struct RunEventsQuery {
    /// Max events per page. Clamped server-side to [1, 1000].
    #[serde(default)]
    pub limit: Option<u32>,
    /// Only return events with `seq` strictly greater than this watermark
    /// (control filters server-side). Lets incremental pollers such as the
    /// onboarding crawl preview avoid re-downloading the whole event log.
    #[serde(default)]
    pub after_seq: Option<u64>,
}

pub async fn list_run_events(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Query(q): Query<RunEventsQuery>,
) -> Result<
    Json<Envelope<Page<quarry_core::job_history::JobHistoryEvent>>>,
    (StatusCode, Json<Envelope<()>>),
> {
    let request_id = RequestKind::new().to_string();
    let parsed_run_id: quarry_core::ids::kinds::RunKind =
        run_id
            .parse()
            .map_err(|e: quarry_core::error::QuarryError| {
                err_response(
                    &request_id,
                    QuarryError::new(ErrorCode::BadRequest, format!("invalid run_id: {e}")),
                )
            })?;
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);

    // Prefer the local Postgres store when wired — saves a round-trip
    // and skips the forward-HMAC overhead.
    #[cfg(feature = "postgres-queue")]
    if let Some(store) = state.event_history.as_ref() {
        let events = store
            .list_events(&claims.org_id, &parsed_run_id, limit)
            .await
            .map_err(|e| err_response(&request_id, e))?;
        return Ok(Json(Envelope::ok(
            request_id,
            Page {
                items: events,
                next_cursor: None,
                total_estimated: None,
            },
        )));
    }

    // Fallback: forward to control plane. Cycle 31 wires the matching
    // Go handler; for now this returns an empty page when
    // control_base_url is unset.
    let _ = parsed_run_id;
    let _ = limit;
    let filter = ListFilter {
        limit: q.limit,
        ..Default::default()
    };
    let page = forward_list::<quarry_core::job_history::JobHistoryEvent>(
        &state,
        &request_id,
        &claims.org_id,
        &format!("/v1/runs/{run_id}/events"),
        &filter,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, page)))
}

// =============================================================================
// Crawl 0-pages fix — GET /v1/jobs/:id/events
//
// The durable crawl handoff (`/v1/crawl`, `/v1/batch`) returns a control
// `job_id`, not a `run_id`: the orchestrator only stamps the Temporal
// `run_id` once the job flips accepted → running, so it is not available
// synchronously at handoff time. `list_run_events` above parses its path
// segment as a `RunKind`, so a `job_id` would 400 there.
//
// This route resolves the job → its events by forwarding to control's
// `GET /v1/jobs/{id}/events` (control indexes events by BOTH job_id and
// run_id — see EmitEvent), which works the instant the first event lands,
// regardless of run_id assignment.
//
// It emits a real `text/event-stream`: one SSE frame per control `Event`
// (`event: <type>` / `data: <json>`), then a terminal `done`. The events
// are finite for a completed/queued crawl, so we drain control once and
// close — this matches the SPA's `readSseStream` consumer and lets the
// gateway forward the bytes verbatim via `proxy_sse_stream`, unlike the
// JSON-returning `/v1/runs/:id/events` which the SSE reader can't parse.
// =============================================================================
pub async fn list_job_events(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(job_id): Path<String>,
    Query(q): Query<RunEventsQuery>,
) -> Response {
    let request_id = RequestKind::new().to_string();
    // Validate the id is a job id (job_*) so we never proxy garbage / a
    // run id (which belongs on /v1/runs/:id/events) to control.
    if let Err(e) = job_id.parse::<quarry_core::ids::kinds::JobKind>() {
        return err_response(
            &request_id,
            QuarryError::new(ErrorCode::BadRequest, format!("invalid job_id: {e}")),
        )
        .into_response();
    }
    let limit = q.limit.unwrap_or(500).clamp(1, 1000).to_string();
    let after_seq = q.after_seq.unwrap_or(0).to_string();

    let events: Vec<serde_json::Value> = if state.control_base_url.is_empty() {
        tracing::debug!(%job_id, "control_base_url unset; emitting empty job-events stream");
        Vec::new()
    } else {
        let path = format!("/v1/jobs/{job_id}/events");
        match forward_one::<serde_json::Value>(
            &state,
            &request_id,
            &claims.org_id,
            &path,
            &[("limit", limit), ("after_seq", after_seq)],
        )
        .await
        {
            // Control serves these events through `httpx.WriteJSON`, which
            // wraps the `[]Event` slice in an envelope: `{"data": [ ... ]}`
            // (and `{"data": null}` when there are none). It is NOT a bare
            // top-level array. Unwrap `data` first — same tolerance as
            // `forward_list` — then accept the inner array; otherwise we'd
            // fall through to `Vec::new()` on every real response and emit
            // only the terminal `done`, which is the 0-pages bug.
            Ok(Some(raw)) => {
                let inner = raw.get("data").cloned().unwrap_or(raw);
                match inner {
                    serde_json::Value::Array(items) => items,
                    _ => Vec::new(),
                }
            }
            Ok(None) => Vec::new(),
            Err(e) => return err_response(&request_id, e).into_response(),
        }
    };

    let stream = async_stream::stream! {
        for event in events {
            // `type` is the SSE event name; the whole event object is the
            // data payload (so the SPA can read payload.pages_visited etc.).
            let name = event
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("event")
                .to_owned();
            match Event::default().event(name).json_data(&event) {
                Ok(ev) => yield Ok::<Event, std::convert::Infallible>(ev),
                Err(err) => {
                    tracing::warn!(error = %err, "job-event serialize failed; skipping frame");
                }
            }
        }
        // Terminal marker so the SPA's onDone fires even when the crawl
        // emitted no terminal `run_completed`/`run_failed` yet (queued job).
        yield Ok(Event::default().data("done"));
    };

    Sse::new(stream).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_query_into_filter_parses_rfc3339_timestamps() {
        let q = ListQuery {
            created_before: Some("2026-05-19T12:00:00Z".into()),
            created_after: Some("2026-05-18T00:00:00Z".into()),
            limit: Some(50),
            sort: Some("oldest".into()),
            status: Some("running".into()),
            cursor: Some("opaque".into()),
        };
        let f = q.into_filter().unwrap();
        assert_eq!(f.status.as_deref(), Some("running"));
        assert_eq!(f.limit, Some(50));
        assert!(matches!(
            f.sort,
            quarry_core::pagination::SortDirection::Oldest
        ));
        assert!(f.created_before.is_some());
    }

    #[test]
    fn list_query_rejects_bad_timestamp() {
        let q = ListQuery {
            created_before: Some("not-a-timestamp".into()),
            ..Default::default()
        };
        let err = q.into_filter().unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[test]
    fn list_query_rejects_unknown_sort() {
        let q = ListQuery {
            sort: Some("alphabetical".into()),
            ..Default::default()
        };
        let err = q.into_filter().unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[test]
    fn create_source_request_decodes_minimal_shape() {
        // The minimal create body is name + url + kind; monitor/preset/config
        // are optional and absent here.
        let req: CreateSourceRequest = serde_json::from_value(serde_json::json!({
            "name": "Acme pricing",
            "url": "https://acme.example/pricing",
            "kind": "scrape"
        }))
        .unwrap();
        assert_eq!(req.name, "Acme pricing");
        assert_eq!(req.kind, "scrape");
        assert!(!req.monitor);
        assert!(req.preset.is_none());
        assert!(req.config.is_none());
    }

    #[test]
    fn create_source_request_never_serializes_org_id() {
        // IDOR invariant at the edge: the control-facing create body carries
        // NO org_id — org is stamped as the `?org_id` query param from the
        // verified JWT claim, so a client body can never smuggle a foreign
        // tenant. Even if a client POSTs an `org_id` field, it deserializes
        // into nothing (the struct has no such field) and re-serializes away.
        let req: CreateSourceRequest = serde_json::from_value(serde_json::json!({
            "name": "x",
            "url": "https://x.example/",
            "kind": "crawl",
            "org_id": "org_attacker",
            "monitor": true,
            "preset": "daily"
        }))
        .unwrap();
        let wire = serde_json::to_string(&req).unwrap();
        assert!(
            !wire.contains("org_id"),
            "create body MUST NOT carry org_id; got {wire}"
        );
        assert!(!wire.contains("org_attacker"));
        assert!(wire.contains("\"monitor\":true"));
        assert!(wire.contains("\"preset\":\"daily\""));
    }

    #[test]
    fn valid_source_kinds_match_control_contract() {
        // Pin the kind allow-list against quarry-control's validSourceKinds.
        assert!(VALID_SOURCE_KINDS.contains(&"crawl"));
        assert!(VALID_SOURCE_KINDS.contains(&"scrape"));
        assert!(VALID_SOURCE_KINDS.contains(&"search"));
        assert!(!VALID_SOURCE_KINDS.contains(&"agent"));
    }
}

#[cfg(test)]
mod artifact_content_type_tests {
    use super::artifact_content_type;
    use quarry_core::artifact::{object_key, ArtifactKind};

    /// The regression this fixes: a screenshot served as an opaque download.
    #[test]
    fn a_stored_screenshot_is_served_as_an_image() {
        let key = object_key("acme", "run_1", "page_1", ArtifactKind::Screenshot);
        assert_eq!(artifact_content_type(&key), "image/png");
    }

    /// A backend that cannot report its key must degrade to the old behaviour,
    /// never to a guessed type.
    #[test]
    fn an_unknown_key_stays_opaque() {
        assert_eq!(artifact_content_type(""), "application/octet-stream");
        assert_eq!(
            artifact_content_type("org=a/run=r/page=p/raw.bin"),
            "application/octet-stream"
        );
    }
}
