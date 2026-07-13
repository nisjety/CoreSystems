use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::jobs::{AdminJob, JobExecutor, RebuildStage};
use crate::model::{acl_tags_field, datetime_field, string_field, QuickwitDocument};
use crate::quickwit::{quote_query_value, QuickwitClient};

#[derive(Clone)]
pub struct RebuildContext {
    pub pool: PgPool,
    pub quickwit: QuickwitClient,
    pub batch_size: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatchObservation {
    Ingest,
    AlreadyComplete,
}

fn observed_batch_action(observed: u64, expected: usize) -> anyhow::Result<BatchObservation> {
    match observed {
        0 => Ok(BatchObservation::Ingest),
        count if count == expected as u64 => Ok(BatchObservation::AlreadyComplete),
        _ => anyhow::bail!("rebuild batch has an ambiguous partial/duplicate index state"),
    }
}

fn deterministic_batch_id(job_id: &str, stage: RebuildStage, end_cursor: &str) -> String {
    let material = format!(
        "quickwit-rebuild-v1\0{job_id}\0{}\0{end_cursor}",
        stage as i16
    );
    format!("qwrb-{}", blake3::hash(material.as_bytes()).to_hex())
}

struct BatchRun<'a> {
    job_id: &'a str,
    runner_id: &'a str,
    stage: RebuildStage,
    start_cursor: Option<&'a str>,
}

impl<'a> BatchRun<'a> {
    fn for_job(job: &'a AdminJob, stage: RebuildStage) -> Result<Self, String> {
        let runner_id = job
            .lease_owner
            .as_deref()
            .ok_or_else(|| "rebuild job has no active lease owner".to_string())?;
        let start_cursor = (job.batch_stage == Some(stage))
            .then_some(job.batch_cursor.as_deref())
            .flatten();
        Ok(Self {
            job_id: &job.job_id,
            runner_id,
            stage,
            start_cursor,
        })
    }
}

async fn ingest_and_checkpoint_batch(
    ctx: &RebuildContext,
    run: Option<&BatchRun<'_>>,
    end_cursor: &str,
    mut docs: Vec<QuickwitDocument>,
) -> anyhow::Result<()> {
    let Some(run) = run else {
        return ctx.quickwit.ingest(&docs).await;
    };
    if end_cursor.is_empty() || end_cursor.len() > 500 {
        anyhow::bail!("invalid rebuild batch cursor");
    }
    let batch_id = deterministic_batch_id(run.job_id, run.stage, end_cursor);
    for doc in &mut docs {
        doc.rebuild_batch_id = Some(batch_id.clone());
    }
    let expected = docs.len();
    match observed_batch_action(ctx.quickwit.count_rebuild_batch(&batch_id).await?, expected)? {
        BatchObservation::Ingest => {
            ctx.quickwit.ingest_rebuild_batch(&docs).await?;
            let committed = ctx.quickwit.count_rebuild_batch(&batch_id).await?;
            if committed != expected as u64 {
                anyhow::bail!("rebuild batch commit could not be proven");
            }
        }
        BatchObservation::AlreadyComplete => {}
    }

    let updated = sqlx::query(
        "UPDATE quickwit_admin_jobs
         SET batch_stage=$3,batch_cursor=$4,updated_at=NOW()
         WHERE job_id=$1 AND status='running' AND lease_owner=$2
           AND lease_until>NOW() AND checkpoint<$3",
    )
    .bind(run.job_id)
    .bind(run.runner_id)
    .bind(run.stage as i16)
    .bind(end_cursor)
    .execute(&ctx.pool)
    .await?;
    if updated.rows_affected() != 1 {
        anyhow::bail!("rebuild batch lease changed before checkpoint");
    }
    Ok(())
}

