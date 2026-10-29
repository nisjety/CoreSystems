//! Typed crawl denial reasons with operator-readable messages.
//!
//! Donor: `internal/crawl/denial.go` (operator-readable rejection codes for
//! V1 crawl). Quarry v2 surfaces these through events and crawl-status
//! responses so dashboards can show *why* a URL was rejected, not just that
//! it was.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum CrawlDenialReason {
    /// URL is outside the configured crawl scope (path/host/depth).
    OutOfScope { detail: String },
    /// URL was blocked by robots.txt.
    RobotsDisallowed { user_agent: String, path: String },
    /// URL exceeded the configured maximum crawl depth.
    DepthExceeded { depth: u32, limit: u32 },
    /// URL exceeded max_pages cap for the run.
    MaxPagesReached { limit: u32 },
    /// URL was already visited (frontier dedup).
    Duplicate,
    /// URL points to an external host while external crawl is disabled.
    ExternalHostDisabled { host: String },
    /// URL was rejected by the security engine (SSRF, blocklist, suspicious URL).
    SecurityRejected { reason: String },
    /// URL did not match the include patterns.
    IncludePatternMiss,
    /// URL matched an exclude pattern.
    ExcludePatternHit { pattern: String },
    /// URL's content-type / extension is in the rejected list (PDFs, binaries, etc).
    ContentTypeRejected { content_type: String },
    /// URL points to a backward (fragment, mailto, tel) destination not crawlable.
    NotCrawlable { reason: String },
}

impl CrawlDenialReason {
    /// Stable code string suitable for logging, metrics labels, and dashboards.
    pub fn code(&self) -> &'static str {
        match self {
            Self::OutOfScope { .. } => "out_of_scope",
            Self::RobotsDisallowed { .. } => "robots_disallowed",
            Self::DepthExceeded { .. } => "depth_exceeded",
            Self::MaxPagesReached { .. } => "max_pages_reached",
            Self::Duplicate => "duplicate",
            Self::ExternalHostDisabled { .. } => "external_host_disabled",
            Self::SecurityRejected { .. } => "security_rejected",
            Self::IncludePatternMiss => "include_pattern_miss",
            Self::ExcludePatternHit { .. } => "exclude_pattern_hit",
            Self::ContentTypeRejected { .. } => "content_type_rejected",
            Self::NotCrawlable { .. } => "not_crawlable",
        }
    }

    /// Operator-readable message safe for surfacing in UIs.
    pub fn message(&self) -> String {
        match self {
            Self::OutOfScope { detail } => format!("URL is outside crawl scope: {detail}"),
            Self::RobotsDisallowed { user_agent, path } => {
                format!("robots.txt for user-agent '{user_agent}' disallows {path}")
            }
            Self::DepthExceeded { depth, limit } => {
                format!("crawl depth {depth} exceeded configured limit {limit}")
            }
            Self::MaxPagesReached { limit } => {
                format!("max_pages limit of {limit} reached for this run")
            }
            Self::Duplicate => "URL already visited (frontier dedup)".to_string(),
            Self::ExternalHostDisabled { host } => {
                format!("external host '{host}' is disabled for this crawl")
            }
            Self::SecurityRejected { reason } => format!("security policy rejection: {reason}"),
            Self::IncludePatternMiss => "URL did not match any include pattern".to_string(),
            Self::ExcludePatternHit { pattern } => {
                format!("URL matched exclude pattern: {pattern}")
            }
            Self::ContentTypeRejected { content_type } => {
                format!("content-type '{content_type}' is in the rejection list")
            }
            Self::NotCrawlable { reason } => format!("URL is not crawlable: {reason}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_unique() {
        let reasons = vec![
            CrawlDenialReason::OutOfScope { detail: "x".into() },
            CrawlDenialReason::RobotsDisallowed {
                user_agent: "ua".into(),
                path: "/p".into(),
            },
            CrawlDenialReason::DepthExceeded { depth: 5, limit: 3 },
            CrawlDenialReason::MaxPagesReached { limit: 100 },
            CrawlDenialReason::Duplicate,
            CrawlDenialReason::ExternalHostDisabled { host: "h".into() },
            CrawlDenialReason::SecurityRejected { reason: "r".into() },
            CrawlDenialReason::IncludePatternMiss,
            CrawlDenialReason::ExcludePatternHit { pattern: "p".into() },
            CrawlDenialReason::ContentTypeRejected {
                content_type: "ct".into(),
            },
            CrawlDenialReason::NotCrawlable { reason: "r".into() },
        ];
        let codes: Vec<&'static str> = reasons.iter().map(|r| r.code()).collect();
        let mut sorted = codes.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(codes.len(), sorted.len(), "codes must be unique");
    }

    #[test]
    fn messages_include_operator_context() {
        assert!(
            CrawlDenialReason::DepthExceeded { depth: 5, limit: 3 }
                .message()
                .contains("5")
        );
        assert!(
            CrawlDenialReason::ExcludePatternHit {
                pattern: "/admin/*".into()
            }
            .message()
            .contains("/admin/*")
        );
    }

    #[test]
    fn serde_round_trip() {
        let reason = CrawlDenialReason::RobotsDisallowed {
            user_agent: "Quarry/1.0".into(),
            path: "/private".into(),
        };
        let json = serde_json::to_string(&reason).unwrap();
        let restored: CrawlDenialReason = serde_json::from_str(&json).unwrap();
        assert_eq!(reason, restored);
        assert!(json.contains("\"code\":\"robots_disallowed\""));
    }
}
