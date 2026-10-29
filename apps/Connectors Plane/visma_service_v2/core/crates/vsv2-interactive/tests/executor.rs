use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Duration;
use serde_json::{json, Value};
use uuid::Uuid;
use vsv2_common::TenantId;
use vsv2_interactive::*;

const TENANT: &str = "t-test";
const OP: &str = "test.update";

/// Reads an ETag via the reader, then builds a PUT with If-Match.
struct UpdateOp;

#[async_trait]
impl Operation for UpdateOp {
    fn name(&self) -> &'static str {
        OP
    }
    async fn build(&self, tenant: &TenantId, reader: &dyn Reader, input: &Value) -> Result<Built> {
        let id = input["id"]
            .as_str()
            .ok_or_else(|| InteractiveError::InvalidInput("id required".into()))?;
        let path = format!("/v3/salesorder/{id}");
        let current = reader.get(tenant, WriteTarget::SalesOrder, &path).await?;
        Ok(Built {
            request: WriteRequest {
                target: WriteTarget::SalesOrder,
                method: WriteMethod::Put,
                path,
                body: json!({"qty": input["qty"]}),
                if_match: current.etag,
            },
            summary: format!("set qty of {id}"),
        })
    }
}

const CREATE_OP: &str = "test.create";

/// Builds a POST with no version, like creating a record.
struct CreateOp;

#[async_trait]
impl Operation for CreateOp {
    fn name(&self) -> &'static str {
        CREATE_OP
    }
    async fn build(&self, _: &TenantId, _: &dyn Reader, _: &Value) -> Result<Built> {
        Ok(Built {
            request: WriteRequest {
                target: WriteTarget::SalesOrder,
                method: WriteMethod::Post,
                path: "/v3/salesorder".into(),
                body: json!({"customer": "C-1"}),
                if_match: None,
            },
            summary: "create an order".into(),
        })
    }
}

struct FakeReader;
#[async_trait]
impl Reader for FakeReader {
    async fn get(&self, _: &TenantId, _: WriteTarget, _: &str) -> Result<ReadResponse> {
        Ok(ReadResponse {
            status: 200,
            body: "{}".into(),
            etag: Some("\"e1\"".into()),
        })
    }
}

#[derive(Default)]
struct FakeTransport {
    sent: Mutex<Vec<WriteRequest>>,
    calls: AtomicUsize,
    status: Mutex<u16>,
}
impl FakeTransport {
    fn with_status(s: u16) -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new(s),
            ..Default::default()
        })
    }
}
#[async_trait]
impl Transport for FakeTransport {
    async fn send(&self, _: &TenantId, req: &WriteRequest) -> Result<WriteResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.sent.lock().unwrap().push(req.clone());
        let status = *self.status.lock().unwrap();
        Ok(WriteResponse {
            status,
            body: format!("resp-{status}"),
            etag: Some("\"e2\"".into()),
            location: None,
        })
    }
}

/// Fails on the nth record call (1-based), to prove "no audit row, no write".
struct FlakySink {
    inner: MemoryAuditSink,
    fail_on_stage: AuditStage,
}
#[async_trait]
impl AuditSink for FlakySink {
    async fn record(&self, e: &AuditEntry) -> Result<()> {
        if e.stage == self.fail_on_stage {
            return Err(InteractiveError::Audit("disk full".into()));
        }
        self.inner.record(e).await
    }
    async fn was_attempted(&self, id: Uuid) -> Result<bool> {
        self.inner.was_attempted(id).await
    }
}

struct Rig {
    exec: Executor,
    sink: Arc<MemoryAuditSink>,
    transport: Arc<FakeTransport>,
}

fn signer() -> ConfirmSigner {
    ConfirmSigner::new(b"unit-test-confirm-secret-0123".to_vec()).unwrap()
}

