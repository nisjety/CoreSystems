//! `salesorder.create`: create a sales order with `POST /api/v3/SalesOrders` on the SalesOrders v3
//! API, scope `visma.net.erp.salesorder:write` (the one `salesorder.patch` already uses).
//!
//! Input: the request body of that endpoint (`NewSalesOrderCommand`). Its shape comes from
//! `schemas/salesorder_v3_create.json`, which `scripts/build_api_reference.py` derives from the API's
//! own Swagger document. Rules beyond the API:
//! - the order is created ON HOLD. `status` must be absent, null or `"Hold"`; it is sent as `"Hold"`.
//!   Releasing it is a separate, approved step (`salesorder.set_status`).
//! - the order number comes from Visma's number series: `orderId` and `overrideNumberSeries` are refused.
//! - a customer by `customer.id` or `customer.gln`, and at least one line with an `inventoryId` and a
//!   positive `quantity`. The schema only requires `customer` and `type`, and says nothing of the rest.
//!
//! Unknown fields are refused, because the API would drop them silently.

use std::sync::OnceLock;

use async_trait::async_trait;
use serde_json::{json, Value};
use vsv2_common::TenantId;

use super::order_read::{invalid, path_safe};
use crate::error::Result;
use crate::operation::{Built, Operation};
use crate::request::{WriteMethod, WriteRequest, WriteTarget};
use crate::schema;
use crate::transport::Reader;

pub const NAME: &str = "salesorder.create";
const PATH: &str = "/api/v3/SalesOrders";
const SCHEMA_JSON: &str = include_str!("../../schemas/salesorder_v3_create.json");
const HOLD: &str = "Hold";
/// The most lines one create takes; the rest are added with `salesorder.patch`. Also what an approver can be shown.
const MAX_LINES: usize = 200;
const SUMMARY_TEXT_CHARS: usize = 60;

/// The request-body schema, parsed once. `build_api_reference.py` writes it and a test parses it, so
/// the only way this fails is a broken checked-in file, which the tests catch first.
fn body_schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let document: Value =
            serde_json::from_str(SCHEMA_JSON).expect("salesorder_v3_create.json is valid JSON");
        document["schema"].clone()
    })
}

pub struct SalesOrderCreate;

#[async_trait]
impl Operation for SalesOrderCreate {
    fn name(&self) -> &'static str {
        NAME
    }

    async fn build(
        &self,
        _tenant: &TenantId,
        _reader: &dyn Reader,
        input: &Value,
    ) -> Result<Built> {
        // Creating an order reads nothing first: there is no version to protect yet.
        let mut body = input.clone();
        without_empty_status(&mut body);
        check_body(&body)?;
        body["status"] = json!(HOLD);
        let request = WriteRequest {
            target: WriteTarget::SalesOrder,
            method: WriteMethod::Post,
            path: PATH.into(),
            body,
            if_match: None,
        };
        request.validate()?;
        let summary = summarize(&request.body);
        Ok(Built { request, summary })
    }
}

/// The API accepts a null or empty `status` as "not supplied"; so does this operation.
fn without_empty_status(body: &mut Value) {
    let empty = match body.get("status") {
        Some(Value::Null) => true,
        Some(Value::String(status)) => status.trim().is_empty(),
        _ => false,
    };
    if empty {
        if let Some(object) = body.as_object_mut() {
            object.remove("status");
        }
    }
}

