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

#[cfg(test)]
mod tests {
    use super::*;

    fn denial_body(code: &str) -> Value {
        json!({
            "error": "You cannot post in this Space.",
            "code": code,
            "resolvable_by": "space_manager",
            "details": {"entitlement": "space_posts", "required_roles": ["editor", "owner"]}
        })
    }

    /// Every golden operation code keeps the Control status and yields the typed envelope with
    /// the operation code outside and Control's denial code inside `details.reason`.
    #[test]
    fn map_control_denial_keeps_the_status_and_names_the_operation() {
        for operation in [
            OP_THREAD,
            OP_POST,
            OP_RETRIEVAL,
            OP_IMPORT,
            OP_SCHEDULE,
            OP_OWNER_GRANT,
            OP_READ,
        ] {
            for status in [
                StatusCode::FORBIDDEN,
                StatusCode::CONFLICT,
                StatusCode::UNPROCESSABLE_ENTITY,
            ] {
                let mapped = map_control_denial(status, &denial_body("space_role_insufficient"), operation)
                    .expect("a well-formed denial is mapped");
                assert_eq!(mapped["error"]["code"], operation);
                assert_eq!(mapped["error"]["message"], "You cannot post in this Space.");
                let details = &mapped["error"]["details"];
                assert_eq!(details["reason"], "space_role_insufficient");
                assert_eq!(details["resolvable_by"], "space_manager");
                assert_eq!(details["entitlement"], "space_posts");
                assert_eq!(details["required_roles"], json!(["editor", "owner"]));
            }
        }
    }

    #[test]
    fn a_body_without_a_well_formed_code_keeps_the_pass_through() {
        let forbidden = StatusCode::FORBIDDEN;
        // No code at all, and a non-string code.
        assert!(map_control_denial(forbidden, &json!({"error": "nope"}), OP_THREAD).is_none());
        assert!(map_control_denial(forbidden, &json!({"code": 7}), OP_THREAD).is_none());
        assert!(map_control_denial(forbidden, &json!("text"), OP_THREAD).is_none());
        // Not a space_ code.
        assert!(map_control_denial(forbidden, &denial_body("forbidden"), OP_THREAD).is_none());
        assert!(map_control_denial(forbidden, &denial_body("member_not_in_organization"), OP_THREAD).is_none());
        // Too short (needs 3 characters after the prefix), too long (over 60), uppercase, digits.
        assert!(map_control_denial(forbidden, &denial_body("space_ab"), OP_THREAD).is_none());
        assert!(map_control_denial(forbidden, &denial_body(&format!("space_{}", "a".repeat(61))), OP_THREAD).is_none());
        assert!(map_control_denial(forbidden, &denial_body("space_Role_Bad"), OP_THREAD).is_none());
        assert!(map_control_denial(forbidden, &denial_body("space_role_2"), OP_THREAD).is_none());
        // The bounds themselves are accepted.
        assert!(map_control_denial(forbidden, &denial_body("space_abc"), OP_THREAD).is_some());
        assert!(map_control_denial(forbidden, &denial_body(&format!("space_{}", "a".repeat(60))), OP_THREAD).is_some());
    }

    #[test]
    fn a_success_status_is_never_mapped() {
        for status in [StatusCode::OK, StatusCode::CREATED, StatusCode::ACCEPTED] {
            assert!(map_control_denial(status, &denial_body("space_role_insufficient"), OP_POST).is_none());
        }
    }

    #[test]
    fn injected_authority_detail_keys_are_dropped() {
        let body = json!({
            "error": "No.",
            "code": "space_role_insufficient",
            "resolvable_by": "none",
            "details": {
                "token": "tttt",
                "space_decision_token": "tttt",
                "resource_id": "res-1",
                "attestation": "aaaa",
                "org_id": "org-1",
                "state": "archived"
            }
        });
        let mapped = map_control_denial(StatusCode::FORBIDDEN, &body, OP_READ).unwrap();
        let details = mapped["error"]["details"].as_object().unwrap();
        for key in ["token", "space_decision_token", "resource_id", "attestation", "org_id"] {
            assert!(!details.contains_key(key), "{key} must not cross to the browser");
        }
        assert_eq!(details["state"], "archived");
        assert!(!mapped.to_string().contains("tttt"));
    }

    #[test]
    fn non_token_detail_values_and_bad_required_roles_are_dropped() {
        let parse = |details: Value| {
            SpaceDenial::parse(&json!({"code": "space_role_insufficient", "details": details}))
                .unwrap()
                .details_value()
        };
        // Spaces, uppercase, objects, numbers and over-long strings are not tokens.
        let dropped = parse(json!({
            "entitlement": "has space",
            "state": "UPPER",
            "kind": {"nested": "x"},
            "effect": 3,
        }));
        for key in ["entitlement", "state", "kind", "effect"] {
            assert!(dropped.get(key).is_none(), "{key} must be dropped");
        }
        let long = "a".repeat(MAX_DETAIL_TOKEN + 1);
        assert!(parse(json!({"entitlement": long})).get("entitlement").is_none());
        let at_limit = "a".repeat(MAX_DETAIL_TOKEN);
        assert_eq!(parse(json!({"entitlement": at_limit.clone()}))["entitlement"], at_limit);
        // required_roles: empty, more than eight, non-array and any bad element are dropped whole.
        assert!(parse(json!({"required_roles": []})).get("required_roles").is_none());
        assert!(parse(json!({"required_roles": ["a","b","c","d","e","f","g","h","i"]}))
            .get("required_roles")
            .is_none());
        assert!(parse(json!({"required_roles": "owner"})).get("required_roles").is_none());
        assert!(parse(json!({"required_roles": ["owner", "Not A Token"]})).get("required_roles").is_none());
        assert_eq!(
            parse(json!({"required_roles": ["a","b","c","d","e","f","g","h"]}))["required_roles"]
                .as_array()
                .unwrap()
                .len(),
            8
        );
        // Detail keys outside the allowlist never appear, valid token or not.
        assert!(parse(json!({"custom": "ok"})).get("custom").is_none());
    }

