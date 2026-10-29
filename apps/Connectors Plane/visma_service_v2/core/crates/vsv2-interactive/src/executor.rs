use std::sync::Arc;

use chrono::{Duration, Utc};
use serde::Serialize;
use uuid::Uuid;
use vsv2_common::text::printable;
use vsv2_common::TenantId;

use crate::audit::{AuditEntry, AuditSink, AuditStage};
use crate::error::{InteractiveError, Result};
use crate::operation::OperationRegistry;
use crate::plan::{ConfirmSigner, Plan};
use crate::policy::WritePolicy;
use crate::request::WriteMethod;
use crate::transport::{Reader, Transport, WriteResponse};

/// How long a reviewed plan stays executable. The ETag it carries goes stale anyway.
pub const DEFAULT_PLAN_TTL_SECS: i64 = 600;
const AUDIT_DETAIL_MAX_CHARS: usize = 2_000;

#[derive(Debug, Clone, Serialize)]
#[must_use]
pub struct DryRun {
    pub plan: Plan,
    pub confirm_token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Executed {
    pub plan_id: Uuid,
    pub status: u16,
    pub body: String,
    pub etag: Option<String>,
    /// The `Location` header of the answer: where a created record can be read.
    pub location: Option<String>,
    /// The write succeeded but its outcome row could not be written to the audit log.
    pub audit_degraded: bool,
}

fn base_entry(caller: &str, tenant: &TenantId, op: &str, stage: AuditStage) -> AuditEntry {
    AuditEntry {
        audit_id: Uuid::new_v4(),
        plan_id: None,
        tenant_id: tenant.to_string(),
        operation: op.to_string(),
        caller: caller.to_string(),
        stage,
        target: None,
        method: None,
        path: None,
        payload_hash: None,
        payload: None,
        http_status: None,
        detail: None,
        created_at: Utc::now(),
    }
}

fn plan_entry(caller: &str, plan: &Plan, stage: AuditStage) -> AuditEntry {
    AuditEntry {
        plan_id: Some(plan.plan_id),
        target: Some(plan.request.target.as_str().into()),
        method: Some(plan.request.method.as_str().into()),
        path: Some(plan.request.path.clone()),
        payload_hash: Some(plan.payload_hash()),
        ..base_entry(caller, &plan.tenant, &plan.operation, stage)
    }
}

/// Best effort: a terminal audit row that cannot be written must not change what the caller is
/// told about a write that already happened. Returns whether the row was written.
async fn record_terminal(sink: &Arc<dyn AuditSink>, entry: &AuditEntry) -> bool {
    match sink.record(entry).await {
        Ok(()) => true,
        Err(e) => {
            tracing::error!(error = %e, stage = entry.stage.as_str(), plan_id = ?entry.plan_id,
                "AUDIT DEGRADED: could not record the outcome of a write");
            false
        }
    }
}

/// A create that was answered with a gateway or server error may have been applied before the error: it is not
/// known to have failed. Other writes carry the order's version, so repeating them cannot apply them twice.
fn outcome_is_unclear(request: &crate::request::WriteRequest, status: u16) -> bool {
    request.method == WriteMethod::Post && matches!(status, 408 | 500 | 502 | 503 | 504)
}

/// Send and record the outcome. Runs detached from the caller (see [`Executor::execute`]).
async fn send_and_finish(
    transport: Arc<dyn Transport>,
    sink: Arc<dyn AuditSink>,
    caller: String,
    plan: Plan,
) -> Result<Executed> {
    match transport.send(&plan.tenant, &plan.request).await {
        Ok(WriteResponse {
            status,
            body,
            etag,
            location,
        }) if (200..300).contains(&status) => {
            let mut e = plan_entry(&caller, &plan, AuditStage::Succeeded);
            e.http_status = Some(status);
            let located = location
                .as_deref()
                .map(|l| format!("location: {}; ", printable(l, 300)))
                .unwrap_or_default();
            e.detail = Some(printable(
                &format!("{located}{body}"),
                AUDIT_DETAIL_MAX_CHARS,
            ));
            let audited = record_terminal(&sink, &e).await;
            Ok(Executed {
                plan_id: plan.plan_id,
                status,
                body,
                etag,
                location,
                audit_degraded: !audited,
            })
        }
        Ok(WriteResponse { status, body, .. }) if outcome_is_unclear(&plan.request, status) => {
            let shown = printable(&body, AUDIT_DETAIL_MAX_CHARS);
            let mut e = plan_entry(&caller, &plan, AuditStage::Unknown);
            e.http_status = Some(status);
            e.detail = Some(shown.clone());
            record_terminal(&sink, &e).await;
            Err(InteractiveError::OutcomeUnknown(format!(
                "Visma answered {status} to the request, which does not say whether the record was created ({})",
                printable(&shown, 200)
            )))
        }
        Ok(WriteResponse { status, body, .. }) => {
            let mut e = plan_entry(&caller, &plan, AuditStage::Failed);
            e.http_status = Some(status);
            e.detail = Some(printable(&body, AUDIT_DETAIL_MAX_CHARS));
            record_terminal(&sink, &e).await;
            Err(InteractiveError::Upstream {
                status,
                body: printable(&body, AUDIT_DETAIL_MAX_CHARS),
            })
        }
        // The request never left (no token, no scope, rate limit): the write certainly did not happen.
        Err(InteractiveError::NotSent(why)) => {
            let mut e = plan_entry(&caller, &plan, AuditStage::Failed);
            e.detail = Some(printable(
                &format!("not sent: {why}"),
                AUDIT_DETAIL_MAX_CHARS,
            ));
            record_terminal(&sink, &e).await;
            Err(InteractiveError::NotSent(printable(&why, 300)))
        }
        // No HTTP answer: a timeout or reset after the body left may still have applied the write.
        Err(err) => {
            let mut e = plan_entry(&caller, &plan, AuditStage::Unknown);
            e.detail = Some(printable(&err.to_string(), AUDIT_DETAIL_MAX_CHARS));
            record_terminal(&sink, &e).await;
            Err(InteractiveError::OutcomeUnknown(printable(
                &err.to_string(),
                300,
            )))
        }
    }
}

pub struct Executor {
    registry: OperationRegistry,
    policy: WritePolicy,
    signer: ConfirmSigner,
    sink: Arc<dyn AuditSink>,
    transport: Arc<dyn Transport>,
    reader: Arc<dyn Reader>,
    plan_ttl: Duration,
}

impl Executor {
    pub fn new(
        registry: OperationRegistry,
        policy: WritePolicy,
        signer: ConfirmSigner,
        sink: Arc<dyn AuditSink>,
        transport: Arc<dyn Transport>,
        reader: Arc<dyn Reader>,
    ) -> Self {
        Self {
            registry,
            policy,
            signer,
            sink,
            transport,
            reader,
            plan_ttl: Duration::seconds(DEFAULT_PLAN_TTL_SECS),
        }
    }

