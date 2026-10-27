//! Typed Space denial mapping (WS03-S5).
//!
//! Control's Space issuers answer a refusal with the historical `error` string
//! plus an additive `code`, `resolvable_by` and allowlisted `details` (WS03-S2,
//! `user-core/internal/spaces/denial.go`). This module turns that body into the
//! browser envelope `{error:{code,message,details:{reason,resolvable_by,...}}}`
//! where `code` is the OPERATION that was refused and `details.reason` is
//! Control's denial code.
//!
//! The gateway never decides anything here. It relays a reason Control already
//! chose, drops every detail key that is not on the allowlist, and falls back
//! to today's pass-through whenever the body carries no well-formed reason or
//! the `space_denials_typed` flag is off. The flag is a rollout control, never
//! an authorization input: the status code and the refusal itself are the same
//! with it on or off.

use axum::{http::StatusCode, Json};
use serde_json::{json, Map, Value};

use crate::{config::AppState, envelope::error_with_details};

/// Operation codes the gateway reports for a refused Control decision.
pub(crate) const OP_THREAD: &str = "space_thread_not_authorized";
/// Supplied to WS01 (the append path).
pub(crate) const OP_POST: &str = "space_post_not_authorized";
pub(crate) const OP_RETRIEVAL: &str = "space_retrieval_not_authorized";
pub(crate) const OP_IMPORT: &str = "space_import_not_authorized";
pub(crate) const OP_SCHEDULE: &str = "space_schedule_not_authorized";
pub(crate) const OP_OWNER_GRANT: &str = "space_owner_grant_not_authorized";
pub(crate) const OP_READ: &str = "space_read_not_authorized";

/// The only detail keys that cross to the browser; anything else Control (or a
/// compromised upstream) puts in `details` is dropped.
const DETAIL_ALLOWLIST: &[&str] = &["entitlement", "required_roles", "state", "kind", "effect"];
const RESOLVABLE_BY: &[&str] = &["none", "org_admin", "space_manager", "retry", "operator"];
const DEFAULT_MESSAGE: &str = "This Space action is not authorized.";
const MAX_MESSAGE_CHARS: usize = 200;
const MAX_DETAIL_TOKEN: usize = 64;
const MAX_REQUIRED_ROLES: usize = 8;

/// A denial Control chose, reduced to what the browser may see.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SpaceDenial {
    reason: String,
    resolvable_by: &'static str,
    message: String,
    details: Map<String, Value>,
}

impl SpaceDenial {
    /// `{reason, resolvable_by, ...allowlisted}`: the `details` object of the
    /// browser envelope and of an `unavailable[]` entry.
    pub(crate) fn details_value(&self) -> Value {
        let mut out = Map::new();
        out.insert("reason".to_owned(), json!(self.reason));
        out.insert("resolvable_by".to_owned(), json!(self.resolvable_by));
        for (key, value) in &self.details {
            out.insert(key.clone(), value.clone());
        }
        Value::Object(out)
    }

    /// The Control-supplied refusal message, bounded; a fixed default when
    /// Control sent none.
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    /// Parses a Control refusal body. `None` (pass-through) unless the body is
    /// an object whose `code` is a well-formed `space_*` denial code.
    pub(crate) fn parse(body: &Value) -> Option<Self> {
        let object = body.as_object()?;
        let reason = object.get("code")?.as_str()?.trim();
        if !is_denial_code(reason) {
            return None;
        }
        let resolvable_by = object
            .get("resolvable_by")
            .and_then(Value::as_str)
            .and_then(|value| RESOLVABLE_BY.iter().find(|known| **known == value.trim()))
            .copied()
            .unwrap_or("operator");
        let message = object
            .get("error")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.chars().take(MAX_MESSAGE_CHARS).collect::<String>())
            .unwrap_or_else(|| DEFAULT_MESSAGE.to_owned());
        let mut details = Map::new();
        if let Some(source) = object.get("details").and_then(Value::as_object) {
            for key in DETAIL_ALLOWLIST {
                if let Some(value) = source.get(*key).and_then(|value| sanitize_detail(key, value))
                {
                    details.insert((*key).to_owned(), value);
                }
            }
        }
        Some(Self {
            reason: reason.to_owned(),
            resolvable_by,
            message,
            details,
        })
    }

    /// The denial carried by a non-success Control response, or `None` when the
    /// flag is off, the call succeeded, or the body has no well-formed reason.
    pub(crate) fn from_response(
        state: &AppState,
        status: StatusCode,
        body: &Value,
    ) -> Option<Self> {
        if !state.space_features.denials_typed || status.is_success() {
            return None;
        }
        Self::parse(body)
    }
}

/// The result of asking Control for a decision that may be declined: either the
/// grant, or the (optional) reason Control gave for declining it. A decline
/// without a reason (flag off, older Control, absent endpoint) is `Declined(None)`.
#[derive(Debug)]
pub(crate) enum Decision<T> {
    Granted(T),
    Declined(Option<SpaceDenial>),
}

impl<T> Decision<T> {
    /// The legacy `Option` view, plus the reason when there was one.
    pub(crate) fn split(self) -> (Option<T>, Option<SpaceDenial>) {
        match self {
            Self::Granted(grant) => (Some(grant), None),
            Self::Declined(denial) => (None, denial),
        }
    }
}

/// Maps a Control refusal to `{error:{code:<operation_code>,message,details}}`.
/// Returns `None` (caller keeps today's pass-through) for a success status or a
/// body without a well-formed denial code.
pub(crate) fn map_control_denial(
    status: StatusCode,
    body: &Value,
    operation_code: &'static str,
) -> Option<Value> {
    if status.is_success() {
        return None;
    }
    let denial = SpaceDenial::parse(body)?;
    Some(error_with_details(
        operation_code,
        denial.message(),
        denial.details_value(),
    ))
}

/// A Control response, typed when `space_denials_typed` is on and the body has a
/// reason, otherwise exactly the response Control gave. The status never changes.
pub(crate) fn typed_response(
    state: &AppState,
    status: StatusCode,
    body: Value,
    operation_code: &'static str,
) -> (StatusCode, Json<Value>) {
    if state.space_features.denials_typed {
        if let Some(mapped) = map_control_denial(status, &body, operation_code) {
            return (status, Json(mapped));
        }
    }
    (status, Json(body))
}

/// Adds `details` to an `unavailable[]` entry when a reason is known. Additive:
/// the entry keeps its `section`, `code` and `reason` strings.
pub(crate) fn with_gap_details(mut entry: Value, denial: Option<&SpaceDenial>) -> Value {
    if let (Some(denial), Some(object)) = (denial, entry.as_object_mut()) {
        object.insert("details".to_owned(), denial.details_value());
    }
    entry
}

fn is_denial_code(code: &str) -> bool {
    let Some(rest) = code.strip_prefix("space_") else {
        return false;
    };
    (3..=60).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

fn is_detail_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DETAIL_TOKEN
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// A detail value passes only as a short lowercase token (or, for
/// `required_roles`, a short list of them); everything else is dropped.
fn sanitize_detail(key: &str, value: &Value) -> Option<Value> {
    if key == "required_roles" {
        let roles = value.as_array()?;
        if roles.is_empty() || roles.len() > MAX_REQUIRED_ROLES {
            return None;
        }
        let tokens: Option<Vec<Value>> = roles
            .iter()
            .map(|role| {
                role.as_str()
                    .filter(|role| is_detail_token(role))
                    .map(|role| json!(role))
            })
            .collect();
        return tokens.map(Value::Array);
    }
    value
        .as_str()
        .filter(|value| is_detail_token(value))
        .map(|value| json!(value))
}