    #[test]
    fn an_unknown_resolvable_by_becomes_operator() {
        for raw in [json!("root"), json!(5), Value::Null] {
            let denial = SpaceDenial::parse(&json!({"code": "space_role_insufficient", "resolvable_by": raw})).unwrap();
            assert_eq!(denial.details_value()["resolvable_by"], "operator");
        }
        let absent = SpaceDenial::parse(&json!({"code": "space_role_insufficient"})).unwrap();
        assert_eq!(absent.details_value()["resolvable_by"], "operator");
        for known in RESOLVABLE_BY {
            let denial = SpaceDenial::parse(&json!({"code": "space_role_insufficient", "resolvable_by": known})).unwrap();
            assert_eq!(denial.details_value()["resolvable_by"], *known);
        }
    }

    #[test]
    fn the_message_is_bounded_and_defaults_when_missing() {
        let long = "x".repeat(500);
        let denial = SpaceDenial::parse(&json!({"code": "space_role_insufficient", "error": long})).unwrap();
        assert_eq!(denial.message().chars().count(), MAX_MESSAGE_CHARS);
        // Multi-byte characters are counted as characters, not bytes.
        let wide = "\u{00e5}".repeat(300);
        let denial = SpaceDenial::parse(&json!({"code": "space_role_insufficient", "error": wide})).unwrap();
        assert_eq!(denial.message().chars().count(), MAX_MESSAGE_CHARS);
        for missing in [json!({"code": "space_role_insufficient"}), json!({"code": "space_role_insufficient", "error": "   "}), json!({"code": "space_role_insufficient", "error": 4})] {
            let denial = SpaceDenial::parse(&missing).unwrap();
            assert_eq!(denial.message(), DEFAULT_MESSAGE);
        }
    }

    #[test]
    fn from_response_needs_the_flag_a_failure_status_and_a_reason() {
        let mut state = AppState::for_tests();
        let body = denial_body("space_role_insufficient");
        assert!(SpaceDenial::from_response(&state, StatusCode::FORBIDDEN, &body).is_none());
        state.space_features.denials_typed = true;
        assert!(SpaceDenial::from_response(&state, StatusCode::FORBIDDEN, &body).is_some());
        assert!(SpaceDenial::from_response(&state, StatusCode::OK, &body).is_none());
        assert!(SpaceDenial::from_response(&state, StatusCode::FORBIDDEN, &json!({"error": "x"})).is_none());
    }

    #[test]
    fn typed_response_is_byte_identical_pass_through_with_the_flag_off() {
        let state = AppState::for_tests();
        let body = denial_body("space_role_insufficient");
        let (status, Json(out)) = typed_response(&state, StatusCode::FORBIDDEN, body.clone(), OP_THREAD);
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(out, body);
    }

    #[test]
    fn typed_response_maps_with_the_flag_on_and_keeps_the_status() {
        let mut state = AppState::for_tests();
        state.space_features.denials_typed = true;
        let (status, Json(out)) =
            typed_response(&state, StatusCode::CONFLICT, denial_body("space_role_insufficient"), OP_POST);
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(out["error"]["code"], OP_POST);
        assert_eq!(out["error"]["details"]["reason"], "space_role_insufficient");
        // An untyped body stays as Control sent it, flag on.
        let untyped = json!({"error": "forbidden"});
        let (status, Json(out)) = typed_response(&state, StatusCode::FORBIDDEN, untyped.clone(), OP_POST);
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(out, untyped);
    }

    #[test]
    fn gap_details_are_additive_and_keep_the_entry_strings() {
        let entry = json!({"section": "runs", "code": "runs_read_not_authorized", "reason": "Not allowed."});
        let denial = SpaceDenial::parse(&denial_body("space_role_insufficient")).unwrap();
        let with = with_gap_details(entry.clone(), Some(&denial));
        assert_eq!(with["section"], "runs");
        assert_eq!(with["code"], "runs_read_not_authorized");
        assert_eq!(with["reason"], "Not allowed.");
        assert_eq!(with["details"]["reason"], "space_role_insufficient");
        assert_eq!(with_gap_details(entry.clone(), None), entry);
    }

    #[test]
    fn decision_split_returns_the_grant_or_the_optional_reason() {
        let (grant, denial) = Decision::Granted(7).split();
        assert_eq!(grant, Some(7));
        assert!(denial.is_none());
        let (grant, denial) = Decision::<i32>::Declined(None).split();
        assert!(grant.is_none() && denial.is_none());
        let reason = SpaceDenial::parse(&denial_body("space_role_insufficient"));
        let (grant, denial) = Decision::<i32>::Declined(reason.clone()).split();
        assert!(grant.is_none());
        assert_eq!(denial, reason);
    }
}
