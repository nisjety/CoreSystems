//! The refusal a Zero Data Retention organization gets on a durable resource (WS4.H).
//!
//! Schedules, change monitors, snapshots and browser profiles are durable by
//! construction: the resource lives in Control Plane or in a Quarry store and
//! outlives the request. ADR-4 says a ZDR organization cannot create them. Until
//! the owner signs ADR-4 off and customers are told, the edge only *observes*:
//! [`refuse`] answers `Ok(())` for a ZDR caller and emits one content-free
//! `zdr.would_refuse` event naming the row, so a week of logs shows which ZDR
//! organizations would be affected. The same call enforces once
//! `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` is on (`AppState::zdr_refuse_durable_resources`):
//! it answers 403 with `details.zdr_refusal.code == "zdr_durable_mutation_forbidden"`
//! (a code of the Model Plane's closed refusal catalogue) and emits `zdr.refused`.
//!
//! What the events carry: the matrix row, the action and the organization id.
//! Never a URL, host, title, query or error text; the log-field lint reads this file
//! too. The organization id is there on purpose (the observation week exists to
//! learn who is affected), and it is not caller content.
//!
//! Wind-down stays open: the routes that only list, pause, delete or read what an
//! organization already holds never call this.

use axum::http::StatusCode;
use axum::Json;

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};

use crate::auth::Claims;
use crate::state::AppState;

/// The catalogue code (`model-plane.zdr-refusals`, v1) every durable-resource
/// refusal carries in `details.zdr_refusal.code`.
pub const REFUSAL_CODE: &str = "zdr_durable_mutation_forbidden";

/// The error of a refused request, ready to return from a handler.
pub type Refused = (StatusCode, Json<Envelope<()>>);

/// Refuses (or, in observe mode, notes) a durable-resource write for a caller
/// whose posture is `zdr_active`.
///
/// * not ZDR: `Ok(())`, nothing logged;
/// * ZDR, flag off: `zdr.would_refuse` is emitted and `Ok(())` is returned, so the
///   request runs exactly as it did before this slice;
/// * ZDR, flag on: `zdr.refused` is emitted and a 403 is returned. Call it before any
///   validation, store access or forward to Control Plane, so a refused request
///   never reaches either.
///
/// `row` is the matrix row id (`quarry_core::zdr_matrix::ROWS`) and `action` a short
/// closed label of the operation (`create`, `trigger`, `snapshot`, ...).
pub fn refuse(
    state: &AppState,
    org_id: &str,
    zdr_active: bool,
    request_id: &str,
    row: &'static str,
    action: &'static str,
) -> Result<(), Refused> {
    if !zdr_active {
        return Ok(());
    }
    if !state.zdr_refuse_durable_resources {
        tracing::info!(row, action, org_id = %org_id, "zdr.would_refuse");
        return Ok(());
    }
    tracing::info!(row, action, org_id = %org_id, "zdr.refused");
    Err(respond(request_id, refusal_error(action)))
}

/// [`refuse`] for a request that carries verified claims: the posture is the one
/// every ZDR decision takes, [`Claims::effective_zdr`] (the body's flag raised, never
/// lowered, by the token's floor), which also marks the request span for the log filter.
pub fn refuse_for_claims(
    state: &AppState,
    claims: &Claims,
    requested: Option<bool>,
    request_id: &str,
    row: &'static str,
    action: &'static str,
) -> Result<(), Refused> {
    let zdr_active = claims.effective_zdr(requested.unwrap_or(false));
    refuse(state, &claims.org_id, zdr_active, request_id, row, action)
}

/// The `error.details` object every ZDR durable-resource refusal carries:
/// `{ zdr_refusal: { code, action } }`, with no `feature` key (the catalogue has none
/// for these). Shared by the observe/enforce helper above and by the refusals that
/// were enforced before WS4.H (durable crawl and batch, a persistent profile or a
/// resume of a ZDR checkpoint at run start), so every ZDR refusal names the same code
/// and the same closed `action` labels (`docs/ERROR_REASONS.md`).
pub fn refusal_details(action: &str) -> serde_json::Value {
    serde_json::json!({
        "zdr_refusal": { "code": REFUSAL_CODE, "action": action }
    })
}

/// A 403 `FORBIDDEN` ZDR refusal with its own message and the shared details. For a
/// refusal that was enforced before WS4.H and keeps its message word for word; the
/// message names ZDR and carries no caller data.
pub fn forbidden_with_details(message: impl Into<String>, action: &str) -> QuarryError {
    QuarryError::new(ErrorCode::Forbidden, message).with_details(refusal_details(action))
}

/// The response of a refusal error: its own HTTP status and the standard envelope.
pub fn respond(request_id: &str, error: QuarryError) -> Refused {
    (
        StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::FORBIDDEN),
        Json(Envelope::<()>::err(request_id, error)),
    )
}

/// The error body of an enforced refusal. The message names ZDR (clients and the
/// existing durable-job refusal read it that way) and no caller data.
fn refusal_error(action: &'static str) -> QuarryError {
    forbidden_with_details(
        "ZDR active: durable resources are not available under zero data retention \
         (the resource would be retained)",
        action,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_refusal_is_a_403_forbidden_with_the_catalogue_code_and_no_feature_key() {
        let error = refusal_error("create");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(error.code.http_status(), 403);
        assert!(error.message.contains("ZDR"));
        let details = error.details.expect("details");
        assert_eq!(
            details["zdr_refusal"]["code"],
            "zdr_durable_mutation_forbidden"
        );
        assert_eq!(details["zdr_refusal"]["action"], "create");
        assert!(details["zdr_refusal"].get("feature").is_none());
    }

    #[test]
    fn the_shared_builders_give_the_same_details_for_any_message() {
        let error = forbidden_with_details("ZDR active: a message of its own", "crawl");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(error.message, "ZDR active: a message of its own");
        assert_eq!(error.details, Some(refusal_details("crawl")));
        assert_eq!(
            refusal_details("crawl"),
            serde_json::json!({
                "zdr_refusal": { "code": "zdr_durable_mutation_forbidden", "action": "crawl" }
            })
        );
        let (status, body) = respond("req_1", error);
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body.0.meta.request_id, "req_1");
    }

    #[test]
    fn the_message_carries_no_url_or_org() {
        let message = refusal_error("snapshot").message;
        assert!(!message.contains("http"));
        assert!(!message.contains("org"));
    }
}
