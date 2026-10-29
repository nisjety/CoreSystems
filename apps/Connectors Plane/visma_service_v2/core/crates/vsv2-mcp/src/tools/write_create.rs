//! Creating a sales order: a plan tool and an execute tool over the Layer 3 operation `salesorder.create`.
//! Adds no power of its own; see [`super::write`] for the shared approval and execution path.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use vsv2_interactive::executor::DEFAULT_PLAN_TTL_SECS;
use vsv2_interactive::Executor;

use super::write::{approval_instruction, failed, plan_output, ExecuteAs, PatchExecute};
use crate::config::ApprovalMode;
use crate::tool::{parse_args, Tool, ToolDef, ToolError, ToolKind};
use crate::visma::Tenants;

pub const CREATE_PLAN: &str = "visma_salesorder_create_plan";
pub const CREATE_EXECUTE: &str = "visma_salesorder_create_execute";
const OPERATION: &str = "salesorder.create";

pub struct CreatePlan {
    pub executor: Arc<Executor>,
    pub tenants: Tenants,
    pub caller: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreatePlanArgs {
    order: Map<String, Value>,
    #[serde(default)]
    tenant: Option<String>,
}

#[async_trait]
impl Tool for CreatePlan {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: CREATE_PLAN,
            description: format!(
                "STEP 1 of creating a sales order in Visma. Builds the exact POST to the SalesOrders v3 API \
                 WITHOUT sending it, and checks `order` against Visma's own request schema: a misspelt field is \
                 refused, not dropped. Every order is created ON HOLD; release it afterwards with \
                 visma_salesorder_status_plan once it has been checked. `order` needs `type` (e.g. SO), \
                 `customer` with `id` (or `gln`) and at least one entry in `orderLines` with `inventoryId` and a \
                 positive `quantity`; the order number comes from Visma's number series. Returns `plan` and \
                 `confirm_token` (valid {} minutes). Show the user the summary, then call {CREATE_EXECUTE} with \
                 this plan; the user is asked to approve before anything is created. Writes must be enabled for \
                 the tenant and salesorder.create must be allowed.",
                DEFAULT_PLAN_TTL_SECS / 60
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "order": {"type": "object", "description": "the request body of POST /api/v3/SalesOrders; find its fields with visma_search_api (api salesorder_v3)"},
                    "tenant": {"type": "string"}
                },
                "required": ["order"],
                "additionalProperties": false
            }),
            kind: ToolKind::Plan,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: CreatePlanArgs = parse_args(args)?;
        let tenant = self.tenants.resolve(a.tenant.as_deref())?;
        let input = Value::Object(a.order);
        let dry_run = self
            .executor
            .dry_run(&self.caller, &tenant, OPERATION, &input)
            .await
            .map_err(failed)?;
        Ok(plan_output(&dry_run, CREATE_EXECUTE))
    }
}

fn describe_execute(mode: ApprovalMode) -> String {
    format!(
        "STEP 2: CREATE the sales order prepared by {CREATE_PLAN} in Visma, on hold. This MODIFIES Visma: pass \
         the plan and confirm_token unchanged. {} A plan runs at most once and expires. If the result says the \
         outcome is unknown, check Visma before planning again, or the order may be created twice. Every attempt \
         is audited.",
        approval_instruction(mode, "order")
    )
}

/// The execute step: the shared approval and execution path under the create tool's name.
pub fn create_execute(inner: PatchExecute) -> ExecuteAs {
    ExecuteAs {
        inner,
        name: CREATE_EXECUTE,
        describe: describe_execute,
        enrich: add_created_order,
    }
}

