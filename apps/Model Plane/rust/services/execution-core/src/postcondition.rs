//! Deterministic postcondition verifiers (verevon-roadmap.md §3b, P1 item 3).
//!
//! # What this is for
//!
//! The Verified Outcome Foundation's first producer
//! ([`crate::approval_delivery_worker`]) judges an outcome *structurally*:
//! "the boundary handed back an authoritative receipt id, so call it
//! verified". That is a real signal but a weak one — it proves the provider
//! answered, not that the effect exists. A provider can return an id for
//! work it later drops, and a compromised or buggy boundary can return an id
//! for work it never did.
//!
//! A **postcondition** verifier answers the stronger question by going back
//! to the system of record and asking whether the effect is actually there.
//! The distinction is carried on the wire by
//! [`pb::VerificationResult::method`] — `"structural"` vs `"postcondition"` —
//! precisely so a reader can tell today's mechanical check from a real one,
//! and so a future stronger check is never retroactively credited to an
//! older weaker judgment.
//!
//! # The invariant that matters
//!
//! **A verifier may only ever strengthen a claim it actually checked.** When
//! it cannot reach the provider, has no verifier for the action, or gets an
//! ambiguous answer, it returns [`PostconditionOutcome::Inconclusive`] and
//! the caller keeps the existing structural judgment unchanged. An outage
//! must never read as a refutation, and an unreachable provider must never
//! silently upgrade a claim to "independently verified".
//!
//! The one direction that *does* override: a [`PostconditionOutcome::Refuted`]
//! turns a structural success into a verified failure. Catching exactly that
//! case — the boundary said yes, the system of record says no — is the whole
//! reason this layer exists (`Verevon-ai-first.md` §8 risk 7,
//! "false-success risk").

use serde_json::Value;

use crate::shipping_tools::ShippingToolsClient;

/// Booking states that mean shipping-core really holds this shipment. Kept
/// deliberately narrow: anything not named here is not treated as confirmed.
const CONFIRMED_BOOKING_STATUSES: &[&str] = &["booked", "confirmed", "in_transit", "delivered"];

/// Booking states that positively contradict a claimed successful booking.
const REFUTING_BOOKING_STATUSES: &[&str] = &["cancelled", "canceled", "failed", "rejected"];

/// The three-valued judgment a postcondition verifier can reach. Three, not
/// two, because "I could not tell" must be representable — collapsing it into
/// either success or failure is how false claims get made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostconditionOutcome {
    /// The system of record confirms the effect exists as claimed.
    Confirmed { detail: String },
    /// The system of record positively contradicts the claim.
    Refuted { detail: String },
    /// No independent judgment was reached. The caller must fall back to its
    /// existing (structural) judgment rather than treat this as either
    /// outcome.
    Inconclusive { detail: String },
}

impl PostconditionOutcome {
    /// The `method` string for a [`pb::VerificationResult`] built from this
    /// outcome. `Inconclusive` deliberately has none: it never produces a
    /// postcondition-method result at all.
    #[must_use]
    pub fn method(&self) -> Option<&'static str> {
        match self {
            Self::Confirmed { .. } | Self::Refuted { .. } => Some("postcondition"),
            Self::Inconclusive { .. } => None,
        }
    }

    #[must_use]
    pub fn detail(&self) -> &str {
        match self {
            Self::Confirmed { detail }
            | Self::Refuted { detail }
            | Self::Inconclusive { detail } => detail,
        }
    }
}

