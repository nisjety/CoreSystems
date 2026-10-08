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
    /// `documents.source` of the document this chunk belongs to, written as the payload key
    /// `source` so the dense arm can scope a search to a corpus (see `source_stamp`). `None`
    /// writes no key: a point whose document could not be found is unstamped, which a
    /// scoped search treats as not in the corpus, and the backfill tool stamps it once its
    /// document can be found. Never a blank string.
    pub source: Option<String>,
    pub metadata: HashMap<String, String>,
}

/// The payload keys the writer sets from the point's own fields. `metadata` is for
/// everything else: an entry under one of these names would replace a fact about the point
/// (its tenant, its document, its corpus) with whatever the caller put there.
const OWNED_KEYS: [&str; 6] = [
    "knowledge_id",
    "document_id",
    "org_id",
    "chunk_index",
    "text",
    crate::source_stamp::PAYLOAD_KEY,
];

/// The points exactly as Qdrant receives them. A function of its own so a test can
/// read the payload that would be written without a Qdrant to write it to; the
/// payload is the contract the retrieval engine reads (the batch pipeline's tests
/// pin its keys).
pub fn point_structs(points: Vec<EmbeddingPoint>) -> Vec<PointStruct> {
    points
        .into_iter()
        .map(|p| {
            let mut payload: HashMap<String, QdrantValue> = HashMap::new();
            // Free-form metadata first, so the keys the writer owns are set after it and
            // always win.
            for (k, v) in p.metadata {
                if OWNED_KEYS.contains(&k.as_str()) {
                    tracing::warn!(
                        key = %k,
                        "payload metadata names a key the writer owns; the point's own value is kept"
                    );
                    continue;
                }
                payload.insert(
                    k,
                    QdrantValue {
                        kind: Some(QdrantKind::StringValue(v)),
                    },
                );
            }
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
            if let Some(source) = p.source.as_deref().and_then(crate::source_stamp::stampable) {
                payload.insert(
                    crate::source_stamp::PAYLOAD_KEY.into(),
                    QdrantValue {
                        kind: Some(QdrantKind::StringValue(source.to_owned())),
                    },
                );
            }

            PointStruct::new(p.knowledge_id, p.vector, payload)
        })
        .collect()
}

