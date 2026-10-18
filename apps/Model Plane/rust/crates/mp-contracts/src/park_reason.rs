//! The closed set of wire tokens for [`ParkReason`] (build plan WS03 S4).
//!
//! The strings are the serde snake_case names, so a token that crosses the
//! `SaveLoopCheckpoint` / `RunDetail.park_reason` wire and a fold's own JSON agree.
//! This impl lives apart from `loop_fold.rs` so the two never edit the same hunk.

use crate::loop_fold::ParkReason;

impl ParkReason {
    /// Every reason, in declaration order.
    pub const ALL: [Self; 5] = [
        Self::CredentialWindow,
        Self::ProviderExhausted,
        Self::UpstreamUnavailable,
        Self::CrashInterrupted,
        Self::ReauthRequired,
    ];

    /// The wire token: the serde snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CredentialWindow => "credential_window",
            Self::ProviderExhausted => "provider_exhausted",
            Self::UpstreamUnavailable => "upstream_unavailable",
            Self::CrashInterrupted => "crash_interrupted",
            Self::ReauthRequired => "reauth_required",
        }
    }

    /// Parse a wire token. Exact match only: no trimming, no case folding, so
    /// free text is never accepted as a reason.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|reason| reason.as_str() == token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_round_trips_through_its_wire_token_and_matches_serde() {
        assert_eq!(ParkReason::ALL.len(), 5);
        for reason in ParkReason::ALL {
            let token = reason.as_str();
            assert_eq!(ParkReason::parse(token), Some(reason), "{token}");
            assert_eq!(
                serde_json::to_string(&reason).expect("serialise"),
                format!("\"{token}\""),
                "{token} must equal the serde name"
            );
        }
        assert_eq!(ParkReason::CredentialWindow.as_str(), "credential_window");
        assert_eq!(ParkReason::ProviderExhausted.as_str(), "provider_exhausted");
        assert_eq!(ParkReason::UpstreamUnavailable.as_str(), "upstream_unavailable");
        assert_eq!(ParkReason::CrashInterrupted.as_str(), "crash_interrupted");
        assert_eq!(ParkReason::ReauthRequired.as_str(), "reauth_required");
    }

    #[test]
    fn parse_rejects_everything_outside_the_closed_set() {
        for bad in [
            "",
            " ",
            "CREDENTIAL_WINDOW",
            "credential_window ",
            " credential_window",
            "credential-window",
            "CredentialWindow",
            "token=abc",
            "unknown",
        ] {
            assert_eq!(ParkReason::parse(bad), None, "{bad:?}");
        }
    }
}
