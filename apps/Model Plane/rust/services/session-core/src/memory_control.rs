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

/// Drops semantic-store entries that must not be recalled: ids this user
/// forgot, and ids of durable rows that are no longer current
/// (`is_latest = false`, retired by a lineage write or the dedup backfill).
///
/// The durable read paths filter `is_latest = true` in SQL, but a semantic
/// mirror carries no such flag. Without the second half of the query a
/// superseded row that was ever mirrored keeps resurfacing through chat
/// recall, the memory panel and grounding's own semantic tier. An entry
/// whose id has no durable row at all is kept: the semantic tier can hold
/// memories the durable index never had.
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
    let hidden: Vec<String> = sqlx::query_scalar(
        "SELECT memory_id FROM forgotten_memory_ids WHERE org_id = $1 AND user_id = $2 AND memory_id = ANY($3) \
         UNION \
         SELECT id FROM agent_memory WHERE org_id = $1 AND id = ANY($3) AND is_latest = false",
    )
    .bind(org)
    .bind(user)
    .bind(ids)
    .fetch_all(pool)
    .await?;
    retain_recallable(entries, &hidden);
    Ok(())
}

fn retain_recallable(entries: &mut Vec<MemoryEntry>, hidden: &[String]) {
    entries.retain(|entry| !hidden.contains(&entry.memory_id));
}

/// Best-effort removal of the semantic mirrors of rows a lineage write just
/// retired. [`filter_forgotten`] already keeps them out of every read, so a
/// degraded delete here costs storage, not correctness.
pub(crate) async fn erase_retired_mirrors(
    letta: &crate::letta_adapter::LettaMemoryAdapter,
    org: &str,
    user: &str,
    retired_ids: &[String],
) {
    for memory_id in retired_ids {
        let outcome = letta.delete_detailed(org, user, memory_id).await;
        if outcome.degradation_reason.is_some() {
            tracing::warn!(
                memory_id = %memory_id,
                "retired memory mirror cleanup degraded; recall remains suppressed"
            );
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(memory_id: &str) -> MemoryEntry {
        MemoryEntry {
            memory_id: memory_id.to_owned(),
            content: format!("content of {memory_id}"),
            ..MemoryEntry::default()
        }
    }

    #[test]
    fn hidden_ids_are_dropped_and_everything_else_keeps_its_order() {
        let mut entries = vec![entry("a"), entry("retired"), entry("b"), entry("forgotten")];
        retain_recallable(
            &mut entries,
            &["forgotten".to_owned(), "retired".to_owned()],
        );
        let ids: Vec<&str> = entries.iter().map(|e| e.memory_id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    /// A superseded row's semantic mirror must not come back through recall,
    /// the memory panel or grounding. Before this filter checked `is_latest`,
    /// the retired row below survived `filter_forgotten` untouched.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_retired_rows_mirror_is_filtered_but_a_current_or_unknown_one_is_not() {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let org = format!("retired-filter-{}", mp_ids::new_ulid());
        let (current, retired) = (mp_ids::new_ulid(), mp_ids::new_ulid());
        for (id, key, latest) in [
            (&current, "user:llm:tone", true),
            (&retired, "user:llm:preferred_tone", false),
        ] {
            sqlx::query(
                "INSERT INTO agent_memory (id, org_id, scope, key, content, owner, is_latest) \
                 VALUES ($1, $2, 'user', $3, 'User prefers a warmer tone.', 'user-a', $4)",
            )
            .bind(id)
            .bind(&org)
            .bind(key)
            .bind(latest)
            .execute(&pool)
            .await
            .unwrap();
        }

        let mut entries = vec![entry(&current), entry(&retired), entry("semantic-only")];
        filter_forgotten(&pool, &org, "user-a", &mut entries)
            .await
            .unwrap();
        let ids: Vec<&str> = entries.iter().map(|e| e.memory_id.as_str()).collect();
        assert_eq!(ids, [current.as_str(), "semantic-only"]);

        // Retirement is tenant-scoped like forgetting: another org's filter
        // never consults this org's rows.
        let mut foreign = vec![entry(&retired)];
        filter_forgotten(&pool, "another-org", "user-a", &mut foreign)
            .await
            .unwrap();
        assert_eq!(foreign.len(), 1);
    }
}