pub async fn upsert_vectors(
    qdrant: &Qdrant,
    collection: &str,
    points: Vec<EmbeddingPoint>,
) -> anyhow::Result<()> {
    if points.is_empty() {
        return Ok(());
    }

    let qdrant_points = point_structs(points);

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

#[cfg(test)]
mod payload_tests {
    use std::collections::{BTreeSet, HashMap};

    use qdrant_client::qdrant::{value::Kind as QdrantKind, PointStruct};

    use super::{point_structs, EmbeddingPoint};

    fn point() -> EmbeddingPoint {
        EmbeddingPoint {
            knowledge_id: "kid-1".to_owned(),
            document_id: "doc-1".to_owned(),
            org_id: "org-1".to_owned(),
            chunk_index: 3,
            text: "Driftsmarginen økte til 31 prosent.".to_owned(),
            vector: vec![0.25, 0.5],
            source: None,
            metadata: HashMap::new(),
        }
    }

    fn keys_of(point: &PointStruct) -> BTreeSet<String> {
        point.payload.keys().cloned().collect()
    }

    fn keys(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn string_of(point: &PointStruct, key: &str) -> Option<String> {
        match point.payload.get(key)?.kind.as_ref()? {
            QdrantKind::StringValue(value) => Some(value.clone()),
            _ => None,
        }
    }

    /// The tenant, document and chunk keys the retrieval engine's filters and its
    /// ownership gate read are facts about the point, set by the writer. Nothing a
    /// caller puts in `metadata` may replace them: a `org_id` smuggled in there would
    /// move the point to another tenant.
    #[test]
    fn metadata_cannot_replace_the_keys_the_writer_owns() {
        let mut unit = point();
        unit.metadata = HashMap::from([
            ("org_id".to_owned(), "org-evil".to_owned()),
            ("document_id".to_owned(), "doc-evil".to_owned()),
            ("knowledge_id".to_owned(), "kid-evil".to_owned()),
            ("text".to_owned(), "evil text".to_owned()),
            (
                "document_date".to_owned(),
                "2026-03-14T00:00:00+00:00".to_owned(),
            ),
        ]);

        let converted = point_structs(vec![unit]);
        let payload = &converted[0];

        assert_eq!(string_of(payload, "org_id").as_deref(), Some("org-1"));
        assert_eq!(string_of(payload, "document_id").as_deref(), Some("doc-1"));
        assert_eq!(string_of(payload, "knowledge_id").as_deref(), Some("kid-1"));
        assert_eq!(
            string_of(payload, "text").as_deref(),
            Some("Driftsmarginen økte til 31 prosent.")
        );
        // A key that is not the writer's own still passes through.
        assert_eq!(
            string_of(payload, "document_date").as_deref(),
            Some("2026-03-14T00:00:00+00:00")
        );
    }

    /// The corpus scope reads this key: a new point says which source its document is from,
    /// as a plain string, beside the keys it always had.
    #[test]
    fn a_new_point_carries_the_documents_source() {
        let mut unit = point();
        unit.source = Some("quarry".to_owned());

        let converted = point_structs(vec![unit]);
        let payload = &converted[0];

        assert_eq!(string_of(payload, "source").as_deref(), Some("quarry"));
        assert_eq!(string_of(payload, "org_id").as_deref(), Some("org-1"));
        assert_eq!(string_of(payload, "document_id").as_deref(), Some("doc-1"));
        assert_eq!(string_of(payload, "knowledge_id").as_deref(), Some("kid-1"));
        assert_eq!(
            keys_of(payload),
            keys(&[
                "knowledge_id",
                "document_id",
                "org_id",
                "chunk_index",
                "text",
                "source"
            ]),
            "the stamp is the only key added to the payload the retrieval engine already reads"
        );
    }

    /// A point whose document could not be found, or whose source is blank, writes no key
    /// at all: an empty `source` would look stamped to the backfill and match no filter.
    #[test]
    fn a_point_with_no_usable_source_carries_no_source_key() {
        for source in [None, Some(String::new()), Some("  ".to_owned())] {
            let mut unit = point();
            unit.source = source.clone();
            let converted = point_structs(vec![unit]);
            assert_eq!(
                keys_of(&converted[0]),
                keys(&[
                    "knowledge_id",
                    "document_id",
                    "org_id",
                    "chunk_index",
                    "text"
                ]),
                "source {source:?}"
            );
        }
    }

    /// The stamp is the document's, not whatever a caller supplies as free-form metadata.
    #[test]
    fn metadata_cannot_forge_the_source() {
        let mut unit = point();
        unit.source = Some("quarry".to_owned());
        unit.metadata = HashMap::from([("source".to_owned(), "sharepoint".to_owned())]);
        let converted = point_structs(vec![unit]);
        assert_eq!(
            string_of(&converted[0], "source").as_deref(),
            Some("quarry")
        );

        // And with no stamp of its own, metadata does not supply one.
        let mut unit = point();
        unit.metadata = HashMap::from([("source".to_owned(), "quarry".to_owned())]);
        let converted = point_structs(vec![unit]);
        assert_eq!(string_of(&converted[0], "source"), None);
    }

    /// The source is copied as the document has it: the filter on the other side and the SQL
    /// predicate behind it compare the raw string.
    #[test]
    fn the_source_is_written_verbatim() {
        let mut unit = point();
        unit.source = Some("Chat-Upload".to_owned());
        let converted = point_structs(vec![unit]);
        assert_eq!(
            string_of(&converted[0], "source").as_deref(),
            Some("Chat-Upload")
        );
    }
}
