//! Goal-bound schedules: "Keep working while I'm away" (Wave 3 EP-7 S6).
//!
//! A thread goal can continue while its owner is away only as a WS20 scheduled run (owner decision
//! D6 (b), taken by its recommendation): each fire runs read-only under a stored-consent contract,
//! as the service, in the service-owned scheduled thread (D9), with a fresh Control fire decision.
//! Nothing runs in the user's identity, and writes are withheld as drafts by the scheduled lane.
//!
//! The schedule is created through the existing create path, `POST /v1/cron`: the BFF has Control
//! sign the create decision over the exact template, so the template cannot be built here after
//! the decision exists. A template that names a `goal_id` is a goal-bound create and passes this
//! admission before it is forwarded: the switch, a zero-retention or waived token (refused: the
//! scheduled lane denies ZDR, DEC-32), the judge (a goal nobody judges would only run until a cap,
//! so it is refused), and the template's shape (purpose `goal`, at most [`MAX_RUNS`] runs, at most
//! one fire per [`MIN_INTERVAL_MINUTES`] minutes, a read-only contract, the default scheduled
//! workflow only). Session-core binds the schedule to the goal only when the Control-verified
//! creator owns the goal, active, on a personal thread (migration 0094), so ownership and Space
//! are decided there, not from anything a client sends.
//!
//! The switch is `MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED`, default off. Off, a template naming a
//! goal is refused, because session-core would bind it; every other create is untouched.

use axum::extract::{Path, State};
use axum::{http::StatusCode, Extension, Json};
use mp_contracts::model_plane::v1::{GetGoalTurnForJudgeRequest, InferRequest};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{verify_goal_judge_caller, Claims, VerifiedInferenceBearer};
use crate::goal_judge::{should_judge_turn, EvidenceLine};
use crate::http_routes::{require_non_zdr_durable_mutation, HttpJsonError};
use crate::state::AppState;
use crate::thread_goal::{judge_goal_turn, read_goal_turn_for_judge, GoalJudgeCall};

/// The switch. Opt-in: only the literal `true` enables it.
pub const ENABLED_ENV: &str = "MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED";

/// The most runs one goal-bound schedule may make (EP-7 S6). Session-core holds the same bound.
pub const MAX_RUNS: u64 = 20;

/// The shortest time between two fires of a goal-bound schedule.
pub const MIN_INTERVAL_MINUTES: u32 = 30;

/// The template purpose that marks a goal-bound schedule, so the Automations hub can label it.
pub const PURPOSE: &str = "goal";

/// The only contract class an unattended goal may hold.
const READ_CLASS: &str = "read";

/// The longest goal id the template may name (`goal_` and a ulid, with room to spare).
const MAX_GOAL_ID_LEN: usize = 64;

/// Whether the switch is on in this process. Read at each call, as the other goal switches are.
#[must_use]
pub fn enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// Whether a goal can be set to keep working while away on this deployment: this switch, the
/// thread goals switch and the judge's switch are all on.
#[must_use]
pub fn available() -> bool {
    enabled() && crate::thread_goal_routes::enabled() && crate::goal_judge::goal_judge_enabled()
}

/// Whether a schedule create names a goal. Only such a create is admitted here; every other
/// create is forwarded as it always was.
#[must_use]
pub fn names_goal(body: &Value) -> bool {
    body.get("task_template")
        .and_then(Value::as_object)
        .is_some_and(|template| template.contains_key("goal_id"))
}

/// The switches the admission reads, passed in so the rules are testable without the environment.
#[derive(Debug, Clone, Copy)]
pub struct Switches {
    pub unattended: bool,
    pub thread_goals: bool,
    pub judge: bool,
}

impl Switches {
    fn from_env() -> Self {
        Self {
            unattended: enabled(),
            thread_goals: crate::thread_goal_routes::enabled(),
            judge: crate::goal_judge::goal_judge_enabled(),
        }
    }
}

/// A typed error body; `details` is always an object.
fn typed(status: StatusCode, code: &str, message: &str, details: Value) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": details } })),
    )
}

/// A template field that does not hold to the rules, named so the dialog can point at it.
fn invalid(field: &str, message: &str) -> HttpJsonError {
    typed(
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_goal_schedule",
        message,
        json!({ "field": field }),
    )
}

/// Admit a goal-bound schedule create, with the switches read from the environment.
pub(crate) fn admit(claims: &Claims, body: &Value) -> Result<(), HttpJsonError> {
    admit_with(claims, body, Switches::from_env())
}

