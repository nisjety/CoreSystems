use std::collections::HashMap;

use qdrant_client::qdrant::{
    value::Kind as QdrantKind, Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance,
    Filter, PointStruct, UpsertPointsBuilder, Value as QdrantValue, VectorParamsBuilder,
};
use qdrant_client::Qdrant;

pub async fn ensure_collection(qdrant: &Qdrant, collection: &str, dim: u64) -> anyhow::Result<()> {
    let exists = qdrant.collection_exists(collection).await?;
    if !exists {
        qdrant
            .create_collection(
                CreateCollectionBuilder::new(collection)
                    .vectors_config(VectorParamsBuilder::new(dim, Distance::Cosine)),
            )
            .await?;
        tracing::info!(collection, "qdrant collection created");
    }
    Ok(())
}

pub struct EmbeddingPoint {
    pub knowledge_id: String,
    pub document_id: String,
    pub org_id: String,
    pub chunk_index: i32,
    pub text: String,
    pub vector: Vec<f32>,
    pub metadata: HashMap<String, String>,
}

pub async fn upsert_vectors(
    qdrant: &Qdrant,
    collection: &str,
    points: Vec<EmbeddingPoint>,
) -> anyhow::Result<()> {
    if points.is_empty() {
        return Ok(());
    }

    let qdrant_points: Vec<PointStruct> = points
        .into_iter()
        .map(|p| {
            let mut payload: HashMap<String, QdrantValue> = HashMap::new();
            payload.insert(
                "knowledge_id".into(),
                QdrantValue {
                    kind: Some(QdrantKind::StringValue(p.knowledge_id.clone())),
                },
            );
            payload.insert(
                "document_id".into(),
                QdrantValue {
                    kind: Some(QdrantKind::StringValue(p.document_id.clone())),
                },
            );
            payload.insert(
                "org_id".into(),
                QdrantValue {
                    kind: Some(QdrantKind::StringValue(p.org_id.clone())),
                },
            );
            payload.insert(
                "chunk_index".into(),
                QdrantValue {
                    kind: Some(QdrantKind::IntegerValue(p.chunk_index as i64)),
                },
            );
            payload.insert(
                "text".into(),
                QdrantValue {
                    kind: Some(QdrantKind::StringValue(p.text)),
                },
            );
            for (k, v) in p.metadata {
                payload.insert(
                    k,
                    QdrantValue {
                        kind: Some(QdrantKind::StringValue(v)),
                    },
                );
            }

            PointStruct::new(p.knowledge_id, p.vector, payload)
        })
        .collect();

    qdrant
        .upsert_points(UpsertPointsBuilder::new(collection, qdrant_points).wait(true))
        .await?;

    Ok(())
}

/// Deletes specific points by knowledge_id. Used to purge chunks orphaned by a
/// content re-chunk (the new chunks have different IDs and are upserted
/// separately, so these IDs are disjoint and safe to delete at any time).
pub async fn delete_vectors_by_ids(
    qdrant: &Qdrant,
    collection: &str,
    org_id: &str,
    document_id: &str,
    knowledge_ids: &[String],
) -> anyhow::Result<()> {
    let filter = tenant_document_delete_filter(org_id, document_id, knowledge_ids)?;

    qdrant
        .delete_points(
            DeletePointsBuilder::new(collection)
                .points(filter)
                .wait(true),
        )
        .await?;

    tracing::info!(count = knowledge_ids.len(), "qdrant vectors deleted by id");
    Ok(())
}

/// Legacy visual-vector cleanup. The page-image contract currently carries
/// only a document id; the signed text deletion path below uses the stricter
/// tenant/document/chunk filter and must not call this helper.
pub async fn delete_vectors_by_document(
    qdrant: &Qdrant,
    collection: &str,
    document_id: &str,
) -> anyhow::Result<()> {
    if document_id.trim().is_empty() {
        anyhow::bail!("document id required for vector deletion");
    }
    let filter = Filter::must([Condition::matches("document_id", document_id.to_owned())]);
    qdrant
        .delete_points(
            DeletePointsBuilder::new(collection)
                .points(filter)
                .wait(true),
        )
        .await?;
    Ok(())
}

fn tenant_document_delete_filter(
    org_id: &str,
    document_id: &str,
    knowledge_ids: &[String],
) -> anyhow::Result<Filter> {
    if org_id.trim().is_empty()
        || document_id.trim().is_empty()
        || knowledge_ids.is_empty()
        || knowledge_ids.iter().any(|id| id.trim().is_empty())
    {
        anyhow::bail!("tenant-bound vector deletion requires org, document, and chunk ids");
    }
    Ok(Filter::must([
        Condition::matches("org_id", org_id.to_owned()),
        Condition::matches("document_id", document_id.to_owned()),
        Condition::matches("knowledge_id", knowledge_ids.to_vec()),
    ]))
}

#[cfg(test)]
mod deletion_scope_tests {
    use super::tenant_document_delete_filter;

    #[test]
    fn deletion_filter_requires_tenant_document_and_exact_chunk_ids() {
        let filter = tenant_document_delete_filter(
            "org-a",
            "doc-a",
            &["kid-a".to_owned(), "kid-b".to_owned()],
        )
        .expect("bounded filter");
        assert_eq!(filter.must.len(), 3);
        assert!(tenant_document_delete_filter("", "doc-a", &["kid-a".to_owned()]).is_err());
        assert!(tenant_document_delete_filter("org-a", "doc-a", &[]).is_err());
    }
}
