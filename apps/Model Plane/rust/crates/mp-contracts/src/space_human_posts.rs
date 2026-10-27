//! Space human-post switches (build plan WS01-S3).
//!
//! Both switches are rollout controls, never authorization inputs: Control's signed decision and
//! Session Core's member gate decide who may post. Each is ON only for an explicit on-word, so an
//! unset, empty or mistyped value keeps the member path closed.

use crate::agentic_subscription::switch_value_is_on;

/// Session Core: lets a Space member who does not own a thread append a `post` to it.
pub const ENABLED_ENV: &str = "SPACE_HUMAN_POSTS_CORE";

/// model-gateway: serves the two human-post routes.
pub const API_ENV: &str = "SPACE_HUMAN_POSTS_API";

/// Pure form of [`core_enabled`]: `value` is the raw variable, `None` when unset.
#[must_use]
pub fn enabled_from(value: Option<&str>) -> bool {
    switch_value_is_on(value)
}

/// Whether the member append path is on for this Session Core process.
#[must_use]
pub fn core_enabled() -> bool {
    enabled_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// Whether the human-post routes are on for this model-gateway process.
#[must_use]
pub fn api_enabled() -> bool {
    enabled_from(std::env::var(API_ENV).ok().as_deref())
}
