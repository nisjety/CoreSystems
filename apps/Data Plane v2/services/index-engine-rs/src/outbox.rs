use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::Context as JsContext;
use event_envelope_rs::EventSigner;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};

pub const DELETION_SUBJECT: &str = "dataplane.knowledge.units.deleted";
const DEFAULT_LEASE_SECONDS: i32 = 30;
type IntentRow = (i64, String, String, Value, Option<String>, String, i32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletionIntent {
    pub outbox_id: i64,
    pub org_id: String,
    pub document_id: String,
    pub knowledge_ids: Vec<String>,
    pub user_id: Option<String>,
    pub idempotency_key: String,
    pub attempts: i32,
}

pub async fn delete_and_enqueue(
    pool: &PgPool,
    org_id: &str,
    document_id: &str,
    user_id: Option<&str>,
    idempotency_key: &str,
    zdr: bool,
) -> anyhow::Result<usize> {
    validate_identity(org_id, document_id, user_id, idempotency_key)?;
    if zdr {
        anyhow::bail!("restrictive-ZDR deletion cannot create durable state or event intent");
    }
    let mut tx = pool.begin().await?;
    let knowledge_ids = sqlx::query_scalar::<_, String>(
        "SELECT knowledge_id FROM knowledge_units WHERE org_id=$1 AND document_id=$2 ORDER BY knowledge_id FOR UPDATE",
    )
    .bind(org_id)
    .bind(document_id)
    .fetch_all(&mut *tx)
    .await?;

    if knowledge_ids.is_empty() {
        tx.commit().await?;
        return Ok(0);
    }
    if knowledge_ids.iter().any(|id| id.trim().is_empty()) {
        anyhow::bail!("knowledge deletion contains an empty chunk id");
    }
    enqueue_intent(
        &mut tx,
        org_id,
        document_id,
        &knowledge_ids,
        user_id,
        idempotency_key,
        zdr,
    )
    .await?;

    sqlx::query("DELETE FROM knowledge_units WHERE org_id=$1 AND document_id=$2")
        .bind(org_id)
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(knowledge_ids.len())
}

pub async fn enqueue_intent(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    document_id: &str,
    knowledge_ids: &[String],
    user_id: Option<&str>,
    idempotency_key: &str,
    zdr: bool,
) -> anyhow::Result<()> {
    validate_identity(org_id, document_id, user_id, idempotency_key)?;
    if zdr {
        anyhow::bail!("restrictive-ZDR deletion cannot create durable event intent");
    }
    if knowledge_ids.is_empty() || knowledge_ids.iter().any(|id| id.trim().is_empty()) {
        anyhow::bail!("deletion intent requires non-empty chunk ids");
    }
    let knowledge_json = serde_json::to_value(knowledge_ids)?;
    let inserted = sqlx::query(
        "INSERT INTO index_deletion_outbox
         (org_id,document_id,knowledge_ids,user_id,idempotency_key)
         VALUES ($1,$2,$3,$4,$5)
         ON CONFLICT (idempotency_key) DO NOTHING",
    )
    .bind(org_id)
    .bind(document_id)
    .bind(&knowledge_json)
    .bind(user_id)
    .bind(idempotency_key)
    .execute(&mut **tx)
    .await?;

    if inserted.rows_affected() == 0 {
        let existing: Option<(String, String, Value)> = sqlx::query_as(
            "SELECT org_id,document_id,knowledge_ids FROM index_deletion_outbox WHERE idempotency_key=$1 FOR UPDATE",
        )
        .bind(idempotency_key)
        .fetch_optional(&mut **tx)
        .await?;
        if existing.as_ref()
            != Some(&(
                org_id.to_owned(),
                document_id.to_owned(),
                knowledge_json.clone(),
            ))
        {
            anyhow::bail!("deletion idempotency key conflicts with another scoped intent");
        }
    }

    Ok(())
}

pub async fn claim(
    pool: &PgPool,
    lease_owner: &str,
    lease_seconds: i32,
) -> anyhow::Result<Option<DeletionIntent>> {
    if lease_owner.trim().is_empty() || lease_seconds <= 0 {
        anyhow::bail!("valid outbox lease owner and duration required");
    }
    let row: Option<IntentRow> = sqlx::query_as(
        "WITH candidate AS (
           SELECT outbox_id FROM index_deletion_outbox
           WHERE (status='pending' AND available_at<=NOW())
              OR (status='leased' AND lease_until<=NOW())
           ORDER BY outbox_id FOR UPDATE SKIP LOCKED LIMIT 1
         )
         UPDATE index_deletion_outbox o
         SET status='leased', lease_owner=$1,
             lease_until=NOW()+make_interval(secs=>$2),
             attempts=attempts+1, updated_at=NOW()
         FROM candidate c WHERE o.outbox_id=c.outbox_id
         RETURNING o.outbox_id,o.org_id,o.document_id,o.knowledge_ids,
                   o.user_id,o.idempotency_key,o.attempts",
    )
    .bind(lease_owner)
    .bind(lease_seconds)
    .fetch_optional(pool)
    .await?;
    row.map(intent_from_row).transpose()
}

