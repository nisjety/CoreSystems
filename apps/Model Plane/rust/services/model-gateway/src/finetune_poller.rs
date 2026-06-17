//! Polling worker for fine-tuning jobs (Wave 7 slice 2c).
//!
//! On a configurable interval (default 60s) the worker:
//!
//!   1. Calls `FinetuneJobs.ListActiveJobs` to pull every in-flight row
//!      (`status` in `queued`/`running` with a non-empty `azure_job_id`).
//!   2. For each, calls `AzureFinetuneClient::get_finetune_job` to refresh
//!      provider state.
//!   3. Decides a [`PollerAction`] from `(current_row, azure_response)`.
//!   4. Executes the action via Azure (`create_deployment`) and/or
//!      session-core (`UpdateJobStatus`).
//!
//! The decision logic in [`decide_action`] is **pure** — no I/O, no time —
//! so unit tests cover every transition. Side effects in [`apply_action`]
//! are best-effort: failures log a warn and move on to the next row, so a
//! single Azure flake never stalls the worker.
//!
//! Disabled when:
//!   * `FINETUNE_ENABLED != "1"` (matches the route gate), or
//!   * `AzureFinetuneClient` is `None` (no creds configured).

use std::sync::Arc;
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1 as pb;
use mp_contracts::model_plane::v1::finetune_jobs_client::FinetuneJobsClient;
use serde_json::json;
use tonic::transport::Channel;
use tracing::{info, warn};

use crate::finetune_azure::{AzureFinetuneClient, AzureJobStatus, DeploymentTier};
use crate::finetune_routes::{
    azure_status_is_terminal, map_azure_status_to_local, publish_finetune_event,
};
use crate::state::DynPublisher;

const FEATURE_ENV: &str = "FINETUNE_ENABLED";
const INTERVAL_ENV: &str = "FINETUNE_POLLER_INTERVAL_SECS";
const DEFAULT_INTERVAL_SECS: u64 = 60;
const DEPLOYMENT_NAME_PREFIX: &str = "ft-";
/// Length of the `job_id` suffix included in the deployment name. ULIDs are 26
/// chars; we take 12 so the full name fits Azure's 64-char deployment-name
/// limit even with any future prefix expansion.
const DEPLOYMENT_NAME_SUFFIX_LEN: usize = 12;

/// What the worker should do for a given (row, Azure-response) pair.
/// Modeled as an enum so [`decide_action`] is a pure total function and
/// every branch is named (exhaustive `match` enforced).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PollerAction {
    /// Provider says the job is still in flight and the local row already
    /// matches that state. Nothing to write.
    NoOp,

    /// Update status + `error_message` + (optionally) `fine_tuned_model`. The
    /// `terminal` flag tells `UpdateJobStatus` to stamp `completed_at`.
    UpdateStatus {
        new_status: String,
        error_message: String,
        fine_tuned_model: String,
        terminal: bool,
    },

    /// Job just transitioned to `succeeded` — provision an Azure deployment
    /// for the new fine-tuned model, then write the `deployment_name` back
    /// alongside the status flip. `deployment_name` is the value the
    /// worker will use when calling Azure.
    DeployThenUpdate {
        deployment_name: String,
        fine_tuned_model: String,
    },
}

/// Pure decision: given the current local row and what Azure says now,
/// return the next action. Never panics, never blocks.
pub(crate) fn decide_action(current: &pb::FinetuneJob, azure: &AzureJobStatus) -> PollerAction {
    let new_status = map_azure_status_to_local(&azure.status).to_owned();
    let terminal = azure_status_is_terminal(&new_status);
    let error_message = azure
        .error
        .as_ref()
        .map(|e| format!("{}: {}", e.code, e.message))
        .unwrap_or_default();

    // Succeeded + we don't yet have a deployment → provision one before
    // flipping status to `succeeded`. The route layer surfaces a job as
    // "still running" until deployment_name is populated; this keeps the
    // contract: `succeeded` ⇒ deployable.
    if new_status == "succeeded" && current.deployment_name.is_empty() {
        return PollerAction::DeployThenUpdate {
            deployment_name: generate_deployment_name(&current.job_id),
            fine_tuned_model: azure.fine_tuned_model.clone(),
        };
    }

    // No-op short circuit: same status, no new error, no new fine-tuned-model
    // disclosure to apply.
    if new_status == current.status
        && error_message == current.error_message
        && azure.fine_tuned_model == current.fine_tuned_model
    {
        return PollerAction::NoOp;
    }

    PollerAction::UpdateStatus {
        new_status,
        error_message,
        fine_tuned_model: azure.fine_tuned_model.clone(),
        terminal,
    }
}

