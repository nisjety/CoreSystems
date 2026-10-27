//! Space memory containment flag (build plan WS17-S1).
//!
//! `MEMORY_SPACE_CONTAINMENT` is a kill switch, not an authorization input: containment is
//! ON unless the variable is set to exactly `0`, `false` or `off` (case-insensitive,
//! trimmed). An unset, empty or unrecognised value keeps containment on, so a typo can never
//! widen what a shared room may learn or recall.

/// Environment variable read by session-core.
pub const MEMORY_SPACE_CONTAINMENT_ENV: &str = "MEMORY_SPACE_CONTAINMENT";

/// Pure form of [`containment_enabled`]: `value` is the raw variable, `None` when unset.
#[must_use]
pub fn containment_enabled_from(value: Option<&str>) -> bool {
    !matches!(
        value.map(|raw| raw.trim().to_ascii_lowercase()).as_deref(),
        Some("0" | "false" | "off")
    )
}

/// Whether Space memory containment is on for this process.
#[must_use]
pub fn containment_enabled() -> bool {
    containment_enabled_from(std::env::var(MEMORY_SPACE_CONTAINMENT_ENV).ok().as_deref())
}