    pub fn with_plan_ttl(mut self, ttl: Duration) -> Self {
        self.plan_ttl = ttl;
        self
    }

    pub fn operations(&self) -> Vec<&'static str> {
        self.registry.names()
    }

    async fn deny(
        &self,
        caller: &str,
        tenant: &TenantId,
        op: &str,
        why: InteractiveError,
    ) -> InteractiveError {
        let mut e = base_entry(caller, tenant, op, AuditStage::Denied);
        e.detail = Some(why.to_string());
        if let Err(audit_err) = self.sink.record(&e).await {
            tracing::error!(error = %audit_err, "could not audit a denied write");
        }
        why
    }

    /// Step 1: build, validate, audit and return the plan. Sends nothing.
    pub async fn dry_run(
        &self,
        caller: &str,
        tenant: &TenantId,
        operation: &str,
        input: &serde_json::Value,
    ) -> Result<DryRun> {
        if let Err(why) = self.policy.check(tenant, operation) {
            return Err(self.deny(caller, tenant, operation, why).await);
        }
        let op = self.registry.get(operation)?;
        let built = op.build(tenant, self.reader.as_ref(), input).await?;
        built.request.validate()?;
        // Refuse here what could not be sent at all (the credentials lack the scope), before anyone is asked.
        if let Err(why) = self.transport.preflight(tenant, &built.request).await {
            return Err(self.deny(caller, tenant, operation, why).await);
        }

        let plan = Plan {
            plan_id: Uuid::new_v4(),
            operation: operation.to_string(),
            tenant: tenant.clone(),
            request: built.request,
            summary: built.summary,
            created_at: Utc::now(),
        };
        let mut e = plan_entry(caller, &plan, AuditStage::DryRun);
        e.payload = Some(plan.request.body.clone());
        e.detail = Some(plan.summary.clone());
        self.sink.record(&e).await?;

        let confirm_token = self.signer.sign(&plan);
        Ok(DryRun {
            plan,
            confirm_token,
        })
    }