/// Build a deployment name for a fine-tuned model. Stable per `job_id` so the
/// poller is idempotent — re-running the same job's deploy step produces the
/// same name (Azure returns 200 on PUT for an existing deployment).
pub(crate) fn generate_deployment_name(job_id: &str) -> String {
    let suffix: String = job_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(DEPLOYMENT_NAME_SUFFIX_LEN)
        .collect::<String>()
        .to_lowercase();
    format!("{DEPLOYMENT_NAME_PREFIX}{suffix}")
}

/// Apply the decided action. Best-effort — any failure logs and returns
/// without touching the row, so the next tick will retry. When `publisher`
/// is `Some`, emits an `mp.v1.finetune.{org_id}.{event}` NATS event on
/// each transition so subscribers (Convex projector, UI) wake immediately.
async fn apply_action(
    azure: &AzureFinetuneClient,
    client: &mut FinetuneJobsClient<Channel>,
    publisher: Option<&DynPublisher>,
    row: &pb::FinetuneJob,
    action: PollerAction,
) {
    match action {
        PollerAction::NoOp => {}
        PollerAction::UpdateStatus {
            new_status,
            error_message,
            fine_tuned_model,
            terminal,
        } => {
            apply_update_status(
                client,
                publisher,
                row,
                &new_status,
                &error_message,
                &fine_tuned_model,
                terminal,
            )
            .await;
        }
        PollerAction::DeployThenUpdate {
            deployment_name,
            fine_tuned_model,
        } => {
            apply_deploy_then_update(
                azure,
                client,
                publisher,
                row,
                &deployment_name,
                &fine_tuned_model,
            )
            .await;
        }
    }
}

/// Persist a non-deploy status transition and emit the matching lifecycle event.
async fn apply_update_status(
    client: &mut FinetuneJobsClient<Channel>,
    publisher: Option<&DynPublisher>,
    row: &pb::FinetuneJob,
    new_status: &str,
    error_message: &str,
    fine_tuned_model: &str,
    terminal: bool,
) {
    let event_type = match new_status {
        "failed" => mp_events::subjects::FINETUNE_EVENT_FAILED,
        "cancelled" => mp_events::subjects::FINETUNE_EVENT_CANCELLED,
        _ => mp_events::subjects::FINETUNE_EVENT_TRANSITIONED,
    };
    if let Err(e) = client
        .update_job_status(pb::UpdateFinetuneJobStatusRequest {
            job_id: row.job_id.clone(),
            org_id: row.org_id.clone(),
            status: new_status.to_owned(),
            error_message: error_message.to_owned(),
            fine_tuned_model: fine_tuned_model.to_owned(),
            deployment_name: String::new(),
            actual_cost_usd: 0.0,
            set_completed: terminal,
            // Empty = leave the persisted tier unchanged; a status-only
            // transition never touches the deployment SKU.
            deployment_tier: String::new(),
        })
        .await
    {
        warn!(error = %e, job_id = %row.job_id, "poller update_job_status failed");
        return;
    }
    if let Some(p) = publisher {
        publish_finetune_event(
            p,
            &row.org_id,
            "system",
            &row.job_id,
            event_type,
            json!({
                "job_id": row.job_id,
                "status": new_status,
                "error_message": error_message,
                "fine_tuned_model": fine_tuned_model,
                "terminal": terminal,
            }),
        )
        .await;
    }
}

