//! `POST /v1/internal/schedules/preflight`: the fire-time check of an executable
//! scheduled job (WS15.S2).
//!
//! The orchestrator's `ScheduledJobWF` asks this route before every fire. In S2
//! the answer is always allow, with the posture `unchecked` and the entitlement
//! `floor`, so the wrapper's shape is final and S4 changes only the answer: ZDR
//! posture and plan entitlement are checked here at fire time.
//!
//! Authentication follows `run_page`: the shared runtime bearer token names the
//! caller, and the `X-Quarry-Run-Sig` run binding ties the tenant to the request.
//! The binding is signed over `org_id + "\n" + "schedule:" + schedule_id`, so a
//! leaked token cannot ask about another tenant's schedule. When the edge has no
//! shared secret the binding is not enforced, as for `run_page`.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};

use crate::state::AppState;

/// The fire-time question: which schedule, and which tenant it belongs to.
#[derive(Debug, Deserialize)]
pub struct PreflightRequest {
    pub schedule_id: String,
    pub org_id: String,
}

/// The answer. `posture` and `entitlement` are closed vocabularies: S2 writes
/// `unchecked` and `floor`; S4 writes the checked values.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PreflightResponse {
    pub allow: bool,
    pub posture: &'static str,
    pub entitlement: &'static str,
}

/// The S2 answer. Kept as a function so the route and the tests share it.
pub fn s2_answer() -> PreflightResponse {
    PreflightResponse {
        allow: true,
        posture: "unchecked",
        entitlement: "floor",
    }
}

/// The canonical run-binding target for a schedule: `schedule:<id>`. The
/// orchestrator signs exactly this string (`runBindingSig(secret, org, "schedule:"+id)`).
pub fn binding_target(schedule_id: &str) -> String {
    format!("schedule:{schedule_id}")
}

/// Refuses a request whose identifiers cannot be signed unambiguously.
fn validate(req: &PreflightRequest) -> Result<(), QuarryError> {
    if req.org_id.trim().is_empty() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "preflight requires org_id in the body",
        ));
    }
    if req.schedule_id.trim().is_empty() || req.schedule_id.len() > 128 {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "preflight requires a schedule_id of at most 128 characters",
        ));
    }
    if req.org_id.contains('\n') || req.schedule_id.contains('\n') {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "identifiers may not contain a newline",
        ));
    }
    Ok(())
}

fn refuse(
    request_id: &str,
    err: QuarryError,
) -> (StatusCode, Json<Envelope<()>>) {
    (
        StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::BAD_REQUEST),
        Json(Envelope::<()>::err(request_id, err)),
    )
}

/// The route handler. Runs the runtime-token check first, then validates the
/// body, then checks the run binding when the edge has a shared secret.
pub async fn preflight(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PreflightRequest>,
) -> Result<Json<PreflightResponse>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::routes::verify_runtime_token(&headers).map_err(|e| refuse(&request_id, e))?;
    validate(&req).map_err(|e| refuse(&request_id, e))?;

    if let Some(signer) = state.internal_signer.as_ref() {
        let sig = headers
            .get("x-quarry-run-sig")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let target = binding_target(&req.schedule_id);
        if !signer.verify_run_binding(&req.org_id, &target, sig) {
            tracing::warn!(had_sig = !sig.is_empty(), "schedule preflight binding failed");
            let err = QuarryError::new(ErrorCode::Unauthorized, "preflight org binding invalid");
            return Err(refuse(&request_id, err));
        }
    }

    Ok(Json(s2_answer()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s2_answer_allows_with_the_floor_entitlement() {
        let a = s2_answer();
        assert!(a.allow);
        assert_eq!(a.posture, "unchecked");
        assert_eq!(a.entitlement, "floor");
    }

    #[test]
    fn binding_target_is_schedule_prefixed() {
        assert_eq!(binding_target("sch_abc"), "schedule:sch_abc");
    }

    #[test]
    fn validate_refuses_newlines_and_empty_identifiers() {
        let ok = PreflightRequest {
            schedule_id: "sch_1".into(),
            org_id: "org_1".into(),
        };
        assert!(validate(&ok).is_ok());
        let empty_org = PreflightRequest {
            schedule_id: "sch_1".into(),
            org_id: " ".into(),
        };
        assert!(validate(&empty_org).is_err());
        let newline = PreflightRequest {
            schedule_id: "sch_1\nx".into(),
            org_id: "org_1".into(),
        };
        assert!(validate(&newline).is_err());
    }
}