#[async_trait::async_trait]
impl JobExecutor for RebuildContext {
    async fn execute_stage(&self, job: &AdminJob, stage: RebuildStage) -> Result<(), String> {
        if job.clear {
            return Err("clear is not executable without Quickwit task-completion proof".into());
        }
        let org_id = job.org_id.as_deref();
        if stage == RebuildStage::EmptyScopePreflight {
            let count = self
                .quickwit
                .count_documents(org_id)
                .await
                .map_err(|_| "rebuild preflight failed".to_string())?;
            return if count == 0 {
                Ok(())
            } else {
                Err("rebuild scope is not empty; destructive replacement remains disabled".into())
            };
        }
        let result = match stage {
            RebuildStage::EmptyScopePreflight => unreachable!("handled above"),
            RebuildStage::KnowledgeUnits => {
                let run = BatchRun::for_job(job, stage)?;
                rebuild_knowledge_units(self, org_id, Some(&run)).await
            }
            RebuildStage::WikiVersions => {
                let run = BatchRun::for_job(job, stage)?;
                rebuild_wiki_versions(self, org_id, Some(&run)).await
            }
            RebuildStage::SourceObjects => {
                let run = BatchRun::for_job(job, stage)?;
                rebuild_source_objects(self, org_id, Some(&run)).await
            }
            RebuildStage::RetrievalLogs => {
                let run = BatchRun::for_job(job, stage)?;
                rebuild_retrieval_logs(self, org_id, Some(&run)).await
            }
            RebuildStage::WikiSourceLogs => {
                let run = BatchRun::for_job(job, stage)?;
                rebuild_wiki_source_logs(self, org_id, Some(&run)).await
            }
        };
        result
            .map(|_| ())
            .map_err(|_| "rebuild stage failed".to_string())
    }
}

#[derive(Debug, Default, Serialize)]
pub struct RebuildStats {
    pub knowledge_units: usize,
    pub wiki_versions: usize,
    pub source_objects: usize,
    pub retrieval_logs: usize,
    pub wiki_source_logs: usize,
}

pub async fn rebuild_all(
    ctx: Arc<RebuildContext>,
    org_id: Option<String>,
    clear: bool,
) -> anyhow::Result<RebuildStats> {
    tracing::info!(org_id = ?org_id, clear, "Quickwit rebuild started");
    ctx.quickwit.ensure_index().await?;

    if clear {
        if let Some(org) = org_id.as_deref() {
            ctx.quickwit
                .delete_by_query(&format!("org_id:{}", quote_query_value(org)))
                .await?;
        } else {
            ctx.quickwit.clear_index().await?;
            ctx.quickwit.ensure_index().await?;
        }
    }

    // Field order preserves the original sequential await order.
    let stats = RebuildStats {
        knowledge_units: rebuild_knowledge_units(&ctx, org_id.as_deref(), None).await?,
        wiki_versions: rebuild_wiki_versions(&ctx, org_id.as_deref(), None).await?,
        source_objects: rebuild_source_objects(&ctx, org_id.as_deref(), None).await?,
        retrieval_logs: rebuild_retrieval_logs(&ctx, org_id.as_deref(), None).await?,
        wiki_source_logs: rebuild_wiki_source_logs(&ctx, org_id.as_deref(), None).await?,
    };

    tracing::info!(?stats, "Quickwit rebuild complete");
    Ok(stats)
}

pub async fn index_knowledge_unit_by_id(
    ctx: &RebuildContext,
    knowledge_id: &str,
) -> anyhow::Result<bool> {
    let row = sqlx::query_as::<_, KnowledgeRow>(
        r#"
        SELECT
            ku.knowledge_id,
            ku.document_id,
            ku.org_id,
            ku.chunk_index,
            ku.text,
            ku.content_hash,
            ku.metadata AS knowledge_metadata,
            ku.updated_at AS knowledge_updated_at,
            d.source,
            d.title,
            d.metadata AS document_metadata,
            d.updated_at AS document_updated_at
        FROM knowledge_units ku
        JOIN documents d ON d.document_id = ku.document_id
        WHERE ku.knowledge_id = $1
          AND ku.embedding_status = 'done'
          AND d.deleted_at IS NULL
          AND LOWER(BTRIM(d.zdr_classification)) IN ('internal', 'public', 'sensitive')
        "#,
    )
    .bind(knowledge_id)
    .fetch_optional(&ctx.pool)
    .await?;

    if let Some(row) = row {
        let doc = knowledge_row_to_document(row);
        ctx.quickwit.ingest(&[doc]).await?;
        return Ok(true);
    }

    Ok(false)
}

