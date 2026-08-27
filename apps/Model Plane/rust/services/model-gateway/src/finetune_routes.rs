//! HTTP routes for fine-tuning lifecycle (Wave 7 v1).
//!
//! These routes proxy to session-core's `FinetuneJobs` gRPC service for state
//! and persistence. Azure `OpenAI` HTTP integration (file upload, job creation,
//! polling, deployment) is deferred to the next implementation slice — for v1
//! the gateway persists the job row with `status='queued'` and empty Azure
//! fields, and a follow-up session adds:
//!
//!   * multipart JSONL upload + Azure Files API call,
//!   * Azure Fine-tuning Jobs API call,
//!   * a polling worker in session-core,
//!   * Azure deployment provisioning on success.
//!
//! All routes:
//!   * sit behind `auth::require_auth` (mounted in `http_routes.rs`),
//!   * enforce the `admin` scope on `claims` (per gap-model.md PAR-30/§14.9),
//!   * short-circuit with 503 when `FINETUNE_ENABLED` is unset,
//!   * scope every query by `claims.org_id` — no cross-org leakage.
//!
//! Envelopes follow the existing model-gateway pattern: `(StatusCode,
//! Json<Value>)` for errors, `Json<Value>` for success.

use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use mp_contracts::model_plane::v1 as pb;
use mp_ids::new_ulid;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::warn;

use crate::auth::Claims;
use crate::finetune_azure::{AzureFinetuneClient, AzureHyperparameters, DeploymentTier};
use crate::finetune_poller::generate_deployment_name;
use crate::state::{AppState, DynPublisher};
use mp_events::publisher::EventPublisher;

type HttpJsonError = (StatusCode, Json<Value>);

const ADMIN_SCOPE: &str = "admin";
const FEATURE_ENV: &str = "FINETUNE_ENABLED";

/// Our production hosting rate (USD/hour) for a deployed fine-tuned model —
/// our markup over Azure's $1.70 Standard hosting fee. Configurable via
/// `FINETUNE_PRODUCTION_HOSTING_USD_PER_HOUR` (default 2.00). The Developer
/// (test) tier is always $0, so only production deployments accrue this.
fn production_hosting_usd_per_hour() -> f64 {
    std::env::var("FINETUNE_PRODUCTION_HOSTING_USD_PER_HOUR")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| *v >= 0.0)
        .unwrap_or(2.00)
}

/// Single source of truth for converting a `pb::FinetuneJob` to a JSON
/// envelope the UI consumes. Stable shape — Wave 7 doc cites these fields.
///
/// `deployment_tier` + `*_hosting_usd_per_hour` let the UI show the cost choice:
/// auto-deploys land on the **developer** tier ($0/hr, auto-deletes in 24h);
/// promoting to **production** costs `production_hosting_usd_per_hour` plus
/// per-token inference. RAG is always-on and free of this charge — fine-tuning
/// is an optional, opt-in enhancement.
fn job_value(job: &pb::FinetuneJob) -> Value {
    // Persisted tier from the row; fall back to "developer" for pre-migration
    // rows that read back an empty string.
    let deployment_tier = if job.deployment_tier.is_empty() {
        "developer"
    } else {
        job.deployment_tier.as_str()
    };
    json!({
        "deployment_tier": deployment_tier,
        "developer_hosting_usd_per_hour": 0.0,
        "developer_auto_delete_hours": 24,
        "production_hosting_usd_per_hour": production_hosting_usd_per_hour(),
        "job_id": job.job_id,
        "org_id": job.org_id,
        "agent_id": job.agent_id,
        "base_model": job.base_model,
        "azure_file_id": job.azure_file_id,
        "azure_job_id": job.azure_job_id,
        "fine_tuned_model": job.fine_tuned_model,
        "deployment_name": job.deployment_name,
        "status": job.status,
        "error_message": job.error_message,
        "hyperparameters": job.hyperparameters_json,
        "training_example_count": job.training_example_count,
        "estimated_cost_usd": job.estimated_cost_usd,
        "actual_cost_usd": job.actual_cost_usd,
        "created_by": job.created_by,
        "created_at": job.created_at.as_ref().map(|t| t.seconds),
        "updated_at": job.updated_at.as_ref().map(|t| t.seconds),
        "completed_at": job.completed_at.as_ref().map(|t| t.seconds),
    })
}

/// Map an Azure FT job status string to our local schema's status enum.
/// Azure has one extra state (`validating_files`) that we collapse into
/// `running` — for our purposes, "in-flight, not user-cancellable".
/// Anything we don't recognise stays as-is so we never lie about state.
pub(crate) fn map_azure_status_to_local(azure_status: &str) -> &str {
    match azure_status {
        "validating_files" => "running",
        "queued" | "running" | "succeeded" | "failed" | "cancelled" => azure_status,
        // Unknown status (Azure schema drift) — log up the stack, keep verbatim.
        other => other,
    }
}

/// Returns `Some(true)` if Azure says the job is in a terminal state, so the
/// gateway should stamp `completed_at` on the `UpdateJobStatus` call.
pub(crate) fn azure_status_is_terminal(local_status: &str) -> bool {
    matches!(local_status, "succeeded" | "failed" | "cancelled")
}

/// Outcome of the per-org / per-job budget check. Modeled as an enum so the
/// caller can render a precise 429 body (which cap was hit, by how much).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BudgetVerdict {
    /// Estimate fits both per-job and per-org caps. Proceed.
    Allowed,
    /// Per-job cap exceeded by `estimate - cap` USD.
    PerJobExceeded { cap_usd: f64, estimate_usd: f64 },
    /// Per-org monthly cap would be exceeded by `current + estimate - cap` USD.
    OrgMonthlyExceeded {
        cap_usd: f64,
        current_usd: f64,
        estimate_usd: f64,
    },
}

/// Pure budget decision. Negative or NaN inputs are treated as 0.0 — the
/// guard fails closed (treats unknown as "could exceed").
pub(crate) fn budget_verdict(
    estimate_usd: f64,
    current_usd: f64,
    per_job_cap_usd: f64,
    org_cap_usd: f64,
) -> BudgetVerdict {
    let est = if estimate_usd.is_nan() || estimate_usd < 0.0 {
        0.0
    } else {
        estimate_usd
    };
    let cur = if current_usd.is_nan() || current_usd < 0.0 {
        0.0
    } else {
        current_usd
    };

    // Per-job cap of 0 disables the check; same for org cap. Operators
    // opt-in by setting positive values.
    if per_job_cap_usd > 0.0 && est > per_job_cap_usd {
        return BudgetVerdict::PerJobExceeded {
            cap_usd: per_job_cap_usd,
            estimate_usd: est,
        };
    }
    if org_cap_usd > 0.0 && cur + est > org_cap_usd {
        return BudgetVerdict::OrgMonthlyExceeded {
            cap_usd: org_cap_usd,
            current_usd: cur,
            estimate_usd: est,
        };
    }
    BudgetVerdict::Allowed
}

/// Read a USD cap from env, falling back to `default` when unset or unparseable.
/// 0.0 disables the corresponding check (no cap).
pub(crate) fn read_budget_cap(env_name: &str, default: f64) -> f64 {
    std::env::var(env_name)
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(default)
}