fn rig(policy: WritePolicy, status: u16) -> Rig {
    let sink = Arc::new(MemoryAuditSink::default());
    let transport = FakeTransport::with_status(status);
    let exec = Executor::new(
        OperationRegistry::new().with(UpdateOp),
        policy,
        signer(),
        sink.clone(),
        transport.clone(),
        Arc::new(FakeReader),
    );
    Rig {
        exec,
        sink,
        transport,
    }
}

fn allow() -> WritePolicy {
    WritePolicy::new([TENANT], [OP])
}

fn tenant() -> TenantId {
    TenantId::new(TENANT)
}

#[tokio::test]
async fn dry_run_builds_the_request_with_etag_audits_and_sends_nothing() {
    let r = rig(allow(), 200);
    let d = r
        .exec
        .dry_run("alice", &tenant(), OP, &json!({"id":"SO1","qty":3}))
        .await
        .unwrap();

    assert_eq!(d.plan.request.if_match.as_deref(), Some("\"e1\""));
    assert_eq!(d.plan.request.path, "/v3/salesorder/SO1");
    assert_eq!(r.transport.calls.load(Ordering::SeqCst), 0);
    assert_eq!(r.sink.stages(), vec![AuditStage::DryRun]);
    assert_eq!(r.sink.entries()[0].caller, "alice");
}

#[tokio::test]
async fn full_cycle_audits_dry_run_then_executing_then_succeeded() {
    let r = rig(allow(), 200);
    let d = r
        .exec
        .dry_run("alice", &tenant(), OP, &json!({"id":"SO1","qty":3}))
        .await
        .unwrap();
    let out = r
        .exec
        .execute("alice", &d.plan, &d.confirm_token)
        .await
        .unwrap();

    assert_eq!(out.status, 200);
    assert_eq!(r.transport.sent.lock().unwrap()[0], d.plan.request);
    assert_eq!(
        r.sink.stages(),
        vec![
            AuditStage::DryRun,
            AuditStage::Executing,
            AuditStage::Succeeded
        ]
    );
}

