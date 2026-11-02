//! Space room timeline switches (build plan WS15-S6).
//!
//! Both switches are rollout controls, never authorization inputs: Control's per-Space read
//! decision and Session Core's SQL predicates decide what a caller may see. Each is ON only for an
//! explicit on-word, so an unset, empty or mistyped value keeps the feature closed. The thread
//! activity trigger and backfill (migration 0078) always run, inert until read.

use crate::agentic_subscription::switch_value_is_on;

/// Session Core: serve `SpaceRoomReadService.ListSpaceTimeline` and the `include_first` tail
/// transcript of `ListConversation`.
pub const CORE_ENV: &str = "SPACE_TIMELINE_CORE";
/// model-gateway: serve `GET /v1/spaces/:space_id/timeline` and the tail transcript parameters.
pub const API_ENV: &str = "SPACE_TIMELINE_API";

/// The `failed_precondition` message Session Core answers with while the summary backfill is
/// unfinished, and the error code the model-gateway route reports for it.
pub const BACKFILL_PENDING_MESSAGE: &str = "summary_backfill_pending";

/// Most posts one window returns (the request default is [`DEFAULT_LIMIT`]).
pub const MAX_LIMIT: u32 = 100;
/// Window size when the request names none.
pub const DEFAULT_LIMIT: u32 = 50;
/// Most pinned posts returned with the first page.
pub const MAX_PINNED: usize = 10;
/// Most caller-named thread ids re-read per request.
pub const MAX_REFRESH_IDS: usize = 30;

/// Pure form of [`core_enabled`]: `value` is the raw variable, `None` when unset.
#[must_use]
pub fn enabled_from(value: Option<&str>) -> bool {
    switch_value_is_on(value)
}

/// Whether Session Core answers the timeline in this process.
#[must_use]
pub fn core_enabled() -> bool {
    enabled_from(std::env::var(CORE_ENV).ok().as_deref())
}

/// Whether model-gateway serves the timeline route in this process.
#[must_use]
pub fn api_enabled() -> bool {
    enabled_from(std::env::var(API_ENV).ok().as_deref())
}
