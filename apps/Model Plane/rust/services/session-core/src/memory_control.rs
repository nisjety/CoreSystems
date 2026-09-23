//! User edits/forgetting take precedence over delayed extraction and mirrors.
use mp_contracts::model_plane::v1::MemoryEntry;
use sqlx::{PgPool, Postgres, Transaction};

pub(crate) async fn lock_user(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO user_memory_controls (org_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(org)
    .bind(user)
    .execute(&mut **tx)
    .await?;
    sqlx::query("SELECT 1 FROM user_memory_controls WHERE org_id = $1 AND user_id = $2 FOR UPDATE")
        .bind(org)
        .bind(user)
        .fetch_one(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) async fn permits_extraction(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
    message_id: &str,
) -> Result<bool, sqlx::Error> {
    lock_user(tx, org, user).await?;
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM messages m JOIN threads t ON t.id = m.thread_id JOIN user_memory_controls c ON c.org_id = t.org_id AND c.user_id = t.user_id WHERE m.id = $3 AND t.org_id = $1 AND t.user_id = $2 AND m.created_at > c.extract_after)")
        .bind(org).bind(user).bind(message_id).fetch_one(&mut **tx).await
}

pub(crate) async fn record_forgetting(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
    memory_id: &str,
) -> Result<(), sqlx::Error> {
    lock_user(tx, org, user).await?;
    // clock_timestamp is measured after the lock, not when a waiting tx began.
    sqlx::query("UPDATE user_memory_controls SET extract_after = clock_timestamp() WHERE org_id = $1 AND user_id = $2")
        .bind(org).bind(user).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO forgotten_memory_ids (org_id, user_id, memory_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(org).bind(user).bind(memory_id).execute(&mut **tx).await?;
    Ok(())
}

pub(crate) async fn filter_forgotten(
    pool: &PgPool,
    org: &str,
    user: &str,
    entries: &mut Vec<MemoryEntry>,
) -> Result<(), sqlx::Error> {
    if entries.is_empty() {
        return Ok(());
    }
    let ids: Vec<&str> = entries
        .iter()
        .map(|entry| entry.memory_id.as_str())
        .collect();
    let forgotten: Vec<String> = sqlx::query_scalar("SELECT memory_id FROM forgotten_memory_ids WHERE org_id = $1 AND user_id = $2 AND memory_id = ANY($3)")
        .bind(org).bind(user).bind(ids).fetch_all(pool).await?;
    entries.retain(|entry| !forgotten.contains(&entry.memory_id));
    Ok(())
}

pub(crate) async fn erase_late_mirror(
    pool: &PgPool,
    letta: &crate::letta_adapter::LettaMemoryAdapter,
    org: &str,
    user: &str,
    memory_id: &str,
) {
    let forgotten = sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM forgotten_memory_ids WHERE org_id = $1 AND user_id = $2 AND memory_id = $3)")
        .bind(org).bind(user).bind(memory_id).fetch_one(pool).await;
    match forgotten {
        Ok(true) => {
            let outcome = letta.delete_detailed(org, user, memory_id).await;
            if outcome.degradation_reason.is_some() {
                tracing::warn!(
                    memory_id,
                    "late forgotten memory mirror cleanup degraded; recall remains suppressed"
                );
            }
        }
        Ok(false) => {}
        Err(_) => tracing::warn!(memory_id, "late memory mirror deletion check unavailable"),
    }
}
