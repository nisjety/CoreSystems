//! `salesorder.set_status`: put an open sales order on hold, or release a held one, with
//! `PATCH /api/v3/SalesOrders/{type}/{orderId}` and the body `{"status": "Hold"}` or `{"status": "Open"}`
//! (SalesOrders v3, scope `visma.net.erp.salesorder:write`).
//!
//! Input: `{"type": "SO", "orderId": "000100", "status": "Hold"}`.
//!
//! Only these two moves are offered. The API also takes `BackOrder`, `Cancelled` and `Completed`, but a
//! cancelled or completed order cannot be reopened, so they stay in Visma. A hold is placed only on an
//! order that is plainly `Open`, and released only from `On Hold`: a credit hold or an approval hold has
//! its own owner in Visma and is not this operation's to lift.
//!
//! The version check is the same as for `salesorder.patch`: the ETag comes from a fresh read at plan
//! time, so an order somebody else changed in the meantime fails with 412 instead of being overwritten.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use vsv2_common::TenantId;

use super::order_read::{invalid, path_safe, read_order, OrderStatus};
use crate::error::Result;
use crate::operation::{Built, Operation};
use crate::request::{WriteMethod, WriteRequest, WriteTarget};
use crate::transport::Reader;

pub const NAME: &str = "salesorder.set_status";

const OPEN: &str = "Open";
const HOLD: &str = "Hold";
const ON_HOLD: &str = "On Hold";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(rename = "type")]
    order_type: String,
    #[serde(rename = "orderId")]
    order_id: String,
    status: String,
}

/// The two moves, named after the status the order ends up in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Move {
    Hold,
    Release,
}

impl Move {
    fn parse(status: &str) -> Result<Self> {
        let status = status.trim();
        if status.eq_ignore_ascii_case(HOLD) {
            Ok(Self::Hold)
        } else if status.eq_ignore_ascii_case(OPEN) {
            Ok(Self::Release)
        } else {
            Err(invalid(format!(
                "status must be \"Hold\" or \"Open\", got {status:?}. Cancelling, completing and back-ordering \
                 an order are not offered: do them in Visma."
            )))
        }
    }

    /// The status the API is asked for.
    fn target(self) -> &'static str {
        match self {
            Self::Hold => HOLD,
            Self::Release => OPEN,
        }
    }

    /// Refuse unless the order is in the one state this move starts from.
    fn check(self, current: &OrderStatus, label: &str) -> Result<()> {
        if current.closed {
            return Err(invalid(format!(
                "sales order {label} is {}; closed orders keep their status",
                current.label
            )));
        }
        match self {
            Self::Hold if current.hold || current.is(ON_HOLD) || current.is(HOLD) => Err(invalid(
                format!("sales order {label} is already on hold; nothing to do"),
            )),
            Self::Hold if !current.is(OPEN) => Err(invalid(format!(
                "sales order {label} is {}; only an Open order can be put on hold",
                current.label
            ))),
            Self::Release if !current.on_hold() => Err(invalid(format!(
                "sales order {label} is {}; only an order that is On Hold can be released here",
                current.label
            ))),
            _ => Ok(()),
        }
    }

    fn summary(self, label: &str, current: &OrderStatus) -> String {
        match self {
            Self::Hold => format!("Put sales order {label} on hold (now {})", current.label),
            Self::Release => format!(
                "Release sales order {label} from hold to Open (now {})",
                current.label
            ),
        }
    }
}

pub struct SalesOrderSetStatus;