#[tokio::test]
async fn a_plan_can_only_be_executed_once() {
    let r = rig(allow(), 200);
    let d = r
        .exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":3}))
        .await
        .unwrap();
    r.exec
        .execute("a", &d.plan, &d.confirm_token)
        .await
        .unwrap();

    let again = r.exec.execute("a", &d.plan, &d.confirm_token).await;
    assert!(matches!(again, Err(InteractiveError::AlreadyExecuted(_))));
    assert_eq!(r.transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_failed_attempt_also_blocks_replay_because_the_write_may_have_landed() {
    let r = rig(allow(), 500);
    let d = r
        .exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":3}))
        .await
        .unwrap();
    assert!(r
        .exec
        .execute("a", &d.plan, &d.confirm_token)
        .await
        .is_err());
    let again = r.exec.execute("a", &d.plan, &d.confirm_token).await;
    assert!(matches!(again, Err(InteractiveError::AlreadyExecuted(_))));
    assert_eq!(r.transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tampered_plan_is_rejected_and_nothing_is_sent() {
    let r = rig(allow(), 200);
    let d = r
        .exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":3}))
        .await
        .unwrap();
    let mut evil = d.plan.clone();
    evil.request.body = json!({"qty": 99999});

    let res = r.exec.execute("a", &evil, &d.confirm_token).await;
    assert!(matches!(res, Err(InteractiveError::TokenInvalid)));
    assert_eq!(r.transport.calls.load(Ordering::SeqCst), 0);
    assert_eq!(r.sink.stages().last(), Some(&AuditStage::Denied));
}

#[tokio::test]
async fn default_policy_denies_and_audits_the_denial() {
    let r = rig(WritePolicy::deny_all(), 200);
    let res = r
        .exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":1}))
        .await;
    assert!(matches!(res, Err(InteractiveError::Denied(_))));
    assert_eq!(r.sink.stages(), vec![AuditStage::Denied]);
    assert_eq!(r.transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn policy_is_rechecked_at_execute_so_a_revoked_tenant_cannot_use_an_old_plan() {
    let permissive = rig(allow(), 200);
    let d = permissive
        .exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":1}))
        .await
        .unwrap();

    // Same secret, but writes switched off since the plan was made.
    let strict = rig(WritePolicy::deny_all(), 200);
    let res = strict.exec.execute("a", &d.plan, &d.confirm_token).await;
    assert!(matches!(res, Err(InteractiveError::Denied(_))));
    assert_eq!(strict.transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn expired_plans_are_refused() {
    let sink = Arc::new(MemoryAuditSink::default());
    let transport = FakeTransport::with_status(200);
    let exec = Executor::new(
        OperationRegistry::new().with(UpdateOp),
        allow(),
        signer(),
        sink,
        transport.clone(),
        Arc::new(FakeReader),
    )
    .with_plan_ttl(Duration::seconds(-1));
    let d = exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":1}))
        .await
        .unwrap();

    let res = exec.execute("a", &d.plan, &d.confirm_token).await;
    assert!(matches!(res, Err(InteractiveError::PlanExpired { .. })));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn upstream_412_is_audited_as_failed_and_surfaced() {
    let r = rig(allow(), 412);
    let d = r
        .exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":1}))
        .await
        .unwrap();
    let res = r.exec.execute("a", &d.plan, &d.confirm_token).await;

    assert!(matches!(
        res,
        Err(InteractiveError::Upstream { status: 412, .. })
    ));
    let last = r.sink.entries().pop().unwrap();
    assert_eq!(
        (last.stage, last.http_status),
        (AuditStage::Failed, Some(412))
    );
}

#[tokio::test]
async fn if_the_pre_send_audit_fails_nothing_is_sent() {
    let flaky = Arc::new(FlakySink {
        inner: MemoryAuditSink::default(),
        fail_on_stage: AuditStage::Executing,
    });
    let transport = FakeTransport::with_status(200);
    let exec = Executor::new(
        OperationRegistry::new().with(UpdateOp),
        allow(),
        signer(),
        flaky,
        transport.clone(),
        Arc::new(FakeReader),
    );
    let d = exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":1}))
        .await
        .unwrap();

    let res = exec.execute("a", &d.plan, &d.confirm_token).await;
    assert!(matches!(res, Err(InteractiveError::Audit(_))));
    assert_eq!(
        transport.calls.load(Ordering::SeqCst),
        0,
        "no audit row, no write"
    );
}

#[tokio::test]
async fn unknown_operation_and_bad_input_are_clean_errors() {
    let r = rig(WritePolicy::new([TENANT], ["nope", OP]), 200);
    assert!(matches!(
        r.exec.dry_run("a", &tenant(), "nope", &json!({})).await,
        Err(InteractiveError::UnknownOperation(_))
    ));
    assert!(matches!(
        r.exec.dry_run("a", &tenant(), OP, &json!({})).await,
        Err(InteractiveError::InvalidInput(_))
    ));
}

// ---------------------------------------------------------------------------------------------
// Race, cancellation and outcome-reporting guarantees (from the 2026-10-09 code reviews)
// ---------------------------------------------------------------------------------------------

/// Takes `delay` before answering, so two executes can overlap.
struct SlowTransport {
    calls: AtomicUsize,
    delay: std::time::Duration,
}
#[async_trait]
impl Transport for SlowTransport {
    async fn send(&self, _: &TenantId, _: &WriteRequest) -> Result<WriteResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        Ok(WriteResponse {
            status: 202,
            body: "".into(),
            etag: None,
            location: None,
        })
    }
}

/// Fails like a dropped connection: no HTTP answer at all.
struct BrokenTransport;
#[async_trait]
impl Transport for BrokenTransport {
    async fn send(&self, _: &TenantId, _: &WriteRequest) -> Result<WriteResponse> {
        Err(InteractiveError::Common(vsv2_common::Error::Http(
            "connection reset by peer".into(),
        )))
    }
}

fn executor_with(sink: Arc<dyn AuditSink>, transport: Arc<dyn Transport>) -> Executor {
    Executor::new(
        OperationRegistry::new().with(UpdateOp),
        allow(),
        signer(),
        sink,
        transport,
        Arc::new(FakeReader),
    )
}

#[tokio::test]
async fn two_concurrent_executes_of_one_plan_send_exactly_once() {
    let sink = Arc::new(MemoryAuditSink::default());
    let transport = Arc::new(SlowTransport {
        calls: AtomicUsize::new(0),
        delay: std::time::Duration::from_millis(50),
    });
    let exec = executor_with(sink.clone(), transport.clone());
    let d = exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":1}))
        .await
        .unwrap();

    let (r1, r2) = tokio::join!(
        exec.execute("a", &d.plan, &d.confirm_token),
        exec.execute("b", &d.plan, &d.confirm_token)
    );
    let outcomes = [r1.is_ok(), r2.is_ok()];
    assert_eq!(
        outcomes.iter().filter(|ok| **ok).count(),
        1,
        "exactly one wins: {outcomes:?}"
    );
    let loser = if r1.is_err() { r1 } else { r2 };
    assert!(
        matches!(loser, Err(InteractiveError::AlreadyExecuted(_))),
        "{loser:?}"
    );
    assert_eq!(
        transport.calls.load(Ordering::SeqCst),
        1,
        "the write was sent once"
    );
    let claims = sink
        .stages()
        .iter()
        .filter(|s| **s == AuditStage::Executing)
        .count();
    assert_eq!(claims, 1, "one claim row");
}

#[tokio::test]
async fn a_cancelled_caller_does_not_abandon_the_write_or_its_audit_row() {
    let sink = Arc::new(MemoryAuditSink::default());
    let transport = Arc::new(SlowTransport {
        calls: AtomicUsize::new(0),
        delay: std::time::Duration::from_millis(100),
    });
    let exec = executor_with(sink.clone(), transport.clone());
    let d = exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":1}))
        .await
        .unwrap();

    // The caller gives up after 20ms, mid-send.
    let cancelled = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        exec.execute("a", &d.plan, &d.confirm_token),
    )
    .await;
    assert!(cancelled.is_err(), "the caller was cancelled");

    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        sink.stages(),
        vec![
            AuditStage::DryRun,
            AuditStage::Executing,
            AuditStage::Succeeded
        ],
        "the outcome was still recorded"
    );
    // ...and the plan can not be replayed.
    assert!(matches!(
        exec.execute("a", &d.plan, &d.confirm_token).await,
        Err(InteractiveError::AlreadyExecuted(_))
    ));
}

#[tokio::test]
async fn a_failing_outcome_row_after_a_2xx_is_reported_as_success_with_a_degraded_flag() {
    let flaky = Arc::new(FlakySink {
        inner: MemoryAuditSink::default(),
        fail_on_stage: AuditStage::Succeeded,
    });
    let transport = FakeTransport::with_status(202);
    let exec = executor_with(flaky, transport.clone());
    let d = exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":1}))
        .await
        .unwrap();

    let out = exec
        .execute("a", &d.plan, &d.confirm_token)
        .await
        .expect("the write itself succeeded");
    assert!(
        out.audit_degraded,
        "the caller learns the audit row is missing"
    );
    assert_eq!(out.status, 202);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_transport_error_is_an_unknown_outcome_not_a_clean_failure_and_blocks_replay() {
    let sink = Arc::new(MemoryAuditSink::default());
    let exec = executor_with(sink.clone(), Arc::new(BrokenTransport));
    let d = exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":1}))
        .await
        .unwrap();

    let err = exec
        .execute("a", &d.plan, &d.confirm_token)
        .await
        .unwrap_err();
    assert!(
        matches!(err, InteractiveError::OutcomeUnknown(_)),
        "{err:?}"
    );
    assert!(err.to_string().contains("Verify the record"), "{err}");
    assert_eq!(sink.stages().last(), Some(&AuditStage::Unknown));
    assert!(matches!(
        exec.execute("a", &d.plan, &d.confirm_token).await,
        Err(InteractiveError::AlreadyExecuted(_))
    ));
}

#[tokio::test]
async fn preflight_refuses_bad_plans_without_claiming_them() {
    let sink = Arc::new(MemoryAuditSink::default());
    let transport = FakeTransport::with_status(202);
    let exec = executor_with(sink.clone(), transport.clone());
    let d = exec
        .dry_run("a", &tenant(), OP, &json!({"id":"SO1","qty":1}))
        .await
        .unwrap();

    assert!(exec.preflight("a", &d.plan, &d.confirm_token).await.is_ok());
    assert!(matches!(
        exec.preflight("a", &d.plan, "deadbeef").await,
        Err(InteractiveError::TokenInvalid)
    ));
    assert!(
        !sink.stages().contains(&AuditStage::Executing),
        "preflight never claims the plan"
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    // a successful preflight can still be followed by a normal execute
    assert!(exec.execute("a", &d.plan, &d.confirm_token).await.is_ok());
}

// ---------------------------------------------------------------------------------------------
// Review fixes (2026-10-14): where a created record is, what a failure before sending means, and
// how a gateway error to a create is reported.
// ---------------------------------------------------------------------------------------------

/// Answers every send with a fixed outcome; the plan-time check can be made to fail.
struct ScriptedTransport {
    answer: Mutex<Option<Result<WriteResponse>>>,
    preflight_refusal: Option<&'static str>,
    sent: AtomicUsize,
}
impl ScriptedTransport {
    fn answering(answer: Result<WriteResponse>) -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(Some(answer)),
            preflight_refusal: None,
            sent: AtomicUsize::new(0),
        })
    }
}
#[async_trait]
impl Transport for ScriptedTransport {
    async fn send(&self, _: &TenantId, _: &WriteRequest) -> Result<WriteResponse> {
        self.sent.fetch_add(1, Ordering::SeqCst);
        self.answer
            .lock()
            .unwrap()
            .take()
            .expect("one answer per test")
    }
    async fn preflight(&self, _: &TenantId, _: &WriteRequest) -> Result<()> {
        match self.preflight_refusal {
            Some(why) => Err(InteractiveError::Common(vsv2_common::Error::Auth(
                why.into(),
            ))),
            None => Ok(()),
        }
    }
}

fn create_executor(transport: Arc<ScriptedTransport>) -> (Executor, Arc<MemoryAuditSink>) {
    let sink = Arc::new(MemoryAuditSink::default());
    let exec = Executor::new(
        OperationRegistry::new().with(CreateOp).with(UpdateOp),
        WritePolicy::new([TENANT], [CREATE_OP, OP]),
        signer(),
        sink.clone(),
        transport,
        Arc::new(FakeReader),
    );
    (exec, sink)
}

fn response(status: u16, location: Option<&str>) -> Result<WriteResponse> {
    Ok(WriteResponse {
        status,
        body: r#"{"warnings":[]}"#.into(),
        etag: None,
        location: location.map(String::from),
    })
}

#[tokio::test]
async fn the_address_of_a_created_record_is_returned_and_audited() {
    let location = "/api/v3/SalesOrders/SO/000123";
    let (exec, sink) = create_executor(ScriptedTransport::answering(response(201, Some(location))));
    let d = exec
        .dry_run("ima", &tenant(), CREATE_OP, &json!({}))
        .await
        .unwrap();
    let done = exec
        .execute("ima", &d.plan, &d.confirm_token)
        .await
        .unwrap();
    assert_eq!(
        (done.status, done.location.as_deref()),
        (201, Some(location))
    );
    let last = sink.entries().pop().unwrap();
    assert_eq!(last.stage, AuditStage::Succeeded);
    assert!(
        last.detail.unwrap().contains(location),
        "the audit row says which record was created"
    );
}

#[tokio::test]
async fn a_failure_before_the_request_leaves_is_failed_not_unknown_and_the_plan_is_spent() {
    let (exec, sink) = create_executor(ScriptedTransport::answering(Err(
        InteractiveError::NotSent("no wire scope configured for SalesOrderWrite".into()),
    )));
    let d = exec
        .dry_run("ima", &tenant(), CREATE_OP, &json!({}))
        .await
        .unwrap();
    let e = exec
        .execute("ima", &d.plan, &d.confirm_token)
        .await
        .unwrap_err();
    assert!(matches!(e, InteractiveError::NotSent(_)), "{e}");
    assert!(!e.to_string().contains("outcome unknown"), "{e}");
    let stages = sink.stages();
    assert_eq!(stages.last(), Some(&AuditStage::Failed), "{stages:?}");
    assert!(!stages.contains(&AuditStage::Unknown));
    assert!(matches!(
        exec.execute("ima", &d.plan, &d.confirm_token).await,
        Err(InteractiveError::AlreadyExecuted(_))
    ));
}

#[tokio::test]
async fn a_gateway_error_to_a_create_may_have_created_the_record_but_to_an_edit_it_did_not_apply() {
    for status in [408, 500, 502, 503, 504] {
        let (exec, sink) = create_executor(ScriptedTransport::answering(response(status, None)));
        let d = exec
            .dry_run("ima", &tenant(), CREATE_OP, &json!({}))
            .await
            .unwrap();
        let e = exec
            .execute("ima", &d.plan, &d.confirm_token)
            .await
            .unwrap_err();
        assert!(
            matches!(e, InteractiveError::OutcomeUnknown(_)),
            "{status}: {e}"
        );
        assert_eq!(sink.stages().last(), Some(&AuditStage::Unknown), "{status}");
    }
    // A refusal with a client error says the record was not created.
    let (exec, sink) = create_executor(ScriptedTransport::answering(response(400, None)));
    let d = exec
        .dry_run("ima", &tenant(), CREATE_OP, &json!({}))
        .await
        .unwrap();
    assert!(matches!(
        exec.execute("ima", &d.plan, &d.confirm_token).await,
        Err(InteractiveError::Upstream { status: 400, .. })
    ));
    assert_eq!(sink.stages().last(), Some(&AuditStage::Failed));
    // An edit carries the record's version, so a repeat cannot apply it twice: a 502 stays a plain refusal.
    let (exec, sink) = create_executor(ScriptedTransport::answering(response(502, None)));
    let d = exec
        .dry_run("ima", &tenant(), OP, &json!({"id": "SO1", "qty": 1}))
        .await
        .unwrap();
    assert!(matches!(
        exec.execute("ima", &d.plan, &d.confirm_token).await,
        Err(InteractiveError::Upstream { status: 502, .. })
    ));
    assert_eq!(sink.stages().last(), Some(&AuditStage::Failed));
}

#[tokio::test]
async fn a_plan_the_credentials_could_not_send_is_refused_before_anybody_is_asked() {
    let transport = Arc::new(ScriptedTransport {
        answer: Mutex::new(None),
        preflight_refusal: Some(
            "the sign-in does not include the scope visma.net.erp.salesorder:write",
        ),
        sent: AtomicUsize::new(0),
    });
    let (exec, sink) = create_executor(transport.clone());
    let e = exec
        .dry_run("ima", &tenant(), CREATE_OP, &json!({}))
        .await
        .map(|_| ())
        .expect_err("refused");
    assert!(
        e.to_string().contains("visma.net.erp.salesorder:write"),
        "{e}"
    );
    assert_eq!(
        sink.stages(),
        vec![AuditStage::Denied],
        "refused and audited, no plan made"
    );
    assert_eq!(transport.sent.load(Ordering::SeqCst), 0);
}