/// Provision the deployment first, then flip the job to `succeeded`, emitting
/// the `deployed` and `succeeded` lifecycle events on success.
async fn apply_deploy_then_update(
    azure: &AzureFinetuneClient,
    client: &mut FinetuneJobsClient<Channel>,
    publisher: Option<&DynPublisher>,
    row: &pb::FinetuneJob,
    deployment_name: &str,
    fine_tuned_model: &str,
) {
    // Provision deployment first — if it fails the status flip
    // doesn't happen and the next tick retries. Operators can
    // inspect via Azure portal or the gateway log.
    //
    // Auto-deploys always land on the **Developer** tier: $0/hr hosting that
    // auto-deletes after 24h, so a fine-tune candidate can never silently rack
    // up the hourly hosting charge. Promoting to the paid production tier is an
    // explicit, separate operator action (POST /v1/finetune/jobs/:id/deploy).
    if let Err(e) = azure
        .create_deployment(deployment_name, fine_tuned_model, DeploymentTier::Developer)
        .await
    {
        warn!(
            error = %e,
            job_id = %row.job_id,
            deployment_name = %deployment_name,
            "poller create_deployment failed; will retry next tick"
        );
        return;
    }

    if let Err(e) = client
        .update_job_status(pb::UpdateFinetuneJobStatusRequest {
            job_id: row.job_id.clone(),
            org_id: row.org_id.clone(),
            status: "succeeded".to_owned(),
            error_message: String::new(),
            fine_tuned_model: fine_tuned_model.to_owned(),
            deployment_name: deployment_name.to_owned(),
            actual_cost_usd: 0.0,
            set_completed: true,
            // Auto-deploys always land on the free Developer tier.
            deployment_tier: DeploymentTier::Developer.as_str().to_owned(),
        })
        .await
    {
        warn!(error = %e, job_id = %row.job_id, "poller update_job_status (succeeded) failed");
        return;
    }
    if let Some(p) = publisher {
        // Two events on succeed: `deployed` (deployment_name set) +
        // `succeeded` (terminal status). Subscribers interested in
        // one or the other can filter; the loose coupling is the
        // point.
        publish_finetune_event(
            p,
            &row.org_id,
            "system",
            &row.job_id,
            mp_events::subjects::FINETUNE_EVENT_DEPLOYED,
            json!({
                "job_id": row.job_id,
                "deployment_name": deployment_name,
                "fine_tuned_model": fine_tuned_model,
            }),
        )
        .await;
        publish_finetune_event(
            p,
            &row.org_id,
            "system",
            &row.job_id,
            mp_events::subjects::FINETUNE_EVENT_SUCCEEDED,
            json!({
                "job_id": row.job_id,
                "status": "succeeded",
                "deployment_name": deployment_name,
                "fine_tuned_model": fine_tuned_model,
            }),
        )
        .await;
    }
}

/// One pass over all active jobs. Public for tests; the spawn entry point is
/// [`run`]. When `publisher` is `Some`, emits NATS lifecycle events on
/// transitions.
pub(crate) async fn tick(
    azure: &AzureFinetuneClient,
    client: &mut FinetuneJobsClient<Channel>,
    publisher: Option<&DynPublisher>,
) -> Result<usize, tonic::Status> {
    let resp = client
        .list_active_jobs(pb::ListActiveFinetuneJobsRequest { limit: 0 })
        .await?
        .into_inner();

    let count = resp.jobs.len();
    for row in resp.jobs {
        // Refresh from Azure. On failure: log + skip this row, try the next.
        let azure_status = match azure.get_finetune_job(&row.azure_job_id).await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, job_id = %row.job_id, azure_job_id = %row.azure_job_id, "azure get failed");
                continue;
            }
        };
        let action = decide_action(&row, &azure_status);
        apply_action(azure, client, publisher, &row, action).await;
    }

    Ok(count)
}