#[async_trait]
impl Operation for SalesOrderSetStatus {
    fn name(&self) -> &'static str {
        NAME
    }

    async fn build(&self, tenant: &TenantId, reader: &dyn Reader, input: &Value) -> Result<Built> {
        let Input {
            order_type,
            order_id,
            status,
        } = serde_json::from_value(input.clone()).map_err(|e| invalid(e.to_string()))?;
        path_safe("type", &order_type, 2)?;
        path_safe("orderId", &order_id, 15)?;
        let step = Move::parse(&status)?;

        let fresh = read_order(tenant, reader, &order_type, &order_id).await?;
        let label = format!("{order_type}/{order_id}");
        step.check(&fresh.status, &label)?;

        Ok(Built {
            summary: step.summary(&label, &fresh.status),
            request: WriteRequest {
                target: WriteTarget::SalesOrder,
                method: WriteMethod::Patch,
                path: fresh.path,
                body: json!({"status": step.target()}),
                if_match: Some(fresh.etag),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::InteractiveError;
    use crate::transport::ReadResponse;

    struct Stub(ReadResponse);
    #[async_trait]
    impl Reader for Stub {
        async fn get(&self, _: &TenantId, target: WriteTarget, path: &str) -> Result<ReadResponse> {
            assert_eq!(target, WriteTarget::SalesOrder);
            assert_eq!(path, "/api/v3/SalesOrders/SO/000100");
            Ok(self.0.clone())
        }
    }

    fn order(description: &str, hold: bool, cancelled: bool) -> Stub {
        Stub(ReadResponse {
            status: 200,
            body: json!({"orderId": "000100",
                "status": {"description": description, "cancelled": cancelled, "hold": hold}})
            .to_string(),
            etag: Some("\"v7\"".into()),
        })
    }

    fn open() -> Stub {
        order("Open", false, false)
    }

    fn held() -> Stub {
        order("On Hold", true, false)
    }

    fn input(status: &str) -> Value {
        json!({"type": "SO", "orderId": "000100", "status": status})
    }

    async fn build(stub: &Stub, input: Value) -> Result<Built> {
        SalesOrderSetStatus
            .build(&TenantId::new("t"), stub, &input)
            .await
    }

    fn refusal(result: Result<Built>) -> String {
        match result {
            Err(InteractiveError::InvalidInput(message)) => message,
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_open_order_is_put_on_hold_with_the_fresh_version() {
        let built = build(&open(), input("Hold")).await.unwrap();
        assert_eq!(built.request.target, WriteTarget::SalesOrder);
        assert_eq!(built.request.method, WriteMethod::Patch);
        assert_eq!(built.request.path, "/api/v3/SalesOrders/SO/000100");
        assert_eq!(built.request.body, json!({"status": "Hold"}));
        assert_eq!(built.request.if_match.as_deref(), Some("\"v7\""));
        assert_eq!(
            built.summary,
            "Put sales order SO/000100 on hold (now Open)"
        );
        assert!(built.request.validate().is_ok());
    }

    #[tokio::test]
    async fn a_held_order_is_released_to_open() {
        let built = build(&held(), input("Open")).await.unwrap();
        assert_eq!(built.request.body, json!({"status": "Open"}));
        assert_eq!(
            built.summary,
            "Release sales order SO/000100 from hold to Open (now On Hold)"
        );
    }

    #[tokio::test]
    async fn the_status_is_matched_without_case_and_sent_in_the_api_spelling() {
        assert_eq!(
            build(&open(), input("hold")).await.unwrap().request.body,
            json!({"status": "Hold"})
        );
        assert_eq!(
            build(&held(), input(" OPEN ")).await.unwrap().request.body,
            json!({"status": "Open"})
        );
    }

    #[tokio::test]
    async fn only_hold_and_open_are_offered() {
        for status in ["Cancelled", "Completed", "BackOrder", "", "Shipping"] {
            let message = refusal(build(&open(), input(status)).await);
            assert!(
                message.contains("\"Hold\" or \"Open\""),
                "{status}: {message}"
            );
        }
    }

    #[tokio::test]
    async fn a_hold_is_refused_unless_the_order_is_plainly_open() {
        let message = refusal(build(&held(), input("Hold")).await);
        assert!(message.contains("already on hold"), "{message}");
        let message = refusal(build(&order("Back Order", false, false), input("Hold")).await);
        assert!(message.contains("only an Open order"), "{message}");
        let message = refusal(build(&order("unknown", false, false), input("Hold")).await);
        assert!(message.contains("only an Open order"), "{message}");
    }

    #[tokio::test]
    async fn an_order_read_back_as_hold_is_released_like_one_read_back_as_on_hold() {
        let built = build(&order("Hold", true, false), input("Open"))
            .await
            .unwrap();
        assert_eq!(built.request.body, json!({"status": "Open"}));
        let message = refusal(build(&order("Hold", true, false), input("Hold")).await);
        assert!(message.contains("already on hold"), "{message}");
    }

    #[tokio::test]
    async fn invoiced_and_voided_orders_keep_their_status_too() {
        for label in ["Invoiced", "Voided"] {
            for status in ["Hold", "Open"] {
                let message = refusal(build(&order(label, false, false), input(status)).await);
                assert!(
                    message.contains("closed orders"),
                    "{label}/{status}: {message}"
                );
            }
        }
    }

    #[tokio::test]
    async fn a_release_is_refused_unless_the_order_is_on_hold() {
        let message = refusal(build(&open(), input("Open")).await);
        assert!(message.contains("On Hold can be released"), "{message}");
        // A credit hold has its own owner in Visma, even if the API also sets the hold flag.
        let message = refusal(build(&order("Credit Hold", true, false), input("Open")).await);
        assert!(message.contains("Credit Hold"), "{message}");
        // A hold flag without the label is not trusted either.
        assert!(build(&order("Open", true, false), input("Open"))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn closed_orders_keep_their_status() {
        for stub in [
            order("Completed", false, false),
            order("Cancelled", false, true),
            order("Open", false, true),
        ] {
            for status in ["Hold", "Open"] {
                let message = refusal(build(&stub, input(status)).await);
                assert!(message.contains("closed orders"), "{message}");
            }
        }
    }

    #[tokio::test]
    async fn a_missing_order_and_a_missing_version_are_reported() {
        let missing = Stub(ReadResponse {
            status: 404,
            body: String::new(),
            etag: None,
        });
        assert!(refusal(build(&missing, input("Hold")).await).contains("not found"));
        let no_etag = Stub(ReadResponse {
            status: 200,
            body: json!({"status": {"description": "Open", "cancelled": false, "hold": false}})
                .to_string(),
            etag: Some("*****".into()),
        });
        assert!(refusal(build(&no_etag, input("Hold")).await).contains("ETag"));
        let forbidden = Stub(ReadResponse {
            status: 403,
            body: "no".into(),
            etag: None,
        });
        assert!(matches!(
            build(&forbidden, input("Hold")).await,
            Err(InteractiveError::ReadFailed { status: 403, .. })
        ));
    }

    #[tokio::test]
    async fn bad_input_is_refused_before_anything_is_read() {
        struct NoReads;
        #[async_trait]
        impl Reader for NoReads {
            async fn get(&self, _: &TenantId, _: WriteTarget, _: &str) -> Result<ReadResponse> {
                panic!("a refused input must not read");
            }
        }
        for bad in [
            json!({"type": "SO/", "orderId": "1", "status": "Hold"}),
            json!({"type": "SO", "orderId": "../x", "status": "Hold"}),
            json!({"type": "SO", "orderId": "1"}),
            json!({"type": "SO", "orderId": "1", "status": "Hold", "extra": 1}),
            json!({"type": "SO", "orderId": "1", "status": "Cancelled"}),
            json!("SO/1"),
        ] {
            let result = SalesOrderSetStatus
                .build(&TenantId::new("t"), &NoReads, &bad)
                .await;
            assert!(
                matches!(result, Err(InteractiveError::InvalidInput(_))),
                "{bad}"
            );
        }
    }
}