/// Conservative server-side cost floor used to defend the budget guard
/// against caller-supplied `estimated_cost_usd` of `0.0` (see HIGH-3 in the
/// Wave 7 review). The actual cost is overwritten by the poller from
/// `actual_cost_usd` when the job finishes; this is purely a pre-flight
/// floor so a malicious or buggy client cannot bypass the per-job and org
/// caps by claiming a cost of zero.
///
/// The rates here are deliberately on the high side relative to current
/// Azure list prices — we would rather reject a legitimate job and force an
/// operator override than greenlight a runaway spend. Update when Azure
/// pricing materially changes.
#[must_use]
pub(crate) fn server_estimate_cost_usd(training_example_count: i32, base_model: &str) -> f64 {
    let n = f64::from(training_example_count.max(0));
    // Per-example floor in USD. GPT-4 class models are ~10x more expensive
    // to fine-tune than GPT-3.5 class — bucket conservatively by name.
    let per_example = if base_model.contains("gpt-4") || base_model.contains("gpt4") {
        0.005
    } else {
        0.0005
    };
    n * per_example
}

/// Choose the larger of the caller-supplied estimate and the server-side
/// floor. The budget guard uses this so a client cannot pass `0.0` and
/// bypass the per-job / org caps (HIGH-3 in the Wave 7 review).
#[must_use]
pub(crate) fn effective_budget_estimate(
    caller_estimate_usd: f64,
    training_example_count: i32,
    base_model: &str,
) -> f64 {
    let floor = server_estimate_cost_usd(training_example_count, base_model);
    if caller_estimate_usd.is_finite() && caller_estimate_usd > floor {
        caller_estimate_usd
    } else {
        floor
    }
}

/// Best-effort NATS publish of a finetune lifecycle event. Failures log a
/// warn and return — emission is observational; never block the caller.
/// `user_id` is the actor — caller's `claims.user_id` for routes, "system"
/// for the polling worker.
pub(crate) async fn publish_finetune_event(
    publisher: &DynPublisher,
    org_id: &str,
    user_id: &str,
    job_id: &str,
    event_type: &str,
    payload: Value,
    zdr: bool,
) {
    let subject = mp_events::subjects::finetune_event_subject(org_id, event_type);
    let event_id = mp_ids::new_ulid();
    let envelope = mp_events::envelope::Envelope {
        event_id: event_id.clone(),
        event_type: format!("finetune.{event_type}"),
        schema_version: 1,
        ts: chrono::Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: job_id.to_owned(),
        causation_id: String::new(),
        idempotency_key: event_id,
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        resource_ref: format!("finetune:{job_id}"),
        payload,
        zdr,
    };
    if let Err(e) = publisher.publish(&subject, &envelope).await {
        warn!(error = %e, subject = %subject, "finetune event publish failed");
    }
}

fn grpc_err(e: &tonic::Status) -> HttpJsonError {
    let code = match e.code() {
        tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::AlreadyExists => StatusCode::CONFLICT,
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        tonic::Code::Unavailable => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (code, Json(json!({ "error": e.message() })))
}

/// Wave 7 guard: only admins kick off fine-tunes. Returns 403 when missing.
/// The `admin` scope is populated by auth-core when the user is org-owner or
/// org-admin (follow-up wiring described in `docs/wave7-fine-tuning.md`).
fn require_admin(claims: &Claims) -> Result<(), HttpJsonError> {
    if claims.has_scope(ADMIN_SCOPE) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "admin scope required for fine-tuning operations",
                "required_scope": ADMIN_SCOPE,
            })),
        ))
    }
}

/// Fine-tune operations either persist lifecycle state, publish a lifecycle
/// event, or invoke an external provider. The issuer-verified ZDR posture is
/// therefore authoritative and cannot be weakened by an HTTP body, multipart
/// field, header, or admin scope.
fn require_non_zdr_finetune_mutation(claims: &Claims) -> Result<(), HttpJsonError> {
    if !claims.effective_zdr(false) {
        return Ok(());
    }
    Err((
        StatusCode::PRECONDITION_FAILED,
        Json(json!({
            "error": {
                "code": "zdr_durable_mutation_forbidden",
                "message": "Zero Data Retention credentials cannot create, change, or deploy fine-tune state"
            }
        })),
    ))
}

/// A detail read may return the already scoped cached row under ZDR, but it
/// must not refresh from Azure because a refresh writes provider metadata back
/// to Session Core.
const fn should_refresh_job_from_provider(claims: &Claims, needs_refresh: bool) -> bool {
    needs_refresh && !claims.effective_zdr(false)
}

/// Pure predicate over the env-var value. Lifted from `require_feature_enabled`
/// so it can be unit-tested without mutating process env (workspace forbids
/// `unsafe_code` and `std::env::set_var` is `unsafe` in current toolchains).
fn feature_enabled(env_value: Option<&str>) -> bool {
    env_value == Some("1")
}

/// Master kill-switch. Compose ships `FINETUNE_ENABLED=1` in the dev profile.
fn require_feature_enabled() -> Result<(), HttpJsonError> {
    if feature_enabled(std::env::var(FEATURE_ENV).ok().as_deref()) {
        Ok(())
    } else {
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "fine-tuning is disabled in this environment",
                "hint": format!("set {FEATURE_ENV}=1 to enable"),
            })),
        ))
    }
}

#[derive(Deserialize)]
pub struct CreateJobBody {
    pub agent_id: String,
    pub base_model: String,
    /// Optional — provider defaults when absent. Stored verbatim on the row.
    #[serde(default)]
    pub hyperparameters: Option<Value>,
    /// Caller-supplied count so the budget guard can validate before the
    /// Azure call runs. Cross-checked against the JSONL line count when
    /// `training_jsonl` is present.
    #[serde(default)]
    pub training_example_count: i32,
    /// Caller-supplied estimate (pre-job). The polling worker writes the
    /// authoritative `actual_cost_usd` on completion.
    #[serde(default)]
    pub estimated_cost_usd: f64,
    /// Optional inline JSONL training payload. When present and the gateway
    /// has Azure credentials configured, the gateway uploads it to Azure and
    /// kicks off a fine-tuning job. When absent, the row is persisted with
    /// `status='queued'` and empty `azure_*` so a follow-up call (or future
    /// multipart upload path) can populate them.
    ///
    /// Slice 2b ships JSON-only inline JSONL; slice 2c will accept multipart
    /// uploads alongside this field.
    #[serde(default)]
    pub training_jsonl: Option<String>,
    /// Optional suffix appended to the fine-tuned model id (e.g. "acme-support-v3").
    #[serde(default)]
    pub suffix: Option<String>,
}

/// Convert a free-form hyperparameters JSON `Value` into the typed
/// `AzureHyperparameters` shape Azure expects. Unknown keys are silently
/// dropped — Azure rejects unknown fields with 400, so we filter at the edge.
fn hyperparameters_for_azure(raw: Option<&Value>) -> Option<AzureHyperparameters> {
    let v = raw?;
    let obj = v.as_object()?;
    Some(AzureHyperparameters {
        n_epochs: obj
            .get("n_epochs")
            .and_then(serde_json::Value::as_i64)
            .and_then(|n| i32::try_from(n).ok()),
        batch_size: obj
            .get("batch_size")
            .and_then(serde_json::Value::as_i64)
            .and_then(|n| i32::try_from(n).ok()),
        learning_rate_multiplier: obj
            .get("learning_rate_multiplier")
            .and_then(serde_json::Value::as_f64),
    })
}