pub async fn mark_delivered(
    pool: &PgPool,
    outbox_id: i64,
    lease_owner: &str,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE index_deletion_outbox
         SET status='delivered',delivered_at=NOW(),lease_owner=NULL,
             lease_until=NULL,last_error=NULL,updated_at=NOW()
         WHERE outbox_id=$1 AND status='leased' AND lease_owner=$2",
    )
    .bind(outbox_id)
    .bind(lease_owner)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn mark_failed(
    pool: &PgPool,
    outbox_id: i64,
    lease_owner: &str,
    error: &str,
) -> anyhow::Result<bool> {
    let safe_error: String = error.chars().take(512).collect();
    let result = sqlx::query(
        "UPDATE index_deletion_outbox
         SET status='pending',available_at=NOW()+INTERVAL '1 second',
             lease_owner=NULL,lease_until=NULL,last_error=$3,updated_at=NOW()
         WHERE outbox_id=$1 AND status='leased' AND lease_owner=$2",
    )
    .bind(outbox_id)
    .bind(lease_owner)
    .bind(safe_error)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn run_publisher(
    pool: PgPool,
    js: JsContext,
    signer: Arc<EventSigner>,
) -> anyhow::Result<()> {
    let lease_owner = format!("index-engine:{}", uuid::Uuid::new_v4());
    loop {
        let Some(intent) = claim(&pool, &lease_owner, DEFAULT_LEASE_SECONDS).await? else {
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        };
        let result = publish_intent(&js, signer.as_ref(), &intent).await;
        match result {
            Ok(()) => {
                if !mark_delivered(&pool, intent.outbox_id, &lease_owner).await? {
                    tracing::warn!(
                        outbox_id = intent.outbox_id,
                        "deletion outbox lease changed after publish"
                    );
                }
            }
            Err(error) => {
                tracing::warn!(outbox_id = intent.outbox_id, %error, "signed deletion publish failed; retry scheduled");
                let _ =
                    mark_failed(&pool, intent.outbox_id, &lease_owner, &error.to_string()).await?;
            }
        }
    }
}

async fn publish_intent(
    js: &JsContext,
    signer: &EventSigner,
    intent: &DeletionIntent,
) -> anyhow::Result<()> {
    let payload = intent_payload(intent)?;
    let envelope = signer.sign(
        DELETION_SUBJECT,
        &intent.org_id,
        intent.user_id.as_deref(),
        false,
        &serde_json::to_vec(&payload)?,
    )?;
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Nats-Msg-Id", intent.idempotency_key.as_str());
    js.publish_with_headers(DELETION_SUBJECT, headers, envelope.into())
        .await?
        .await?;
    Ok(())
}

pub fn intent_payload(intent: &DeletionIntent) -> anyhow::Result<Value> {
    validate_identity(
        &intent.org_id,
        &intent.document_id,
        intent.user_id.as_deref(),
        &intent.idempotency_key,
    )?;
    if intent.knowledge_ids.is_empty() || intent.knowledge_ids.iter().any(|id| id.trim().is_empty())
    {
        anyhow::bail!("deletion intent requires non-empty chunk ids");
    }
    Ok(serde_json::json!({
        "document_id": intent.document_id,
        "org_id": intent.org_id,
        "knowledge_ids": intent.knowledge_ids,
        "user_id": intent.user_id,
        "zdr": false,
        "idempotency_key": intent.idempotency_key,
    }))
}

fn intent_from_row(row: IntentRow) -> anyhow::Result<DeletionIntent> {
    Ok(DeletionIntent {
        outbox_id: row.0,
        org_id: row.1,
        document_id: row.2,
        knowledge_ids: serde_json::from_value(row.3)?,
        user_id: row.4,
        idempotency_key: row.5,
        attempts: row.6,
    })
}

fn validate_identity(
    org_id: &str,
    document_id: &str,
    user_id: Option<&str>,
    idempotency_key: &str,
) -> anyhow::Result<()> {
    if org_id.trim().is_empty()
        || document_id.trim().is_empty()
        || user_id.is_some_and(|user| user.trim().is_empty())
        || !(8..=128).contains(&idempotency_key.len())
        || idempotency_key != idempotency_key.trim()
    {
        anyhow::bail!("invalid tenant-scoped deletion identity");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_payload_rejects_ambiguous_scope() {
        let mut intent = DeletionIntent {
            outbox_id: 1,
            org_id: "org-a".into(),
            document_id: "doc-a".into(),
            knowledge_ids: vec!["kid-a".into()],
            user_id: None,
            idempotency_key: "delete-event-1".into(),
            attempts: 1,
        };
        assert!(intent_payload(&intent).is_ok());
        intent.org_id.clear();
        assert!(intent_payload(&intent).is_err());
        intent.org_id = "org-a".into();
        intent.knowledge_ids.clear();
        assert!(intent_payload(&intent).is_err());
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn postgres_outbox_is_atomic_tenant_scoped_and_crash_retryable() {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to disposable PostgreSQL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
            .expect("connect disposable postgres");
        sqlx::raw_sql(
            "CREATE TABLE knowledge_units (
               knowledge_id TEXT PRIMARY KEY,
               org_id TEXT NOT NULL,
               document_id TEXT NOT NULL
             )",
        )
        .execute(&pool)
        .await
        .expect("create minimal knowledge table");
        sqlx::raw_sql(include_str!(
            "../../../infra/postgres/migrations/20260711170000_index_deletion_outbox.sql"
        ))
        .execute(&pool)
        .await
        .expect("apply outbox migration");

        for (kid, org, doc) in [
            ("kid-a1", "org-a", "doc-a"),
            ("kid-a2", "org-a", "doc-a"),
            ("kid-b1", "org-b", "doc-a"),
        ] {
            sqlx::query("INSERT INTO knowledge_units VALUES ($1,$2,$3)")
                .bind(kid)
                .bind(org)
                .bind(doc)
                .execute(&pool)
                .await
                .expect("insert fixture chunk");
        }

        let idempotency = "delete-event-fixture-1";
        assert!(
            delete_and_enqueue(&pool, "org-a", "doc-a", Some("user-a"), idempotency, true,)
                .await
                .is_err(),
            "ZDR delete must fail before durable mutation"
        );
        assert_eq!(
            delete_and_enqueue(&pool, "org-a", "doc-a", Some("user-a"), idempotency, false,)
                .await
                .expect("atomic delete and enqueue"),
            2
        );
        let org_a_remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM knowledge_units WHERE org_id='org-a' AND document_id='doc-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let org_b_remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM knowledge_units WHERE org_id='org-b' AND document_id='doc-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(org_a_remaining, 0);
        assert_eq!(org_b_remaining, 1, "cross-tenant chunk was deleted");
        assert_eq!(
            delete_and_enqueue(&pool, "org-a", "doc-a", Some("user-a"), idempotency, false,)
                .await
                .unwrap(),
            0,
            "redelivery must not duplicate intent"
        );

        let first = claim(&pool, "worker-a", 60)
            .await
            .unwrap()
            .expect("first lease");
        assert_eq!(first.knowledge_ids, vec!["kid-a1", "kid-a2"]);
        assert_eq!(first.attempts, 1);
        sqlx::query(
            "UPDATE index_deletion_outbox SET lease_until=NOW()-INTERVAL '1 second' WHERE outbox_id=$1",
        )
        .bind(first.outbox_id)
        .execute(&pool)
        .await
        .unwrap();
        let retried = claim(&pool, "worker-b", 60)
            .await
            .unwrap()
            .expect("expired lease reclaimed after simulated crash");
        assert_eq!(retried.outbox_id, first.outbox_id);
        assert_eq!(retried.attempts, 2);
        assert!(
            mark_failed(&pool, retried.outbox_id, "worker-b", "broker unavailable")
                .await
                .unwrap()
        );
        sqlx::query("UPDATE index_deletion_outbox SET available_at=NOW() WHERE outbox_id=$1")
            .bind(retried.outbox_id)
            .execute(&pool)
            .await
            .unwrap();
        let final_claim = claim(&pool, "worker-c", 60)
            .await
            .unwrap()
            .expect("failed publish retried");
        assert_eq!(final_claim.attempts, 3);
        assert!(mark_delivered(&pool, final_claim.outbox_id, "worker-c")
            .await
            .unwrap());
        assert!(!mark_delivered(&pool, final_claim.outbox_id, "worker-c")
            .await
            .unwrap());

        sqlx::query("INSERT INTO knowledge_units VALUES ('kid-b2','org-b','doc-b')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            delete_and_enqueue(&pool, "org-b", "doc-b", None, idempotency, false)
                .await
                .is_err()
        );
        let collision_survived: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM knowledge_units WHERE org_id='org-b' AND document_id='doc-b'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            collision_survived, 1,
            "conflicting intent did not roll back"
        );

        sqlx::query("INSERT INTO knowledge_units VALUES ('kid-c1','org-c','doc-c')")
            .execute(&pool)
            .await
            .unwrap();
        let mut reindex_tx = pool.begin().await.unwrap();
        enqueue_intent(
            &mut reindex_tx,
            "org-c",
            "doc-c",
            &["kid-c1".into()],
            Some("user-c"),
            "rechunk-event-fixture-1",
            false,
        )
        .await
        .unwrap();
        sqlx::query("DELETE FROM knowledge_units WHERE org_id='org-c' AND document_id='doc-c'")
            .execute(&mut *reindex_tx)
            .await
            .unwrap();
        reindex_tx.rollback().await.unwrap();
        let rollback_pair: (i64, i64) = sqlx::query_as(
            "SELECT
               (SELECT COUNT(*) FROM knowledge_units WHERE org_id='org-c' AND document_id='doc-c'),
               (SELECT COUNT(*) FROM index_deletion_outbox WHERE idempotency_key='rechunk-event-fixture-1')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            rollback_pair,
            (1, 0),
            "re-chunk delete and intent were not atomic"
        );

        sqlx::raw_sql(include_str!(
            "../../../infra/postgres/migrations/20260711170000_index_deletion_outbox.down.sql"
        ))
        .execute(&pool)
        .await
        .expect("rollback outbox migration");
    }
}