pub async fn index_document_knowledge_units(
    ctx: &RebuildContext,
    document_id: &str,
) -> anyhow::Result<usize> {
    let rows = sqlx::query_as::<_, KnowledgeRow>(
        r#"
        SELECT
            ku.knowledge_id,
            ku.document_id,
            ku.org_id,
            ku.chunk_index,
            ku.text,
            ku.content_hash,
            ku.metadata AS knowledge_metadata,
            ku.updated_at AS knowledge_updated_at,
            d.source,
            d.title,
            d.metadata AS document_metadata,
            d.updated_at AS document_updated_at
        FROM knowledge_units ku
        JOIN documents d ON d.document_id = ku.document_id
        WHERE ku.document_id = $1
          AND ku.embedding_status = 'done'
          AND d.deleted_at IS NULL
          AND LOWER(BTRIM(d.zdr_classification)) IN ('internal', 'public', 'sensitive')
        ORDER BY ku.chunk_index
        "#,
    )
    .bind(document_id)
    .fetch_all(&ctx.pool)
    .await?;

    let docs: Vec<_> = rows.into_iter().map(knowledge_row_to_document).collect();
    let count = docs.len();
    ctx.quickwit.ingest(&docs).await?;
    Ok(count)
}

pub async fn index_source_object_by_id(
    ctx: &RebuildContext,
    source_object_id: &str,
) -> anyhow::Result<bool> {
    let row = sqlx::query_as::<_, SourceObjectRow>(
        r#"
        SELECT
            source_object_id, org_id, connector, source, external_id, site_id,
            drive_id, item_id, path, name, mime_type, quickxor_hash, sha1_hash,
            content_hash, acl_tags, metadata, modified_at, updated_at
        FROM source_objects
        WHERE source_object_id = $1
          AND deleted_at IS NULL
        "#,
    )
    .bind(source_object_id)
    .fetch_optional(&ctx.pool)
    .await?;

    if let Some(row) = row {
        ctx.quickwit
            .ingest(&[source_object_row_to_document(row)])
            .await?;
        return Ok(true);
    }

    Ok(false)
}

pub async fn index_wiki_event(ctx: &RebuildContext, event: &Value) -> anyhow::Result<()> {
    let org_id = string_field(event, &["org_id"]).unwrap_or_default();
    if org_id.is_empty() {
        anyhow::bail!("wiki event missing org_id");
    }
    let page_id = string_field(event, &["page_id"]);
    let version_id = string_field(event, &["version_id"]);
    let timestamp = Utc::now();

    let doc = QuickwitDocument {
        timestamp,
        rebuild_batch_id: None,
        entity_type: "wiki_version".into(),
        org_id,
        document_id: page_id.clone(),
        knowledge_id: version_id,
        source_object_id: None,
        chunk_index: None,
        source: Some("wiki".into()),
        title: string_field(event, &["title"]),
        body: string_field(event, &["content"]).unwrap_or_default(),
        site_id: None,
        drive_id: None,
        item_id: None,
        path: string_field(event, &["path"]),
        mime_type: Some("text/markdown".into()),
        modified_at: Some(timestamp),
        quickxor_hash: None,
        sha1_hash: None,
        content_hash: None,
        acl_tags: acl_tags_field(event),
        metadata: event.clone(),
    };

    ctx.quickwit.ingest(&[doc]).await
}

