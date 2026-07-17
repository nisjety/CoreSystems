//! CAG — pinned permanent-memory context.
//!
//! Org-scoped facts an operator/agent pins once and every context pack carries
//! from then on, WITHOUT a retrieval loop: pins are prepended (priority-first)
//! into `/v1/retrieve` context packs and served standalone via
//! `POST /v1/context/preload` — the cache-augmented-generation path where the
//! model preloads its permanent memory instead of retrieving.
//!
//! Authorization: org is pinned from the verified `AuthContext` at the API
//! boundary (`pin_request_org`), and every query here is org-scoped. Pins are
//! org-shared by design (like wiki) — they are not per-user ownable resources.

use sqlx::PgPool;

/// Hard cap on pins fetched per pack/preload — a runaway pin list must not eat
/// the whole context budget fetch.
pub const MAX_PINS: i64 = 50;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ContextPin {
    pub pin_id: String,
    pub title: String,
    pub content: String,
    pub priority: i32,
}

const LIST_PINS_SQL: &str = "SELECT pin_id, title, content, priority
     FROM context_pins
     WHERE org_id = $1
     ORDER BY priority ASC, created_at ASC
     LIMIT $2";

const UPSERT_PIN_SQL: &str =
    "INSERT INTO context_pins (pin_id, org_id, title, content, priority, pinned_by)
     VALUES ($1, $2, $3, $4, $5, $6)
     ON CONFLICT (pin_id) DO UPDATE
       SET title = EXCLUDED.title, content = EXCLUDED.content,
           priority = EXCLUDED.priority, updated_at = NOW()
       WHERE context_pins.org_id = EXCLUDED.org_id";

const DELETE_PIN_SQL: &str = "DELETE FROM context_pins WHERE org_id = $1 AND pin_id = $2";

fn list_pins_sql() -> &'static str {
    LIST_PINS_SQL
}
fn upsert_pin_sql() -> &'static str {
    UPSERT_PIN_SQL
}
fn delete_pin_sql() -> &'static str {
    DELETE_PIN_SQL
}

/// Org's pins, priority-first (lower number = more important = packed first).
pub async fn list_pins(pool: &PgPool, org_id: &str, limit: i64) -> anyhow::Result<Vec<ContextPin>> {
    let rows = sqlx::query_as::<_, (String, String, String, i32)>(list_pins_sql())
        .bind(org_id)
        .bind(limit.clamp(1, MAX_PINS))
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(pin_id, title, content, priority)| ContextPin {
            pin_id,
            title,
            content,
            priority,
        })
        .collect())
}

/// Creates or updates a pin. The org predicate on the UPDATE arm means a
/// colliding pin_id from another org is a no-op, never a cross-org overwrite.
/// Returns `false` in exactly that no-op case (0 rows affected) so the caller
/// can surface a conflict instead of a misleading success.
pub async fn upsert_pin(
    pool: &PgPool,
    org_id: &str,
    pin_id: &str,
    title: &str,
    content: &str,
    priority: i32,
    pinned_by: Option<&str>,
) -> anyhow::Result<bool> {
    let result = sqlx::query(upsert_pin_sql())
        .bind(pin_id)
        .bind(org_id)
        .bind(title)
        .bind(content)
        .bind(priority)
        .bind(pinned_by)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Deletes an org's pin. Returns whether a row was removed.
pub async fn delete_pin(pool: &PgPool, org_id: &str, pin_id: &str) -> anyhow::Result<bool> {
    let result = sqlx::query(delete_pin_sql())
        .bind(org_id)
        .bind(pin_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_queries_are_org_scoped_and_priority_ordered() {
        assert!(list_pins_sql().contains("WHERE org_id = $1"));
        assert!(list_pins_sql().contains("ORDER BY priority ASC"));
        assert!(list_pins_sql().contains("LIMIT $2"));
        // Upsert cannot cross orgs: the conflict-update arm re-checks org.
        assert!(upsert_pin_sql().contains("ON CONFLICT (pin_id) DO UPDATE"));
        assert!(upsert_pin_sql().contains("WHERE context_pins.org_id = EXCLUDED.org_id"));
        // Delete requires the org, not just the pin id.
        assert!(delete_pin_sql().contains("org_id = $1 AND pin_id = $2"));
    }
}