/// [`admit`] over injected switches: the switches first (a deployment without the feature answers
/// the same for every caller), then the token's retention, then the judge, then the template.
pub(crate) fn admit_with(
    claims: &Claims,
    body: &Value,
    switches: Switches,
) -> Result<(), HttpJsonError> {
    if !(switches.unattended && switches.thread_goals) {
        return Err(typed(
            StatusCode::NOT_FOUND,
            "goal_unattended_disabled",
            "keeping a goal working while away is not enabled on this deployment",
            json!({}),
        ));
    }
    // The scheduled lane keeps an answer and a transcript: never for a ZDR or waived token.
    crate::http_routes::require_non_zdr_durable_mutation(claims)?;
    check_admission(body, switches.judge)
}

/// The judge and template rules, apart from the token: the pure core of [`admit_with`].
pub(crate) fn check_admission(body: &Value, judge: bool) -> Result<(), HttpJsonError> {
    if !judge {
        return Err(typed(
            StatusCode::CONFLICT,
            "goal_unattended_needs_judge",
            "a goal keeps working while away only when its completion judge is on",
            json!({}),
        ));
    }
    let template = body
        .get("task_template")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("task_template", "the schedule needs a task template"))?;
    let goal_id = template
        .get("goal_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !valid_goal_id(goal_id) {
        return Err(invalid("goal_id", "goal_id is not a goal id"));
    }
    if template.get("purpose").and_then(Value::as_str) != Some(PURPOSE) {
        return Err(invalid(
            "purpose",
            "a goal-bound schedule has purpose \"goal\"",
        ));
    }
    let max_runs = template.get("goal_max_runs").and_then(Value::as_u64);
    if !max_runs.is_some_and(|runs| (1..=MAX_RUNS).contains(&runs)) {
        return Err(invalid(
            "goal_max_runs",
            "goal_max_runs must be from 1 to 20",
        ));
    }
    // A goal runs on the default scheduled workflow only: its digest-bound goal text is the work.
    if template.contains_key("workflow_type") || template.contains_key("workflow_input") {
        return Err(invalid(
            "workflow_type",
            "a goal-bound schedule runs the default scheduled workflow",
        ));
    }
    let expr = body
        .get("schedule_expr")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !fires_at_most_every_half_hour(expr) {
        return Err(invalid(
            "schedule_expr",
            "a goal-bound schedule fires at most once every 30 minutes",
        ));
    }
    let contract = template
        .get("approval_contract")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid(
                "approval_contract",
                "a goal-bound schedule needs a contract",
            )
        })?;
    if !contract_is_read_only(contract) {
        return Err(invalid(
            "approval_contract",
            "a goal-bound schedule approves read-only tools only",
        ));
    }
    Ok(())
}

