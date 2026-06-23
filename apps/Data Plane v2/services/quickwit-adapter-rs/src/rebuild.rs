use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::model::{acl_tags_field, datetime_field, string_field, QuickwitDocument};
use crate::quickwit::{quote_query_value, QuickwitClient};

#[derive(Clone)]
pub struct RebuildContext {
    pub pool: PgPool,
    pub quickwit: QuickwitClient,
    pub batch_size: i64,
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
        knowledge_units: rebuild_knowledge_units(&ctx, org_id.as_deref()).await?,
        wiki_versions: rebuild_wiki_versions(&ctx, org_id.as_deref()).await?,
        source_objects: rebuild_source_objects(&ctx, org_id.as_deref()).await?,
        retrieval_logs: rebuild_retrieval_logs(&ctx, org_id.as_deref()).await?,
        wiki_source_logs: rebuild_wiki_source_logs(&ctx, org_id.as_deref()).await?,
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
) -> anyhow::Result<usize> {
    let mut offset = 0_i64;
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
              AND ($1::TEXT IS NULL OR ku.org_id = $1)
            ORDER BY ku.knowledge_id
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(org_id)
        .bind(ctx.batch_size)
        .bind(offset)
        .fetch_all(&ctx.pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let docs: Vec<_> = rows.into_iter().map(knowledge_row_to_document).collect();
        total += docs.len();
        ctx.quickwit.ingest(&docs).await?;
        offset += ctx.batch_size;
    }

    Ok(total)
}

async fn rebuild_wiki_versions(
    ctx: &RebuildContext,
    org_id: Option<&str>,
) -> anyhow::Result<usize> {
    let mut offset = 0_i64;
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
            ORDER BY v.version_id
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(org_id)
        .bind(ctx.batch_size)
        .bind(offset)
        .fetch_all(&ctx.pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let docs: Vec<_> = rows.into_iter().map(wiki_version_row_to_document).collect();
        total += docs.len();
        ctx.quickwit.ingest(&docs).await?;
        offset += ctx.batch_size;
    }

    Ok(total)
}

async fn rebuild_source_objects(
    ctx: &RebuildContext,
    org_id: Option<&str>,
) -> anyhow::Result<usize> {
    let mut offset = 0_i64;
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
            ORDER BY source_object_id
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(org_id)
        .bind(ctx.batch_size)
        .bind(offset)
        .fetch_all(&ctx.pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let docs: Vec<_> = rows
            .into_iter()
            .map(source_object_row_to_document)
            .collect();
        total += docs.len();
        ctx.quickwit.ingest(&docs).await?;
        offset += ctx.batch_size;
    }

    Ok(total)
}

async fn rebuild_retrieval_logs(
    ctx: &RebuildContext,
    org_id: Option<&str>,
) -> anyhow::Result<usize> {
    let mut offset = 0_i64;
    let mut total = 0_usize;

    loop {
        let rows = sqlx::query_as::<_, RetrievalRunRow>(
            r#"
            SELECT
                trace_id, org_id, query, filters_json, mode_mix_applied, zdr_mode, created_at
            FROM retrieval_runs
            WHERE ($1::TEXT IS NULL OR org_id = $1)
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(org_id)
        .bind(ctx.batch_size)
        .bind(offset)
        .fetch_all(&ctx.pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let docs: Vec<_> = rows
            .into_iter()
            .map(retrieval_run_row_to_document)
            .collect();
        total += docs.len();
        ctx.quickwit.ingest(&docs).await?;
        offset += ctx.batch_size;
    }

    Ok(total)
}

async fn rebuild_wiki_source_logs(
    ctx: &RebuildContext,
    org_id: Option<&str>,
) -> anyhow::Result<usize> {
    let mut offset = 0_i64;
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
            ORDER BY l.created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(org_id)
        .bind(ctx.batch_size)
        .bind(offset)
        .fetch_all(&ctx.pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let docs: Vec<_> = rows
            .into_iter()
            .map(wiki_source_log_row_to_document)
            .collect();
        total += docs.len();
        ctx.quickwit.ingest(&docs).await?;
        offset += ctx.batch_size;
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