/// Field-level problems first (names, types, required fields), then the rules the API does not state.
fn check_body(body: &Value) -> Result<()> {
    if let Err(problems) = schema::check(body_schema(), body) {
        return Err(invalid(problems.join("; ")));
    }
    path_safe("type", body["type"].as_str().unwrap_or(""), 2)?;
    for refused in ["orderId", "overrideNumberSeries"] {
        if body.get(refused).is_some() {
            return Err(invalid(format!(
                "`{refused}` is not accepted: the order number comes from Visma's number series (order types that need a number typed in are not supported)"
            )));
        }
    }
    for linked in ["originalOrderType", "originalOrderId"] {
        if body.get(linked).is_some() {
            return Err(invalid(format!(
                "`{linked}` is not accepted: linking a new order to an existing one (a copy, a return) is not offered"
            )));
        }
    }
    check_status(body)?;
    let customer = &body["customer"];
    if customer.get("id").is_none_or(Value::is_null)
        && customer.get("gln").is_none_or(Value::is_null)
    {
        return Err(invalid(
            "set customer.id (or customer.gln): the order needs a customer",
        ));
    }
    for field in ["id", "gln"] {
        if !customer[field].is_null() {
            plain(&customer[field], &format!("customer.{field}"))?;
        }
    }
    let lines = body
        .get("orderLines")
        .and_then(Value::as_array)
        .filter(|lines| !lines.is_empty())
        .ok_or_else(|| {
            invalid("at least one line is required (orderLines[].inventoryId and quantity)")
        })?;
    if lines.len() > MAX_LINES {
        return Err(invalid(format!(
            "an order takes at most {MAX_LINES} lines in one create, got {}: add the rest afterwards with salesorder.patch (newOrderLines)",
            lines.len()
        )));
    }
    for (index, line) in lines.iter().enumerate() {
        plain(
            &line["inventoryId"],
            &format!("orderLines[{index}].inventoryId"),
        )?;
        if line["quantity"].as_f64().filter(|q| *q > 0.0).is_none() {
            return Err(invalid(format!(
                "orderLines[{index}].quantity must be a number greater than zero"
            )));
        }
    }
    Ok(())
}

fn check_status(body: &Value) -> Result<()> {
    match body.get("status").and_then(Value::as_str) {
        None => Ok(()),
        Some(status) if status.eq_ignore_ascii_case(HOLD) => Ok(()),
        Some(status) => Err(invalid(format!(
            "orders are created on hold: `status` must be absent or \"Hold\", got {status:?}. \
             Release the order with salesorder.set_status after it has been checked."
        ))),
    }
}

/// A non-empty string, trimmed.
fn text(node: &Value) -> Option<&str> {
    node.as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// An identifier as it is sent: one line of plain text with no control characters, bidirectional overrides or
/// stray spaces, so what the approver reads is what Visma receives.
fn plain<'a>(node: &'a Value, label: &str) -> Result<&'a str> {
    let text = node
        .as_str()
        .ok_or_else(|| invalid(format!("{label} must be a string")))?;
    if text.is_empty() || text != vsv2_common::text::one_line(text, usize::MAX) {
        return Err(invalid(format!(
            "{label} must be plain text on one line, not empty and without leading, trailing or repeated spaces or control characters"
        )));
    }
    Ok(text)
}

