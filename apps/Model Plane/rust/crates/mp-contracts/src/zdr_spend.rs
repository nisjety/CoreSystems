//! The ZDR spend row: the one content-free kind of envelope a zero-data-retention
//! turn may put on the usage ledger (DEF-Z-06b, FU-30).
//!
//! A ZDR turn's spend used to travel as an ordinary usage envelope whose `zdr`
//! flag was set to false, so that two guards (the publisher's drop of every
//! `zdr: true` envelope, and cost-core's refusal of one) would let it through.
//! That labelled a ZDR turn as non-ZDR, and any field later added to the usage
//! envelope would have been persisted for ZDR turns with it.
//!
//! The spend row is now its own kind. Its `event_type` is [`EVENT_TYPE`], its
//! `zdr` flag stays true, and its payload carries only the fields of
//! [`PAYLOAD_FIELDS`]: ids, token counts, the amount and its currency, the cost
//! basis and the retention class. The publisher admits a `zdr: true` envelope
//! only when [`admits`] says it is this kind with nothing outside that list,
//! and cost-core, which owns the ledger, enforces the same list on its side
//! (ADR-0005: the owner enforces). Every other `zdr: true` envelope is still
//! dropped and still refused.
//!
//! This module reads no environment and does no I/O. The same field list is
//! mirrored in cost-core's `zdr_spend.go`; a change to one is a change to both.

use serde_json::{Map, Value};

/// The `event_type` of a ZDR spend row.
pub const EVENT_TYPE: &str = "ZDR_SPEND";

/// The only currency a spend row states its amount in.
pub const CURRENCY: &str = "USD";

/// The longest an id or model string of a spend row may be, in bytes. It is the
/// ledger's own dimension limit, so a row the publisher admits is one the ledger
/// can store.
pub const MAX_ID_BYTES: usize = 256;

/// The `retention_class` values a spend row may carry: the closed tokens of a ZDR
/// turn and of a waived one.
pub const RETENTION_CLASSES: &[&str] = &["zdr", "zdr_waived"];

/// The `cost_basis` values a spend row may carry: how the amount was established.
/// Without it the ledger would price a subscription turn from the catalogue.
pub const COST_BASES: &[&str] = &["metered", "subscription", "customer_connection"];

/// Payload fields that must be present. The amount, the cache counts and the cost
/// basis are optional: an answer whose model has no known price states no amount.
pub const REQUIRED_FIELDS: &[&str] = &[
    "request_id",
    "org_id",
    "user_id",
    "model",
    "provider_used",
    "input_tokens",
    "output_tokens",
    "currency",
    "retention_class",
];

/// Every payload field a spend row may carry. Nothing outside this list is
/// admitted, and nothing is added to it without an owner decision.
pub const PAYLOAD_FIELDS: &[&str] = &[
    "request_id",
    "org_id",
    "user_id",
    "model",
    "provider_used",
    "input_tokens",
    "output_tokens",
    "cache_read_input_tokens",
    "cache_creation_input_tokens",
    "cost_usd",
    "currency",
    "cost_basis",
    "retention_class",
];

fn is_id(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        !text.is_empty()
            && text.len() <= MAX_ID_BYTES
            && text == text.trim()
            && !text.chars().any(char::is_control)
    })
}

fn is_count(value: &Value) -> bool {
    value.as_u64().is_some()
}

fn is_amount(value: &Value) -> bool {
    value
        .as_f64()
        .is_some_and(|amount| amount.is_finite() && amount >= 0.0)
}

fn is_one_of(value: &Value, allowed: &[&str]) -> bool {
    value.as_str().is_some_and(|text| allowed.contains(&text))
}

fn field_is_valid(name: &str, value: &Value) -> bool {
    match name {
        "request_id" | "org_id" | "user_id" | "model" => is_id(value),
        // A direct answer that no deployment served names none, so the provider may be empty.
        "provider_used" => value.as_str().is_some_and(|text| {
            text.len() <= MAX_ID_BYTES && text == text.trim() && !text.chars().any(char::is_control)
        }),
        "input_tokens"
        | "output_tokens"
        | "cache_read_input_tokens"
        | "cache_creation_input_tokens" => is_count(value),
        "cost_usd" => is_amount(value),
        "currency" => value.as_str() == Some(CURRENCY),
        "cost_basis" => is_one_of(value, COST_BASES),
        "retention_class" => is_one_of(value, RETENTION_CLASSES),
        _ => false,
    }
}

/// Whether `payload` is a spend row's payload: an object whose every key is in
/// [`PAYLOAD_FIELDS`], with every [`REQUIRED_FIELDS`] key present, and each value
/// of the type and range its field takes. A payload with any other key, or a
/// value of another shape, is not one.
#[must_use]
pub fn payload_is_allowlisted(payload: &Value) -> bool {
    let Some(object): Option<&Map<String, Value>> = payload.as_object() else {
        return false;
    };
    object
        .iter()
        .all(|(name, value)| PAYLOAD_FIELDS.contains(&name.as_str()) && field_is_valid(name, value))
        && REQUIRED_FIELDS
            .iter()
            .all(|name| object.contains_key(*name))
}

/// Whether a `zdr: true` envelope of this `event_type` and `payload` is a ZDR
/// spend row the publisher may let through. True only for [`EVENT_TYPE`] with an
/// allowlisted payload.
#[must_use]
pub fn admits(event_type: &str, payload: &Value) -> bool {
    event_type == EVENT_TYPE && payload_is_allowlisted(payload)
}
