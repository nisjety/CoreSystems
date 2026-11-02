//! Space message search switches (build plan WS13-S1).
//!
//! Both switches are rollout controls, never authorization inputs: Control's signed read decision
//! and Session Core's SQL predicates decide who may read what. Each is ON only for an explicit
//! on-word, so an unset, empty or mistyped value keeps search closed.

use crate::agentic_subscription::switch_value_is_on;

/// Session Core: serves `SpaceHistoryService` (`SearchMessages`, `ResolveMessage`).
pub const SEARCH_ENV: &str = "SPACE_MESSAGE_SEARCH_CORE";

/// model-gateway: serves the two Space history POST routes (WS13-S2).
pub const API_ENV: &str = "SPACE_MESSAGE_SEARCH_API";

/// Pure form of [`core_enabled`]: `value` is the raw variable, `None` when unset.
#[must_use]
pub fn enabled_from(value: Option<&str>) -> bool {
    switch_value_is_on(value)
}

/// Whether Space message search answers in this Session Core process.
#[must_use]
pub fn core_enabled() -> bool {
    enabled_from(std::env::var(SEARCH_ENV).ok().as_deref())
}

/// Whether the Space history routes are on in this model-gateway process.
#[must_use]
pub fn api_enabled() -> bool {
    enabled_from(std::env::var(API_ENV).ok().as_deref())
}
