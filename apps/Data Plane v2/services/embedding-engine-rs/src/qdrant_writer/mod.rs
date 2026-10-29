use std::collections::HashMap;

use qdrant_client::qdrant::{
    value::Kind as QdrantKind, Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance,
    Filter, PointStruct, QuantizationType, ScalarQuantization, UpsertPointsBuilder,
    Value as QdrantValue, VectorParamsBuilder,
};
use qdrant_client::Qdrant;

/// int8 scalar quantization for new collections (plan P2-2, closes D13).
///
/// # Why, and why now
///
/// A 3072-dim float32 vector is ~12 KB; int8 scalar quantization stores a 1-byte
/// proxy per dimension, so the searchable copy is ~4x smaller and fits in RAM at
/// corpus sizes where the raw vectors would not.
///
/// The reason this lands *now*, while the corpus is small, is that
/// `quantization_config` is fixed at collection creation: switching it later
/// means recreating the collection and re-embedding everything. Doing it at 221
/// points costs nothing; doing it at a million is a migration.
///
/// `quantile: 0.99` clips the extreme 1% of the value distribution before
/// choosing the int8 scale, so a handful of outlier dimensions cannot compress
/// the range that every other value has to share.
///
/// `always_ram: true` keeps the quantized vectors resident even when the raw
/// vectors spill to disk — that is the entire point of quantizing, and without it
/// the fast path can still fault to disk.
///
/// Accuracy: quantization is lossy, so this is only sound because Qdrant
/// rescores candidates against the raw vectors. Rescoring is on by default for
/// quantized collections; the raw vectors are retained, not replaced. If a
/// future change disables rescore or oversampling, recall becomes approximate —
/// re-measure against the golden set (P0.5) before doing that.
///
/// NOTE: applies to collections created from here on. The four collections that
/// already exist keep `quantization_config: None` until they are recreated,
/// which is a re-embed and therefore rides with the reindex.
fn int8_quantization() -> ScalarQuantization {
    ScalarQuantization {
        r#type: QuantizationType::Int8 as i32,
        quantile: Some(0.99),
        always_ram: Some(true),
    }
}

pub async fn ensure_collection(qdrant: &Qdrant, collection: &str, dim: u64) -> anyhow::Result<()> {
    let exists = qdrant.collection_exists(collection).await?;
    if !exists {
        qdrant
            .create_collection(
                CreateCollectionBuilder::new(collection)
                    .vectors_config(VectorParamsBuilder::new(dim, Distance::Cosine))
                    .quantization_config(int8_quantization()),
            )
            .await?;
        tracing::info!(
            collection,
            dim,
            quantization = "scalar-int8(quantile=0.99,always_ram)",
            "qdrant collection created"
        );
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