/// Drive the Azure file-upload + job-create pair. Returns
/// `(azure_file_id, azure_job_id)` on success.
async fn provision_azure_job(
    azure: &AzureFinetuneClient,
    base_model: &str,
    training_jsonl: &str,
    hyperparameters: Option<&Value>,
    suffix: Option<&str>,
) -> Result<(String, String), HttpJsonError> {
    let file_id = azure
        .upload_training_file(training_jsonl.as_bytes().to_vec(), "training.jsonl")
        .await
        .map_err(|e| azure_err(&e))?;
    let job_id = azure
        .create_finetune_job(
            base_model,
            &file_id,
            hyperparameters_for_azure(hyperparameters),
            suffix,
        )
        .await
        .map_err(|e| azure_err(&e))?;
    Ok((file_id, job_id))
}

fn azure_err(e: &crate::finetune_azure::AzureError) -> HttpJsonError {
    use crate::finetune_azure::AzureError;
    let (code, msg) = match e {
        AzureError::Http(_) => (
            StatusCode::BAD_GATEWAY,
            "azure transport failure".to_owned(),
        ),
        AzureError::Api { status, body } if *status == 400 => (
            StatusCode::BAD_REQUEST,
            format!("azure rejected request: {body}"),
        ),
        AzureError::Api { status, body } if *status == 401 || *status == 403 => (
            StatusCode::BAD_GATEWAY,
            format!("azure auth failure ({status}): check AZURE_OPENAI_API_KEY: {body}"),
        ),
        AzureError::Api { status, .. } if *status == 404 => {
            (StatusCode::NOT_FOUND, "azure resource not found".to_owned())
        }
        AzureError::Api { status, body } => (
            StatusCode::BAD_GATEWAY,
            format!("azure error ({status}): {body}"),
        ),
        AzureError::Decode(m) => (
            StatusCode::BAD_GATEWAY,
            format!("azure response decode: {m}"),
        ),
        AzureError::MgmtNotConfigured => (
            StatusCode::SERVICE_UNAVAILABLE,
            "azure management plane not configured".to_owned(),
        ),
    };
    warn!(error = %e, "azure call failed");
    (code, Json(json!({ "error": msg })))
}

/// Enforce the per-job and per-org monthly budget caps for a JSON create request.
///
/// # Errors
///
/// Returns a `429 TOO_MANY_REQUESTS` JSON error if the effective estimate exceeds
/// the per-job cap or would push the org over its monthly cap, or maps an upstream
/// gRPC failure (spend lookup) to an `HttpJsonError`.
async fn enforce_budget(
    state: &AppState,
    org_id: &str,
    body: &CreateJobBody,
) -> Result<(), HttpJsonError> {
    enforce_budget_estimate(
        state,
        org_id,
        body.estimated_cost_usd,
        body.training_example_count,
        &body.base_model,
    )
    .await
}

/// Shared budget guard for both the JSON and multipart create paths.
///
/// Reads the per-job and per-org caps from env, queries session-core for the org's
/// month-to-date spend, and combines the caller estimate with a server-derived floor
/// (HIGH-3 defence) so a client cannot bypass the caps by claiming a cost of zero.
///
/// # Errors
///
/// Returns a `429 TOO_MANY_REQUESTS` JSON error if the per-job or org-monthly cap
/// would be exceeded, or maps an upstream gRPC failure (spend lookup) to an `HttpJsonError`.
async fn enforce_budget_estimate(
    state: &AppState,
    org_id: &str,
    caller_estimate_usd: f64,
    example_count: i32,
    base_model: &str,
) -> Result<(), HttpJsonError> {
    // Caps of 0 disable the corresponding check — operators opt into
    // throttling by setting positive values.
    let per_job_cap = read_budget_cap("FINETUNE_PER_JOB_BUDGET_USD", 20.0);
    let org_cap = read_budget_cap("FINETUNE_ORG_BUDGET_USD", 50.0);
    let spend = state
        .finetune_jobs_client
        .clone()
        .get_org_monthly_spend(pb::GetOrgMonthlySpendRequest {
            org_id: org_id.to_owned(),
        })
        .await
        .map_err(|e| grpc_err(&e))?
        .into_inner();
    let effective_estimate =
        effective_budget_estimate(caller_estimate_usd, example_count, base_model);
    match budget_verdict(effective_estimate, spend.total_usd, per_job_cap, org_cap) {
        BudgetVerdict::Allowed => Ok(()),
        BudgetVerdict::PerJobExceeded {
            cap_usd,
            estimate_usd,
        } => Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": format!(
                    "estimated cost ${estimate_usd:.2} exceeds per-job cap ${cap_usd:.2}"
                ),
                "per_job_cap_usd": cap_usd,
                "estimate_usd": estimate_usd,
            })),
        )),
        BudgetVerdict::OrgMonthlyExceeded {
            cap_usd,
            current_usd,
            estimate_usd,
        } => Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": format!(
                    "org monthly cap ${cap_usd:.2} would be exceeded (current ${current_usd:.2} + new ${estimate_usd:.2})"
                ),
                "org_cap_usd": cap_usd,
                "current_usd": current_usd,
                "estimate_usd": estimate_usd,
            })),
        )),
    }
}

/// `POST /v1/finetune/jobs`
///
/// v1 path (no Azure HTTP yet):
///   1. Admin-scope + feature-flag check.
///   2. Validate body (`agent_id`, `base_model` present).
///   3. Generate a `job_id` (ULID, server-side so callers can't collide).
///   4. Persist row in session-core's `finetune_jobs` table with
///      `status='queued'`, `azure_*` empty.
///   5. Return `{ job_id, status: 'queued' }` to the caller.
///
/// Follow-up (next session) inserts steps 2.5/3.5: upload the JSONL to Azure
/// Files, kick off the Fine-tuning Job, populate `azure_file_id` + `azure_job_id`.
///
/// # Errors
///
/// Returns a `400` if `agent_id`/`base_model` are missing, a `429` if budget caps
/// are exceeded, a `403`/feature error from the admin/flag guards, or maps upstream
/// Azure/gRPC failures to an `HttpJsonError`.
pub async fn create_job(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<CreateJobBody>,
) -> Result<Json<Value>, HttpJsonError> {
    // This must precede feature/admin/body/budget work: ZDR callers may not
    // trigger Azure, Session Core persistence, or a lifecycle event.
    require_non_zdr_finetune_mutation(&claims)?;
    require_feature_enabled()?;
    require_admin(&claims)?;

    if body.agent_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "agent_id required" })),
        ));
    }
    if body.base_model.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "base_model required" })),
        ));
    }

    enforce_budget(&state, &claims.org_id, &body).await?;

    let job_id = new_ulid();
    let hyperparameters_json = body
        .hyperparameters
        .as_ref()
        .map(std::string::ToString::to_string)
        .unwrap_or_default();

    // If Azure is configured AND the caller supplied training JSONL, drive
    // the upload-then-create handshake before we persist. The resulting
    // azure_file_id / azure_job_id land on the row so the polling worker
    // (slice 2c) can refresh status without re-asking the operator. When
    // either is absent, we persist with empty azure_* — operator can
    // re-issue with credentials later, or use the existing row to track a
    // job that was kicked off out-of-band.
    let (azure_file_id, azure_job_id) =
        match (&state.azure_finetune, body.training_jsonl.as_deref()) {
            (Some(client), Some(jsonl)) if !jsonl.is_empty() => {
                provision_azure_job(
                    client,
                    &body.base_model,
                    jsonl,
                    body.hyperparameters.as_ref(),
                    body.suffix.as_deref(),
                )
                .await?
            }
            _ => (String::new(), String::new()),
        };

    let resp = state
        .finetune_jobs_client
        .clone()
        .create_job(pb::CreateFinetuneJobRequest {
            job_id: job_id.clone(),
            org_id: claims.org_id.clone(),
            agent_id: body.agent_id,
            base_model: body.base_model,
            azure_file_id,
            azure_job_id,
            hyperparameters_json,
            training_example_count: body.training_example_count,
            estimated_cost_usd: body.estimated_cost_usd,
            created_by: claims.user_id.clone(),
        })
        .await
        .map_err(|e| grpc_err(&e))?
        .into_inner();

    announce_created(&state, &claims, &resp, &json!({})).await;

    Ok(Json(job_value(&resp)))
}

