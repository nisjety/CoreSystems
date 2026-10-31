//! `salesorder.patch` — modify an existing sales order via
//! `PATCH /api/v3/SalesOrders/{type}/{orderId}` (SalesOrder V3, scope
//! `visma.net.erp.salesorder:write`).
//!
//! Input: `{"type": "SO", "orderId": "000100", "changes": { ...PatchSalesOrderCommand fields... }}`
//!
//! The version check is never bypassed: the ETag always comes from a fresh GET at dry-run time
//! (the API's `*****` wildcard is not offered), so a concurrent edit makes execute fail with 412
//! instead of being overwritten.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Map, Value};
use vsv2_common::TenantId;

use super::order_read::{invalid, path_safe, read_order};
use crate::error::Result;
use crate::operation::{Built, Operation};
use crate::request::{WriteMethod, WriteRequest, WriteTarget};
use crate::transport::Reader;

pub const NAME: &str = "salesorder.patch";

const MAX_LINES_PER_PATCH: usize = 200;

/// `PatchSalesOrderCommand` fields this operation accepts. Anything else is rejected (typos must
/// not be silently dropped by the API).
const ALLOWED_FIELDS: &[&str] = &[
    "date",
    "description",
    "note",
    "requestOn",
    "cancelBy",
    "salesPersonId",
    "ownerId",
    "branchId",
    "projectId",
    "customer",
    "shipping",
    "billing",
    "freight",
    "customFields",
    "orderLines",
    "newOrderLines",
    "recalculateShipment",
];

/// Fields the API has but that need their own, separately reviewed operation.
const DEFERRED_FIELDS: &[(&str, &str)] = &[
    (
        "removeLines",
        "removing lines is a deletion; Layer 3 offers no delete operations",
    ),
    (
        "status",
        "status changes are their own operation: salesorder.set_status (hold or release)",
    ),
    (
        "updateCompleted",
        "completing orders needs a dedicated operation",
    ),
    (
        "discounts",
        "discount changes (incl. remove) need a dedicated operation",
    ),
    ("attachments", "attachments need a dedicated operation"),
    (
        "currencyId",
        "currency/tax changes affect accounting; needs a dedicated operation",
    ),
    (
        "currencyRate",
        "currency/tax changes affect accounting; needs a dedicated operation",
    ),
    (
        "taxZoneId",
        "currency/tax changes affect accounting; needs a dedicated operation",
    ),
    (
        "financialInformation",
        "invoicing data needs a dedicated operation",
    ),
    (
        "paymentSettings",
        "payment settings need a dedicated operation",
    ),
    ("rotRutDetails", "ROT/RUT needs a dedicated operation"),
    ("isRotRutDeductible", "ROT/RUT needs a dedicated operation"),
    ("print", "print settings are not supported yet"),
    ("useReplacementCostForMarginAndProfit", "not supported yet"),
    ("originalOrderType", "order lineage cannot be rewritten"),
    ("originalOrderId", "order lineage cannot be rewritten"),
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(rename = "type")]
    order_type: String,
    #[serde(rename = "orderId")]
    order_id: String,
    changes: Map<String, Value>,
}

pub struct SalesOrderPatch;

fn validate_changes(changes: &Map<String, Value>) -> Result<()> {
    if changes.is_empty() {
        return Err(invalid("changes is empty; nothing to patch"));
    }
    for key in changes.keys() {
        if let Some((_, why)) = DEFERRED_FIELDS.iter().find(|(f, _)| f == key) {
            return Err(invalid(format!(
                "field `{key}` is not supported by {NAME}: {why}"
            )));
        }
        if !ALLOWED_FIELDS.contains(&key.as_str()) {
            return Err(invalid(format!(
                "unknown field `{key}`; allowed: {}",
                ALLOWED_FIELDS.join(", ")
            )));
        }
    }

    if let Some(v) = changes.get("orderLines") {
        let lines = line_array("orderLines", v)?;
        for (i, l) in lines.iter().enumerate() {
            let id = l.get("lineId").and_then(Value::as_i64).filter(|n| *n >= 1);
            if id.is_none() {
                return Err(invalid(format!(
                    "orderLines[{i}] needs an integer lineId >= 1"
                )));
            }
        }
    }
    if let Some(v) = changes.get("newOrderLines") {
        line_array("newOrderLines", v)?;
    }
    Ok(())
}

fn line_array<'a>(name: &str, v: &'a Value) -> Result<&'a Vec<Value>> {
    let arr = v
        .as_array()
        .ok_or_else(|| invalid(format!("{name} must be an array")))?;
    if arr.is_empty() || arr.len() > MAX_LINES_PER_PATCH {
        return Err(invalid(format!(
            "{name} must hold 1-{MAX_LINES_PER_PATCH} lines, got {}",
            arr.len()
        )));
    }
    if arr.iter().any(|l| !l.is_object()) {
        return Err(invalid(format!("every entry of {name} must be an object")));
    }
    Ok(arr)
}

