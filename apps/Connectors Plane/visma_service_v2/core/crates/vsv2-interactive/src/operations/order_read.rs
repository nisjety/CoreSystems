//! What the sales-order operations share: identifier checks, and the fresh read of an order with its
//! version (ETag) and status. Every operation that changes an existing order starts here, so the
//! version check and the closed-order rule are written once.

use serde_json::Value;
use vsv2_common::TenantId;

use crate::error::{InteractiveError, Result};
use crate::request::WriteTarget;
use crate::transport::Reader;

/// Orders in these states are closed; changing them is refused before any request is built.
const CLOSED_STATUSES: &[&str] = &["completed", "cancelled", "canceled", "invoiced", "voided"];

pub(super) fn invalid(message: impl Into<String>) -> InteractiveError {
    InteractiveError::InvalidInput(message.into())
}

/// Letters, digits, `-` and `_` only, so the value is safe to put in a URL path.
pub(super) fn path_safe(label: &str, value: &str, max: usize) -> Result<()> {
    let ok = !value.is_empty()
        && value.len() <= max
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then_some(()).ok_or_else(|| {
        invalid(format!(
            "{label} must be 1-{max} chars of A-Z a-z 0-9 - _, got {value:?}"
        ))
    })
}

/// The order's status as the API reports it: `{"description": "On Hold", "cancelled": false,
/// "hold": true}` (verified live 2026-10-09 and 2026-10-14). A plain string is also accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OrderStatus {
    /// What Visma calls the status, or `unknown` when the order carries none.
    pub label: String,
    /// The user-hold flag.
    pub hold: bool,
    /// Completed or cancelled: the order no longer changes.
    pub closed: bool,
}

impl OrderStatus {
    pub(super) fn of(order: &Value) -> Self {
        let status = order.get("status");
        let label = status
            .and_then(|v| {
                v.as_str()
                    .or_else(|| v.get("description").and_then(Value::as_str))
            })
            .unwrap_or("unknown")
            .to_string();
        let flag =
            |name: &str| status.and_then(|v| v.get(name)).and_then(Value::as_bool) == Some(true);
        let closed =
            flag("cancelled") || CLOSED_STATUSES.contains(&label.to_ascii_lowercase().as_str());
        Self {
            hold: flag("hold"),
            label,
            closed,
        }
    }

    pub(super) fn is(&self, label: &str) -> bool {
        self.label.eq_ignore_ascii_case(label)
    }

    /// Whether the order carries a status at all. An order without one is not changed: the shape of the answer is
    /// not what this code was written for.
    pub(super) fn readable(&self) -> bool {
        self.label != "unknown"
    }

    /// Held by a person: the hold flag, and Visma's own word for it (the API reads it back as `On Hold`; `Hold`
    /// is the word it is written with). A credit hold or an approval hold is a different state with another owner.
    pub(super) fn on_hold(&self) -> bool {
        self.hold && (self.is("On Hold") || self.is("Hold"))
    }
}

/// An order as it is right now, with the version a change must be made against.
#[derive(Debug)]
pub(super) struct FreshOrder {
    pub path: String,
    pub etag: String,
    pub status: OrderStatus,
}

