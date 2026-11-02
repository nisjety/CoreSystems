//! Space message lifecycle switches (build plan WS12-S4, WS12-S6).
//!
//! Both switches are rollout controls, never authorization inputs: Control's signed
//! `model.thread.revise` decision and Session Core's revise gate decide who may edit or delete.
//! Each is ON only for an explicit on-word, so an unset, empty or mistyped value keeps the path
//! closed. The READ side (the fold of revisions into every transcript, context and Dreaming
//! read) has no switch: it is inert until a revision exists and must never be turned off while
//! one does, or a deleted message would reappear.

use crate::agentic_subscription::switch_value_is_on;

/// Session Core: accepts `ReviseMessage`.
pub const CORE_ENV: &str = "SPACE_MESSAGE_REVISE_CORE";

/// model-gateway: serves the edit, delete and target routes.
pub const API_ENV: &str = "SPACE_MESSAGE_REVISE_API";

/// Pure form of [`core_enabled`]: `value` is the raw variable, `None` when unset.
#[must_use]
pub fn enabled_from(value: Option<&str>) -> bool {
    switch_value_is_on(value)
}

/// Whether `ReviseMessage` is on for this Session Core process.
#[must_use]
pub fn core_enabled() -> bool {
    enabled_from(std::env::var(CORE_ENV).ok().as_deref())
}

/// Whether the revise routes are on for this model-gateway process.
#[must_use]
pub fn api_enabled() -> bool {
    enabled_from(std::env::var(API_ENV).ok().as_deref())
}