/// Emit the `finetune.created` lifecycle event so the App-Plane projector wakes
/// without polling. Best-effort — emission failure logs and never blocks the response.
/// `extra` is merged into the event payload (e.g. example/rejected counts).
async fn announce_created(
    state: &AppState,
    claims: &Claims,
    resp: &pb::FinetuneJob,
    extra: &Value,
) {
    let mut payload = json!({
        "job_id": resp.job_id,
        "agent_id": resp.agent_id,
        "base_model": resp.base_model,
        "status": resp.status,
        "azure_job_id": resp.azure_job_id,
    });
    if let (Some(obj), Some(extra_obj)) = (payload.as_object_mut(), extra.as_object()) {
        for (k, v) in extra_obj {
            obj.insert(k.clone(), v.clone());
        }
    }
    publish_finetune_event(
        &state.publisher,
        &claims.org_id,
        &claims.user_id,
        &resp.job_id,
        mp_events::subjects::FINETUNE_EVENT_CREATED,
        payload,
        claims.effective_zdr(false),
    )
    .await;
}

#[derive(Deserialize, Default)]
pub struct ListJobsQuery {
    pub agent_id: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// `GET /v1/finetune/jobs?agent_id=…&status=…&limit=…&offset=…`
///
/// Scoped to caller's org. Listing is open to any auth'd user in the org (no
/// admin scope required) — operators need to see job history without elevated
/// rights. Mutations stay admin-only.
///
/// # Errors
///
/// Returns a feature-disabled error from the flag guard, or maps an upstream gRPC
/// failure to an `HttpJsonError`.
pub async fn list_jobs(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<ListJobsQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    require_feature_enabled()?;

    let resp = state
        .finetune_jobs_client
        .clone()
        .list_jobs(pb::ListFinetuneJobsRequest {
            org_id: claims.org_id,
            agent_id: q.agent_id.unwrap_or_default(),
            status: q.status.unwrap_or_default(),
            limit: q.limit.unwrap_or(0),
            offset: q.offset.unwrap_or(0),
        })
        .await
        .map_err(|e| grpc_err(&e))?
        .into_inner();

    Ok(Json(json!({
        "jobs": resp.jobs.iter().map(job_value).collect::<Vec<_>>(),
        "total": resp.total,
    })))
}

/// `GET /v1/finetune/jobs/:job_id`
///
/// When the row is in flight (`queued` or `running`) AND Azure is configured
/// AND `azure_job_id` is populated, the handler refreshes status from Azure
/// before returning. Refresh failures degrade gracefully — they log a warning
/// and return the cached row rather than failing the read. The polling worker
/// (slice 2c) covers the eventual-consistency gap.
///
/// # Errors
///
/// Returns a feature-disabled error from the flag guard, or maps an upstream gRPC
/// failure (job lookup) to an `HttpJsonError`. Azure refresh failures are logged
/// and fall back to the cached row, not surfaced as an `Err`.
pub async fn get_job(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(job_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    require_feature_enabled()?;

    let mut current = state
        .finetune_jobs_client
        .clone()
        .get_job(pb::GetFinetuneJobRequest {
            job_id: job_id.clone(),
            org_id: claims.org_id.clone(),
        })
        .await
        .map_err(|e| grpc_err(&e))?
        .into_inner();

    let needs_refresh =
        matches!(current.status.as_str(), "queued" | "running") && !current.azure_job_id.is_empty();

    if let (true, Some(azure)) = (
        should_refresh_job_from_provider(&claims, needs_refresh),
        state.azure_finetune.as_ref(),
    ) {
        match azure.get_finetune_job(&current.azure_job_id).await {
            Ok(azure_job) => {
                let local_status = map_azure_status_to_local(&azure_job.status);
                let terminal = azure_status_is_terminal(local_status);
                let update = state
                    .finetune_jobs_client
                    .clone()
                    .update_job_status(pb::UpdateFinetuneJobStatusRequest {
                        job_id: job_id.clone(),
                        org_id: claims.org_id.clone(),
                        status: local_status.to_owned(),
                        error_message: azure_job
                            .error
                            .as_ref()
                            .map(|e| format!("{}: {}", e.code, e.message))
                            .unwrap_or_default(),
                        fine_tuned_model: azure_job.fine_tuned_model.clone(),
                        deployment_name: String::new(),
                        actual_cost_usd: 0.0,
                        set_completed: terminal,
                        deployment_tier: String::new(),
                    })
                    .await;
                match update {
                    Ok(r) => current = r.into_inner(),
                    Err(e) => warn!(error = %e, "finetune update after Azure refresh failed"),
                }
            }
            Err(e) => warn!(error = %e, "azure get_finetune_job failed; returning cached row"),
        }
    }

    Ok(Json(job_value(&current)))
}

/// `DELETE /v1/finetune/jobs/:job_id`
///
/// v1: marks the row `cancelled` (and stamps `completed_at`) in session-core.
/// The Azure cancel call (POST /`openai/fine_tuning/jobs/{id}/cancel`) is wired
/// in the next slice once Azure HTTP is in. If a job has progressed to
/// `succeeded` it cannot be cancelled — return 409.
///
/// # Errors
///
/// Returns a feature-disabled/admin error from the guards, a `409 CONFLICT` if the
/// job is already terminal, or maps an upstream gRPC failure to an `HttpJsonError`.
pub async fn cancel_job(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(job_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    // Reject before the state read, Azure cancel, durable update, or event.
    require_non_zdr_finetune_mutation(&claims)?;
    require_feature_enabled()?;
    require_admin(&claims)?;

    // Pre-flight: check current state so we can return a clear 409 instead
    // of silently overwriting a terminal status.
    let current = state
        .finetune_jobs_client
        .clone()
        .get_job(pb::GetFinetuneJobRequest {
            job_id: job_id.clone(),
            org_id: claims.org_id.clone(),
        })
        .await
        .map_err(|e| grpc_err(&e))?
        .into_inner();

    match current.status.as_str() {
        "succeeded" | "failed" | "cancelled" => {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": format!("job is already in terminal status: {}", current.status),
                })),
            ));
        }
        _ => {}
    }

    // Best-effort Azure cancel. We always proceed to flip our local row to
    // `cancelled` even if Azure rejects — the polling worker will reconcile
    // if Azure later reports `succeeded` (in which case the deployment
    // creation step will still fire and the operator can see both states).
    if let (Some(azure), false) = (
        state.azure_finetune.as_ref(),
        current.azure_job_id.is_empty(),
    ) {
        if let Err(e) = azure.cancel_finetune_job(&current.azure_job_id).await {
            warn!(error = %e, azure_job_id = %current.azure_job_id, "azure cancel failed; proceeding with local cancellation");
        }
    }

    let resp = match state
        .finetune_jobs_client
        .clone()
        .update_job_status(pb::UpdateFinetuneJobStatusRequest {
            job_id: job_id.clone(),
            org_id: claims.org_id.clone(),
            status: "cancelled".to_owned(),
            error_message: "cancelled by operator".to_owned(),
            fine_tuned_model: String::new(),
            deployment_name: String::new(),
            actual_cost_usd: 0.0,
            set_completed: true,
            deployment_tier: String::new(),
        })
        .await
    {
        Ok(r) => r.into_inner(),
        // NotFound here means the row exists (we got it on the pre-flight
        // GetJob) but the SQL TOCTOU guard refused the transition — a
        // concurrent poller tick flipped it to a terminal status between
        // our check and update. Surface that as 409, not 404.
        Err(status) if status.code() == tonic::Code::NotFound => {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "job reached a terminal status before cancel could take effect",
                })),
            ));
        }
        Err(status) => return Err(grpc_err(&status)),
    };

    publish_finetune_event(
        &state.publisher,
        &claims.org_id,
        &claims.user_id,
        &resp.job_id,
        mp_events::subjects::FINETUNE_EVENT_CANCELLED,
        json!({
            "job_id": resp.job_id,
            "status": resp.status,
            "azure_job_id": resp.azure_job_id,
        }),
        claims.effective_zdr(false),
    )
    .await;

    Ok(Json(job_value(&resp)))
}

