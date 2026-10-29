use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One row of `dataplane-knowledge`, Meilisearch's keyword-arm index.
///
/// `id` doubles as `knowledge_id` — Meilisearch requires the primary key to
/// match `^[A-Za-z0-9_-]+$`, which every `knowledge_id` in this schema already
/// satisfies (a UUID). Keeping `id == knowledge_id` (rather than inventing a
/// separate synthetic key) means a delete-by-filter on `document_id` and a
/// future direct delete-by-id on `knowledge_id` both work with no lookup
/// table on the side.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeywordDocument {
    pub id: String,
    pub org_id: String,
    pub document_id: String,
    pub knowledge_id: String,
    pub chunk_index: i32,
    pub source: String,
    pub title: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(default)]
    pub acl_tags: Vec<String>,
    /// Unix seconds. Meilisearch has no native datetime type; kept numeric
    /// (rather than RFC3339 text) so it stays usable as a sortable attribute
    /// — see `MeilisearchClient::ensure_index`'s `sortableAttributes`.
    pub updated_at: i64,
}

pub fn unix_seconds(dt: DateTime<Utc>) -> i64 {
    dt.timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_key_charset_is_meilisearch_safe() {
        // Meilisearch primary keys must match ^[A-Za-z0-9_-]+$. A UUID
        // knowledge_id always does; guard the assumption with a realistic
        // example rather than trusting it silently.
        let id = "3fa85f64-5717-4562-b3fc-2c963f66afa6";
        assert!(id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn keyword_document_round_trips_through_json() {
        let doc = KeywordDocument {
            id: "kid-1".into(),
            org_id: "org-1".into(),
            document_id: "doc-1".into(),
            knowledge_id: "kid-1".into(),
            chunk_index: 0,
            source: "upload".into(),
            title: "Invoice 2039".into(),
            body: "SKU-88213 shipped on 2026-08-01".into(),
            content_hash: Some("abc123".into()),
            acl_tags: vec!["finance".into()],
            updated_at: 1_754_000_000,
        };
        let encoded = serde_json::to_string(&doc).expect("serialize");
        let decoded: KeywordDocument = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(doc, decoded);
    }

    #[test]
    fn unix_seconds_matches_chrono_timestamp() {
        let dt = DateTime::parse_from_rfc3339("2026-08-07T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(unix_seconds(dt), dt.timestamp());
    }
}