/// Verify a claimed shipment booking against shipping-core, the system of
/// record for bookings.
///
/// This is the pilot action of `verevon-roadmap.md` P1 item 3 — the same
/// action whose live `verified_success` the roadmap's §9.4 entry cites. The
/// structural check there only established that a `booking_id` came back;
/// this re-reads that booking from shipping-core and judges its actual
/// state.
///
/// A 404 is a refutation, not an error: `booking_id` came from
/// shipping-core's own create path, so shipping-core not having it is a
/// genuine contradiction rather than a lookup miss. Transport failures stay
/// inconclusive.
pub async fn verify_shipment_booking(
    client: &ShippingToolsClient,
    booking_id: &str,
    org_id: &str,
) -> PostconditionOutcome {
    match client.get_booking(booking_id, org_id).await {
        Ok(Some(record)) => judge_booking_record(booking_id, &record),
        Ok(None) => PostconditionOutcome::Refuted {
            detail: format!(
                "shipping-core has no booking {booking_id} for this organization, \
                 though the booking call returned that id"
            ),
        },
        Err(error) => PostconditionOutcome::Inconclusive {
            detail: format!("could not read booking {booking_id} back: {error}"),
        },
    }
}

/// Judge one shipping-core booking record. Split out from the request so the
/// judgment rules are testable without a live shipping-core.
#[must_use]
pub fn judge_booking_record(booking_id: &str, record: &Value) -> PostconditionOutcome {
    let status = record
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if status.is_empty() {
        return PostconditionOutcome::Inconclusive {
            detail: format!("booking {booking_id} exists but reports no status"),
        };
    }
    if REFUTING_BOOKING_STATUSES.contains(&status.as_str()) {
        return PostconditionOutcome::Refuted {
            detail: format!("shipping-core reports booking {booking_id} as {status}"),
        };
    }
    if !CONFIRMED_BOOKING_STATUSES.contains(&status.as_str()) {
        // An in-between state (e.g. `pending`) or a status this build does
        // not know about. Neither confirms nor contradicts — and guessing
        // would be exactly the overclaim this module exists to prevent.
        return PostconditionOutcome::Inconclusive {
            detail: format!(
                "shipping-core reports booking {booking_id} as {status}, \
                 which is neither a confirmed nor a failed state"
            ),
        };
    }

    let tracking_no = record
        .get("tracking_no")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if tracking_no.is_empty() {
        return PostconditionOutcome::Inconclusive {
            detail: format!(
                "shipping-core reports booking {booking_id} as {status} but carries \
                 no tracking number, so the carrier handoff is unconfirmed"
            ),
        };
    }

    PostconditionOutcome::Confirmed {
        detail: format!(
            "shipping-core independently reports booking {booking_id} as {status} \
             with tracking number {tracking_no}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Provider actions (execute_provider_action)
// ---------------------------------------------------------------------------

/// What a read-back is *capable of* proving. This distinction is the heart of
/// honest verification and is deliberately in the type system rather than in
/// a comment.
///
/// Shipping's `GET /api/bookings/{id}` asks about one specific effect, so its
/// answer is total: absence is a genuine refutation. A provider *listing*
/// (Slack's `conversations.history`, Gmail's message list) is bounded — it
/// returns a window, not the universe. Finding the receipt in that window
/// proves the effect exists; **not** finding it proves nothing at all,
/// because the effect may simply lie past the window. Treating that absence
/// as a refutation would manufacture false failures, which is exactly as
/// harmful as the false successes this layer exists to catch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadBackStrength {
    /// A direct lookup of the exact effect by its own id. Absence refutes.
    AuthoritativeById,
    /// A bounded listing. Presence confirms; absence is inconclusive.
    BoundedList,
}

/// A planned read-back: which frozen-surface operation to call, with what
/// params, and what its answer is worth.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderReadBack {
    pub operation: &'static str,
    pub params: Value,
    pub strength: ReadBackStrength,
}

/// Plan the read-back for a completed provider write, or `None` when this
/// build has no verifier for that operation.
///
/// Every operation named here is already in the **frozen** actions-surface
/// contract (`docs/actions-surface-operations.md`); this adds no provider
/// capability, it only re-uses existing reads to check existing writes.
/// Returning `None` is the correct, common answer — it leaves the structural
/// judgment untouched rather than guessing.
///
/// Not verifiable by design, and worth stating rather than leaving to a
/// future reader to rediscover:
/// - Microsoft `mail.send` returns `202 Accepted` with **no id at all**, so
///   the dispatcher never even reaches a `Completed` disposition for it (the
///   receipt heuristic finds nothing and it fails closed as
///   `invalid_continuation`). There is nothing to match a read-back against.
#[must_use]
pub fn plan_provider_read_back(operation: &str, body: &Value) -> Option<ProviderReadBack> {
    match operation.trim() {
        // Slack chat.postMessage returns the message `ts`; conversations.history
        // lists a bounded window of the same channel's messages.
        "message.send" | "slack.message.send" => {
            let channel = body.get("channel").and_then(Value::as_str)?.trim();
            if channel.is_empty() {
                return None;
            }
            Some(ProviderReadBack {
                operation: "messages.list",
                params: serde_json::json!({ "channel": channel, "limit": 100 }),
                strength: ReadBackStrength::BoundedList,
            })
        }
        // Gmail send returns the created message id; the message list returns
        // a bounded window that a just-sent message should fall inside.
        "gmail.send" | "google.gmail.send" => Some(ProviderReadBack {
            operation: "gmail.messages",
            params: serde_json::json!({ "maxResults": 100 }),
            strength: ReadBackStrength::BoundedList,
        }),
        _ => None,
    }
}

/// Judge a completed read-back against the receipt the write claimed.
///
/// The asymmetry enforced here is the whole point: presence always confirms,
/// but absence only refutes when the read-back was authoritative for that
/// exact id.
#[must_use]
pub fn judge_provider_read_back(
    plan: &ProviderReadBack,
    receipt_id: &str,
    result: &Value,
) -> PostconditionOutcome {
    let receipt_id = receipt_id.trim();
    if receipt_id.is_empty() {
        return PostconditionOutcome::Inconclusive {
            detail: "no provider receipt id to match against".to_owned(),
        };
    }

    if response_contains_identifier(result, receipt_id, 0) {
        return PostconditionOutcome::Confirmed {
            detail: format!(
                "provider's own {} read independently lists {receipt_id}",
                plan.operation
            ),
        };
    }

    match plan.strength {
        ReadBackStrength::AuthoritativeById => PostconditionOutcome::Refuted {
            detail: format!(
                "provider's own {} lookup does not contain {receipt_id}",
                plan.operation
            ),
        },
        ReadBackStrength::BoundedList => PostconditionOutcome::Inconclusive {
            detail: format!(
                "{receipt_id} was not in the bounded {} window, which neither \
                 confirms nor contradicts the write",
                plan.operation
            ),
        },
    }
}

/// Does this response carry `identifier` as an actual identifier value?
///
/// Matches only on string values at identifier-shaped keys, never on a
/// substring of arbitrary text — a receipt id appearing inside a message
/// *body* would say nothing about whether the message exists, and matching it
/// would produce a false `Confirmed`.
fn response_contains_identifier(value: &Value, identifier: &str, depth: u8) -> bool {
    if depth > 6 {
        return false;
    }
    match value {
        Value::Object(map) => {
            for key in ["ts", "id", "message_id", "messageId", "provider_message_id"] {
                if map
                    .get(key)
                    .and_then(Value::as_str)
                    .is_some_and(|candidate| candidate.trim() == identifier)
                {
                    return true;
                }
            }
            map.values()
                .any(|nested| response_contains_identifier(nested, identifier, depth + 1))
        }
        Value::Array(items) => items
            .iter()
            .any(|item| response_contains_identifier(item, identifier, depth + 1)),
        _ => false,
    }
}

/// Verify a claimed provider write by re-reading the provider through an
/// existing frozen-surface read operation.
///
/// Returns `Inconclusive` — leaving the structural judgment intact — when no
/// verifier exists for the operation or the read itself fails.
pub async fn verify_provider_action(
    client: &crate::integration_tools::IntegrationActionsClient,
    org_id: &str,
    connection_id: &str,
    operation: &str,
    body: &Value,
    receipt_id: &str,
) -> PostconditionOutcome {
    let Some(plan) = plan_provider_read_back(operation, body) else {
        return PostconditionOutcome::Inconclusive {
            detail: format!("no postcondition verifier is defined for {operation}"),
        };
    };
    match client
        .read_action_json(org_id, connection_id, plan.operation, plan.params.clone())
        .await
    {
        Ok(result) => judge_provider_read_back(&plan, receipt_id, &result),
        Err(error) => PostconditionOutcome::Inconclusive {
            detail: format!("could not read {} back: {error}", plan.operation),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_booked_shipment_with_tracking_is_confirmed() {
        let outcome = judge_booking_record(
            "bk_1",
            &json!({ "status": "booked", "tracking_no": "LC652849244NO" }),
        );
        assert!(matches!(outcome, PostconditionOutcome::Confirmed { .. }));
        assert_eq!(outcome.method(), Some("postcondition"));
        assert!(outcome.detail().contains("LC652849244NO"));
    }

    #[test]
    fn a_cancelled_booking_is_refuted() {
        let outcome = judge_booking_record("bk_1", &json!({ "status": "cancelled" }));
        assert!(matches!(outcome, PostconditionOutcome::Refuted { .. }));
        assert_eq!(outcome.method(), Some("postcondition"));
    }

    #[test]
    fn a_pending_booking_is_inconclusive_not_refuted() {
        let outcome = judge_booking_record("bk_1", &json!({ "status": "pending" }));
        assert!(
            matches!(outcome, PostconditionOutcome::Inconclusive { .. }),
            "an in-flight state must not be reported as a failure"
        );
        assert_eq!(outcome.method(), None);
    }

    #[test]
    fn an_unknown_status_is_inconclusive_never_confirmed() {
        let outcome = judge_booking_record(
            "bk_1",
            &json!({ "status": "some_future_state", "tracking_no": "X1" }),
        );
        assert!(
            matches!(outcome, PostconditionOutcome::Inconclusive { .. }),
            "a status this build does not know must never read as confirmed"
        );
    }

    #[test]
    fn a_booked_shipment_without_tracking_is_inconclusive() {
        let outcome = judge_booking_record("bk_1", &json!({ "status": "booked" }));
        assert!(matches!(outcome, PostconditionOutcome::Inconclusive { .. }));
        assert!(outcome.detail().contains("no tracking number"));
    }

    #[test]
    fn a_record_without_a_status_is_inconclusive() {
        let outcome = judge_booking_record("bk_1", &json!({ "tracking_no": "X1" }));
        assert!(matches!(outcome, PostconditionOutcome::Inconclusive { .. }));
    }

    #[test]
    fn status_matching_is_case_and_whitespace_insensitive() {
        let outcome = judge_booking_record(
            "bk_1",
            &json!({ "status": "  BOOKED ", "tracking_no": "X1" }),
        );
        assert!(matches!(outcome, PostconditionOutcome::Confirmed { .. }));
    }

    #[test]
    fn only_a_judged_outcome_carries_the_postcondition_method() {
        assert_eq!(
            PostconditionOutcome::Confirmed {
                detail: String::new()
            }
            .method(),
            Some("postcondition")
        );
        assert_eq!(
            PostconditionOutcome::Refuted {
                detail: String::new()
            }
            .method(),
            Some("postcondition")
        );
        assert_eq!(
            PostconditionOutcome::Inconclusive {
                detail: String::new()
            }
            .method(),
            None,
            "an inconclusive check must never claim it performed a postcondition verification"
        );
    }

    // --- Provider-action read-backs ----------------------------------------

    #[test]
    fn slack_send_plans_a_channel_scoped_history_read() {
        let plan = plan_provider_read_back(
            "slack.message.send",
            &json!({ "channel": "C123", "text": "hello" }),
        )
        .expect("slack message.send has a verifier");
        assert_eq!(plan.operation, "messages.list");
        assert_eq!(plan.params["channel"], json!("C123"));
        assert_eq!(plan.strength, ReadBackStrength::BoundedList);
    }

    #[test]
    fn slack_send_without_a_channel_has_no_plan() {
        assert!(plan_provider_read_back("slack.message.send", &json!({ "text": "hi" })).is_none());
        assert!(
            plan_provider_read_back("slack.message.send", &json!({ "channel": "  " })).is_none()
        );
    }

    #[test]
    fn an_operation_with_no_verifier_plans_nothing() {
        // Including a write that genuinely cannot be verified: Microsoft
        // mail.send returns no id at all.
        for operation in [
            "mail.send",
            "microsoft.mail.send",
            "issues.create",
            "whatever",
        ] {
            assert!(
                plan_provider_read_back(operation, &json!({})).is_none(),
                "{operation} must not claim a verifier it does not have"
            );
        }
    }

    #[test]
    fn a_receipt_present_in_the_listing_is_confirmed() {
        let plan = ProviderReadBack {
            operation: "messages.list",
            params: json!({}),
            strength: ReadBackStrength::BoundedList,
        };
        let result = json!({ "data": { "action": { "result": {
            "messages": [{ "ts": "1699999999.000100" }, { "ts": "1700000000.000200" }]
        }}}});
        let outcome = judge_provider_read_back(&plan, "1700000000.000200", &result);
        assert!(matches!(outcome, PostconditionOutcome::Confirmed { .. }));
        assert_eq!(outcome.method(), Some("postcondition"));
    }

    #[test]
    fn absence_from_a_bounded_list_is_inconclusive_never_refuted() {
        // The asymmetry that keeps this layer honest: a window that does not
        // contain the receipt has not proven the effect is missing.
        let plan = ProviderReadBack {
            operation: "messages.list",
            params: json!({}),
            strength: ReadBackStrength::BoundedList,
        };
        let result = json!({ "messages": [{ "ts": "1699999999.000100" }] });
        let outcome = judge_provider_read_back(&plan, "1700000000.000200", &result);
        assert!(
            matches!(outcome, PostconditionOutcome::Inconclusive { .. }),
            "a bounded listing can prove presence but never absence"
        );
        assert_eq!(outcome.method(), None);
    }

    #[test]
    fn absence_from_an_authoritative_lookup_is_refuted() {
        let plan = ProviderReadBack {
            operation: "issue.get",
            params: json!({}),
            strength: ReadBackStrength::AuthoritativeById,
        };
        let outcome = judge_provider_read_back(&plan, "42", &json!({ "id": "43" }));
        assert!(matches!(outcome, PostconditionOutcome::Refuted { .. }));
    }

    #[test]
    fn a_receipt_appearing_only_inside_message_text_is_not_a_match() {
        // Someone quoting an id in a message body must never confirm that the
        // id's own message exists.
        let plan = ProviderReadBack {
            operation: "messages.list",
            params: json!({}),
            strength: ReadBackStrength::BoundedList,
        };
        let result = json!({ "messages": [
            { "ts": "1699999999.000100", "text": "see 1700000000.000200 for details" }
        ]});
        let outcome = judge_provider_read_back(&plan, "1700000000.000200", &result);
        assert!(
            matches!(outcome, PostconditionOutcome::Inconclusive { .. }),
            "matching must be on identifier fields, not arbitrary text"
        );
    }

    #[test]
    fn an_empty_receipt_is_inconclusive() {
        let plan = ProviderReadBack {
            operation: "messages.list",
            params: json!({}),
            strength: ReadBackStrength::BoundedList,
        };
        let outcome = judge_provider_read_back(&plan, "   ", &json!({ "messages": [] }));
        assert!(matches!(outcome, PostconditionOutcome::Inconclusive { .. }));
    }

    #[test]
    fn gmail_send_plans_a_bounded_message_list() {
        let plan = plan_provider_read_back("gmail.send", &json!({ "raw": "…" }))
            .expect("gmail.send has a verifier");
        assert_eq!(plan.operation, "gmail.messages");
        assert_eq!(plan.strength, ReadBackStrength::BoundedList);
    }
}