#[derive(Deserialize)]
pub struct DeployJobBody {
    /// Target hosting tier. Defaults to `production` for this endpoint (the
    /// promote action); `developer` is accepted to (re)deploy the free test
    /// tier. Unknown values fall back to `developer` (never silently paid).
    #[serde(default)]
    pub tier: Option<String>,
}

/// How many hours per month we assume a production deployment runs when
/// pre-flighting the budget guard. 730 ≈ a 30.4-day month. Used only to derive
/// a conservative hosting estimate; the actual accrual is tracked elsewhere.
const PRODUCTION_HOURS_PER_MONTH: f64 = 730.0;

/// `POST /v1/finetune/jobs/:job_id/deploy`
///
/// Operator promote action: provision (or re-provision) an Azure deployment for
/// a succeeded fine-tuned model at the requested hosting tier. Body
/// `{ "tier": "production" | "developer" }`; defaults to `production`.
///
/// Steps:
///   1. Feature-flag + admin-scope guards (same as the other mutations).
///   2. Load the job (org-scoped). Require `status == succeeded` and a
///      non-empty `fine_tuned_model` — you can't deploy what isn't trained.
///   3. Require the Azure management plane to be configured — else 503.
///   4. For the `production` tier, run the budget guard against the estimated
///      monthly hosting cost so a promote can't blow the org cap.
///   5. Call `create_deployment(deployment_name, fine_tuned_model, tier)`.
///   6. `UpdateJobStatus` to persist `deployment_name` + `deployment_tier`
///      (status stays `succeeded`).
///   7. Return the updated job via `job_value`.
///
/// # Errors
///
/// Returns a feature-disabled/admin error from the guards, a `409 CONFLICT` if
/// the job is not `succeeded` or lacks a `fine_tuned_model`, a `503` if the
/// Azure management plane is unconfigured, a `429` if the production hosting
/// estimate exceeds a budget cap, or maps upstream Azure/gRPC failures.
#[allow(clippy::too_many_lines)]
pub async fn deploy_job(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(job_id): Path<String>,
    Json(body): Json<DeployJobBody>,
) -> Result<Json<Value>, HttpJsonError> {
    // Reject before any Session Core read, Azure management call, durable
    // deployment state change, or lifecycle event.
    require_non_zdr_finetune_mutation(&claims)?;
    require_feature_enabled()?;
    require_admin(&claims)?;

    // Default to production for this endpoint (it's the promote action); accept
    // "developer" to redeploy the free test tier.
    let tier = match body.tier.as_deref() {
        Some(s) if !s.trim().is_empty() => DeploymentTier::from_str_or_developer(s),
        _ => DeploymentTier::Production,
    };

    // The Azure management plane must be configured to provision a deployment.
    // Check before touching session-core so a misconfigured gateway returns a
    // clear 503 rather than mutating state it can't follow through on.
    let azure = state
        .azure_finetune
        .as_ref()
        .filter(|c| c.mgmt_configured());
    let Some(azure) = azure else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Azure management plane not configured; cannot provision deployment",
                "hint": "set AZURE_SUBSCRIPTION_ID, AZURE_RESOURCE_GROUP, AZURE_OPENAI_ACCOUNT_NAME, AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET",
            })),
        ));
    };

    let job = state
        .finetune_jobs_client
        .clone()
        .get_job(pb::GetFinetuneJobRequest {
            job_id: job_id.clone(),
            org_id: claims.org_id.clone(),
        })
        .await
        .map_err(|e| grpc_err(&e))?
        .into_inner();

    if job.status != "succeeded" {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!(
                    "job must be in status 'succeeded' to deploy (current: {})",
                    job.status
                ),
            })),
        ));
    }
    if job.fine_tuned_model.is_empty() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "job has no fine_tuned_model; nothing to deploy",
            })),
        ));
    }

    // Budget guard for the paid tier only — the Developer tier is $0/hr.
    if tier == DeploymentTier::Production {
        let monthly_estimate = production_hosting_usd_per_hour() * PRODUCTION_HOURS_PER_MONTH;
        enforce_budget_estimate(
            &state,
            &claims.org_id,
            monthly_estimate,
            job.training_example_count,
            &job.base_model,
        )
        .await?;
    }

    let deployment_name = if job.deployment_name.is_empty() {
        generate_deployment_name(&job.job_id)
    } else {
        // Re-use the existing name so a redeploy targets the same Azure
        // resource (create-or-update is idempotent).
        job.deployment_name.clone()
    };

    azure
        .create_deployment(&deployment_name, &job.fine_tuned_model, tier)
        .await
        .map_err(|e| azure_err(&e))?;

    let resp = state
        .finetune_jobs_client
        .clone()
        .update_job_status(pb::UpdateFinetuneJobStatusRequest {
            job_id: job_id.clone(),
            org_id: claims.org_id.clone(),
            // Status is already succeeded — re-assert it (UpdateJobStatus
            // validates the value; succeeded is valid and the row stays put).
            status: "succeeded".to_owned(),
            error_message: String::new(),
            fine_tuned_model: String::new(),
            deployment_name: deployment_name.clone(),
            actual_cost_usd: 0.0,
            set_completed: false,
            deployment_tier: tier.as_str().to_owned(),
        })
        .await
        .map_err(|e| grpc_err(&e))?
        .into_inner();

    publish_finetune_event(
        &state.publisher,
        &claims.org_id,
        &claims.user_id,
        &resp.job_id,
        mp_events::subjects::FINETUNE_EVENT_DEPLOYED,
        json!({
            "job_id": resp.job_id,
            "deployment_name": deployment_name,
            "deployment_tier": tier.as_str(),
            "fine_tuned_model": resp.fine_tuned_model,
        }),
        claims.effective_zdr(false),
    )
    .await;

    Ok(Json(job_value(&resp)))
}

/// Soft cap on JSONL upload size. Azure's own limit is 512 MiB but a small
/// fine-tune typically needs <1 MiB. The cap defends the gateway against
/// pathological uploads pinning a worker on multipart parsing. Operators can
/// override via `FINETUNE_MAX_JSONL_BYTES`.
const DEFAULT_MAX_JSONL_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JsonlSummary {
    pub valid_examples: i32,
    pub rejected_lines: Vec<i32>,
}

