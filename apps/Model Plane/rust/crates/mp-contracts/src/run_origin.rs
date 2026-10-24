//! The closed run-origin vocabulary (build plan WS20 S2, DEC-17).
//!
//! A run's origin is stamped at birth as `runs.metadata.origin`. Only these four
//! tokens may be stamped, so an origin is never a free-text label. A run with no
//! stamp predates the field and reads as `Chat`. The Go twin is
//! `capability-core/internal/models/run_origin.go`; both pin the same table, so a
//! token means the same thing to the writer and to every reader.

use serde::{Deserialize, Serialize};

/// The `runs.metadata` key the origin is stored under.
pub const ORIGIN_METADATA_KEY: &str = "origin";

/// Who started a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOrigin {
    /// A person's turn in a chat thread.
    Chat,
    /// A prepared run fired by a schedule.
    Schedule,
    /// A run started by an event trigger.
    Trigger,
    /// A run started through the public API.
    Api,
}

impl RunOrigin {
    /// Every origin, in declaration order.
    pub const ALL: [Self; 4] = [Self::Chat, Self::Schedule, Self::Trigger, Self::Api];

    /// The wire token: the serde snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Schedule => "schedule",
            Self::Trigger => "trigger",
            Self::Api => "api",
        }
    }

    /// Parse a wire token. Exact match only: no trimming, no case folding, so a
    /// look-alike such as `"Schedule"` or `"scheduled"` is refused, never read.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|origin| origin.as_str() == token)
    }

    /// The origin a run's stored metadata names. An absent or null stamp reads as
    /// `Chat`; a stamp that is not one of the closed tokens is `None`, so the
    /// caller refuses it rather than treating it as a chat.
    #[must_use]
    pub fn from_metadata(metadata: &serde_json::Value) -> Option<Self> {
        match metadata.get(ORIGIN_METADATA_KEY) {
            None | Some(serde_json::Value::Null) => Some(Self::Chat),
            Some(serde_json::Value::String(token)) => Self::parse(token),
            Some(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The known answers the Go twin repeats: each origin and its wire token.
    const TABLE: [(RunOrigin, &str); 4] = [
        (RunOrigin::Chat, "chat"),
        (RunOrigin::Schedule, "schedule"),
        (RunOrigin::Trigger, "trigger"),
        (RunOrigin::Api, "api"),
    ];

    #[test]
    fn run_origin_table() {
        assert_eq!(TABLE.len(), RunOrigin::ALL.len());
        for origin in RunOrigin::ALL {
            let (_, token) = TABLE
                .into_iter()
                .find(|(listed, _)| *listed == origin)
                .unwrap_or_else(|| panic!("{origin:?} is missing from the table"));
            assert_eq!(origin.as_str(), token);
            assert_eq!(RunOrigin::parse(token), Some(origin), "{token}");
            assert_eq!(
                serde_json::to_string(&origin).expect("serialise"),
                format!("\"{token}\""),
                "{token} must equal the serde name"
            );
        }
    }

    #[test]
    fn parse_refuses_everything_outside_the_closed_set() {
        for bad in [
            "",
            " schedule",
            "schedule ",
            "Schedule",
            "scheduled",
            "task-runner",
            "mail_task",
            "space",
            "turn_origin=chat",
        ] {
            assert_eq!(RunOrigin::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn absent_origin_reads_as_chat_and_an_unknown_stamp_is_refused() {
        assert_eq!(RunOrigin::from_metadata(&json!({})), Some(RunOrigin::Chat));
        assert_eq!(
            RunOrigin::from_metadata(&json!({"origin": null})),
            Some(RunOrigin::Chat)
        );
        assert_eq!(
            RunOrigin::from_metadata(&json!({"origin": "schedule", "source": "scheduled_run"})),
            Some(RunOrigin::Schedule)
        );
        assert_eq!(
            RunOrigin::from_metadata(&json!({"origin": "sheduled"})),
            None
        );
        assert_eq!(RunOrigin::from_metadata(&json!({"origin": 1})), None);
    }
}
