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
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use serde::Deserialize;

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::RequestKind;
use quarry_core::pagination::{ListFilter, Page};
use quarry_core::resources::{
    ArtifactSummary, BenchmarkSummary, JobResourceKind, JobSummary, RequestQueueSummary,
    ScheduleSummary, Snapshot, Source, TeamActivityEntry, TeamConcurrency, TeamCreditUsage,
    TeamQueueStatus, TeamTokenUsage,
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
    resp.json::<Page<T>>().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::Internal,
            format!("control-plane GET {path} parse: {e}"),
        )
    })
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
) -> Result<Json<Envelope<Page<ScheduleSummary>>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let filter = q.into_filter().map_err(|e| err_response(&request_id, e))?;
    let page = forward_list::<ScheduleSummary>(
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
}