/// `GET /api/v3/SalesOrders/{type}/{orderId}`: the path, a usable ETag and the status. A missing order
/// is reported as such; a missing or wildcard ETag is refused, because a change without a version
/// would overwrite whatever someone else did in between.
pub(super) async fn read_order(
    tenant: &TenantId,
    reader: &dyn Reader,
    order_type: &str,
    order_id: &str,
) -> Result<FreshOrder> {
    path_safe("type", order_type, 2)?;
    path_safe("orderId", order_id, 15)?;
    let path = format!("/api/v3/SalesOrders/{order_type}/{order_id}");
    let current = reader.get(tenant, WriteTarget::SalesOrder, &path).await?;
    match current.status {
        200 => {}
        404 => {
            return Err(invalid(format!(
                "sales order {order_type}/{order_id} not found"
            )))
        }
        status => {
            return Err(InteractiveError::ReadFailed {
                status,
                body: vsv2_common::text::one_line(&current.body, 300),
            })
        }
    }
    let etag = current
        .etag
        .filter(|e| !e.trim().is_empty() && !e.trim().chars().all(|c| c == '*'))
        .ok_or_else(|| {
            invalid("API returned no usable ETag for the order; refusing to change it without a version")
        })?;
    let order: Value = serde_json::from_str(&current.body)
        .map_err(|e| invalid(format!("could not parse current order: {e}")))?;
    Ok(FreshOrder {
        path,
        etag,
        status: OrderStatus::of(&order),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_status_object_and_a_plain_string_are_both_read() {
        let held = OrderStatus::of(
            &json!({"status": {"description": "On Hold", "cancelled": false, "hold": true}}),
        );
        assert_eq!(
            (held.label.as_str(), held.hold, held.closed),
            ("On Hold", true, false)
        );
        let plain = OrderStatus::of(&json!({"status": "Cancelled"}));
        assert!(plain.closed && !plain.hold);
        let none = OrderStatus::of(&json!({}));
        assert_eq!((none.label.as_str(), none.closed), ("unknown", false));
    }

    #[test]
    fn the_cancelled_flag_closes_an_order_whatever_its_description() {
        let status = OrderStatus::of(
            &json!({"status": {"description": "Open", "cancelled": true, "hold": false}}),
        );
        assert!(status.closed);
    }

    #[test]
    fn invoiced_and_voided_orders_are_closed_and_a_missing_status_is_not_readable() {
        for label in ["Invoiced", "Voided"] {
            assert!(
                OrderStatus::of(&json!({"status": {"description": label}})).closed,
                "{label}"
            );
        }
        assert!(!OrderStatus::of(&json!({})).readable());
        assert!(OrderStatus::of(&json!({"status": "Open"})).readable());
    }

    #[test]
    fn a_hold_is_a_person_s_hold_in_either_of_visma_s_spellings_but_never_a_credit_hold() {
        let status = |label: &str, hold: bool| {
            OrderStatus::of(&json!({"status": {"description": label, "hold": hold}}))
        };
        assert!(status("On Hold", true).on_hold() && status("Hold", true).on_hold());
        assert!(!status("On Hold", false).on_hold(), "the flag must agree");
        assert!(!status("Credit Hold", true).on_hold() && !status("Open", true).on_hold());
    }

    #[test]
    fn labels_compare_without_case() {
        let status = OrderStatus::of(&json!({"status": {"description": "On hold", "hold": true}}));
        assert!(status.is("On Hold") && !status.is("Open"));
    }

    #[tokio::test]
    async fn a_failed_read_is_reported_as_a_read_in_one_bounded_line() {
        struct Broken;
        #[async_trait::async_trait]
        impl Reader for Broken {
            async fn get(
                &self,
                _: &TenantId,
                _: WriteTarget,
                _: &str,
            ) -> Result<crate::transport::ReadResponse> {
                Ok(crate::transport::ReadResponse {
                    status: 502,
                    body: format!("<html>\u{1b}[31m\n{}", "gateway ".repeat(20_000)),
                    etag: None,
                })
            }
        }
        let e = read_order(&TenantId::new("t"), &Broken, "SO", "1")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            e.starts_with("could not read the record (status 502)"),
            "{e}"
        );
        assert!(
            !e.contains("refused the write") && !e.contains('\n') && !e.contains('\u{1b}'),
            "{e}"
        );
        assert!(e.chars().count() < 400, "{}", e.chars().count());
    }

    #[test]
    fn identifiers_must_be_safe_in_a_url_path() {
        assert!(path_safe("orderId", "000100", 15).is_ok());
        for bad in ["", "a b", "../x", "SO/", "1234567890123456"] {
            assert!(path_safe("orderId", bad, 15).is_err(), "{bad:?}");
        }
    }
}