async fn rebuild_knowledge_units(
    ctx: &RebuildContext,
    org_id: Option<&str>,
    run: Option<&BatchRun<'_>>,
) -> anyhow::Result<usize> {
    let mut cursor = run.and_then(|run| run.start_cursor).map(str::to_owned);
    let mut total = 0_usize;

    loop {
        let rows = sqlx::query_as::<_, KnowledgeRow>(
            r#"
            SELECT
                ku.knowledge_id,
                ku.document_id,
                ku.org_id,
                ku.chunk_index,
                ku.text,
                ku.content_hash,
                ku.metadata AS knowledge_metadata,
                ku.updated_at AS knowledge_updated_at,
                d.source,
                d.title,
                d.metadata AS document_metadata,
                d.updated_at AS document_updated_at
            FROM knowledge_units ku
            JOIN documents d ON d.document_id = ku.document_id
            WHERE ku.embedding_status = 'done'
              AND d.deleted_at IS NULL
              AND LOWER(BTRIM(d.zdr_classification)) IN ('internal', 'public', 'sensitive')
              AND ($1::TEXT IS NULL OR ku.org_id = $1)
              AND ($2::TEXT IS NULL OR ku.knowledge_id > $2)
            ORDER BY ku.knowledge_id
            LIMIT $3
            "#,
        )
        .bind(org_id)
        .bind(cursor.as_deref())
        .bind(ctx.batch_size)
        .fetch_all(&ctx.pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let end_cursor = rows.last().expect("non-empty batch").knowledge_id.clone();
        let docs: Vec<_> = rows.into_iter().map(knowledge_row_to_document).collect();
        total += docs.len();
        ingest_and_checkpoint_batch(ctx, run, &end_cursor, docs).await?;
        cursor = Some(end_cursor);
    }

    Ok(total)
}

async fn rebuild_wiki_versions(
    ctx: &RebuildContext,
    org_id: Option<&str>,
    run: Option<&BatchRun<'_>>,
) -> anyhow::Result<usize> {
    let mut cursor = run.and_then(|run| run.start_cursor).map(str::to_owned);
    let mut total = 0_usize;

    loop {
        let rows = sqlx::query_as::<_, WikiVersionRow>(
            r#"
            SELECT
                p.org_id,
                p.page_id,
                v.version_id,
                p.workspace_id,
                p.title,
                p.path,
                v.content,
                v.source_refs,
                v.metadata,
                COALESCE(v.published_at, v.created_at) AS version_timestamp
            FROM wiki_page_versions v
            JOIN wiki_pages p ON p.page_id = v.page_id
            WHERE p.page_status <> 'deleted'
              AND ($1::TEXT IS NULL OR p.org_id = $1)
              AND ($2::TEXT IS NULL OR v.version_id > $2)
            ORDER BY v.version_id
            LIMIT $3
            "#,
        )
        .bind(org_id)
        .bind(cursor.as_deref())
        .bind(ctx.batch_size)
        .fetch_all(&ctx.pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let end_cursor = rows.last().expect("non-empty batch").version_id.clone();
        let docs: Vec<_> = rows.into_iter().map(wiki_version_row_to_document).collect();
        total += docs.len();
        ingest_and_checkpoint_batch(ctx, run, &end_cursor, docs).await?;
        cursor = Some(end_cursor);
    }

    Ok(total)
}

async fn rebuild_source_objects(
    ctx: &RebuildContext,
    org_id: Option<&str>,
    run: Option<&BatchRun<'_>>,
) -> anyhow::Result<usize> {
    let mut cursor = run.and_then(|run| run.start_cursor).map(str::to_owned);
    let mut total = 0_usize;

    loop {
        let rows = sqlx::query_as::<_, SourceObjectRow>(
            r#"
            SELECT
                source_object_id, org_id, connector, source, external_id, site_id,
                drive_id, item_id, path, name, mime_type, quickxor_hash, sha1_hash,
                content_hash, acl_tags, metadata, modified_at, updated_at
            FROM source_objects
            WHERE deleted_at IS NULL
              AND ($1::TEXT IS NULL OR org_id = $1)
              AND ($2::TEXT IS NULL OR source_object_id > $2)
            ORDER BY source_object_id
            LIMIT $3
            "#,
        )
        .bind(org_id)
        .bind(cursor.as_deref())
        .bind(ctx.batch_size)
        .fetch_all(&ctx.pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let end_cursor = rows
            .last()
            .expect("non-empty batch")
            .source_object_id
            .clone();
        let docs: Vec<_> = rows
            .into_iter()
            .map(source_object_row_to_document)
            .collect();
        total += docs.len();
        ingest_and_checkpoint_batch(ctx, run, &end_cursor, docs).await?;
        cursor = Some(end_cursor);
    }

    Ok(total)
}

async fn rebuild_retrieval_logs(
    ctx: &RebuildContext,
    org_id: Option<&str>,
    run: Option<&BatchRun<'_>>,
) -> anyhow::Result<usize> {
    let mut cursor = run.and_then(|run| run.start_cursor).map(str::to_owned);
    let mut total = 0_usize;

    loop {
        let rows = sqlx::query_as::<_, RetrievalRunRow>(
            r#"
            SELECT
                trace_id, org_id, query, filters_json, mode_mix_applied, zdr_mode, created_at
            FROM retrieval_runs
            WHERE ($1::TEXT IS NULL OR org_id = $1)
              AND COALESCE(NULLIF(LOWER(BTRIM(zdr_mode)), ''), 'off')
                  IN ('off', 'disabled', 'reject')
              AND ($2::TEXT IS NULL OR trace_id > $2)
            ORDER BY trace_id
            LIMIT $3
            "#,
        )
        .bind(org_id)
        .bind(cursor.as_deref())
        .bind(ctx.batch_size)
        .fetch_all(&ctx.pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let end_cursor = rows.last().expect("non-empty batch").trace_id.clone();
        let docs: Vec<_> = rows
            .into_iter()
            .filter(|row| retrieval_log_is_durable(row.zdr_mode.as_deref()))
            .map(retrieval_run_row_to_document)
            .collect();
        total += docs.len();
        ingest_and_checkpoint_batch(ctx, run, &end_cursor, docs).await?;
        cursor = Some(end_cursor);
    }

    Ok(total)
}

fn retrieval_log_is_durable(zdr_mode: Option<&str>) -> bool {
    matches!(
        zdr_mode
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        None | Some("") | Some("off") | Some("disabled") | Some("reject")
    )
}

#[cfg(test)]
mod zdr_rebuild_tests {
    use std::sync::{Arc, Mutex};

    use axum::{extract::State, http::StatusCode, routing::post, Router};
    use sqlx::postgres::PgPoolOptions;

    use super::{
        deterministic_batch_id, index_document_knowledge_units, index_knowledge_unit_by_id,
        observed_batch_action, rebuild_knowledge_units, retrieval_log_is_durable, BatchObservation,
        RebuildContext,
    };

    #[test]
    fn retrieval_rebuild_fails_closed_for_ephemeral_and_unknown_postures() {
        for mode in [Some("ephemeral"), Some("on"), Some("future-mode")] {
            assert!(!retrieval_log_is_durable(mode));
        }
        for mode in [
            None,
            Some(""),
            Some("off"),
            Some("disabled"),
            Some("reject"),
        ] {
            assert!(retrieval_log_is_durable(mode));
        }
    }

    #[test]
    fn rebuild_batch_identity_is_deterministic_and_bound_to_job_stage_and_cursor() {
        let first = deterministic_batch_id("job-a", super::RebuildStage::KnowledgeUnits, "kid-9");
        assert_eq!(
            first,
            deterministic_batch_id("job-a", super::RebuildStage::KnowledgeUnits, "kid-9")
        );
        assert_ne!(
            first,
            deterministic_batch_id("job-a", super::RebuildStage::KnowledgeUnits, "kid-10")
        );
        assert_ne!(
            first,
            deterministic_batch_id("job-a", super::RebuildStage::WikiVersions, "kid-9")
        );
    }

    #[test]
    fn rebuild_batch_retry_skips_complete_batch_and_fails_closed_on_partial_batch() {
        assert_eq!(
            observed_batch_action(0, 3).unwrap(),
            BatchObservation::Ingest
        );
        assert_eq!(
            observed_batch_action(3, 3).unwrap(),
            BatchObservation::AlreadyComplete
        );
        assert!(observed_batch_action(1, 3).is_err());
        assert!(observed_batch_action(4, 3).is_err());
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn every_knowledge_ingest_source_excludes_restricted_deleted_and_unknown_documents() {
        async fn capture(
            State(bodies): State<Arc<Mutex<Vec<String>>>>,
            body: String,
        ) -> StatusCode {
            bodies.lock().expect("capture bodies").push(body);
            StatusCode::OK
        }

        let database_url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to disposable PostgreSQL");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("disposable postgres");
        sqlx::raw_sql(
            r#"
            CREATE TABLE documents (
              document_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, source TEXT NOT NULL,
              title TEXT NOT NULL, metadata JSONB NOT NULL DEFAULT '{}',
              updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), deleted_at TIMESTAMPTZ,
              zdr_classification TEXT NOT NULL
            );
            CREATE TABLE knowledge_units (
              knowledge_id TEXT PRIMARY KEY, document_id TEXT NOT NULL, org_id TEXT NOT NULL,
              chunk_index INTEGER NOT NULL, text TEXT NOT NULL, content_hash TEXT,
              metadata JSONB NOT NULL DEFAULT '{}', updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              embedding_status TEXT NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("minimal disposable schema");

        for (suffix, classification, deleted) in [
            ("allowed", "internal", false),
            ("restricted", "restricted", false),
            ("unknown", "future-policy", false),
            ("deleted", "internal", true),
        ] {
            let doc_id = format!("fixture-doc-{suffix}");
            sqlx::query(
                "INSERT INTO documents
                 (document_id,org_id,source,title,deleted_at,zdr_classification)
                 VALUES ($1,'fixture-org','fixture','fixture title',
                    CASE WHEN $2 THEN NOW() ELSE NULL END,$3)",
            )
            .bind(&doc_id)
            .bind(deleted)
            .bind(classification)
            .execute(&pool)
            .await
            .expect("document fixture");
            sqlx::query(
                "INSERT INTO knowledge_units
                 (knowledge_id,document_id,org_id,chunk_index,text,embedding_status)
                 VALUES ($1,$2,'fixture-org',0,$3,'done')",
            )
            .bind(format!("fixture-kid-{suffix}"))
            .bind(doc_id)
            .bind(format!("synthetic-{suffix}-body"))
            .execute(&pool)
            .await
            .expect("knowledge fixture");
        }

        let bodies = Arc::new(Mutex::new(Vec::<String>::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("local listener");
        let addr = listener.local_addr().expect("listener address");
        let app = Router::new()
            .route("/api/v1/{index}/ingest", post(capture))
            .with_state(bodies.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.expect("mock Quickwit") });
        let config_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../infra/quickwit/dataplane-corpus-index.yaml"
        );
        let ctx = RebuildContext {
            pool: pool.clone(),
            quickwit: crate::quickwit::QuickwitClient::new(
                format!("http://{addr}"),
                "fixture-index",
                config_path,
            )
            .expect("Quickwit client"),
            batch_size: 10,
        };

        for suffix in ["restricted", "unknown", "deleted"] {
            assert!(
                !index_knowledge_unit_by_id(&ctx, &format!("fixture-kid-{suffix}"))
                    .await
                    .expect("filtered by-id lookup")
            );
            assert_eq!(
                index_document_knowledge_units(&ctx, &format!("fixture-doc-{suffix}"))
                    .await
                    .expect("filtered document lookup"),
                0
            );
        }
        assert!(bodies.lock().expect("captured bodies").is_empty());

        rebuild_knowledge_units(&ctx, Some("fixture-org"), None)
            .await
            .expect("filtered rebuild");
        let captured = bodies.lock().expect("captured bodies").join("\n");
        assert!(captured.contains("fixture-kid-allowed"));
        for suffix in ["restricted", "unknown", "deleted"] {
            assert!(!captured.contains(&format!("fixture-kid-{suffix}")));
            assert!(!captured.contains(&format!("synthetic-{suffix}-body")));
        }
    }
}

async fn rebuild_wiki_source_logs(
    ctx: &RebuildContext,
    org_id: Option<&str>,
    run: Option<&BatchRun<'_>>,
) -> anyhow::Result<usize> {
    let mut cursor = run.and_then(|run| run.start_cursor).map(str::to_owned);
    let mut total = 0_usize;

    loop {
        let rows = sqlx::query_as::<_, WikiSourceLogRow>(
            r#"
            SELECT
                p.org_id,
                p.page_id,
                p.title,
                p.path,
                l.log_id,
                l.original_chunks,
                l.processing_model,
                l.metadata,
                l.created_at
            FROM wiki_source_logs l
            JOIN wiki_pages p ON p.page_id = l.page_id
            WHERE ($1::TEXT IS NULL OR p.org_id = $1)
              AND ($2::TEXT IS NULL OR l.log_id > $2)
            ORDER BY l.log_id
            LIMIT $3
            "#,
        )
        .bind(org_id)
        .bind(cursor.as_deref())
        .bind(ctx.batch_size)
        .fetch_all(&ctx.pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let end_cursor = rows.last().expect("non-empty batch").log_id.clone();
        let docs: Vec<_> = rows
            .into_iter()
            .map(wiki_source_log_row_to_document)
            .collect();
        total += docs.len();
        ingest_and_checkpoint_batch(ctx, run, &end_cursor, docs).await?;
        cursor = Some(end_cursor);
    }

    Ok(total)
}

#[derive(sqlx::FromRow)]
struct KnowledgeRow {
    knowledge_id: String,
    document_id: String,
    org_id: String,
    chunk_index: i32,
    text: String,
    content_hash: Option<String>,
    knowledge_metadata: Value,
    knowledge_updated_at: DateTime<Utc>,
    source: String,
    title: String,
    document_metadata: Value,
    document_updated_at: DateTime<Utc>,
}

fn knowledge_row_to_document(row: KnowledgeRow) -> QuickwitDocument {
    let metadata = json!({
        "knowledge": row.knowledge_metadata,
        "document": row.document_metadata,
    });
    let lookup = metadata.get("document").unwrap_or(&metadata);
    let fallback = metadata.get("knowledge").unwrap_or(&metadata);

    QuickwitDocument {
        timestamp: row.knowledge_updated_at,
        rebuild_batch_id: None,
        entity_type: "knowledge_unit".into(),
        org_id: row.org_id,
        document_id: Some(row.document_id),
        knowledge_id: Some(row.knowledge_id),
        source_object_id: None,
        chunk_index: Some(row.chunk_index.into()),
        source: Some(row.source),
        title: Some(row.title),
        body: row.text,
        site_id: string_field(lookup, &["site_id"])
            .or_else(|| string_field(fallback, &["site_id"])),
        drive_id: string_field(lookup, &["drive_id"])
            .or_else(|| string_field(fallback, &["drive_id"])),
        item_id: string_field(lookup, &["item_id"])
            .or_else(|| string_field(fallback, &["item_id"])),
        path: string_field(lookup, &["path", "url"])
            .or_else(|| string_field(fallback, &["path", "url"])),
        mime_type: string_field(lookup, &["mime_type", "content_type"])
            .or_else(|| string_field(fallback, &["mime_type", "content_type"])),
        modified_at: datetime_field(lookup, &["modified_at", "last_modified_at"])
            .or(Some(row.document_updated_at)),
        quickxor_hash: string_field(lookup, &["quickxor_hash"])
            .or_else(|| string_field(fallback, &["quickxor_hash"])),
        sha1_hash: string_field(lookup, &["sha1_hash"])
            .or_else(|| string_field(fallback, &["sha1_hash"])),
        content_hash: row.content_hash,
        acl_tags: {
            let mut tags = acl_tags_field(lookup);
            tags.extend(acl_tags_field(fallback));
            tags.sort();
            tags.dedup();
            tags
        },
        metadata,
    }
}

#[derive(sqlx::FromRow)]
struct WikiVersionRow {
    org_id: String,
    page_id: String,
    version_id: String,
    workspace_id: String,
    title: String,
    path: String,
    content: Option<String>,
    source_refs: Option<Value>,
    metadata: Option<Value>,
    version_timestamp: DateTime<Utc>,
}

fn wiki_version_row_to_document(row: WikiVersionRow) -> QuickwitDocument {
    let metadata = json!({
        "workspace_id": row.workspace_id,
        "source_refs": row.source_refs.unwrap_or(Value::Null),
        "version_metadata": row.metadata.unwrap_or_else(|| json!({})),
    });
    QuickwitDocument {
        timestamp: row.version_timestamp,
        rebuild_batch_id: None,
        entity_type: "wiki_version".into(),
        org_id: row.org_id,
        document_id: Some(row.page_id),
        knowledge_id: Some(row.version_id),
        source_object_id: None,
        chunk_index: None,
        source: Some("wiki".into()),
        title: Some(row.title),
        body: row.content.unwrap_or_default(),
        site_id: None,
        drive_id: None,
        item_id: None,
        path: Some(row.path),
        mime_type: Some("text/markdown".into()),
        modified_at: Some(row.version_timestamp),
        quickxor_hash: None,
        sha1_hash: None,
        content_hash: None,
        acl_tags: acl_tags_field(&metadata),
        metadata,
    }
}

#[derive(sqlx::FromRow)]
struct SourceObjectRow {
    source_object_id: String,
    org_id: String,
    connector: String,
    source: String,
    external_id: String,
    site_id: Option<String>,
    drive_id: Option<String>,
    item_id: Option<String>,
    path: Option<String>,
    name: String,
    mime_type: Option<String>,
    quickxor_hash: Option<String>,
    sha1_hash: Option<String>,
    content_hash: Option<String>,
    acl_tags: Vec<String>,
    metadata: Value,
    modified_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

fn source_object_row_to_document(row: SourceObjectRow) -> QuickwitDocument {
    let metadata = json!({
        "connector": row.connector,
        "external_id": row.external_id,
        "source_object": row.metadata,
    });
    QuickwitDocument {
        timestamp: row.updated_at,
        rebuild_batch_id: None,
        entity_type: "source_object".into(),
        org_id: row.org_id,
        document_id: Some(row.source_object_id.clone()),
        knowledge_id: Some(row.source_object_id.clone()),
        source_object_id: Some(row.source_object_id),
        chunk_index: None,
        source: Some(row.source),
        title: Some(row.name.clone()),
        body: [Some(row.name), row.path.clone()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n"),
        site_id: row.site_id,
        drive_id: row.drive_id,
        item_id: row.item_id,
        path: row.path,
        mime_type: row.mime_type,
        modified_at: row.modified_at.or(Some(row.updated_at)),
        quickxor_hash: row.quickxor_hash,
        sha1_hash: row.sha1_hash,
        content_hash: row.content_hash,
        acl_tags: row.acl_tags,
        metadata,
    }
}

#[derive(sqlx::FromRow)]
struct RetrievalRunRow {
    trace_id: String,
    org_id: String,
    query: String,
    filters_json: Option<Value>,
    mode_mix_applied: Option<Value>,
    zdr_mode: Option<String>,
    created_at: DateTime<Utc>,
}

fn retrieval_run_row_to_document(row: RetrievalRunRow) -> QuickwitDocument {
    let metadata = json!({
        "trace_id": row.trace_id,
        "filters_json": row.filters_json,
        "mode_mix_applied": row.mode_mix_applied,
        "zdr_mode": row.zdr_mode,
    });
    QuickwitDocument {
        timestamp: row.created_at,
        rebuild_batch_id: None,
        entity_type: "retrieval_log".into(),
        org_id: row.org_id,
        document_id: Some(row.trace_id.clone()),
        knowledge_id: Some(row.trace_id),
        source_object_id: None,
        chunk_index: None,
        source: Some("retrieval".into()),
        title: Some("Retrieval query".into()),
        body: row.query,
        site_id: None,
        drive_id: None,
        item_id: None,
        path: None,
        mime_type: Some("application/json".into()),
        modified_at: Some(row.created_at),
        quickxor_hash: None,
        sha1_hash: None,
        content_hash: None,
        acl_tags: Vec::new(),
        metadata,
    }
}

#[derive(sqlx::FromRow)]
struct WikiSourceLogRow {
    org_id: String,
    page_id: String,
    title: String,
    path: String,
    log_id: String,
    original_chunks: Option<Value>,
    processing_model: Option<String>,
    metadata: Option<Value>,
    created_at: DateTime<Utc>,
}

fn wiki_source_log_row_to_document(row: WikiSourceLogRow) -> QuickwitDocument {
    let metadata = json!({
        "log_id": row.log_id,
        "original_chunks": row.original_chunks,
        "processing_model": row.processing_model,
        "source_log_metadata": row.metadata,
    });
    QuickwitDocument {
        timestamp: row.created_at,
        rebuild_batch_id: None,
        entity_type: "wiki_source_log".into(),
        org_id: row.org_id,
        document_id: Some(row.page_id.clone()),
        knowledge_id: Some(row.log_id),
        source_object_id: None,
        chunk_index: None,
        source: Some("wiki_source_log".into()),
        title: Some(row.title),
        body: metadata.to_string(),
        site_id: None,
        drive_id: None,
        item_id: None,
        path: Some(row.path),
        mime_type: Some("application/json".into()),
        modified_at: Some(row.created_at),
        quickxor_hash: None,
        sha1_hash: None,
        content_hash: None,
        acl_tags: Vec::new(),
        metadata,
    }
}
