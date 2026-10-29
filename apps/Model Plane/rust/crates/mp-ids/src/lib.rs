//! Typed newtype identifiers with ULID validation.
//!
//! Every canonical identifier in the Model Plane is a ULID string.
//! These newtypes ensure compile-time distinction between ID kinds
//! while providing runtime validation via `TryFrom<String>`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdError {
    #[error("invalid ULID format: {0}")]
    InvalidUlid(String),
}

/// Validate that a string is a well-formed ULID (26-character Crockford Base32).
fn validate_ulid(value: &str) -> Result<(), IdError> {
    ulid::Ulid::from_string(value).map_err(|_| IdError::InvalidUlid(value.to_owned()))?;
    Ok(())
}

/// Generate a new random ULID string.
pub fn new_ulid() -> String {
    ulid::Ulid::new().to_string()
}

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Create a new randomly generated ID.
            #[must_use]
            pub fn generate() -> Self {
                Self(new_ulid())
            }

            /// Return the inner string reference.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume and return the inner string.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                validate_ulid(&value)?;
                Ok(Self(value))
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                validate_ulid(value)?;
                Ok(Self(value.to_owned()))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

define_id!(
    /// Identifies an agent definition (capability-core owns).
    AgentId
);
define_id!(
    /// Identifies a conversation thread (session-core owns).
    ThreadId
);
define_id!(
    /// Groups threads within a user interaction session (session-core owns).
    SessionKey
);
define_id!(
    /// Identifies a single agent run (session-core owns metadata).
    RunId
);
define_id!(
    /// Identifies one step within a run (execution-core owns).
    StepId
);
define_id!(
    /// Identifies a resumable checkpoint snapshot (session-core owns).
    CheckpointId
);
define_id!(
    /// Identifies an active sandbox lease (sandbox-manager owns).
    SandboxLeaseId
);
define_id!(
    /// Identifies an active browser grant (browser-broker owns).
    BrowserLeaseId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_valid_ulid() {
        let id = ThreadId::generate();
        assert!(ThreadId::try_from(id.as_str()).is_ok());
    }

    #[test]
    fn rejects_invalid_ulid() {
        assert!(RunId::try_from("not-a-ulid".to_owned()).is_err());
    }

    #[test]
    fn roundtrip_serde() {
        let id = AgentId::generate();
        let json = serde_json::to_string(&id).expect("serialize");
        let parsed: AgentId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(id, parsed);
    }

    #[test]
    fn different_types_not_interchangeable() {
        let raw = new_ulid();
        let thread = ThreadId::try_from(raw.clone()).expect("valid");
        let run = RunId::try_from(raw).expect("valid");
        // They hold the same string but are different types — this is the point.
        assert_eq!(thread.as_str(), run.as_str());
    }
}
