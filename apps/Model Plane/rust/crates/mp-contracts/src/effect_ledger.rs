//! The effect ledger's shared vocabulary (build plan WS03 S5).
//!
//! session-core keeps one ledger row per tool call: reserved before the call
//! starts, finalized after it ends. A loop that dies between the two leaves the
//! row `reserved`, and nothing can say whether the effect happened. The loop that
//! resumes the run must not guess: it reads the row and closes the call honestly.
//!
//! session-core and execution-core both read these types, so a class, a status or
//! a digest cannot mean something different on each side. A row carries the tool,
//! its class and a digest of its arguments only; never the arguments themselves.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::loop_fold::LedgerStatus;

/// A digest is 64 lowercase hex characters (a blake3 hash).
pub const ARGS_DIGEST_HEX_LEN: usize = 64;

/// What a tool call may change outside the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectClass {
    /// No side effect at all.
    NoEffect,
    /// Reads state without changing it.
    Read,
    /// Changes state the platform owns.
    Write,
    /// Changes something outside the platform: a message sent, a payment made.
    External,
}

impl EffectClass {
    /// The stored form of the class.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoEffect => "none",
            Self::Read => "read",
            Self::Write => "write",
            Self::External => "external",
        }
    }

    /// Parse the stored form; any other value is not a class.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::NoEffect),
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "external" => Some(Self::External),
            _ => None,
        }
    }

    /// Whether running the call a second time could repeat its effect. Such a
    /// call is never re-dispatched when the first attempt's outcome is unknown.
    pub fn is_effectful(self) -> bool {
        matches!(self, Self::Write | Self::External)
    }
}

/// The outcomes a finalize may record. `reserved` is the open state, not an outcome.
pub const FINAL_OUTCOMES: [&str; 4] = ["completed", "failed", "not_run", "unknown_outcome"];

/// Whether `value` is an outcome a finalize may record.
pub fn is_final_outcome(value: &str) -> bool {
    FINAL_OUTCOMES.contains(&value)
}

/// The ledger state a row's `status` column names, or `None` for an unknown value.
pub fn ledger_status_from_row(status: &str) -> Option<LedgerStatus> {
    match status {
        "reserved" => Some(LedgerStatus::Reserved),
        "completed" => Some(LedgerStatus::Completed),
        "failed" => Some(LedgerStatus::Failed),
        "not_run" => Some(LedgerStatus::NotRun),
        "unknown_outcome" => Some(LedgerStatus::Unknown),
        _ => None,
    }
}

/// The `status` column value for a ledger state.
pub fn ledger_status_to_row(status: LedgerStatus) -> &'static str {
    match status {
        LedgerStatus::Reserved => "reserved",
        LedgerStatus::Completed => "completed",
        LedgerStatus::Failed => "failed",
        LedgerStatus::NotRun => "not_run",
        LedgerStatus::Unknown => "unknown_outcome",
    }
}

/// Whether `value` has the shape of an args digest: 64 lowercase hex characters.
pub fn is_args_digest(value: &str) -> bool {
    value.len() == ARGS_DIGEST_HEX_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The same value with every object's keys in sorted order.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&String, Value> =
                map.iter().map(|(key, item)| (key, canonical(item))).collect();
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, item)| (key.clone(), item))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// A digest of a call's arguments: blake3 over the canonical JSON, so key order
/// and whitespace do not change it. `None` when the arguments are not JSON.
/// The ledger stores this digest and never the arguments.
pub fn args_digest(arguments_json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(arguments_json).ok()?;
    let text = serde_json::to_string(&canonical(&value)).ok()?;
    Some(blake3::hash(text.as_bytes()).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_class_round_trips_and_only_writes_and_external_calls_are_effectful() {
        for class in [
            EffectClass::NoEffect,
            EffectClass::Read,
            EffectClass::Write,
            EffectClass::External,
        ] {
            assert_eq!(EffectClass::parse(class.as_str()), Some(class));
        }
        assert_eq!(EffectClass::parse("destructive"), None);
        assert_eq!(EffectClass::parse(""), None);
        assert!(!EffectClass::NoEffect.is_effectful());
        assert!(!EffectClass::Read.is_effectful());
        assert!(EffectClass::Write.is_effectful());
        assert!(EffectClass::External.is_effectful());
    }

    #[test]
    fn the_row_status_vocabulary_round_trips_through_the_ledger_states() {
        for status in [
            "reserved",
            "completed",
            "failed",
            "not_run",
            "unknown_outcome",
        ] {
            let state = ledger_status_from_row(status).expect("known status");
            assert_eq!(ledger_status_to_row(state), status);
        }
        assert_eq!(ledger_status_from_row("unknown"), None);
    }

    #[test]
    fn a_finalize_may_record_the_four_closing_outcomes_and_never_reserved() {
        for outcome in ["completed", "failed", "not_run", "unknown_outcome"] {
            assert!(is_final_outcome(outcome), "{outcome}");
        }
        assert!(!is_final_outcome("reserved"));
        assert!(!is_final_outcome("Completed"));
    }

    #[test]
    fn the_digest_ignores_key_order_and_whitespace_but_not_values() {
        let one = args_digest(r#"{"b": 2, "a": {"y": 1, "x": [3, 1]}}"#).expect("json");
        let same = args_digest(r#"{"a":{"x":[3,1],"y":1},"b":2}"#).expect("json");
        assert_eq!(one, same);
        assert!(is_args_digest(&one));
        // Array order is data, so it changes the digest, as does any value.
        let reordered = args_digest(r#"{"a":{"x":[1,3],"y":1},"b":2}"#).expect("json");
        assert_ne!(one, reordered);
        let changed = args_digest(r#"{"a":{"x":[3,1],"y":1},"b":3}"#).expect("json");
        assert_ne!(one, changed);
    }

    #[test]
    fn arguments_that_are_not_json_have_no_digest() {
        assert_eq!(args_digest("not json"), None);
        assert_eq!(args_digest(""), None);
    }

    #[test]
    fn a_digest_has_exactly_the_stored_shape() {
        let digest = args_digest("{}").expect("json");
        assert!(is_args_digest(&digest));
        assert!(!is_args_digest(&digest.to_uppercase()));
        assert!(!is_args_digest(&digest[..63]));
        assert!(!is_args_digest(""));
        assert!(!is_args_digest(&format!("{digest}0")));
    }
}
