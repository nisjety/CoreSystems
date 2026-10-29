//! Write tools. Thin wrappers over the Layer 3 [`Executor`]: they add no power of their own.
//! Everything that makes a write safe (allow-list, audit-before-send, one-shot plans, ETag,
//! confirm token) lives in `vsv2-interactive`. This layer adds the human: in the default
//! [`ApprovalMode::Elicitation`] the *user* is asked through the MCP client, and the model cannot
//! answer for them.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use vsv2_interactive::executor::DEFAULT_PLAN_TTL_SECS;
use vsv2_interactive::{DryRun, Executor, Plan};

use crate::config::ApprovalMode;
use crate::tool::{parse_args, CallContext, Tool, ToolDef, ToolError, ToolKind};
use crate::visma::Tenants;

pub const PATCH_PLAN: &str = "visma_salesorder_patch_plan";
pub const PATCH_EXECUTE: &str = "visma_salesorder_patch_execute";
const OPERATION: &str = "salesorder.patch";
/// Longest request body the approval prompt shows. A body over it is not approved at all, since a prompt that
/// hid part of the request would not be an approval of the request.
const PROMPT_BODY_MAX_CHARS: usize = 8_000;

pub(super) fn failed(e: vsv2_interactive::InteractiveError) -> ToolError {
    ToolError::Failed(e.to_string())
}

/// What every plan tool returns: the summary, the signed plan and what to call next.
pub(super) fn plan_output(dry_run: &DryRun, execute_tool: &str) -> Value {
    json!({
        "summary": dry_run.plan.summary,
        "expires_in_secs": DEFAULT_PLAN_TTL_SECS,
        "plan": dry_run.plan,
        "confirm_token": dry_run.confirm_token,
        "next_step": format!("Call {execute_tool} with this exact plan and confirm_token; the user will be asked to approve."),
    })
}

/// The sentence in an execute tool's description that says who approves, for the client's approval mode.
/// `noun` is what is approved: "change", "order", "status change".
pub(super) fn approval_instruction(mode: ApprovalMode, noun: &str) -> String {
    match mode {
        ApprovalMode::Elicitation => format!(
            "The user is asked to approve the exact {noun} in a prompt shown by their client; you cannot approve \
             on their behalf, and if they decline nothing is written."
        ),
        ApprovalMode::PermissionPrompt => format!(
            "Set user_approved=true ONLY after the user has explicitly approved the {noun}'s summary and body \
             in this conversation."
        ),
    }
}

pub struct PatchPlan {
    pub executor: Arc<Executor>,
    pub tenants: Tenants,
    pub caller: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanArgs {
    #[serde(rename = "type")]
    order_type: String,
    #[serde(rename = "orderId")]
    order_id: String,
    changes: Map<String, Value>,
    #[serde(default)]
    tenant: Option<String>,
}

#[async_trait]
impl Tool for PatchPlan {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: PATCH_PLAN,
            description: format!(
                "STEP 1 of editing a sales order. Reads the order's current version and builds the \
                 exact change WITHOUT applying it; returns a `plan` and `confirm_token` (valid {} \
                 minutes). Tell the user what you intend to change, then call {PATCH_EXECUTE}; the \
                 user is asked to approve the exact change before anything is written. Allowed \
                 `changes` fields: description, note, date, requestOn, cancelBy, salesPersonId, \
                 ownerId, branchId, projectId, customer, shipping, billing, freight, customFields, \
                 recalculateShipment, orderLines (existing lines, each needs lineId) and \
                 newOrderLines. Status changes, line removal, discounts, currency/tax and ROT/RUT \
                 are not supported. Closed (completed/cancelled) orders are refused. Writes must be \
                 enabled for the tenant or this is refused.",
                DEFAULT_PLAN_TTL_SECS / 60
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "type": {"type": "string", "description": "order type, e.g. SO"},
                    "orderId": {"type": "string"},
                    "changes": {"type": "object", "description": "PatchSalesOrderCommand fields to change"},
                    "tenant": {"type": "string"}
                },
                "required": ["type", "orderId", "changes"],
                "additionalProperties": false
            }),
            kind: ToolKind::Plan,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: PlanArgs = parse_args(args)?;
        let tenant = self.tenants.resolve(a.tenant.as_deref())?;
        let input = json!({"type": a.order_type, "orderId": a.order_id, "changes": a.changes});
        let d = self
            .executor
            .dry_run(&self.caller, &tenant, OPERATION, &input)
            .await
            .map_err(failed)?;
        Ok(plan_output(&d, PATCH_EXECUTE))
    }
}