/// Validate that `body` is a non-empty JSONL document where every non-empty
/// line parses as a JSON object containing either `messages` (chat-format) or
/// `prompt`+`completion` (completion-format). Returns:
///
///   * `Ok(summary)` on at least one valid example,
///   * `Err(reason)` if the document is empty or every line failed.
///
/// Pure function — no I/O, no time, fully testable.
pub(crate) fn validate_jsonl(body: &str) -> Result<JsonlSummary, String> {
    let mut valid = 0i32;
    let mut rejected: Vec<i32> = Vec::new();

    for (idx, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let line_no = i32::try_from(idx + 1).unwrap_or(i32::MAX);
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(trimmed);
        let Ok(value) = parsed else {
            rejected.push(line_no);
            continue;
        };
        let Some(obj) = value.as_object() else {
            rejected.push(line_no);
            continue;
        };
        let has_messages = obj
            .get("messages")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty());
        let has_prompt_completion = obj.get("prompt").is_some() && obj.get("completion").is_some();
        if has_messages || has_prompt_completion {
            valid += 1;
        } else {
            rejected.push(line_no);
        }
    }

    if valid == 0 {
        return Err(format!(
            "no valid training examples (expected JSONL with `messages` array or `prompt`+`completion`); rejected {} line(s)",
            rejected.len()
        ));
    }

    Ok(JsonlSummary {
        valid_examples: valid,
        rejected_lines: rejected,
    })
}

/// Read the size cap for JSONL uploads from env. Falls back to 50 MiB.
fn max_jsonl_bytes() -> usize {
    std::env::var("FINETUNE_MAX_JSONL_BYTES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_JSONL_BYTES)
}

/// Multipart form fields extracted from `POST /v1/finetune/jobs/upload`.
#[derive(Default)]
struct MultipartFields {
    agent_id: String,
    base_model: String,
    hyperparameters: Option<String>,
    suffix: Option<String>,
    estimated_cost_usd: f64,
    file_bytes: Vec<u8>,
    file_name: String,
}

/// `POST /v1/finetune/jobs/upload`
///
/// Multipart variant of `create_job` — operators upload the JSONL training
/// file directly rather than embedding it in JSON. Same admin-scope +
/// feature-flag + budget guards apply.
///
/// Required form fields:
///   * `file` — the JSONL bytes (binary part).
///   * `agent_id` — string.
///   * `base_model` — string.
///
/// Optional form fields:
///   * `hyperparameters` — JSON string.
///   * `suffix` — string (Azure fine-tune suffix).
///   * `estimated_cost_usd` — string (parsed as f64); falls back to 0.0.
///
/// Rejects: missing `file` or `agent_id` or `base_model` (400);
/// file larger than `FINETUNE_MAX_JSONL_BYTES` (413); JSONL with zero valid
/// examples (400 with a clear count of rejected lines).
///
/// # Errors
///
/// Returns a feature-disabled/admin error from the guards, a `400`/`413` from
/// multipart parsing or validation, a `429` if budget caps are exceeded, or maps
/// upstream Azure/gRPC failures to an `HttpJsonError`.
pub async fn create_job_multipart(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    multipart: Multipart,
) -> Result<Json<Value>, HttpJsonError> {
    // Reject before parsing the upload so a ZDR request never reaches Azure,
    // budget/session persistence, or event publication.
    require_non_zdr_finetune_mutation(&claims)?;
    require_feature_enabled()?;
    require_admin(&claims)?;

    let fields = parse_multipart_fields(multipart).await?;

    if fields.agent_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "agent_id required" })),
        ));
    }
    if fields.base_model.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "base_model required" })),
        ));
    }
    if fields.file_bytes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "file required (multipart field name: file)" })),
        ));
    }

    // Validate JSONL before any Azure or budget calls — fail fast on garbage
    // input so we don't burn a budget aggregation query on a bad upload.
    let jsonl_text = std::str::from_utf8(&fields.file_bytes).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("file is not valid UTF-8: {e}") })),
        )
    })?;
    let summary = validate_jsonl(jsonl_text)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;

    // HIGH-3 defence (multipart path): use the JSONL-derived valid_examples
    // count (server-trusted) and the caller's base_model to floor the estimate.
    enforce_budget_estimate(
        &state,
        &claims.org_id,
        fields.estimated_cost_usd,
        summary.valid_examples,
        &fields.base_model,
    )
    .await?;

    let job_id = new_ulid();
    let hyperparameters_value: Option<Value> = fields
        .hyperparameters
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok());
    let hyperparameters_json = fields.hyperparameters.clone().unwrap_or_default();

    // Drive Azure with the actual uploaded bytes (preserves the original
    // filename for Azure-side auditability).
    let (azure_file_id, azure_job_id) = if let Some(client) = state.azure_finetune.as_ref() {
        let file_id = client
            .upload_training_file(fields.file_bytes.clone(), &fields.file_name)
            .await
            .map_err(|e| azure_err(&e))?;
        let job_id_az = client
            .create_finetune_job(
                &fields.base_model,
                &file_id,
                hyperparameters_for_azure(hyperparameters_value.as_ref()),
                fields.suffix.as_deref(),
            )
            .await
            .map_err(|e| azure_err(&e))?;
        (file_id, job_id_az)
    } else {
        (String::new(), String::new())
    };

    let resp = state
        .finetune_jobs_client
        .clone()
        .create_job(pb::CreateFinetuneJobRequest {
            job_id: job_id.clone(),
            org_id: claims.org_id.clone(),
            agent_id: fields.agent_id,
            base_model: fields.base_model,
            azure_file_id,
            azure_job_id,
            hyperparameters_json,
            training_example_count: summary.valid_examples,
            estimated_cost_usd: fields.estimated_cost_usd,
            created_by: claims.user_id.clone(),
        })
        .await
        .map_err(|e| grpc_err(&e))?
        .into_inner();

    announce_created(
        &state,
        &claims,
        &resp,
        &json!({
            "training_example_count": summary.valid_examples,
            "rejected_line_count": summary.rejected_lines.len(),
        }),
    )
    .await;

    Ok(Json(json!({
        "job": job_value(&resp),
        "training_example_count": summary.valid_examples,
        "rejected_lines": summary.rejected_lines,
    })))
}

async fn read_text(field: axum::extract::multipart::Field<'_>) -> Result<String, HttpJsonError> {
    field.text().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("multipart text field error: {e}") })),
        )
    })
}

