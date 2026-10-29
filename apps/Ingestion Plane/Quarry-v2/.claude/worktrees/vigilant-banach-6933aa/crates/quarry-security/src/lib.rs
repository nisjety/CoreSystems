//! quarry-security — preflight + discovered-URL security checks.
//!
//! Donor: `internal/security/` (Go). Port plan: heuristics + repo provider + crawl checks.
//!
//! Verdict-first: every URL gets a `Verdict { allow | block | escalate }` with reasons.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

pub mod heur;
pub mod preflight;
pub mod urlsig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Block,
    Escalate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    pub decision: Decision,
    pub reasons: Vec<String>,
    pub risk_score: u8,
}

impl Verdict {
    pub fn allow() -> Self {
        Self {
            decision: Decision::Allow,
            reasons: vec![],
            risk_score: 0,
        }
    }
    pub fn block(reason: impl Into<String>) -> Self {
        Self {
            decision: Decision::Block,
            reasons: vec![reason.into()],
            risk_score: 100,
        }
    }
}

#[async_trait]
pub trait SecurityEngine: Send + Sync {
    async fn preflight(&self, url: &Url) -> Verdict;
    async fn check_discovered(&self, parent: &Url, child: &Url) -> Verdict;
    /// Whether private/loopback hosts may be reached. Default false.
    fn allow_private_hosts(&self) -> bool {
        false
    }
}