pub struct PatchExecute {
    pub executor: Arc<Executor>,
    pub tenants: Tenants,
    pub caller: String,
    pub mode: ApprovalMode,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecArgs {
    plan: Plan,
    confirm_token: String,
    /// Only used in `permission_prompt` mode.
    #[serde(default)]
    user_approved: Option<bool>,
}

/// Text that reorders or hides what is around it, written out so the person sees it instead of being fooled by it.
fn show_escaped(text: &str) -> String {
    text.chars()
        .map(|c| {
            let hidden = c.is_control()
                || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}');
            if hidden {
                format!("\\u{{{:04X}}}", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// What the person sees: the summary, the company, the request and the COMPLETE body, all from the signed plan
/// and not from the model's words. A body too long to show in full is refused rather than cut.
fn approval_message(plan: &Plan, company: Option<&str>) -> Result<String, ToolError> {
    let body = serde_json::to_string(&plan.request.body).unwrap_or_default();
    let length = body.chars().count();
    if length > PROMPT_BODY_MAX_CHARS {
        return Err(ToolError::Failed(format!(
            "not executed: the change is too large to show the person in full ({length} characters, the limit is {PROMPT_BODY_MAX_CHARS}). Make it smaller, for example by splitting it into several changes."
        )));
    }
    let company = match company {
        Some(name) => format!(
            "{} ({})",
            vsv2_common::text::one_line(name, 100),
            plan.tenant
        ),
        None => plan.tenant.to_string(),
    };
    Ok(format!(
        "Apply this change to Visma?\n\n{}\n\nCompany: {company}\nRequest: {} {}\nBody (compact JSON, complete):\n{}",
        vsv2_common::text::one_line(&plan.summary, 600),
        plan.request.method.as_str(),
        plan.request.path,
        show_escaped(&body)
    ))
}

impl PatchExecute {
    /// Gate on a human. Returns `Ok(())` only on an explicit yes.
    async fn require_approval(
        &self,
        plan: &Plan,
        user_approved: Option<bool>,
        ctx: &CallContext,
    ) -> Result<(), ToolError> {
        match self.mode {
            ApprovalMode::Elicitation => {
                let Some(approver) = &ctx.approver else {
                    return Err(ToolError::Failed(
                        "not executed: this connection cannot ask the user for approval. Use an MCP client \
                         that supports elicitation, or have the operator set VSV2_MCP_WRITE_APPROVAL=permission_prompt."
                            .into(),
                    ));
                };
                let company = self.tenants.name_of(plan.tenant.as_str());
                let message = approval_message(plan, company.as_deref())?;
                match approver.approve(&message).await {
                    Ok(true) => Ok(()),
                    Ok(false) => Err(ToolError::Failed("not executed: the user did not approve the change.".into())),
                    Err(why) => Err(ToolError::Failed(format!(
                        "not executed: {why}. Have the operator set VSV2_MCP_WRITE_APPROVAL=permission_prompt \
                         if the client cannot show approval prompts."
                    ))),
                }
            }
            ApprovalMode::PermissionPrompt => {
                if user_approved == Some(true) {
                    Ok(())
                } else {
                    Err(ToolError::Failed(
                        "not executed: user_approved is not true. Show the user the plan summary and body, ask for \
                         approval, and only then set user_approved=true."
                            .into(),
                    ))
                }
            }
        }
    }
}

#[async_trait]
impl Tool for PatchExecute {
    fn def(&self) -> ToolDef {
        let (how, props, required) = match self.mode {
            ApprovalMode::Elicitation => (
                "The user is asked to approve the exact change in a prompt shown by their client; you \
                 cannot approve on their behalf, and if they decline nothing is written.",
                json!({
                    "plan": {"type": "object", "description": "the `plan` object from the plan tool, unchanged"},
                    "confirm_token": {"type": "string"}
                }),
                json!(["plan", "confirm_token"]),
            ),
            ApprovalMode::PermissionPrompt => (
                "Set user_approved=true ONLY after the user has explicitly approved the plan's summary and \
                 body in this conversation.",
                json!({
                    "plan": {"type": "object", "description": "the `plan` object from the plan tool, unchanged"},
                    "confirm_token": {"type": "string"},
                    "user_approved": {"type": "boolean", "description": "true only after explicit user approval"}
                }),
                json!(["plan", "confirm_token", "user_approved"]),
            ),
        };
        ToolDef {
            name: PATCH_EXECUTE,
            description: format!(
                "STEP 2: APPLY a sales-order change prepared by {PATCH_PLAN}. This MODIFIES Visma. Pass the \
                 plan and confirm_token unchanged. {how} A plan runs at most once and expires; if the order \
                 changed in the meantime Visma rejects it (412) and a new plan is needed. If the result says \
                 the outcome is unknown, check the order in Visma before planning again. Every attempt is audited."
            ),
            input_schema: json!({
                "type": "object",
                "properties": props,
                "required": required,
                "additionalProperties": false
            }),
            kind: ToolKind::Write,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        self.call_with_context(args, &CallContext::none()).await
    }

    async fn call_with_context(&self, args: Value, ctx: &CallContext) -> Result<Value, ToolError> {
        let a: ExecArgs = parse_args(args)?;
        // The plan (incl. tenant) is bound by the confirm token; also insist the tenant is one we manage.
        self.tenants.resolve(Some(a.plan.tenant.as_str()))?;
        // Reject a bad/expired/replayed plan before bothering the person.
        self.executor
            .preflight(&self.caller, &a.plan, &a.confirm_token)
            .await
            .map_err(failed)?;
        self.require_approval(&a.plan, a.user_approved, ctx).await?;
        let done = self
            .executor
            .execute(&self.caller, &a.plan, &a.confirm_token)
            .await
            .map_err(failed)?;
        Ok(json!({
            "executed": true,
            "plan_id": done.plan_id,
            "http_status": done.status,
            "new_etag": done.etag,
            "location": done.location,
            "response": done.body,
            "audit_degraded": done.audit_degraded,
        }))
    }
}

/// The patch tool's approval and execution path under another name and in other words, for the
/// operations that are not edits (create, status). It is the same code: the same checks, the same prompt
/// and the same audit; only the tool's name and description change.
pub struct ExecuteAs {
    pub inner: PatchExecute,
    pub name: &'static str,
    /// The tool description for the client's approval mode.
    pub describe: fn(ApprovalMode) -> String,
    /// Adds what the operation's caller needs from a successful result (for a create, which order it made).
    pub enrich: fn(&mut Value),
}

/// For operations whose result needs nothing added.
pub fn no_enrichment(_: &mut Value) {}

#[async_trait]
impl Tool for ExecuteAs {
    fn def(&self) -> ToolDef {
        let mut def = self.inner.def();
        def.name = self.name;
        def.description = (self.describe)(self.inner.mode);
        def
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        self.call_with_context(args, &CallContext::none()).await
    }

    async fn call_with_context(&self, args: Value, ctx: &CallContext) -> Result<Value, ToolError> {
        let mut result = self.inner.call_with_context(args, ctx).await?;
        (self.enrich)(&mut result);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Approver;
    use crate::visma::tenants::test_tenants;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use vsv2_common::TenantId;
    use vsv2_interactive::{
        default_registry, AuditStage, ConfirmSigner, MemoryAuditSink, ReadResponse, Reader,
        Result as IResult, Transport, WritePolicy, WriteRequest, WriteResponse, WriteTarget,
    };

    struct Fake {
        sent: AtomicUsize,
        status: u16,
    }
    #[async_trait]
    impl Transport for Fake {
        async fn send(&self, _: &TenantId, _: &WriteRequest) -> IResult<WriteResponse> {
            self.sent.fetch_add(1, Ordering::SeqCst);
            Ok(WriteResponse {
                status: self.status,
                body: "".into(),
                etag: Some("\"e2\"".into()),
                location: None,
            })
        }
    }
    #[async_trait]
    impl Reader for Fake {
        async fn get(&self, _: &TenantId, _: WriteTarget, _: &str) -> IResult<ReadResponse> {
            Ok(ReadResponse {
                status: 200,
                body: json!({"status": {"description": "Open", "cancelled": false, "hold": false}})
                    .to_string(),
                etag: Some("\"e1\"".into()),
            })
        }
    }

    /// Scripted human. Records every question it was asked.
    struct Human {
        answer: Result<bool, String>,
        asked: Mutex<Vec<String>>,
    }
    impl Human {
        fn new(answer: Result<bool, String>) -> Arc<Self> {
            Arc::new(Self {
                answer,
                asked: Mutex::new(vec![]),
            })
        }
        fn times_asked(&self) -> usize {
            self.asked.lock().unwrap().len()
        }
    }
    #[async_trait]
    impl Approver for Human {
        async fn approve(&self, message: &str) -> Result<bool, String> {
            self.asked.lock().unwrap().push(message.to_string());
            self.answer.clone()
        }
    }

    struct Rig {
        plan: PatchPlan,
        exec: PatchExecute,
        fake: Arc<Fake>,
        sink: Arc<MemoryAuditSink>,
    }

    fn rig(policy: WritePolicy, status: u16, mode: ApprovalMode) -> Rig {
        let fake = Arc::new(Fake {
            sent: AtomicUsize::new(0),
            status,
        });
        let sink = Arc::new(MemoryAuditSink::default());
        let executor = Arc::new(Executor::new(
            default_registry(),
            policy,
            ConfirmSigner::new(b"mcp-test-confirm-secret-0123".to_vec()).unwrap(),
            sink.clone(),
            fake.clone(),
            fake.clone(),
        ));
        Rig {
            plan: PatchPlan {
                executor: executor.clone(),
                tenants: test_tenants(),
                caller: "mcp:test".into(),
            },
            exec: PatchExecute {
                executor,
                tenants: test_tenants(),
                caller: "mcp:test".into(),
                mode,
            },
            fake,
            sink,
        }
    }

    fn allow() -> WritePolicy {
        WritePolicy::new(["t-aq"], ["salesorder.patch"])
    }

    fn change() -> Value {
        json!({"type": "SO", "orderId": "000100", "changes": {"description": "from mcp"}})
    }

    fn ctx(h: &Arc<Human>) -> CallContext {
        CallContext::with_approver(h.clone())
    }

    fn exec_args(p: &Value) -> Value {
        json!({"plan": p["plan"], "confirm_token": p["confirm_token"]})
    }

    #[tokio::test]
    async fn plan_then_user_approved_execute_applies_once_and_audits_with_the_caller() {
        let r = rig(allow(), 202, ApprovalMode::Elicitation);
        let p = r.plan.call(change()).await.unwrap();
        assert_eq!(
            r.fake.sent.load(Ordering::SeqCst),
            0,
            "planning sends nothing"
        );
        assert!(p["summary"].as_str().unwrap().contains("SO/000100"));
        assert_eq!(p["plan"]["request"]["if_match"], "\"e1\"");

        let human = Human::new(Ok(true));
        let out = r
            .exec
            .call_with_context(exec_args(&p), &ctx(&human))
            .await
            .unwrap();
        assert_eq!(
            (
                out["executed"].as_bool(),
                out["http_status"].as_u64(),
                out["audit_degraded"].as_bool()
            ),
            (Some(true), Some(202), Some(false))
        );
        assert_eq!(
            r.sink.stages(),
            vec![
                AuditStage::DryRun,
                AuditStage::Executing,
                AuditStage::Succeeded
            ]
        );
        assert!(r.sink.entries().iter().all(|e| e.caller == "mcp:test"));

        // The person saw the real change, built from the signed plan.
        let shown = human.asked.lock().unwrap()[0].clone();
        assert!(
            shown.contains("SO/000100")
                && shown.contains("PATCH /api/v3/SalesOrders/SO/000100")
                && shown.contains("from mcp"),
            "{shown}"
        );

        // Replay: refused before the person is asked again.
        let again = r
            .exec
            .call_with_context(exec_args(&p), &ctx(&human))
            .await
            .unwrap_err()
            .to_string();
        assert!(again.contains("already executed"), "{again}");
        assert_eq!(human.times_asked(), 1);
        assert_eq!(r.fake.sent.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_model_cannot_approve_for_the_user() {
        let r = rig(allow(), 202, ApprovalMode::Elicitation);
        let p = r.plan.call(change()).await.unwrap();

        // A user who declines: nothing is written, whatever the model passes.
        let no = Human::new(Ok(false));
        let mut args = exec_args(&p);
        args["user_approved"] = json!(true); // ignored in this mode
        let e = r
            .exec
            .call_with_context(args.clone(), &ctx(&no))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("did not approve"), "{e}");

        // No way to ask (client without elicitation): fail closed.
        let e = r
            .exec
            .call_with_context(args.clone(), &CallContext::none())
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("cannot ask the user"), "{e}");
        let e = r.exec.call(args.clone()).await.unwrap_err().to_string();
        assert!(e.contains("cannot ask the user"), "{e}");

        // The prompt itself failed: fail closed.
        let broken = Human::new(Err("client does not support elicitation".into()));
        assert!(r.exec.call_with_context(args, &ctx(&broken)).await.is_err());

        assert_eq!(r.fake.sent.load(Ordering::SeqCst), 0);
        assert_eq!(
            r.sink.stages(),
            vec![AuditStage::DryRun],
            "no executing row"
        );
    }

    #[tokio::test]
    async fn bad_plans_are_rejected_before_the_user_is_asked() {
        let r = rig(allow(), 202, ApprovalMode::Elicitation);
        let p = r.plan.call(change()).await.unwrap();
        let mut plan = p["plan"].clone();
        plan["request"]["body"] = json!({"description": "something else"});
        let human = Human::new(Ok(true));
        let e = r
            .exec
            .call_with_context(
                json!({"plan": plan, "confirm_token": p["confirm_token"]}),
                &ctx(&human),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("confirm token"), "{e}");
        assert_eq!(
            human.times_asked(),
            0,
            "nobody is asked to approve a tampered plan"
        );
        assert_eq!(r.fake.sent.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn permission_prompt_mode_needs_the_explicit_flag_and_never_asks_via_elicitation() {
        let r = rig(allow(), 202, ApprovalMode::PermissionPrompt);
        let p = r.plan.call(change()).await.unwrap();
        let human = Human::new(Ok(true));

        let e = r
            .exec
            .call_with_context(exec_args(&p), &ctx(&human))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("user_approved"), "{e}");
        assert_eq!(human.times_asked(), 0);
        assert_eq!(r.fake.sent.load(Ordering::SeqCst), 0);

        let mut args = exec_args(&p);
        args["user_approved"] = json!(true);
        assert_eq!(r.exec.call(args).await.unwrap()["http_status"], 202);
        assert_eq!(r.fake.sent.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn disabled_policy_blocks_planning_and_unsupported_fields_are_explained() {
        let r = rig(WritePolicy::deny_all(), 202, ApprovalMode::Elicitation);
        let e = r.plan.call(change()).await.unwrap_err().to_string();
        assert!(e.contains("write denied"), "{e}");

        let r2 = rig(allow(), 202, ApprovalMode::Elicitation);
        let bad = r2
            .plan
            .call(json!({"type": "SO", "orderId": "1", "changes": {"removeLines": [1]}}))
            .await
            .unwrap_err()
            .to_string();
        assert!(bad.contains("deletion"), "{bad}");
        assert!(matches!(
            r2.plan
                .call(json!({"type": "SO", "orderId": "1", "changes": {}, "tenant": "x"}))
                .await,
            Err(ToolError::InvalidArgs(_))
        ));
    }

    #[tokio::test]
    async fn upstream_rejection_is_reported_and_audited_as_failed() {
        let r = rig(allow(), 412, ApprovalMode::Elicitation);
        let p = r.plan.call(change()).await.unwrap();
        let human = Human::new(Ok(true));
        let e = r
            .exec
            .call_with_context(exec_args(&p), &ctx(&human))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("412"), "{e}");
        assert_eq!(r.sink.stages().last(), Some(&AuditStage::Failed));
    }

    #[test]
    fn schema_and_kinds_follow_the_approval_mode() {
        let e = rig(allow(), 202, ApprovalMode::Elicitation);
        assert_eq!(e.plan.def().kind, ToolKind::Plan);
        assert_eq!(e.exec.def().kind, ToolKind::Write);
        assert!(e.exec.def().description.contains("MODIFIES Visma"));
        assert_eq!(
            e.exec.def().input_schema["required"],
            json!(["plan", "confirm_token"])
        );

        let p = rig(allow(), 202, ApprovalMode::PermissionPrompt);
        assert_eq!(
            p.exec.def().input_schema["required"],
            json!(["plan", "confirm_token", "user_approved"])
        );
    }
}
