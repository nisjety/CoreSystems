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
}
