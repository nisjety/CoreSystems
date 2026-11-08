//! `GET /v1/scrape/{id}`: read back the record of an earlier scrape.
//!
//! A non-ZDR scrape stores one small artifact of kind `scrape_record` holding the
//! normalized output and the effective options (never inline text), and answers
//! with its id as `scrape_id`. This route resolves that id for the same
//! organization. It returns what the original response carried minus the inline
//! parts, which were never stored.
//!
//! A ZDR scrape stores nothing and returns no id. A reader whose organization is
//! under ZDR is refused even for a record written before the organization turned
//! ZDR on: retention that began earlier does not give a ZDR caller a read path.

use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::ArtifactKind;
use quarry_core::output::NormalizedOutput;
use quarry_core::scrape::{ActionOutcomeView, EffectiveScrapeOptions, ScrapeResponse};

fn fail(request_id: &str, error: QuarryError) -> Response {
    (
        StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(Envelope::<()>::err(request_id, error)),
    )
        .into_response()
}

fn not_found(request_id: &str) -> Response {
    fail(
        request_id,
        QuarryError::new(ErrorCode::NotFound, "scrape not found"),
    )
}

/// The stored record: `{output, effective_options}`.
#[derive(serde::Deserialize)]
pub struct ScrapeRecord {
    pub output: NormalizedOutput,
    #[serde(default)]
    pub effective_options: Option<EffectiveScrapeOptions>,
    /// WS13.5: what each scrape-time action did, with artifact references (a
    /// record is never written under ZDR, so there are no inline bytes here).
    #[serde(default)]
    pub actions: Vec<ActionOutcomeView>,
}

pub async fn get_scrape(
    State(state): State<crate::state::AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
) -> Response {
    let request_id = crate::request_id::current();
    if claims.effective_zdr_mode(None).is_active() {
        return fail(
            &request_id,
            QuarryError::new(
                ErrorCode::Forbidden,
                "scrape records are not readable under Zero Data Retention",
            ),
        );
    }
    let Ok(artifact_id) = id.parse::<ArtifactKind>() else {
        return not_found(&request_id);
    };
    // A foreign organization's id reads as not found, never as forbidden.
    let Ok((bytes, key)) = state
        .artifacts
        .get_with_key(&claims.org_id, &artifact_id)
        .await
    else {
        return not_found(&request_id);
    };
    // Any other artifact (a screenshot, a markdown file) is not a scrape.
    if !key.contains("scrape_record") {
        return not_found(&request_id);
    }
    let Ok(record) = serde_json::from_slice::<ScrapeRecord>(&bytes) else {
        return fail(
            &request_id,
            QuarryError::new(ErrorCode::Internal, "the stored scrape record is unreadable"),
        );
    };
    let mut data = ScrapeResponse::from_output(record.output);
    data.effective_options = record.effective_options;
    data.actions = record.actions;
    data.scrape_id = Some(id);
    let mut response = Json(Envelope::ok(request_id, data)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=30"),
    );
    response
}