/// One line for the approver: what is created, for whom, and the first items with their quantities.
fn summarize(body: &Value) -> String {
    let clip = |s: &str| vsv2_common::text::one_line(s, SUMMARY_TEXT_CHARS);
    let order_type = clip(text(&body["type"]).unwrap_or("?"));
    let customer = clip(
        text(&body["customer"]["id"])
            .or_else(|| text(&body["customer"]["gln"]))
            .unwrap_or("?"),
    );
    let lines = body["orderLines"].as_array().map(Vec::len).unwrap_or(0);
    let items: Vec<String> = body["orderLines"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|line| {
            let item = clip(text(&line["inventoryId"])?);
            let quantity = line["quantity"].as_f64()?;
            Some(format!("{item} x{quantity}"))
        })
        .take(3)
        .collect();
    let more = lines.saturating_sub(items.len());
    let tail = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    format!(
        "Create sales order {order_type} for {customer} with {lines} line(s) (on hold): {}{tail}",
        items.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::InteractiveError;
    use crate::transport::ReadResponse;

    /// Creating an order must never read anything; a read here would be a test failure.
    struct NoReads;
    #[async_trait]
    impl Reader for NoReads {
        async fn get(&self, _: &TenantId, _: WriteTarget, _: &str) -> Result<ReadResponse> {
            Err(InteractiveError::Denied(
                "the create operation must not read".into(),
            ))
        }
    }

    fn order() -> Value {
        json!({
            "type": "SO",
            "customer": {"id": "C-1001"},
            "orderLines": [
                {"inventoryId": "ITEM-1", "quantity": 2},
                {"inventoryId": "ITEM-2", "quantity": 1.5}
            ]
        })
    }

    async fn build(input: Value) -> Result<Built> {
        SalesOrderCreate
            .build(&TenantId::new("t"), &NoReads, &input)
            .await
    }

    fn invalid_message(error: InteractiveError) -> String {
        match error {
            InteractiveError::InvalidInput(message) => message,
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn builds_a_post_to_the_v3_order_endpoint_on_the_sales_order_target() {
        let built = build(order()).await.unwrap();
        assert_eq!(built.request.target, WriteTarget::SalesOrder);
        assert_eq!(built.request.method, WriteMethod::Post);
        assert_eq!(built.request.path, "/api/v3/SalesOrders");
        assert_eq!(built.request.if_match, None);
    }

    #[tokio::test]
    async fn the_order_is_sent_on_hold_whether_or_not_the_caller_says_so() {
        assert_eq!(build(order()).await.unwrap().request.body["status"], "Hold");
        for status in [json!("Hold"), json!("hold"), json!(null), json!("")] {
            let mut input = order();
            input["status"] = status;
            assert_eq!(build(input).await.unwrap().request.body["status"], "Hold");
        }
    }

    #[tokio::test]
    async fn opening_the_order_at_creation_is_refused() {
        for status in ["Open", "Completed"] {
            let mut input = order();
            input["status"] = json!(status);
            let message = invalid_message(build(input).await.unwrap_err());
            assert!(message.contains("created on hold"), "{message}");
        }
    }

    #[tokio::test]
    async fn the_summary_names_the_order_customer_lines_and_hold() {
        let summary = build(order()).await.unwrap().summary;
        assert_eq!(
            summary,
            "Create sales order SO for C-1001 with 2 line(s) (on hold): ITEM-1 x2, ITEM-2 x1.5"
        );
        let mut many = order();
        many["orderLines"] = json!([
            {"inventoryId": "A", "quantity": 1}, {"inventoryId": "B", "quantity": 1},
            {"inventoryId": "C", "quantity": 1}, {"inventoryId": "D", "quantity": 1}
        ]);
        assert!(build(many)
            .await
            .unwrap()
            .summary
            .ends_with("A x1, B x1, C x1 and 1 more"));
    }

    #[tokio::test]
    async fn a_misspelt_field_is_refused_with_its_path() {
        let mut input = order();
        input["costumer"] = json!({"id": "C-1001"});
        let message = invalid_message(build(input).await.unwrap_err());
        assert!(message.contains("$.costumer: unknown field"), "{message}");

        let mut nested = order();
        nested["customer"]["idd"] = json!("x");
        let message = invalid_message(build(nested).await.unwrap_err());
        assert!(
            message.contains("$.customer.idd: unknown field"),
            "{message}"
        );
    }

    #[tokio::test]
    async fn the_schema_requires_a_customer_and_a_type() {
        let mut input = order();
        input.as_object_mut().unwrap().remove("type");
        assert!(invalid_message(build(input).await.unwrap_err()).contains("$.type: required"));
        let mut input = order();
        input.as_object_mut().unwrap().remove("customer");
        assert!(invalid_message(build(input).await.unwrap_err()).contains("$.customer: required"));
    }

    #[tokio::test]
    async fn an_order_needs_a_line_with_an_item_and_a_positive_quantity() {
        let mut none = order();
        none["orderLines"] = json!([]);
        assert!(invalid_message(build(none).await.unwrap_err()).contains("at least one line"));

        let mut absent = order();
        absent.as_object_mut().unwrap().remove("orderLines");
        assert!(invalid_message(build(absent).await.unwrap_err()).contains("at least one line"));

        let mut no_item = order();
        no_item["orderLines"] = json!([{"quantity": 1}]);
        assert!(invalid_message(build(no_item).await.unwrap_err()).contains("inventoryId"));

        for quantity in [json!(0), json!(-1)] {
            let mut bad = order();
            bad["orderLines"] = json!([{"inventoryId": "ITEM-1", "quantity": quantity}]);
            assert!(invalid_message(build(bad).await.unwrap_err()).contains("greater than zero"));
        }
        let mut not_a_number = order();
        not_a_number["orderLines"] = json!([{"inventoryId": "ITEM-1", "quantity": "2"}]);
        assert!(invalid_message(build(not_a_number).await.unwrap_err()).contains("expected number"));
        let mut no_quantity = order();
        no_quantity["orderLines"] = json!([{"inventoryId": "ITEM-1"}]);
        assert!(
            invalid_message(build(no_quantity).await.unwrap_err()).contains("greater than zero")
        );
    }

    #[tokio::test]
    async fn an_order_needs_a_customer_id_or_gln() {
        let mut nobody = order();
        nobody["customer"] = json!({"refNo": "PO-7"});
        assert!(invalid_message(build(nobody).await.unwrap_err()).contains("customer.id"));

        let mut by_gln = order();
        by_gln["customer"] = json!({"gln": "7040000000013"});
        let built = build(by_gln).await.unwrap();
        assert!(built.summary.contains("7040000000013"), "{}", built.summary);
    }

    #[tokio::test]
    async fn the_order_number_always_comes_from_the_number_series() {
        for (field, value) in [
            ("orderId", json!("000999")),
            ("overrideNumberSeries", json!(true)),
        ] {
            let mut input = order();
            input[field] = value;
            let message = invalid_message(build(input).await.unwrap_err());
            assert!(message.contains("number series"), "{field}: {message}");
        }
    }

    #[tokio::test]
    async fn an_order_takes_at_most_200_lines_so_the_approver_can_see_every_one() {
        let lines = |n: usize| -> Value {
            (0..n)
                .map(|i| json!({"inventoryId": format!("ITEM-{i}"), "quantity": 1}))
                .collect()
        };
        let mut ok = order();
        ok["orderLines"] = lines(MAX_LINES);
        assert!(build(ok).await.is_ok());
        let mut too_many = order();
        too_many["orderLines"] = lines(MAX_LINES + 1);
        let message = invalid_message(build(too_many).await.unwrap_err());
        assert!(
            message.contains("at most 200 lines") && message.contains("salesorder.patch"),
            "{message}"
        );
    }

    #[tokio::test]
    async fn linking_the_new_order_to_another_one_is_not_offered() {
        for field in ["originalOrderType", "originalOrderId"] {
            let mut input = order();
            input[field] = json!("SO");
            let message = invalid_message(build(input).await.unwrap_err());
            assert!(
                message.contains("not accepted") && message.contains(field),
                "{message}"
            );
        }
    }

    #[tokio::test]
    async fn identifiers_are_sent_exactly_as_the_approver_reads_them() {
        for (path, bad) in [
            ("customer.id", " C-1001"),
            ("customer.id", "C-1001 "),
            ("customer.id", "C-1\nTenant: Aquatiq AS (verified)"),
            ("customer.id", "C\u{202E}1001"),
            ("customer.id", "C  1001"),
            ("line", "  ITEM-1"),
            ("line", "ITEM\r1"),
        ] {
            let mut input = order();
            if path == "line" {
                input["orderLines"][0]["inventoryId"] = json!(bad);
            } else {
                input["customer"]["id"] = json!(bad);
            }
            let message = invalid_message(build(input).await.unwrap_err());
            assert!(
                message.contains("plain text on one line"),
                "{bad:?}: {message}"
            );
        }
    }

    #[tokio::test]
    async fn an_optional_field_given_as_null_is_accepted_and_sent_as_the_api_documents_it() {
        let mut input = order();
        input["orderLines"][0]["unitPrice"] = Value::Null;
        input["description"] = Value::Null;
        let built = build(input).await.unwrap();
        assert!(built.request.body["orderLines"][0]["unitPrice"].is_null());
        // But a required field cannot be null.
        let mut missing = order();
        missing["customer"] = Value::Null;
        assert!(build(missing).await.is_err());
    }

    #[tokio::test]
    async fn the_summary_clips_long_identifiers() {
        let mut input = order();
        input["customer"]["id"] = json!("C".repeat(500));
        let summary = build(input).await.unwrap().summary;
        assert!(summary.chars().count() < 200, "{}", summary.chars().count());
    }

    #[tokio::test]
    async fn the_order_type_must_be_safe_and_short() {
        for bad in ["", "SOX", "S/"] {
            let mut input = order();
            input["type"] = json!(bad);
            assert!(build(input).await.is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn a_body_that_is_not_an_object_is_refused() {
        let message = invalid_message(build(json!(["SO"])).await.unwrap_err());
        assert!(message.contains("expected object"), "{message}");
    }

    #[test]
    fn the_embedded_schema_is_the_v3_order_body() {
        let schema = body_schema();
        assert_eq!(schema["required"], json!(["customer", "type"]));
        assert!(
            schema["properties"]["orderLines"]["items"]["properties"]["inventoryId"].is_object()
        );
        assert_eq!(
            schema["properties"]["customer"]["additionalProperties"],
            json!(false)
        );
    }
}
