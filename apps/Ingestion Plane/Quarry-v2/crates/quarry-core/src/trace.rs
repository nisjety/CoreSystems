//! Run origin and trace context (WS10.S4).
//!
//! These are labels for the run trace, never authority. `origin`, `agent` and the
//! trace `run_id` describe who says they started a run; admission, budget and the
//! control model must not read them (`origin_is_not_authz`). Org and user identity
//! come from the verified token only.
//!
//! `RunTraceContext` is the optional `trace` body of `POST /v1/agent/runs`, the shape
//! Model Plane's execution-core already sends (`WireRunTrace`). `EventTrace` is the
//! per-event stamp carried on `Event::trace`.

use serde::{Deserialize, Serialize};

/// The longest trace label or request id accepted, in bytes.
pub const TRACE_TOKEN_MAX: usize = 64;

/// Who started a run, as a trace label. Serialized lowercase (`user`, `agent`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunOrigin {
    /// A person, through a user token.
    User,
    /// A model agent acting for a person or a service.
    Agent,
    /// A scheduled fire.
    Schedule,
    /// An inbound webhook.
    Webhook,
    /// A service caller with no more specific hint.
    Api,
}

impl RunOrigin {
    /// The lowercase wire token (`user`, `agent`, `schedule`, `webhook`, `api`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Schedule => "schedule",
            Self::Webhook => "webhook",
            Self::Api => "api",
        }
    }

    /// The Model Plane enum spelling (`USER`, `AGENT`, `SCHEDULE`, `WEBHOOK`, `API`).
    #[must_use]
    pub fn as_model_str(self) -> &'static str {
        match self {
            Self::User => "USER",
            Self::Agent => "AGENT",
            Self::Schedule => "SCHEDULE",
            Self::Webhook => "WEBHOOK",
            Self::Api => "API",
        }
    }

    /// Parse a hint (ASCII case ignored). Anything outside the five names is `None`.
    #[must_use]
    pub fn from_hint(hint: &str) -> Option<Self> {
        let hint = hint.trim();
        [
            Self::User,
            Self::Agent,
            Self::Schedule,
            Self::Webhook,
            Self::Api,
        ]
        .into_iter()
        .find(|origin| origin.as_str().eq_ignore_ascii_case(hint))
    }
}

/// The agent identity named in a trace context. Every field is a label.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

impl AgentRef {
    /// Keep only valid tokens; `None` when nothing is left.
    #[must_use]
    pub fn sanitized(self) -> Option<Self> {
        let agent = Self {
            id: keep_token(self.id),
            role: keep_token(self.role),
            parent_id: keep_token(self.parent_id),
        };
        (agent != Self::default()).then_some(agent)
    }
}

/// The optional `trace` body of `POST /v1/agent/runs`: `{run_id?, agent?, origin?}`.
/// Mirrors Model Plane execution-core `WireRunTrace`. Never authority.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunTraceContext {
    /// The caller's (Model Plane) run id, a label for correlation only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentRef>,
    /// The origin hint; read only through [`derive_origin`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

impl RunTraceContext {
    /// Drop every field that is not a valid trace token. Never errors: a hostile or
    /// malformed context loses the bad labels and the run proceeds.
    #[must_use]
    pub fn sanitized(self) -> Self {
        Self {
            run_id: keep_token(self.run_id),
            agent: self.agent.and_then(AgentRef::sanitized),
            origin: keep_token(self.origin),
        }
    }

    /// True when no field survived (or none was sent).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// The trace stamp on an event (`Event::trace`). Every field is optional and a label.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EventTrace {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<RunOrigin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentRef>,
    /// The inbound request id of the call that caused the event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// The caller's (Model Plane) run id, from a service token's trace context only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// The event this one follows from (e.g. the step receipt behind an egress or
    /// artifact event).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_event_id: Option<String>,
}

/// True when `s` is 1..=`max` bytes of `[A-Za-z0-9_-]`.
#[must_use]
pub fn valid_token(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn keep_token(value: Option<String>) -> Option<String> {
    value.filter(|v| valid_token(v, TRACE_TOKEN_MAX))
}

/// The run origin from the verified caller kind and an optional hint.
///
/// A user token yields `User`, or `Agent` when the hint says `agent`; every other
/// hint is ignored (a person cannot claim to be a schedule or a webhook). A service
/// token yields its hint when it names a valid origin, else `Api`.
#[must_use]
pub fn derive_origin(is_service: bool, hint: Option<&str>) -> RunOrigin {
    let hinted = hint
        .filter(|h| valid_token(h.trim(), TRACE_TOKEN_MAX))
        .and_then(RunOrigin::from_hint);
    if is_service {
        hinted.unwrap_or(RunOrigin::Api)
    } else if hinted == Some(RunOrigin::Agent) {
        RunOrigin::Agent
    } else {
        RunOrigin::User
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_origin_user_token_ignores_schedule_and_webhook_hints() {
        for hint in ["schedule", "webhook", "api", "user", "nonsense", ""] {
            assert_eq!(derive_origin(false, Some(hint)), RunOrigin::User, "{hint}");
        }
        assert_eq!(derive_origin(false, None), RunOrigin::User);
    }

    #[test]
    fn derive_origin_user_token_may_claim_agent() {
        assert_eq!(derive_origin(false, Some("agent")), RunOrigin::Agent);
    }

    #[test]
    fn derive_origin_service_token_defaults_to_api() {
        assert_eq!(derive_origin(true, None), RunOrigin::Api);
        assert_eq!(derive_origin(true, Some("robot")), RunOrigin::Api);
        assert_eq!(derive_origin(true, Some("a.b")), RunOrigin::Api);
    }

    #[test]
    fn derive_origin_service_token_accepts_each_hint() {
        for origin in [
            RunOrigin::User,
            RunOrigin::Agent,
            RunOrigin::Schedule,
            RunOrigin::Webhook,
            RunOrigin::Api,
        ] {
            assert_eq!(derive_origin(true, Some(origin.as_str())), origin);
        }
    }

    #[test]
    fn valid_token_rejects_dots_spaces_and_overlong() {
        assert!(valid_token("general-v1_A", 64));
        assert!(!valid_token("", 64));
        assert!(!valid_token("a.b", 64));
        assert!(!valid_token("a b", 64));
        assert!(!valid_token(&"a".repeat(65), 64));
        assert!(valid_token(&"a".repeat(64), 64));
    }

    #[test]
    fn sanitized_drops_invalid_tokens_without_erroring() {
        let ctx: RunTraceContext = serde_json::from_value(serde_json::json!({
            "run_id": "01HZX",
            "agent": { "id": "bad id", "role": "primary", "parent_id": "x/y" },
            "origin": "agent"
        }))
        .unwrap();
        let clean = ctx.sanitized();
        assert_eq!(clean.run_id.as_deref(), Some("01HZX"));
        assert_eq!(
            clean.agent,
            Some(AgentRef {
                id: None,
                role: Some("primary".into()),
                parent_id: None
            })
        );
        assert_eq!(clean.origin.as_deref(), Some("agent"));
        assert!(RunTraceContext::default().sanitized().is_empty());
    }

    #[test]
    fn origin_serializes_lowercase_and_maps_to_model_spelling() {
        assert_eq!(serde_json::to_value(RunOrigin::Webhook).unwrap(), "webhook");
        assert_eq!(RunOrigin::Schedule.as_model_str(), "SCHEDULE");
    }
}