fn summarize(
    order_type: &str,
    order_id: &str,
    status: &str,
    changes: &Map<String, Value>,
) -> String {
    let mut keys: Vec<&String> = changes.keys().collect();
    keys.sort();
    let parts: Vec<String> = keys
        .into_iter()
        .map(|k| match changes[k].as_array() {
            Some(a) => format!(
                "{k}({} line{})",
                a.len(),
                if a.len() == 1 { "" } else { "s" }
            ),
            None => k.clone(),
        })
        .collect();
    format!(
        "Patch sales order {order_type}/{order_id} (status {status}): {}",
        parts.join(", ")
    )
}

#[async_trait]
impl Operation for SalesOrderPatch {
    fn name(&self) -> &'static str {
        NAME
    }

    async fn build(&self, tenant: &TenantId, reader: &dyn Reader, input: &Value) -> Result<Built> {
        let Input {
            order_type,
            order_id,
            changes,
        } = serde_json::from_value(input.clone()).map_err(|e| invalid(e.to_string()))?;
        path_safe("type", &order_type, 2)?;
        path_safe("orderId", &order_id, 15)?;
        validate_changes(&changes)?;

        let fresh = read_order(tenant, reader, &order_type, &order_id).await?;
        if !fresh.status.readable() {
            return Err(invalid(format!(
                "sales order {order_type}/{order_id} has no readable status; it is not changed without knowing it is open"
            )));
        }
        if fresh.status.closed {
            return Err(invalid(format!(
                "sales order {order_type}/{order_id} is {}; closed orders are not patched",
                fresh.status.label
            )));
        }

        Ok(Built {
            summary: summarize(&order_type, &order_id, &fresh.status.label, &changes),
            request: WriteRequest {
                target: WriteTarget::SalesOrder,
                method: WriteMethod::Patch,
                path: fresh.path,
                body: Value::Object(changes),
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
    use serde_json::json;

    struct Stub(ReadResponse);
    #[async_trait]
    impl Reader for Stub {
        async fn get(&self, _: &TenantId, t: WriteTarget, path: &str) -> Result<ReadResponse> {
            assert_eq!(t, WriteTarget::SalesOrder);
            assert_eq!(path, "/api/v3/SalesOrders/SO/000100");
            Ok(self.0.clone())
        }
    }

    fn order(status: &str, etag: Option<&str>) -> Stub {
        Stub(ReadResponse {
            status: 200,
            body: json!({"orderId": "000100",
                "status": {"description": status, "cancelled": false, "hold": false}})
            .to_string(),
            etag: etag.map(String::from),
        })
    }

    async fn build(stub: &Stub, input: Value) -> Result<Built> {
        SalesOrderPatch
            .build(&TenantId::new("t"), stub, &input)
            .await
    }

    fn ok_input() -> Value {
        json!({"type":"SO","orderId":"000100","changes":{
            "description":"x",
            "orderLines":[{"lineId":1,"quantity":5}],
            "newOrderLines":[{"inventoryId":"ABC","quantity":1}]}})
    }

    #[tokio::test]
    async fn builds_a_patch_with_the_fresh_etag_and_a_readable_summary() {
        let b = build(&order("Open", Some("\"v7\"")), ok_input())
            .await
            .unwrap();
        assert_eq!(b.request.method, WriteMethod::Patch);
        assert_eq!(b.request.path, "/api/v3/SalesOrders/SO/000100");
        assert_eq!(b.request.if_match.as_deref(), Some("\"v7\""));
        assert_eq!(b.request.body["description"], "x");
        assert_eq!(
            b.summary,
            "Patch sales order SO/000100 (status Open): description, newOrderLines(1 line), orderLines(1 line)"
        );
        assert!(b.request.validate().is_ok());
    }

    #[tokio::test]
    async fn refuses_missing_or_wildcard_etag() {
        for etag in [None, Some(""), Some("*****"), Some("*"), Some(" ** ")] {
            let r = build(&order("Open", etag), ok_input()).await;
            assert!(
                matches!(r, Err(InteractiveError::InvalidInput(_))),
                "{etag:?}"
            );
        }
    }

    #[tokio::test]
    async fn refuses_closed_orders_case_insensitively() {
        for s in ["Completed", "cancelled", "CANCELED"] {
            let r = build(&order(s, Some("\"v\"")), ok_input()).await;
            assert!(
                matches!(r, Err(InteractiveError::InvalidInput(m)) if m.contains("closed")),
                "{s}"
            );
        }
    }

    #[tokio::test]
    async fn invoiced_and_voided_orders_are_closed_and_an_order_without_a_status_is_not_patched() {
        for label in ["Invoiced", "Voided"] {
            let r = build(&order(label, Some("\"v\"")), ok_input()).await;
            assert!(
                matches!(r, Err(InteractiveError::InvalidInput(m)) if m.contains("closed")),
                "{label}"
            );
        }
        let none = Stub(ReadResponse {
            status: 200,
            body: json!({"orderId": "000100"}).to_string(),
            etag: Some("\"v\"".into()),
        });
        let r = build(&none, ok_input()).await;
        assert!(
            matches!(r, Err(InteractiveError::InvalidInput(m)) if m.contains("no readable status"))
        );
    }

    #[tokio::test]
    async fn the_cancelled_flag_closes_an_order_whatever_its_description() {
        let stub = Stub(ReadResponse {
            status: 200,
            body: json!({"status": {"description": "Open", "cancelled": true, "hold": false}})
                .to_string(),
            etag: Some("\"v\"".into()),
        });
        let r = build(&stub, ok_input()).await;
        assert!(matches!(r, Err(InteractiveError::InvalidInput(m)) if m.contains("closed")));
    }

    #[tokio::test]
    async fn a_held_order_can_still_be_patched_and_a_plain_string_status_is_accepted() {
        let held = Stub(ReadResponse {
            status: 200,
            body: json!({"status": {"description": "On Hold", "cancelled": false, "hold": true}})
                .to_string(),
            etag: Some("\"v\"".into()),
        });
        assert!(build(&held, ok_input())
            .await
            .unwrap()
            .summary
            .contains("On Hold"));
        let plain = Stub(ReadResponse {
            status: 200,
            body: json!({"status": "Cancelled"}).to_string(),
            etag: Some("\"v\"".into()),
        });
        assert!(build(&plain, ok_input()).await.is_err());
    }

    #[tokio::test]
    async fn maps_404_to_a_clear_error_and_other_statuses_to_upstream() {
        let nf = Stub(ReadResponse {
            status: 404,
            body: "".into(),
            etag: None,
        });
        assert!(
            matches!(build(&nf, ok_input()).await, Err(InteractiveError::InvalidInput(m)) if m.contains("not found"))
        );
        let forbidden = Stub(ReadResponse {
            status: 403,
            body: "no".into(),
            etag: None,
        });
        assert!(matches!(
            build(&forbidden, ok_input()).await,
            Err(InteractiveError::ReadFailed { status: 403, .. })
        ));
    }

    fn rejects(input: Value, needle: &str) {
        let stub = order("Open", Some("\"v\""));
        let r = tokio_test::block_on(build(&stub, input));
        match r {
            Err(InteractiveError::InvalidInput(m)) => {
                assert!(
                    m.contains(needle),
                    "message {m:?} should mention {needle:?}"
                )
            }
            other => panic!("expected InvalidInput containing {needle:?}, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unsafe_identifiers() {
        for (t, id) in [
            ("SO/", "1"),
            ("SOX", "1"),
            ("SO", "../x"),
            ("SO", "a b"),
            ("SO", ""),
            ("", "1"),
            ("SO", "1234567890123456"),
        ] {
            rejects(
                json!({"type":t,"orderId":id,"changes":{"description":"x"}}),
                "must be",
            );
        }
    }

    #[test]
    fn deferred_and_unknown_fields_are_rejected_with_a_reason() {
        rejects(
            json!({"type":"SO","orderId":"1","changes":{"removeLines":[1]}}),
            "deletion",
        );
        rejects(
            json!({"type":"SO","orderId":"1","changes":{"status":"Cancelled"}}),
            "status",
        );
        rejects(
            json!({"type":"SO","orderId":"1","changes":{"currencyId":"EUR"}}),
            "accounting",
        );
        rejects(
            json!({"type":"SO","orderId":"1","changes":{"descriptoin":"typo"}}),
            "unknown field",
        );
    }

    #[test]
    fn shape_errors_are_rejected() {
        rejects(json!({"type":"SO","orderId":"1","changes":{}}), "empty");
        rejects(json!({"type":"SO","orderId":"1"}), "changes");
        rejects(
            json!({"type":"SO","orderId":"1","changes":{"description":"x"},"extra":1}),
            "extra",
        );
        rejects(
            json!({"type":"SO","orderId":"1","changes":{"orderLines":[{"quantity":1}]}}),
            "lineId",
        );
        rejects(
            json!({"type":"SO","orderId":"1","changes":{"orderLines":[{"lineId":0}]}}),
            "lineId",
        );
        rejects(
            json!({"type":"SO","orderId":"1","changes":{"orderLines":[]}}),
            "1-200",
        );
        rejects(
            json!({"type":"SO","orderId":"1","changes":{"newOrderLines":"x"}}),
            "array",
        );
        rejects(
            json!({"type":"SO","orderId":"1","changes":{"newOrderLines":[5]}}),
            "object",
        );
    }
}