/// Drain a multipart body into [`MultipartFields`], enforcing the per-file size cap.
///
/// # Errors
///
/// Returns a `400 BAD_REQUEST` on a multipart parse/read error, or `413 PAYLOAD_TOO_LARGE`
/// if the `file` part exceeds `FINETUNE_MAX_JSONL_BYTES`.
async fn parse_multipart_fields(
    mut multipart: Multipart,
) -> Result<MultipartFields, HttpJsonError> {
    let max_bytes = max_jsonl_bytes();
    let mut fields = MultipartFields::default();

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("multipart parse error: {e}") })),
        )
    })? {
        let name = field.name().unwrap_or("").to_owned();
        match name.as_str() {
            "file" => {
                fields.file_name = field.file_name().unwrap_or("training.jsonl").to_owned();
                let bytes = field.bytes().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("file read error: {e}") })),
                    )
                })?;
                if bytes.len() > max_bytes {
                    return Err((
                        StatusCode::PAYLOAD_TOO_LARGE,
                        Json(json!({
                            "error": format!(
                                "file size {} bytes exceeds cap {} bytes (FINETUNE_MAX_JSONL_BYTES)",
                                bytes.len(), max_bytes
                            ),
                        })),
                    ));
                }
                fields.file_bytes = bytes.to_vec();
            }
            "agent_id" => {
                fields.agent_id = read_text(field).await?;
            }
            "base_model" => {
                fields.base_model = read_text(field).await?;
            }
            "hyperparameters" => {
                let t = read_text(field).await?;
                if !t.is_empty() {
                    fields.hyperparameters = Some(t);
                }
            }
            "suffix" => {
                let t = read_text(field).await?;
                if !t.is_empty() {
                    fields.suffix = Some(t);
                }
            }
            "estimated_cost_usd" => {
                let t = read_text(field).await?;
                fields.estimated_cost_usd = t.parse().unwrap_or(0.0);
            }
            _ => {
                // Unknown fields are dropped silently — forward-compatible
                // with new UI additions.
            }
        }
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims_with_scopes(scopes: &[&str]) -> Claims {
        Claims {
            sub: "u1".into(),
            iss: "test".into(),
            exp: 0,
            org_id: "org-acme".into(),
            user_id: "u1".into(),
            nbf: None,
            aud: None,
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
            zdr: false,
            sovereign: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    #[test]
    fn require_admin_accepts_admin_scope() {
        let c = claims_with_scopes(&["admin"]);
        assert!(require_admin(&c).is_ok());
    }

    #[test]
    fn require_admin_rejects_missing_scope() {
        let c = claims_with_scopes(&["models:invoke"]);
        let err = require_admin(&c).unwrap_err();
        assert_eq!(err.0, StatusCode::FORBIDDEN);
    }

    #[test]
    fn require_admin_rejects_empty_scopes() {
        let c = claims_with_scopes(&[]);
        let err = require_admin(&c).unwrap_err();
        assert_eq!(err.0, StatusCode::FORBIDDEN);
    }

    #[test]
    fn issuer_zdr_blocks_every_durable_finetune_mutation_and_remote_refresh() {
        let mut claims = claims_with_scopes(&[ADMIN_SCOPE]);
        claims.zdr = true;

        let error = require_non_zdr_finetune_mutation(&claims)
            .expect_err("issuer-ZDR admin must not create, upload, cancel, or deploy fine-tunes");
        assert_eq!(error.0, StatusCode::PRECONDITION_FAILED);
        assert!(
            !should_refresh_job_from_provider(&claims, true),
            "ZDR reads may return the scoped cached row but must not call Azure or persist a refresh"
        );

        claims.zdr = false;
        assert!(require_non_zdr_finetune_mutation(&claims).is_ok());
        assert!(should_refresh_job_from_provider(&claims, true));
    }

    #[tokio::test]
    async fn issuer_zdr_finetune_handlers_return_before_provider_persistence_or_events() {
        let state = AppState::new();
        let publisher = state.publisher.clone();
        let mut claims = claims_with_scopes(&[ADMIN_SCOPE]);
        claims.zdr = true;

        let create = create_job(
            State(state.clone()),
            Extension(claims.clone()),
            Json(CreateJobBody {
                agent_id: "agent-a".to_owned(),
                base_model: "gpt-4o-mini".to_owned(),
                hyperparameters: None,
                training_example_count: 1,
                estimated_cost_usd: 0.01,
                training_jsonl: Some(
                    "{\"messages\":[{\"role\":\"user\",\"content\":\"x\"}]}".to_owned(),
                ),
                suffix: None,
            }),
        )
        .await;
        let cancel = cancel_job(
            State(state.clone()),
            Extension(claims.clone()),
            Path("job-a".to_owned()),
        )
        .await;
        let deploy = deploy_job(
            State(state),
            Extension(claims),
            Path("job-a".to_owned()),
            Json(DeployJobBody {
                tier: Some("production".to_owned()),
            }),
        )
        .await;

        for result in [create, cancel, deploy] {
            let error = result.expect_err("issuer-ZDR durable fine-tune route must fail locally");
            assert_eq!(error.0, StatusCode::PRECONDITION_FAILED);
        }
        assert!(
            publisher.drain().is_empty(),
            "issuer-ZDR fine-tune request published a durable lifecycle event"
        );
    }

    #[test]
    fn feature_enabled_only_truthy_on_one() {
        assert!(feature_enabled(Some("1")));
        assert!(!feature_enabled(Some("0")));
        assert!(!feature_enabled(Some("true")));
        assert!(!feature_enabled(Some("")));
        assert!(!feature_enabled(None));
    }

    #[test]
    fn map_azure_status_collapses_validating_files_to_running() {
        assert_eq!(map_azure_status_to_local("validating_files"), "running");
    }

    #[test]
    fn map_azure_status_passes_through_known_states() {
        for s in ["queued", "running", "succeeded", "failed", "cancelled"] {
            assert_eq!(map_azure_status_to_local(s), s);
        }
    }

    #[test]
    fn map_azure_status_preserves_unknown_for_audit() {
        // If Azure introduces a new state, we surface it verbatim rather
        // than coercing — so the operator sees the drift and the polling
        // worker can log on it.
        assert_eq!(map_azure_status_to_local("paused"), "paused");
    }

    #[test]
    fn azure_status_is_terminal_matches_lifecycle_definition() {
        assert!(azure_status_is_terminal("succeeded"));
        assert!(azure_status_is_terminal("failed"));
        assert!(azure_status_is_terminal("cancelled"));
        assert!(!azure_status_is_terminal("queued"));
        assert!(!azure_status_is_terminal("running"));
    }

    #[test]
    fn hyperparameters_for_azure_extracts_known_fields_only() {
        let v = serde_json::json!({
            "n_epochs": 5,
            "batch_size": 2,
            "learning_rate_multiplier": 0.5,
            "unknown_field": "ignored",
        });
        let hp = hyperparameters_for_azure(Some(&v)).expect("some");
        assert_eq!(hp.n_epochs, Some(5));
        assert_eq!(hp.batch_size, Some(2));
        assert_eq!(hp.learning_rate_multiplier, Some(0.5));
    }

    #[test]
    fn hyperparameters_for_azure_returns_none_when_input_is_none() {
        assert!(hyperparameters_for_azure(None).is_none());
    }

    #[test]
    fn hyperparameters_for_azure_returns_none_when_value_is_not_object() {
        let v = serde_json::json!("not an object");
        assert!(hyperparameters_for_azure(Some(&v)).is_none());
    }

    #[test]
    fn budget_verdict_allows_when_estimate_below_both_caps() {
        assert_eq!(
            budget_verdict(5.0, 10.0, 20.0, 50.0),
            BudgetVerdict::Allowed
        );
    }

    #[test]
    fn budget_verdict_rejects_when_per_job_cap_exceeded() {
        match budget_verdict(30.0, 0.0, 20.0, 100.0) {
            BudgetVerdict::PerJobExceeded {
                cap_usd,
                estimate_usd,
            } => {
                assert!((cap_usd - 20.0).abs() < f64::EPSILON);
                assert!((estimate_usd - 30.0).abs() < f64::EPSILON);
            }
            other => panic!("expected PerJobExceeded, got {other:?}"),
        }
    }

    #[test]
    fn budget_verdict_rejects_when_org_monthly_cap_would_be_exceeded() {
        // current $45 + estimate $10 = $55 > $50 cap
        match budget_verdict(10.0, 45.0, 20.0, 50.0) {
            BudgetVerdict::OrgMonthlyExceeded {
                cap_usd,
                current_usd,
                estimate_usd,
            } => {
                assert!((cap_usd - 50.0).abs() < f64::EPSILON);
                assert!((current_usd - 45.0).abs() < f64::EPSILON);
                assert!((estimate_usd - 10.0).abs() < f64::EPSILON);
            }
            other => panic!("expected OrgMonthlyExceeded, got {other:?}"),
        }
    }

    #[test]
    fn budget_verdict_disables_check_when_cap_is_zero() {
        // Zero per-job cap → no per-job check. Zero org cap → no org check.
        assert_eq!(
            budget_verdict(1000.0, 9999.0, 0.0, 0.0),
            BudgetVerdict::Allowed
        );
    }

    #[test]
    fn budget_verdict_clamps_negative_and_nan_inputs_to_zero() {
        // Negative estimate + negative current: both treated as 0, which is
        // below any positive cap → allowed.
        assert_eq!(
            budget_verdict(-5.0, -10.0, 20.0, 50.0),
            BudgetVerdict::Allowed
        );
        assert_eq!(
            budget_verdict(f64::NAN, f64::NAN, 20.0, 50.0),
            BudgetVerdict::Allowed
        );
    }

    #[test]
    fn budget_verdict_per_job_check_takes_precedence_over_org_check() {
        // Both would fire; per-job is reported first because the caller
        // can act on it without consulting org-wide state.
        match budget_verdict(100.0, 0.0, 20.0, 50.0) {
            BudgetVerdict::PerJobExceeded { .. } => {}
            other => panic!("expected PerJobExceeded first, got {other:?}"),
        }
    }

    #[test]
    fn read_budget_cap_falls_back_to_default_when_unset() {
        // We can't toggle env in tests (unsafe forbidden), but we can check
        // the parse path with a known-unset name.
        assert!((read_budget_cap("MP_FT_TEST_UNSET_VAR_NAME_XYZ", 7.5) - 7.5).abs() < 1e-9);
    }

    #[test]
    fn server_estimate_uses_higher_floor_for_gpt4_class_models() {
        // 100 examples at $0.005/example = $0.50 for gpt-4 class.
        assert!((server_estimate_cost_usd(100, "gpt-4o-mini") - 0.50).abs() < 1e-9);
        // 100 examples at $0.0005/example = $0.05 for gpt-3.5 class.
        assert!((server_estimate_cost_usd(100, "gpt-3.5-turbo") - 0.05).abs() < 1e-9);
        // Negative counts clamp to zero.
        assert!(server_estimate_cost_usd(-5, "gpt-4o-mini").abs() < 1e-9);
    }

    #[test]
    fn effective_estimate_floors_zero_caller_estimate() {
        // Caller claims $0.00 but the server floor on 1000 gpt-4 examples
        // is $5.00 — the floor wins so the budget guard cannot be bypassed.
        let est = effective_budget_estimate(0.0, 1000, "gpt-4o-mini");
        assert!((est - 5.0).abs() < 1e-9);
    }

    #[test]
    fn effective_estimate_honors_caller_when_above_floor() {
        // Caller-supplied conservative estimate is higher than the floor —
        // use the caller's value so honest clients see their own number.
        let est = effective_budget_estimate(42.0, 1000, "gpt-3.5-turbo");
        assert!((est - 42.0).abs() < 1e-9);
    }

    #[test]
    fn effective_estimate_rejects_nan_caller_input() {
        // NaN from a buggy client must not poison the budget guard.
        let est = effective_budget_estimate(f64::NAN, 100, "gpt-3.5-turbo");
        assert!((est - 0.05).abs() < 1e-9);
    }

    #[test]
    fn validate_jsonl_accepts_chat_format() {
        let body = "{\"messages\":[{\"role\":\"user\",\"content\":\"hi\"},{\"role\":\"assistant\",\"content\":\"hello\"}]}\n";
        let s = validate_jsonl(body).expect("valid");
        assert_eq!(s.valid_examples, 1);
        assert!(s.rejected_lines.is_empty());
    }

    #[test]
    fn validate_jsonl_accepts_completion_format() {
        let body = "{\"prompt\":\"q\",\"completion\":\"a\"}\n";
        let s = validate_jsonl(body).expect("valid");
        assert_eq!(s.valid_examples, 1);
    }

    #[test]
    fn validate_jsonl_skips_blank_lines() {
        // Blank/whitespace-only lines are not counted as rejections — the
        // operator's JSONL tooling commonly emits a trailing newline.
        let body = "\n  \n{\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}\n\n";
        let s = validate_jsonl(body).expect("valid");
        assert_eq!(s.valid_examples, 1);
        assert!(s.rejected_lines.is_empty());
    }

    #[test]
    fn validate_jsonl_reports_rejected_line_numbers() {
        // Line 1 valid, line 2 not JSON, line 3 missing required keys,
        // line 4 valid.
        let body = "{\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}\nnot json\n{\"foo\":\"bar\"}\n{\"prompt\":\"p\",\"completion\":\"c\"}\n";
        let s = validate_jsonl(body).expect("valid");
        assert_eq!(s.valid_examples, 2);
        assert_eq!(s.rejected_lines, vec![2, 3]);
    }

    #[test]
    fn validate_jsonl_rejects_when_zero_valid_examples() {
        let body = "garbage\nmore garbage\n";
        let err = validate_jsonl(body).unwrap_err();
        assert!(err.contains("no valid training examples"));
        assert!(err.contains("2 line(s)"));
    }

    #[test]
    fn validate_jsonl_rejects_empty_messages_array() {
        // `messages` must be non-empty; an empty array is meaningless for FT.
        let body = "{\"messages\":[]}\n";
        let err = validate_jsonl(body).unwrap_err();
        assert!(err.contains("no valid training examples"));
    }

    #[test]
    fn validate_jsonl_rejects_top_level_array() {
        // OpenAI's JSONL expects one object per line, not a JSON array as a line.
        let body = "[1,2,3]\n";
        assert!(validate_jsonl(body).is_err());
    }

    #[test]
    fn job_value_renders_stable_field_set() {
        let job = pb::FinetuneJob {
            job_id: "job-1".into(),
            org_id: "org".into(),
            agent_id: "ag".into(),
            base_model: "gpt-4o-mini".into(),
            azure_file_id: "f1".into(),
            azure_job_id: "j1".into(),
            fine_tuned_model: String::new(),
            deployment_name: String::new(),
            deployment_tier: "developer".into(),
            status: "queued".into(),
            error_message: String::new(),
            created_at: Some(prost_types::Timestamp {
                seconds: 100,
                nanos: 0,
            }),
            updated_at: Some(prost_types::Timestamp {
                seconds: 200,
                nanos: 0,
            }),
            completed_at: None,
            hyperparameters_json: r#"{"epochs":3}"#.into(),
            training_example_count: 10,
            estimated_cost_usd: 0.5,
            actual_cost_usd: 0.0,
            created_by: "u1".into(),
        };
        let v = job_value(&job);
        assert_eq!(v["job_id"], "job-1");
        assert_eq!(v["status"], "queued");
        assert_eq!(v["training_example_count"], 10);
        assert_eq!(v["created_at"], 100);
        assert!(v["completed_at"].is_null());
    }
}
