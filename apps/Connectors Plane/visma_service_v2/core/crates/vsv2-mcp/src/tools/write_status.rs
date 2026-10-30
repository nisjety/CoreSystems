//! Putting a sales order on hold or releasing it: a plan tool and an execute tool over the Layer 3
//! operation `salesorder.set_status`. Adds no power of its own; see [`super::write`] for the shared
//! approval and execution path.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use vsv2_interactive::executor::DEFAULT_PLAN_TTL_SECS;
use vsv2_interactive::Executor;

use super::write::{
    approval_instruction, failed, no_enrichment, plan_output, ExecuteAs, PatchExecute,
};
use crate::config::ApprovalMode;
use crate::tool::{parse_args, Tool, ToolDef, ToolError, ToolKind};
use crate::visma::Tenants;

pub const STATUS_PLAN: &str = "visma_salesorder_status_plan";
pub const STATUS_EXECUTE: &str = "visma_salesorder_status_execute";
const OPERATION: &str = "salesorder.set_status";

pub struct StatusPlan {
    pub executor: Arc<Executor>,
    pub tenants: Tenants,
    pub caller: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusPlanArgs {
    #[serde(rename = "type")]
    order_type: String,
    #[serde(rename = "orderId")]
    order_id: String,
    status: String,
    #[serde(default)]
    tenant: Option<String>,
}

#[async_trait]
impl Tool for StatusPlan {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: STATUS_PLAN,
            description: format!(
                "STEP 1 of putting a sales order on hold or releasing it. Reads the order's current status and \
                 version and builds the exact change WITHOUT applying it. `status` is \"Hold\" (only an Open \
                 order can be put on hold) or \"Open\" (only an order that is On Hold can be released; a credit \
                 hold or approval hold is refused). Cancelling, completing and back-ordering are not offered. \
                 Closed orders are refused. Returns `plan` and `confirm_token` (valid {} minutes). Show the user \
                 the summary, then call {STATUS_EXECUTE}; the user is asked to approve before anything is \
                 changed. Writes must be enabled for the tenant and salesorder.set_status must be allowed.",
                DEFAULT_PLAN_TTL_SECS / 60
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "type": {"type": "string", "description": "order type, e.g. SO"},
                    "orderId": {"type": "string"},
                    "status": {"type": "string", "enum": ["Hold", "Open"], "description": "Hold puts an Open order on hold; Open releases a held order"},
                    "tenant": {"type": "string"}
                },
                "required": ["type", "orderId", "status"],
                "additionalProperties": false
            }),
            kind: ToolKind::Plan,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: StatusPlanArgs = parse_args(args)?;
        let tenant = self.tenants.resolve(a.tenant.as_deref())?;
        let input = json!({"type": a.order_type, "orderId": a.order_id, "status": a.status});
        let dry_run = self
            .executor
            .dry_run(&self.caller, &tenant, OPERATION, &input)
            .await
            .map_err(failed)?;
        Ok(plan_output(&dry_run, STATUS_EXECUTE))
    }
}

fn describe_execute(mode: ApprovalMode) -> String {
    format!(
        "STEP 2: APPLY the hold or release prepared by {STATUS_PLAN}. This MODIFIES Visma: pass the plan and \
         confirm_token unchanged. {} A plan runs at most once and expires; if the order changed in the \
         meantime Visma rejects it (412) and a new plan is needed. If the result says the outcome is unknown, \
         read the order with visma_salesorder_get before planning again. Every attempt is audited.",
        approval_instruction(mode, "status change")
    )
}

