//! Typed web-policy denials. No new `ErrorCode`: a policy denial is `Forbidden`
//! (403) with `details.reason` from a closed set, except an unknown capability
//! in a request, which is `BadRequest` (400) as the WS7.C API states.
//!
//! The message and details never carry a URL, a script or page text. The host
//! is carried only when ZDR is off.

use serde_json::{json, Value};

use super::CapKey;
use crate::error::{ErrorCode, QuarryError};

/// The layer that decided a value. The order is the tie order: when two layers
/// agree on the tightest value, the earlier one is named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionSource {
    Invariant,
    Zdr,
    OrgPolicy,
    Grant,
    Baseline,
    Request,
}

impl DecisionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Invariant => "invariant",
            Self::Zdr => "zdr",
            Self::OrgPolicy => "org_policy",
            Self::Grant => "grant",
            Self::Baseline => "baseline",
            Self::Request => "request",
        }
    }
}

/// Why a request was refused by policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDenial {
    /// A capability resolved to disabled. `policy_rev` is set only when the org layer decided.
    PolicyDenied {
        capability: CapKey,
        decision_source: DecisionSource,
        policy_rev: Option<u64>,
        denied_action: String,
    },
    /// Raw page script. This is WS2.A's refusal, kept byte for byte in reason and message.
    RawScriptDenied,
    /// The org policy could not be read. Refused, never run on the baseline alone.
    PolicyUnreadable,
    /// A request named a key the catalogue does not have.
    UnknownCapability { capability: String },
    /// A feature asked for a ceiling above the resolved one.
    CeilingExceeded {
        capability: CapKey,
        max: String,
        requested: String,
    },
    DomainDenied {
        host: Option<String>,
        decision_source: DecisionSource,
    },
    DomainNotAllowed {
        host: Option<String>,
        decision_source: DecisionSource,
    },
}

/// The message of the raw script refusal. The same text as
/// `QuarryError::raw_script_denied`; a test keeps them equal.
pub(crate) const RAW_SCRIPT_MESSAGE: &str =
    "raw page script execution (evaluate) is not permitted; use typed browser actions";

impl PolicyDenial {
    /// The closed `details.reason` value.
    pub fn code(&self) -> &'static str {
        match self {
            Self::PolicyDenied { .. } => "policy_denied",
            Self::RawScriptDenied => "raw_script_denied",
            Self::PolicyUnreadable => "policy_unreadable",
            Self::UnknownCapability { .. } => "unknown_capability",
            Self::CeilingExceeded { .. } => "ceiling_exceeded",
            Self::DomainDenied { .. } => "domain_denied",
            Self::DomainNotAllowed { .. } => "domain_not_allowed",
        }
    }

    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::UnknownCapability { .. } => ErrorCode::BadRequest,
            _ => ErrorCode::Forbidden,
        }
    }

    /// Operator-readable. Carries no URL and no page text.
    pub fn message(&self) -> String {
        match self {
            Self::PolicyDenied {
                capability,
                decision_source,
                ..
            } => format!(
                "policy denied {} (decided by {})",
                capability.as_str(),
                decision_source.as_str()
            ),
            Self::RawScriptDenied => RAW_SCRIPT_MESSAGE.to_string(),
            Self::PolicyUnreadable => {
                "web policy could not be read; the request is refused until it can be".to_string()
            }
            Self::UnknownCapability { .. } => "unknown web policy capability".to_string(),
            Self::CeilingExceeded {
                capability,
                max,
                requested,
            } => format!(
                "{} is capped at {max}; {requested} was requested",
                capability.as_str()
            ),
            Self::DomainDenied { decision_source, .. } => {
                format!("domain denied by policy ({})", decision_source.as_str())
            }
            Self::DomainNotAllowed { .. } => {
                "domain is not on the allowed list for this run".to_string()
            }
        }
    }

    /// The `details` object of the error envelope.
    pub fn details(&self) -> Value {
        match self {
            Self::PolicyDenied {
                capability,
                decision_source,
                policy_rev,
                denied_action,
            } => {
                let mut details = json!({
                    "reason": self.code(),
                    "capability": capability.as_str(),
                    "decision_source": decision_source.as_str(),
                    "denied_action": denied_action,
                    "retryable": false,
                });
                if let Some(rev) = policy_rev {
                    details["policy_rev"] = json!(rev);
                }
                details
            }
            Self::RawScriptDenied => json!({
                "reason": "raw_script_denied",
                "denied_action": "evaluate",
                "capability": CapKey::ActScript.as_str(),
                "decision_source": DecisionSource::Invariant.as_str(),
                "retryable": false,
            }),
            Self::PolicyUnreadable => json!({
                "reason": self.code(),
                "retryable": false,
            }),
            Self::UnknownCapability { capability } => json!({
                "reason": self.code(),
                "capability": capability,
                "retryable": false,
            }),
            Self::CeilingExceeded {
                capability,
                max,
                requested,
            } => json!({
                "reason": self.code(),
                "capability": capability.as_str(),
                "max": max,
                "requested": requested,
                "retryable": false,
            }),
            Self::DomainDenied {
                host,
                decision_source,
            }
            | Self::DomainNotAllowed {
                host,
                decision_source,
            } => {
                let mut details = json!({
                    "reason": self.code(),
                    "decision_source": decision_source.as_str(),
                    "retryable": false,
                });
                if let Some(host) = host {
                    details["host"] = json!(host);
                }
                details
            }
        }
    }
}

impl From<PolicyDenial> for QuarryError {
    fn from(denial: PolicyDenial) -> Self {
        let code = denial.error_code();
        let message = denial.message();
        let details = denial.details();
        QuarryError::new(code, message).with_details(details)
    }
}
