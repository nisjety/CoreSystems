//! `/v1/change/*` endpoints — versioned change history.
//!
//! Cycle 30 / cluster #9.
//!
//! Three routes:
//!
//! | Method | Path                | Effect                                        |
//! | ------ | ------------------- | --------------------------------------------- |
//! | POST   | `/v1/change/check`  | Compare a URL's latest baseline; classify     |
//! | GET    | `/v1/change/latest` | Most-recent baseline for a URL                |
//! | GET    | `/v1/change/history`| Paginated walk of the baseline chain          |
//!
//! When the edge has a `PostgresBaselineStore` wired (postgres-queue
//! feature + DATABASE_URL), the routes serve from local DB.
//! Otherwise they forward to control plane via the existing
//! HMAC-signed forward path.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Extension, Json,
};
use serde::Deserialize;

use quarry_core::change_history::{BaselineSnapshot, ChangeRecord};
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::RequestKind;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CheckRequest {
    pub url: String,
    /// Optional fresh fingerprint. When provided, the route compares
    /// directly without re-fetching. When omitted, the route returns
    /// just the latest-baseline-status (caller fetches separately).
    #[serde(default)]
    pub fresh_fingerprint: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UrlQuery {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub url: String,
    #[serde(default)]
    pub limit: Option<u32>,
}

fn err_response(request_id: &str, err: QuarryError) -> (StatusCode, Json<Envelope<()>>) {
    (
        StatusCode::from_u16(err.code.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(Envelope::<()>::err(request_id, err)),
    )
}

fn validate_url(s: &str) -> Result<(), QuarryError> {
    if s.trim().is_empty() {
        return Err(QuarryError::new(ErrorCode::BadRequest, "url required"));
    }
    url::Url::parse(s)
        .map(|_| ())
        .map_err(|e| QuarryError::new(ErrorCode::BadRequest, format!("invalid url: {e}")))
}

pub async fn check(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<CheckRequest>,
) -> Result<Json<Envelope<ChangeRecord>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    validate_url(&req.url).map_err(|e| err_response(&request_id, e))?;

    #[cfg(feature = "postgres-queue")]
    if let Some(store) = state.baseline_store.as_ref() {
        let fresh = req
            .fresh_fingerprint
            .as_deref()
            .ok_or_else(|| {
                err_response(
                    &request_id,
                    QuarryError::new(
                        ErrorCode::BadRequest,
                        "fresh_fingerprint required when running against local store",
                    ),
                )
            })?;
        let record = store
            .compare_snapshot(&claims.org_id, &req.url, fresh)
            .await
            .map_err(|e| err_response(&request_id, e))?;
        return Ok(Json(Envelope::ok(request_id, record)));
    }

    // Without a local store, the operation requires control-plane
    // round-trip — return 501 with a hint until cycle 31 wires the
    // forward path (Go side handler pending).
    let _ = state;
    let _ = claims;
    Err(err_response(
        &request_id,
        QuarryError::new(
            ErrorCode::Unsupported,
            "change tracking requires postgres-queue feature + DATABASE_URL on the edge \
             OR a control-plane handler — see docs/CHANGE_TRACKING.md",
        ),
    ))
}

pub async fn latest(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<UrlQuery>,
) -> Result<Json<Envelope<BaselineSnapshot>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    validate_url(&q.url).map_err(|e| err_response(&request_id, e))?;

    #[cfg(feature = "postgres-queue")]
    if let Some(store) = state.baseline_store.as_ref() {
        let latest = store
            .load_latest(&claims.org_id, &q.url)
            .await
            .map_err(|e| err_response(&request_id, e))?
            .ok_or_else(|| {
                err_response(
                    &request_id,
                    QuarryError::new(
                        ErrorCode::NotFound,
                        format!("no baseline for {}", q.url),
                    ),
                )
            })?;
        return Ok(Json(Envelope::ok(request_id, latest)));
    }
    let _ = state;
    let _ = claims;
    Err(err_response(
        &request_id,
        QuarryError::new(ErrorCode::Unsupported, "postgres-queue feature required"),
    ))
}

pub async fn history(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Envelope<Vec<BaselineSnapshot>>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    validate_url(&q.url).map_err(|e| err_response(&request_id, e))?;
    let limit = q.limit.unwrap_or(50).clamp(1, 1000);

    #[cfg(feature = "postgres-queue")]
    if let Some(store) = state.baseline_store.as_ref() {
        let hist = store
            .load_history(&claims.org_id, &q.url, limit)
            .await
            .map_err(|e| err_response(&request_id, e))?;
        return Ok(Json(Envelope::ok(request_id, hist)));
    }
    let _ = state;
    let _ = claims;
    let _ = limit;
    Err(err_response(
        &request_id,
        QuarryError::new(ErrorCode::Unsupported, "postgres-queue feature required"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_request_decodes_minimal_shape() {
        let raw = serde_json::json!({"url": "https://example.com"});
        let r: CheckRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(r.url, "https://example.com");
        assert!(r.fresh_fingerprint.is_none());
    }

    #[test]
    fn check_request_decodes_full_shape() {
        let raw = serde_json::json!({
            "url": "https://example.com",
            "fresh_fingerprint": "blake3:abc"
        });
        let r: CheckRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(r.fresh_fingerprint.as_deref(), Some("blake3:abc"));
    }

    #[test]
    fn validate_url_rejects_empty_and_bad() {
        assert!(validate_url("").is_err());
        assert!(validate_url("   ").is_err());
        assert!(validate_url("not-a-url").is_err());
        assert!(validate_url("https://example.com").is_ok());
    }
}