/// Which order was made: Visma answers a create with the address of the new order, `.../SalesOrders/{type}/{orderId}`,
/// and nothing else about it. The type and number are what the follow-up steps (read it back, release it) need.
fn created_order(location: &str) -> Option<(String, String)> {
    let path = location.split(['?', '#']).next()?;
    let mut segments = path.trim_end_matches('/').rsplit('/');
    let order_id = segments.next()?;
    let order_type = segments.next()?;
    let safe = |text: &str, max: usize| {
        !text.is_empty()
            && text.len() <= max
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    (safe(order_type, 2) && safe(order_id, 15))
        .then(|| (order_type.to_string(), order_id.to_string()))
}

/// Say which order was created, or that Visma did not say.
fn add_created_order(result: &mut Value) {
    let found = result["location"].as_str().and_then(created_order);
    match found {
        Some((order_type, order_id)) => {
            result["created_order"] = json!({"type": order_type, "orderId": order_id});
            result["next_step"] = json!(format!(
                "Read it back with visma_salesorder_get (type {order_type}, orderId {order_id}). It is on hold; release it with visma_salesorder_status_plan only when it has been checked."
            ));
        }
        None => {
            result["created_order"] = Value::Null;
            result["next_step"] = json!(
                "Visma did not return the address of the new order. Find it in Visma before releasing it or planning another create, or the order may be created twice."
            );
        }
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

    /// The default tenant of `test_tenants()`, so the write policy below allows the plans the tests make.
    const TEST_TENANT: &str = "t-aq";

    /// Answers the create POST and nothing else; any read is a test failure.
    #[derive(Default)]
    struct Visma {
        sent: Mutex<Vec<WriteRequest>>,
    }
    #[async_trait]
    impl Transport for Visma {
        async fn send(&self, _: &TenantId, req: &WriteRequest) -> IResult<WriteResponse> {
            self.sent.lock().unwrap().push(req.clone());
            // What the API really answers: the body holds only warnings, the new order is in `Location`.
            Ok(WriteResponse {
                status: 201,
                body: json!({"warnings": []}).to_string(),
                etag: None,
                location: Some("https://salesorder.visma.net/api/v3/SalesOrders/SO/000123".into()),
            })
        }
    }
    #[async_trait]
    impl Reader for Visma {
        async fn get(&self, _: &TenantId, _: WriteTarget, _: &str) -> IResult<ReadResponse> {
            panic!("creating an order must not read anything")
        }
    }

    struct Human(bool);
    #[async_trait]
    impl Approver for Human {
        async fn approve(&self, _: &str) -> Result<bool, String> {
            Ok(self.0)
        }
    }

    fn order() -> Value {
        json!({
            "type": "SO",
            "customer": {"id": "C-1001"},
            "orderLines": [{"inventoryId": "ITEM-1", "quantity": 2}]
        })
    }

    fn executor(visma: Arc<Visma>) -> Arc<Executor> {
        Arc::new(Executor::new(
            default_registry(),
            WritePolicy::new([TEST_TENANT], ["salesorder.create"]),
            ConfirmSigner::new(b"create-test-secret-0123456789abcdef".to_vec())
                .expect("secret is long enough"),
            Arc::new(MemoryAuditSink::default()),
            visma.clone(),
            visma,
        ))
    }

    fn planner(visma: &Arc<Visma>) -> CreatePlan {
        CreatePlan {
            executor: executor(visma.clone()),
            tenants: test_tenants(),
            caller: "test".into(),
        }
    }

    fn executor_tool(visma: &Arc<Visma>, mode: ApprovalMode) -> ExecuteAs {
        create_execute(PatchExecute {
            executor: executor(visma.clone()),
            tenants: test_tenants(),
            caller: "test".into(),
            mode,
        })
    }

    #[tokio::test]
    async fn the_plan_describes_an_order_on_hold_and_sends_nothing() {
        let visma = Arc::new(Visma::default());
        let out = planner(&visma)
            .call(json!({"order": order()}))
            .await
            .unwrap();
        assert!(
            out["summary"].as_str().unwrap().contains("(on hold)"),
            "{out}"
        );
        assert_eq!(out["plan"]["request"]["body"]["status"], "Hold");
        assert_eq!(out["plan"]["request"]["target"], "sales_order");
        assert!(out["confirm_token"].as_str().is_some());
        assert!(
            visma.sent.lock().unwrap().is_empty(),
            "a plan must never send"
        );
    }

    #[tokio::test]
    async fn an_approved_plan_is_sent_once_as_a_post_and_cannot_be_replayed() {
        let visma = Arc::new(Visma::default());
        let planned = planner(&visma)
            .call(json!({"order": order()}))
            .await
            .unwrap();
        let args = json!({
            "plan": planned["plan"],
            "confirm_token": planned["confirm_token"],
            "user_approved": true
        });
        let execute = executor_tool(&visma, ApprovalMode::PermissionPrompt);
        let done = execute.call(args.clone()).await.unwrap();
        assert_eq!(done["executed"], true);
        assert_eq!(done["http_status"], 201);
        assert_eq!(
            done["created_order"],
            json!({"type": "SO", "orderId": "000123"}),
            "{done}"
        );
        assert!(
            done["next_step"]
                .as_str()
                .unwrap()
                .contains("visma_salesorder_get"),
            "{done}"
        );
        {
            let sent = visma.sent.lock().unwrap();
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].path, "/api/v3/SalesOrders");
            assert_eq!(sent[0].method.as_str(), "POST");
            assert_eq!(sent[0].body["status"], "Hold");
        }
        assert!(
            execute.call(args).await.is_err(),
            "a plan runs at most once"
        );
        assert_eq!(visma.sent.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_person_who_declines_stops_the_order_before_it_is_sent() {
        let visma = Arc::new(Visma::default());
        let planned = planner(&visma)
            .call(json!({"order": order()}))
            .await
            .unwrap();
        let args = json!({"plan": planned["plan"], "confirm_token": planned["confirm_token"]});
        let execute = executor_tool(&visma, ApprovalMode::Elicitation);
        let ctx = CallContext::with_approver(Arc::new(Human(false)));
        let err = execute.call_with_context(args, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("did not approve"), "{err}");
        assert!(visma.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_order_the_schema_refuses_never_becomes_a_plan() {
        let visma = Arc::new(Visma::default());
        let mut bad = order();
        bad["costumer"] = json!({"id": "C-1"});
        let err = planner(&visma)
            .call(json!({"order": bad}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
        let mut open = order();
        open["status"] = json!("Open");
        let err = planner(&visma)
            .call(json!({"order": open}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("created on hold"), "{err}");
    }

    #[test]
    fn the_new_order_is_read_from_the_address_visma_returns_it_at() {
        for (location, expected) in [
            (
                "https://salesorder.visma.net/api/v3/SalesOrders/SO/000123",
                Some(("SO", "000123")),
            ),
            ("/api/v3/SalesOrders/SO/000123/", Some(("SO", "000123"))),
            (
                "/api/v3/SalesOrders/I1/10908?expand=lines",
                Some(("I1", "10908")),
            ),
            ("/api/v3/SalesOrders/SO", None),
            ("", None),
            ("https://x/SO/../../etc", None),
            ("/api/v3/SalesOrders/SOX/000123", None),
            ("/api/v3/SalesOrders/SO/0001 23", None),
        ] {
            let found = created_order(location);
            assert_eq!(
                found.as_ref().map(|(t, i)| (t.as_str(), i.as_str())),
                expected,
                "{location:?}"
            );
        }
    }

    #[test]
    fn without_an_address_the_result_says_the_order_must_be_found_before_anything_is_planned_again()
    {
        let mut result = json!({"executed": true, "location": null});
        add_created_order(&mut result);
        assert_eq!(result["created_order"], Value::Null);
        assert!(
            result["next_step"]
                .as_str()
                .unwrap()
                .contains("may be created twice"),
            "{result}"
        );
    }

    #[tokio::test]
    async fn the_person_is_shown_the_company_by_name_and_the_whole_order() {
        let visma = Arc::new(Visma::default());
        let planned = planner(&visma)
            .call(json!({"order": order()}))
            .await
            .unwrap();
        let execute = executor_tool(&visma, ApprovalMode::Elicitation);
        let asked = Arc::new(Recording::default());
        let ctx = CallContext::with_approver(asked.clone());
        let args = json!({"plan": planned["plan"], "confirm_token": planned["confirm_token"]});
        execute.call_with_context(args, &ctx).await.unwrap();
        let shown = asked.0.lock().unwrap()[0].clone();
        assert!(shown.contains("Company: Aquatiq AS (t-aq)"), "{shown}");
        assert!(
            shown.contains("ITEM-1")
                && shown.contains(r#""quantity":2"#)
                && shown.contains("complete"),
            "{shown}"
        );
    }

    #[tokio::test]
    async fn an_order_too_large_to_show_in_full_is_not_put_to_the_person_at_all() {
        let visma = Arc::new(Visma::default());
        let mut big = order();
        big["note"] = json!("x".repeat(9_000));
        let planned = planner(&visma).call(json!({"order": big})).await.unwrap();
        let execute = executor_tool(&visma, ApprovalMode::Elicitation);
        let asked = Arc::new(Recording::default());
        let ctx = CallContext::with_approver(asked.clone());
        let args = json!({"plan": planned["plan"], "confirm_token": planned["confirm_token"]});
        let e = execute
            .call_with_context(args, &ctx)
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("too large to show the person in full"), "{e}");
        assert!(
            asked.0.lock().unwrap().is_empty(),
            "nobody is asked to approve what they cannot see"
        );
        assert!(visma.sent.lock().unwrap().is_empty());
    }

    /// Records the questions the person was asked and says yes.
    #[derive(Default)]
    struct Recording(Mutex<Vec<String>>);
    #[async_trait]
    impl Approver for Recording {
        async fn approve(&self, message: &str) -> Result<bool, String> {
            self.0.lock().unwrap().push(message.to_string());
            Ok(true)
        }
    }

    #[test]
    fn the_create_tools_have_their_names_kinds_and_honest_descriptions() {
        let visma = Arc::new(Visma::default());
        let plan = planner(&visma).def();
        assert_eq!((plan.name, plan.kind), (CREATE_PLAN, ToolKind::Plan));
        assert!(plan.description.contains("ON HOLD"));
        assert!(plan.description.contains("visma_salesorder_status_plan"));
        let execute = executor_tool(&visma, ApprovalMode::Elicitation).def();
        assert_eq!(
            (execute.name, execute.kind),
            (CREATE_EXECUTE, ToolKind::Write)
        );
        assert!(execute
            .description
            .contains("cannot approve on their behalf"));
        let prompted = executor_tool(&visma, ApprovalMode::PermissionPrompt).def();
        assert!(prompted.description.contains("user_approved=true ONLY"));
    }
}