/// `goal_` followed by id characters, bounded.
fn valid_goal_id(id: &str) -> bool {
    id.len() <= MAX_GOAL_ID_LEN
        && id.strip_prefix("goal_").is_some_and(|rest| {
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Whether a five-field cron expression fires at most once every [`MIN_INTERVAL_MINUTES`]. Only
/// the minute field decides it, read conservatively: one fixed minute (at most hourly), `*/30`,
/// or two fixed minutes exactly 30 apart. Anything else, including a valid expression that fires
/// less often in a form this does not read, is refused; the dialog never sends one.
fn fires_at_most_every_half_hour(expr: &str) -> bool {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    let [minute, _, _, _, _] = fields.as_slice() else {
        return false;
    };
    let fixed = |value: &str| {
        !value.is_empty()
            && value.chars().all(|c| c.is_ascii_digit())
            && value.parse::<u32>().is_ok_and(|minute| minute < 60)
    };
    if fixed(minute) || *minute == "*/30" {
        return true;
    }
    match minute.split_once(',') {
        Some((first, second)) if fixed(first) && fixed(second) => {
            let minute = |value: &str| value.parse::<u32>().unwrap_or_default();
            minute(first).abs_diff(minute(second)) == MIN_INTERVAL_MINUTES
        }
        _ => false,
    }
}

/// Whether a stored-consent contract (its JSON text, as the template stores it) approves at least
/// one tool and only tools of the read class. Capability-core checks the tools against the
/// registry at create; this holds the class ceiling of an unattended goal.
fn contract_is_read_only(text: &str) -> bool {
    let Ok(contract) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let Some(tools) = contract.get("tools").and_then(Value::as_array) else {
        return false;
    };
    !tools.is_empty()
        && tools
            .iter()
            .all(|tool| tool.get("class").and_then(Value::as_str) == Some(READ_CLASS))
}

/// The longest id the judge route carries (run, org, schedule or fire key), in bytes.
const MAX_JUDGE_ID_LEN: usize = 128;

/// The body of `POST /internal/v1/goals/:goal_id/judge-turn` (EP-7 S6): ids only. The objective, the
/// criteria and the answer are read by the gateway from session-core, never sent by the caller.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JudgeTurnBody {
    /// The scheduled run the fire started.
    run_id: String,
    /// The org the goal belongs to; must be the caller's own org.
    org_id: String,
    /// The schedule the fire belongs to.
    schedule_id: String,
    /// The fire's key within its schedule.
    fire_key: String,
}

/// An id the judge route carries: 1 to 128 bytes of ASCII letters, digits, `_`, `-`, `.` or `:`.
fn valid_judge_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_JUDGE_ID_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

/// A judge-route field that does not hold to the rules, named so the caller can see which one.
fn invalid_judge(field: &str, message: &str) -> HttpJsonError {
    typed(
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_goal_judge_turn",
        message,
        json!({ "field": field }),
    )
}

/// How session-core's refusal of the judge's read reaches the caller. A catalogued ZDR refusal keeps its
/// own typed body; a fire the goal does not ledger is not found; a refused credential is forbidden; any
/// other failure is a retryable outage that leaves the goal as it stands.
fn judge_read_error(status: &tonic::Status) -> HttpJsonError {
    if let Some(refusal) = mp_contracts::zdr_refusal::Refusal::from_status(status) {
        return crate::zdr_posture::http(&refusal);
    }
    match status.code() {
        tonic::Code::NotFound => typed(
            StatusCode::NOT_FOUND,
            "goal_turn_not_found",
            "no unattended turn of this goal names the fire",
            json!({}),
        ),
        tonic::Code::PermissionDenied | tonic::Code::Unauthenticated => typed(
            StatusCode::FORBIDDEN,
            "goal_judge_forbidden",
            "the goal judge may not read this goal",
            json!({}),
        ),
        _ => typed(
            StatusCode::SERVICE_UNAVAILABLE,
            "goal_judge_unavailable",
            "the goal could not be read for its judge; the goal stays as it was",
            json!({}),
        ),
    }
}

/// `POST /internal/v1/goals/:goal_id/judge-turn` (EP-7 S6): the completion judge of one unattended fire
/// of a goal-bound schedule, called by orchestrator-core once the fire's run has settled. The caller
/// presents its own service token (`model:goal_judge`, pinned to the body's org) and the
/// `aud=inference-core` bearer that the judge call spends. The gateway reads the goal, the fire's turn and
/// its answer from session-core, judges them and records the verdict. The body is ids only; the only
/// content read is the answer, and the response carries the verdict and nothing else. `404` while either
/// switch is off.
pub(crate) async fn judge_goal_turn_route(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: Option<Extension<VerifiedInferenceBearer>>,
    Path(goal_id): Path<String>,
    Json(body): Json<JudgeTurnBody>,
) -> Result<Json<Value>, HttpJsonError> {
    if !(enabled() && crate::goal_judge::goal_judge_enabled()) {
        return Err(typed(
            StatusCode::NOT_FOUND,
            "goal_unattended_disabled",
            "keeping a goal working while away is not enabled on this deployment",
            json!({}),
        ));
    }
    verify_goal_judge_caller(&claims, &body.org_id).map_err(|status| {
        typed(
            status,
            "goal_judge_forbidden",
            "the goal judge is for orchestrator-core, in its own org",
            json!({}),
        )
    })?;
    // The judge's read and verdict are durable writes about the goal, and the scheduled lane denies ZDR.
    require_non_zdr_durable_mutation(&claims)?;
    if !valid_goal_id(&goal_id) {
        return Err(invalid_judge("goal_id", "goal_id is not a goal id"));
    }
    for (field, value) in [
        ("run_id", &body.run_id),
        ("org_id", &body.org_id),
        ("schedule_id", &body.schedule_id),
        ("fire_key", &body.fire_key),
    ] {
        if !valid_judge_id(value) {
            return Err(invalid_judge(
                field,
                "a judge id is letters, digits and _ - . : only",
            ));
        }
    }
    let Some(Extension(inference_bearer)) = bearer else {
        return Err(typed(
            StatusCode::UNAUTHORIZED,
            "goal_judge_bearer_required",
            "the judge call needs the caller's inference bearer",
            json!({}),
        ));
    };
    let Some(tokens) = state.goal_verdict_tokens.clone() else {
        return Err(typed(
            StatusCode::SERVICE_UNAVAILABLE,
            "goal_judge_unavailable",
            "the goal judge credential is not configured",
            json!({}),
        ));
    };
    let read_token = tokens.read_token(&body.org_id).await.map_err(|error| {
        tracing::warn!(%error, "goal judge read token was not minted; the judge did not run");
        typed(
            StatusCode::SERVICE_UNAVAILABLE,
            "goal_judge_unavailable",
            "the goal judge credential is unavailable",
            json!({}),
        )
    })?;
    let read = read_goal_turn_for_judge(
        state.thread_goal_client.clone(),
        &read_token,
        GetGoalTurnForJudgeRequest {
            goal_id: goal_id.clone(),
            org_id: body.org_id.clone(),
            run_id: body.run_id.clone(),
            schedule_id: body.schedule_id.clone(),
            fire_key: body.fire_key.clone(),
        },
    )
    .await
    .map_err(|status| judge_read_error(&status))?;
    let not_found = || {
        typed(
            StatusCode::NOT_FOUND,
            "goal_turn_not_found",
            "no unattended turn of this goal names the fire",
            json!({}),
        )
    };
    let found = read.found;
    let Some(goal) = read.goal.filter(|_| found) else {
        return Err(not_found());
    };
    if goal.id != goal_id || goal.org_id != body.org_id {
        return Err(not_found());
    }
    // Only a completed fire is judged, as an attended turn is (EP-7 S3): a parked, failed or unknown
    // run has no finished answer to judge.
    if !should_judge_turn(true, &read.outcome) {
        return Err(typed(
            StatusCode::CONFLICT,
            "goal_turn_not_judged",
            "only a completed fire is judged",
            json!({ "outcome": read.outcome.clone() }),
        ));
    }
    if read.answer.trim().is_empty() {
        return Err(typed(
            StatusCode::CONFLICT,
            "goal_turn_has_no_answer",
            "the fire recorded no answer to judge",
            json!({}),
        ));
    }
    // The fire's run and closed outcome are the evidence the judge may cite, and so are the names and
    // statuses of the run's tool calls (`tool:<name>:<status>`), read by run id with the same read
    // token as the goal. The scheduled lane denies ZDR and the goal read refuses a waived run, so no
    // such run is read.
    let mut evidence = vec![EvidenceLine {
        id: format!("run:{}", body.run_id),
        line: format!("scheduled_run outcome={}", read.outcome),
    }];
    for line in crate::thread_goal::tool_call_evidence_for_run(
        &state,
        &read_token,
        &body.org_id,
        &body.run_id,
    )
    .await
    {
        if !evidence.iter().any(|kept| kept.id == line.id) {
            evidence.push(line);
        }
    }
    evidence.truncate(crate::goal_judge::MAX_EVIDENCE_LINES);
    let request_id = format!("goal-judge-{}", body.run_id);
    // The scheduled lane names no model of its own, so the judge runs on the auxiliary model with no
    // route floors. The scheduled lane denies ZDR, so the judge is never zero-retention.
    let route = InferRequest::default();
    let recorded = judge_goal_turn(
        &state,
        &inference_bearer,
        GoalJudgeCall {
            route: &route,
            served_model: crate::sse::aux_model(),
            request_id: &request_id,
            org_id: &body.org_id,
            zdr: false,
            goal: &goal,
            turn_no: read.turn_no,
            answer: &read.answer,
            evidence: &evidence,
        },
    )
    .await
    .ok_or_else(|| {
        typed(
            StatusCode::SERVICE_UNAVAILABLE,
            "goal_judge_failed",
            "the goal judge recorded no verdict; the goal stays as it was",
            json!({}),
        )
    })?;
    if recorded.last_verdict.is_empty() {
        return Err(typed(
            StatusCode::SERVICE_UNAVAILABLE,
            "goal_judge_failed",
            "the goal judge recorded no verdict; the goal stays as it was",
            json!({}),
        ));
    }
    Ok(Json(
        json!({ "data": { "verdict": recorded.last_verdict } }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(classes: &[&str]) -> String {
        let tools: Vec<Value> = classes
            .iter()
            .map(|class| json!({ "tool": "web_search", "capability": "cap.web", "class": class }))
            .collect();
        json!({ "v": 1, "tools": tools, "expires_at": "2026-12-01T00:00:00Z" }).to_string()
    }

    fn body() -> Value {
        json!({
            "name": "Keep working: release notes",
            "schedule_expr": "0 * * * *",
            "task_template": {
                "kind": "cron",
                "title": "Keep working: release notes",
                "description": "collect the release notes",
                "purpose": "goal",
                "goal_id": "goal_01JABCDEF",
                "goal_max_runs": 3,
                "approval_contract": contract(&["read"]),
            },
        })
    }

    fn code(error: &HttpJsonError) -> String {
        error.1 .0["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    fn field(error: &HttpJsonError) -> String {
        error.1 .0["error"]["details"]["field"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    #[test]
    fn unattended_names_goal_only_for_a_template_with_goal_id() {
        assert!(names_goal(&body()));
        assert!(!names_goal(&json!({ "task_template": { "kind": "cron" } })));
        assert!(!names_goal(&json!({ "name": "no template" })));
    }

    #[test]
    fn unattended_switch_reads_true_in_any_case_and_nothing_else() {
        for value in [None, Some(""), Some("1"), Some("yes")] {
            assert!(!mp_contracts::loop_fold::switch_from(value), "{value:?}");
        }
        assert!(mp_contracts::loop_fold::switch_from(Some("true")));
        assert!(mp_contracts::loop_fold::switch_from(Some("TRUE")));
    }

    #[test]
    fn unattended_refuses_without_judge() {
        let error = check_admission(&body(), false).unwrap_err();
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert_eq!(code(&error), "goal_unattended_needs_judge");
        assert!(error.1 .0["error"]["details"].is_object());
        assert!(check_admission(&body(), true).is_ok());
    }

    #[test]
    fn unattended_contract_read_class_only() {
        let mut write = body();
        write["task_template"]["approval_contract"] = json!(contract(&["read", "write"]));
        assert_eq!(
            field(&check_admission(&write, true).unwrap_err()),
            "approval_contract"
        );

        let mut empty = body();
        empty["task_template"]["approval_contract"] = json!(contract(&[]));
        assert_eq!(
            field(&check_admission(&empty, true).unwrap_err()),
            "approval_contract"
        );

        let mut missing = body();
        missing["task_template"]
            .as_object_mut()
            .unwrap()
            .remove("approval_contract");
        assert_eq!(
            field(&check_admission(&missing, true).unwrap_err()),
            "approval_contract"
        );

        let mut not_text = body();
        not_text["task_template"]["approval_contract"] = json!({ "tools": [] });
        assert_eq!(
            field(&check_admission(&not_text, true).unwrap_err()),
            "approval_contract"
        );
    }

    /// The body with one edit is refused on `expected`.
    fn refused(edit: impl Fn(&mut Value), expected: &str) {
        let mut candidate = body();
        edit(&mut candidate);
        let error = check_admission(&candidate, true).unwrap_err();
        assert_eq!(error.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(code(&error), "invalid_goal_schedule");
        assert_eq!(field(&error), expected);
    }

    #[test]
    fn unattended_template_limits_hold() {
        refused(
            |b| b["task_template"]["goal_max_runs"] = json!(21),
            "goal_max_runs",
        );
        refused(
            |b| b["task_template"]["goal_max_runs"] = json!(0),
            "goal_max_runs",
        );
        refused(
            |b| b["task_template"]["goal_max_runs"] = json!("3"),
            "goal_max_runs",
        );
        refused(
            |b| b["task_template"]["purpose"] = json!("report"),
            "purpose",
        );
        refused(
            |b| b["task_template"]["goal_id"] = json!("thread-1"),
            "goal_id",
        );
        refused(|b| b["task_template"]["goal_id"] = json!(7), "goal_id");
        refused(
            |b| b["task_template"]["workflow_type"] = json!("DeepTaskWorkflow"),
            "workflow_type",
        );
        refused(
            |b| b["schedule_expr"] = json!("*/5 * * * *"),
            "schedule_expr",
        );
        refused(|b| b["schedule_expr"] = json!("* * * * *"), "schedule_expr");
        refused(
            |b| b["schedule_expr"] = json!("*/45 * * * *"),
            "schedule_expr",
        );
        refused(
            |b| b["schedule_expr"] = json!("0,15 * * * *"),
            "schedule_expr",
        );
        refused(|b| b["schedule_expr"] = json!("0 *"), "schedule_expr");
    }

    #[test]
    fn unattended_interval_accepts_half_hourly_and_slower() {
        for expr in [
            "0 * * * *",
            "*/30 * * * *",
            "15,45 * * * *",
            "0 */4 * * *",
            "30 9 * * 1-5",
        ] {
            assert!(fires_at_most_every_half_hour(expr), "{expr}");
        }
        assert_eq!(MIN_INTERVAL_MINUTES, 30);
        assert_eq!(MAX_RUNS, 20);
    }
}
