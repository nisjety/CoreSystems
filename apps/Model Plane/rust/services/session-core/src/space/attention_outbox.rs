//! Space attention outbox: the Rust side (build plan WS11-S5).
//!
//! The outbox rows themselves are written by the trigger in migration 0080, in the same
//! transaction as the Space event log row they derive from, and published by the existing
//! audit outbox drainer ([`crate::audit_publisher`], `OutboxRoute::SpaceEvent`). This
//! module owns the one thing the database cannot: the boot-time flag.
//!
//! - [`sync_flag`]: writes `SPACE_ATTENTION_EMIT` into `space_attention_settings`. A trigger
//!   cannot read the environment, so the setting row is what it reads. The flag refuses to
//!   turn on while the event log itself is off (the log is the source: with no log rows
//!   there is nothing to project), warns, and stays off.
//!
//! The flag is never an authorization input and never a ZDR control: the projection admits
//! persistent log rows only, whatever the flag says.

use anyhow::Result;
use sqlx::PgPool;
use tracing::{info, warn};

/// Env switch for writing attention outbox rows. Off unless exactly `true` or `1`.
pub(crate) const EMIT_ENV: &str = "SPACE_ATTENTION_EMIT";

/// Whether attention outbox rows are requested. Off by default (release artifacts); compose
/// dev sets it.
pub(crate) fn emit_enabled_from_env() -> bool {
    std::env::var(EMIT_ENV).is_ok_and(|value| {
        let value = value.trim();
        value.eq_ignore_ascii_case("true") || value == "1"
    })
}

/// Writes the flag into `space_attention_settings` and returns the value now stored.
/// Turning it on while `space_event_settings.enabled` is false is refused: the stored value
/// stays off and a warning is logged.
pub(crate) async fn sync_flag(pool: &PgPool, requested: bool) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let log_enabled: bool = sqlx::query_scalar(
        "SELECT coalesce(
             (SELECT enabled FROM space_event_settings WHERE singleton), FALSE)",
    )
    .fetch_one(&mut *tx)
    .await?;
    let effective = requested && log_enabled;
    if requested && !log_enabled {
        warn!(
            "SPACE_ATTENTION_EMIT is on but the Space event log is off \
             (SPACE_REALTIME_EMIT); attention emit stays off"
        );
    }
    sqlx::query(
        "INSERT INTO space_attention_settings (singleton, enabled, updated_at)
         VALUES (TRUE, $1, now())
         ON CONFLICT (singleton) DO UPDATE
            SET enabled = EXCLUDED.enabled, updated_at = now()
            WHERE space_attention_settings.enabled IS DISTINCT FROM EXCLUDED.enabled",
    )
    .bind(effective)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    info!(requested, effective, "space attention outbox flag synced");
    Ok(effective)
}