/// The execute step: the shared approval and execution path under the status tool's name.
pub fn status_execute(inner: PatchExecute) -> ExecuteAs {
    ExecuteAs {
        inner,
        name: STATUS_EXECUTE,
        describe: describe_execute,
        enrich: no_enrichment,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{Approver, CallContext};
    use crate::visma::tenants::test_tenants;
    use std::sync::Mutex;
    use vsv2_common::TenantId;
    use vsv2_interactive::{
        default_registry, ConfirmSigner, MemoryAuditSink, ReadResponse, Reader, Result as IResult,
        Transport, WritePolicy, WriteRequest, WriteResponse, WriteTarget,
    };

    const TEST_TENANT: &str = "t-aq";

    /// One order that is Open or On Hold; records what is sent to it.
    struct Visma {
        held: bool,
        sent: Mutex<Vec<WriteRequest>>,
    }
    impl Visma {
        fn new(held: bool) -> Arc<Self> {
            Arc::new(Self {
                held,
                sent: Mutex::new(vec![]),
            })
        }
    }
    #[async_trait]
    impl Transport for Visma {
        async fn send(&self, _: &TenantId, req: &WriteRequest) -> IResult<WriteResponse> {
            self.sent.lock().unwrap().push(req.clone());
            Ok(WriteResponse {
                status: 200,
                body: String::new(),
                etag: Some("\"e2\"".into()),
                location: None,
            })
        }
    }
    #[async_trait]
    impl Reader for Visma {
        async fn get(&self, _: &TenantId, _: WriteTarget, _: &str) -> IResult<ReadResponse> {
            let (description, hold) = if self.held {
                ("On Hold", true)
            } else {
                ("Open", false)
            };
            Ok(ReadResponse {
                status: 200,
                body: json!({"status": {"description": description, "cancelled": false, "hold": hold}})
                    .to_string(),
                etag: Some("\"e1\"".into()),
            })
        }
    }

    struct Human(bool);
    #[async_trait]
    impl Approver for Human {
        async fn approve(&self, _: &str) -> Result<bool, String> {
            Ok(self.0)
        }
    }

    fn executor(visma: Arc<Visma>) -> Arc<Executor> {
        Arc::new(Executor::new(
            default_registry(),
            WritePolicy::new([TEST_TENANT], ["salesorder.set_status"]),
            ConfirmSigner::new(b"status-test-secret-0123456789abcdef".to_vec())
                .expect("secret is long enough"),
            Arc::new(MemoryAuditSink::default()),
            visma.clone(),
            visma,
        ))
    }

    fn planner(visma: &Arc<Visma>) -> StatusPlan {
        StatusPlan {
            executor: executor(visma.clone()),
            tenants: test_tenants(),
            caller: "test".into(),
        }
    }

    fn executor_tool(visma: &Arc<Visma>, mode: ApprovalMode) -> ExecuteAs {
        status_execute(PatchExecute {
            executor: executor(visma.clone()),
            tenants: test_tenants(),
            caller: "test".into(),
            mode,
        })
    }

    fn args(status: &str) -> Value {
        json!({"type": "SO", "orderId": "000100", "status": status})
    }

    #[tokio::test]
    async fn the_plan_shows_the_hold_with_the_current_version_and_sends_nothing() {
        let visma = Visma::new(false);
        let out = planner(&visma).call(args("Hold")).await.unwrap();
        assert!(
            out["summary"].as_str().unwrap().contains("on hold"),
            "{out}"
        );
        assert_eq!(out["plan"]["request"]["method"], "patch");
        assert_eq!(
            out["plan"]["request"]["path"],
            "/api/v3/SalesOrders/SO/000100"
        );
        assert_eq!(out["plan"]["request"]["body"], json!({"status": "Hold"}));
        assert_eq!(out["plan"]["request"]["if_match"], "\"e1\"");
        assert!(
            visma.sent.lock().unwrap().is_empty(),
            "a plan must never send"
        );
    }

    #[tokio::test]
    async fn an_approved_release_is_sent_once_with_the_version_from_the_plan() {
        let visma = Visma::new(true);
        let planned = planner(&visma).call(args("Open")).await.unwrap();
        let execute = executor_tool(&visma, ApprovalMode::Elicitation);
        let ctx = CallContext::with_approver(Arc::new(Human(true)));
        let call = json!({"plan": planned["plan"], "confirm_token": planned["confirm_token"]});
        let done = execute.call_with_context(call.clone(), &ctx).await.unwrap();
        assert_eq!(done["executed"], true);
        {
            let sent = visma.sent.lock().unwrap();
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].body, json!({"status": "Open"}));
            assert_eq!(sent[0].if_match.as_deref(), Some("\"e1\""));
        }
        assert!(
            execute.call_with_context(call, &ctx).await.is_err(),
            "a plan runs at most once"
        );
        assert_eq!(visma.sent.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_person_who_declines_stops_the_change_before_it_is_sent() {
        let visma = Visma::new(false);
        let planned = planner(&visma).call(args("Hold")).await.unwrap();
        let execute = executor_tool(&visma, ApprovalMode::Elicitation);
        let ctx = CallContext::with_approver(Arc::new(Human(false)));
        let call = json!({"plan": planned["plan"], "confirm_token": planned["confirm_token"]});
        let err = execute.call_with_context(call, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("did not approve"), "{err}");
        assert!(visma.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn moves_the_order_cannot_make_never_become_a_plan() {
        let open = Visma::new(false);
        let err = planner(&open).call(args("Open")).await.unwrap_err();
        assert!(err.to_string().contains("On Hold can be released"), "{err}");
        let held = Visma::new(true);
        let err = planner(&held).call(args("Hold")).await.unwrap_err();
        assert!(err.to_string().contains("already on hold"), "{err}");
        let err = planner(&open).call(args("Cancelled")).await.unwrap_err();
        assert!(err.to_string().contains("\"Hold\" or \"Open\""), "{err}");
    }

    #[test]
    fn the_status_tools_have_their_names_kinds_and_honest_descriptions() {
        let visma = Visma::new(false);
        let plan = planner(&visma).def();
        assert_eq!((plan.name, plan.kind), (STATUS_PLAN, ToolKind::Plan));
        assert_eq!(
            plan.input_schema["properties"]["status"]["enum"],
            json!(["Hold", "Open"])
        );
        let execute = executor_tool(&visma, ApprovalMode::Elicitation).def();
        assert_eq!(
            (execute.name, execute.kind),
            (STATUS_EXECUTE, ToolKind::Write)
        );
        assert!(execute
            .description
            .contains("cannot approve on their behalf"));
        assert!(executor_tool(&visma, ApprovalMode::PermissionPrompt)
            .def()
            .description
            .contains("user_approved=true ONLY"));
    }
}
