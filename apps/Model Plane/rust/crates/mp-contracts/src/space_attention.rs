//! Cross-room attention switch (build plan WS11-S2).
//!
//! The switch is a rollout control, never an authorization input: Control's per-Space read
//! decisions and Session Core's SQL predicates decide what a caller may count or see. It is ON
//! only for an explicit on-word, so an unset, empty or mistyped value keeps the RPC closed.

use crate::agentic_subscription::switch_value_is_on;

/// Session Core and model-gateway: serve `SpaceAttention.GetSpaceAttention` and its HTTP route.
pub const SUMMARY_ENV: &str = "SPACE_ATTENTION_SUMMARY";

/// Most Spaces one call may ask about.
pub const MAX_SPACES: usize = 60;
/// Most headline thread ids one Space may ask about.
pub const MAX_HEADLINE_IDS: usize = 20;
/// Most pending approvals returned per Space.
pub const MAX_PENDING: usize = 20;
/// Unread threads are counted up to this number; above it the result says `unread_capped`.
pub const UNREAD_CAP: usize = 100;

/// Pure form of [`summary_enabled`]: `value` is the raw variable, `None` when unset.
#[must_use]
pub fn enabled_from(value: Option<&str>) -> bool {
    switch_value_is_on(value)
}

/// Whether the attention aggregate answers in this process.
#[must_use]
pub fn summary_enabled() -> bool {
    enabled_from(std::env::var(SUMMARY_ENV).ok().as_deref())
}