/// Top-level spawn target. Loops every `FINETUNE_POLLER_INTERVAL_SECS`
/// (default 60). Returns only on unrecoverable error from the tonic channel
/// itself; transient failures are absorbed inside [`tick`].
///
/// `publisher` is `Arc<DynPublisher>` so the poller shares the same NATS
/// connection the routes use — no duplicate connections, no separate
/// configuration surface.
///
/// # Errors
///
/// Returns an error only on an unrecoverable failure of the tonic channel itself;
/// transient per-tick failures are absorbed inside [`tick`].
pub async fn run(
    azure: AzureFinetuneClient,
    mut client: FinetuneJobsClient<Channel>,
    publisher: Arc<DynPublisher>,
) -> anyhow::Result<()> {
    let interval_secs = std::env::var(INTERVAL_ENV)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_INTERVAL_SECS);
    let mut tick_timer = tokio::time::interval(Duration::from_secs(interval_secs));
    tick_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    info!(interval_secs, "finetune poller started");

    // Track last-seen error signature so a flapping downstream (e.g.
    // session-core not yet up in dev) doesn't fill the log with
    // identical warnings every interval. First observation: WARN with
    // full context. Identical follow-ups: DEBUG (with a count). First
    // success after errors: INFO with the recovery summary so the
    // operator sees both the original error and that things healed.
    let mut last_err_signature: Option<String> = None;
    let mut suppressed_count: u64 = 0;

    loop {
        tick_timer.tick().await;

        // Honour the runtime kill switch — if an operator flips the flag off
        // mid-deploy we stop poking Azure on the next tick.
        if std::env::var(FEATURE_ENV).ok().as_deref() != Some("1") {
            continue;
        }

        let started = Instant::now();
        match tick(&azure, &mut client, Some(publisher.as_ref())).await {
            Ok(n) => {
                metrics::counter!(
                    "mp_gateway_finetune_poller_ticks_total",
                    "status" => "ok",
                )
                .increment(1);
                metrics::histogram!("mp_gateway_finetune_poller_tick_duration_seconds")
                    .record(started.elapsed().as_secs_f64());
                if let Some(prior) = last_err_signature.take() {
                    info!(
                        recovered_from = %prior,
                        suppressed_warnings = suppressed_count,
                        "finetune poller recovered"
                    );
                    suppressed_count = 0;
                }
                if n > 0 {
                    info!(
                        jobs_polled = n,
                        elapsed_ms =
                            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                        "finetune poller tick"
                    );
                }
            }
            Err(e) => {
                metrics::counter!(
                    "mp_gateway_finetune_poller_ticks_total",
                    "status" => "error",
                )
                .increment(1);
                // Coarse signature so request-id jitter doesn't break
                // dedup. gRPC code + first line of the message is the
                // identity of the failure mode.
                let signature = format!(
                    "{:?}:{}",
                    e.code(),
                    e.message().lines().next().unwrap_or("")
                );
                if last_err_signature.as_deref() == Some(signature.as_str()) {
                    suppressed_count += 1;
                    tracing::debug!(
                        error = %e,
                        suppressed = suppressed_count,
                        "finetune poller tick failed (suppressed; same signature)"
                    );
                } else {
                    warn!(error = %e, "finetune poller tick failed");
                    last_err_signature = Some(signature);
                    suppressed_count = 0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finetune_azure::AzureJobError;

    fn row(status: &str, deployment: &str, fine_tuned: &str, err: &str) -> pb::FinetuneJob {
        pb::FinetuneJob {
            job_id: "01HVAB12CDEF34567890123456".to_owned(),
            org_id: "org".into(),
            agent_id: "ag".into(),
            base_model: "gpt-4o-mini".into(),
            azure_file_id: "file-1".into(),
            azure_job_id: "ftjob-1".into(),
            fine_tuned_model: fine_tuned.to_owned(),
            deployment_name: deployment.to_owned(),
            deployment_tier: "developer".to_owned(),
            status: status.to_owned(),
            error_message: err.to_owned(),
            created_at: None,
            updated_at: None,
            completed_at: None,
            hyperparameters_json: String::new(),
            training_example_count: 0,
            estimated_cost_usd: 0.0,
            actual_cost_usd: 0.0,
            created_by: String::new(),
        }
    }

    fn azure(status: &str, fine_tuned: &str, err: Option<(&str, &str)>) -> AzureJobStatus {
        AzureJobStatus {
            id: "ftjob-1".into(),
            status: status.to_owned(),
            fine_tuned_model: fine_tuned.to_owned(),
            error: err.map(|(c, m)| AzureJobError {
                code: c.to_owned(),
                message: m.to_owned(),
            }),
        }
    }

    #[test]
    fn no_op_when_state_matches_and_no_error_and_no_new_fine_tuned_model() {
        let r = row("running", "", "", "");
        let a = azure("running", "", None);
        assert_eq!(decide_action(&r, &a), PollerAction::NoOp);
    }

    #[test]
    fn validating_files_collapses_to_running_noop() {
        // Azure says validating_files; we already locally call that "running".
        let r = row("running", "", "", "");
        let a = azure("validating_files", "", None);
        assert_eq!(decide_action(&r, &a), PollerAction::NoOp);
    }

    #[test]
    fn queued_to_running_transition_emits_update() {
        let r = row("queued", "", "", "");
        let a = azure("running", "", None);
        match decide_action(&r, &a) {
            PollerAction::UpdateStatus {
                new_status,
                terminal,
                error_message,
                fine_tuned_model,
            } => {
                assert_eq!(new_status, "running");
                assert!(!terminal);
                assert_eq!(error_message, "");
                assert_eq!(fine_tuned_model, "");
            }
            other => panic!("expected UpdateStatus, got {other:?}"),
        }
    }

    #[test]
    fn failed_with_error_message_carries_to_local_row() {
        let r = row("running", "", "", "");
        let a = azure(
            "failed",
            "",
            Some(("invalid_training_file", "too few rows")),
        );
        match decide_action(&r, &a) {
            PollerAction::UpdateStatus {
                new_status,
                terminal,
                error_message,
                ..
            } => {
                assert_eq!(new_status, "failed");
                assert!(terminal);
                assert_eq!(error_message, "invalid_training_file: too few rows");
            }
            other => panic!("expected UpdateStatus failed, got {other:?}"),
        }
    }

    #[test]
    fn succeeded_without_deployment_triggers_deploy_then_update() {
        let r = row("running", "", "", "");
        let a = azure("succeeded", "gpt-4o-mini.ft-xyz", None);
        match decide_action(&r, &a) {
            PollerAction::DeployThenUpdate {
                deployment_name,
                fine_tuned_model,
            } => {
                assert!(deployment_name.starts_with("ft-"));
                assert_eq!(fine_tuned_model, "gpt-4o-mini.ft-xyz");
            }
            other => panic!("expected DeployThenUpdate, got {other:?}"),
        }
    }

    #[test]
    fn succeeded_with_existing_deployment_is_noop() {
        // Idempotency: once we've stamped deployment_name, subsequent ticks
        // shouldn't redeploy or update.
        let r = row("succeeded", "ft-existing", "gpt-4o-mini.ft-xyz", "");
        let a = azure("succeeded", "gpt-4o-mini.ft-xyz", None);
        assert_eq!(decide_action(&r, &a), PollerAction::NoOp);
    }

    #[test]
    fn cancelled_transition_is_terminal_update() {
        let r = row("running", "", "", "");
        let a = azure("cancelled", "", None);
        match decide_action(&r, &a) {
            PollerAction::UpdateStatus {
                new_status,
                terminal,
                ..
            } => {
                assert_eq!(new_status, "cancelled");
                assert!(terminal);
            }
            other => panic!("expected UpdateStatus cancelled, got {other:?}"),
        }
    }

    #[test]
    fn generate_deployment_name_is_stable_for_same_job_id() {
        let a = generate_deployment_name("01HVAB12CDEF34567890123456");
        let b = generate_deployment_name("01HVAB12CDEF34567890123456");
        assert_eq!(a, b);
        assert!(a.starts_with("ft-"));
    }

    #[test]
    fn generate_deployment_name_strips_non_alphanumeric_lowercase() {
        // Even if a job_id ever contains separators, the resulting name is
        // Azure-safe (alphanumeric + hyphen only).
        let name = generate_deployment_name("01HV-AB-12_CDEF");
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        assert!(name.starts_with("ft-"));
        // Confirm the suffix capped at expected length (12 chars after prefix).
        assert!(name.len() <= DEPLOYMENT_NAME_PREFIX.len() + DEPLOYMENT_NAME_SUFFIX_LEN);
    }

    #[test]
    fn generate_deployment_name_fits_azure_limit() {
        // Azure deployment-name max is 64 chars. Our derived name must
        // always be well under that, even for the longest plausible job_id.
        let job_id = "X".repeat(64);
        let name = generate_deployment_name(&job_id);
        assert!(name.len() < 64);
    }
}