    /// Everything that can be checked without side effects: token, policy, expiry, path.
    /// `execute` runs this itself; callers may run it first to avoid asking a human to approve a
    /// plan that could not be executed anyway. Denials are audited.
    pub async fn preflight(&self, caller: &str, plan: &Plan, confirm_token: &str) -> Result<()> {
        let refusal = if !self.signer.verify(plan, confirm_token) {
            Some(InteractiveError::TokenInvalid)
        } else if let Err(why) = self.policy.check(&plan.tenant, &plan.operation) {
            Some(why)
        } else {
            let age = Utc::now() - plan.created_at;
            (age > self.plan_ttl).then(|| InteractiveError::PlanExpired {
                age_secs: age.num_seconds(),
                ttl_secs: self.plan_ttl.num_seconds(),
            })
        };
        if let Some(why) = refusal {
            return Err(self.deny(caller, &plan.tenant, &plan.operation, why).await);
        }
        plan.request.validate()?;
        if self.sink.was_attempted(plan.plan_id).await? {
            let why = InteractiveError::AlreadyExecuted(plan.plan_id.to_string());
            return Err(self.deny(caller, &plan.tenant, &plan.operation, why).await);
        }
        Ok(())
    }

    /// Step 2: send a previously reviewed plan. Audit-before-send; at most once per plan.
    ///
    /// The `executing` audit row doubles as an atomic claim (a second concurrent execute of the
    /// same plan fails with `AlreadyExecuted`). The send and the outcome row then run in a detached
    /// task: if the caller is cancelled mid-flight the write still completes and is recorded, and
    /// a failure to record a 2xx outcome is reported as `audit_degraded`, never as a failed write.
    pub async fn execute(
        &self,
        caller: &str,
        plan: &Plan,
        confirm_token: &str,
    ) -> Result<Executed> {
        self.preflight(caller, plan, confirm_token).await?;

        let mut pre = plan_entry(caller, plan, AuditStage::Executing);
        pre.payload = Some(plan.request.body.clone());
        if let Err(e) = self.sink.record(&pre).await {
            // Lost the claim to a concurrent execute: that is a denial, not an audit outage.
            return Err(match e {
                InteractiveError::AlreadyExecuted(_) => {
                    self.deny(caller, &plan.tenant, &plan.operation, e).await
                }
                other => other,
            });
        }

        let task = tokio::spawn(send_and_finish(
            self.transport.clone(),
            self.sink.clone(),
            caller.to_string(),
            plan.clone(),
        ));
        match task.await {
            Ok(res) => res,
            Err(join) => Err(InteractiveError::OutcomeUnknown(format!(
                "the send task ended abnormally ({join})"
            ))),
        }
    }
}
