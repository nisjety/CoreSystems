//! The `source` key of a vector point: which kind of origin the document it was cut
//! from has (`sharepoint`, `quarry`, `chat-upload`, ...), copied from `documents.source`.
//!
//! ## Why the point carries it
//!
//! The retrieval engine's dense arm filters Qdrant on payload keys and nothing else: it
//! cannot join Postgres inside a vector search. The corpus a service principal may read
//! (Quarry's crawled pages, and nothing the organisation's people uploaded or synced) is
//! therefore only enforceable on the dense arm if the point itself says where its document
//! came from. `search::filters::RetrievalFilters` already turns `filters.sources` into a
//! keyword match on the key `source`; until a point carries the key, that match finds
//! nothing, which is a closed door, not a scoped one.
//!
//! The Postgres row stays the authority. A point's stamp is a copy made when the point is
//! written (the batch pipeline, the video-caption path) or when the operator tool
//! `qdrant-backfill-source` stamps a point that has none, and the retrieval engine keeps its
//! own check against `documents.source` after fusion. A copy can only be wrong in one way:
//! `documents.source` can be changed by a re-ingest of the same document key
//! (`documents-api-go` `UPDATE documents SET source=...`), and a chunk that did not change is
//! not re-embedded, so its point is not rewritten. The backfill tool's `--report` mode shows
//! that as a count that no longer matches Postgres.
//!
//! ## Rules
//!
//! * The value is `documents.source` byte for byte: no trimming, no case folding. The filter
//!   on the retrieval side and the SQL predicate behind it compare the same string.
//! * A blank source is no source: the key is omitted, never written empty, because a point
//!   with `source = ""` would look stamped to the backfill and could never match a filter.
//! * The document is found by `(document_id, org_id)` together, with the organisation
//!   predicate written out. This runs on the superuser connection every Data Plane service
//!   uses, where row-level security is inert, so the predicate is the only tenant check.
//!
//! This file is compiled into the service AND into the `qdrant-backfill-source` tool
//! (`#[path]`), so the key and the lookup have one definition.

use std::collections::{BTreeSet, HashMap};

use sqlx::{PgConnection, Row};

/// The payload key. The retrieval engine's `RetrievalFilters::to_qdrant_conditions` reads
/// the same literal for `filters.sources`.
pub(crate) const PAYLOAD_KEY: &str = "source";

/// The source to write for a document, or `None` when it has none worth writing.
pub(crate) fn stampable(source: &str) -> Option<&str> {
    (!source.trim().is_empty()).then_some(source)
}

/// `documents.source` of each `(document_id, org_id)` pair that exists, keyed by the pair.
///
/// A pair that is not in the result has no such document in that organisation (never
/// created, removed, or claimed under another organisation): the caller writes no stamp for
/// it rather than guessing one. Soft-deleted documents are included, because a stamp is a
/// fact about the row and the visibility gates decide about deletion elsewhere.
pub(crate) async fn load(
    conn: &mut PgConnection,
    pairs: &[(&str, &str)],
) -> sqlx::Result<HashMap<(String, String), String>> {
    let pairs: BTreeSet<(&str, &str)> = pairs
        .iter()
        .copied()
        .filter(|(document_id, org_id)| !document_id.is_empty() && !org_id.is_empty())
        .collect();
    if pairs.is_empty() {
        return Ok(HashMap::new());
    }
    let (document_ids, org_ids): (Vec<&str>, Vec<&str>) = pairs.into_iter().unzip();
    let rows = sqlx::query(
        r#"
        SELECT d.document_id, d.org_id, d.source
          FROM documents AS d
          JOIN unnest($1::text[], $2::text[]) AS t(document_id, org_id)
            ON d.document_id = t.document_id AND d.org_id = t.org_id
        "#,
    )
    .bind(&document_ids)
    .bind(&org_ids)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|row| {
            (
                (row.get("document_id"), row.get("org_id")),
                row.get("source"),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_payload_key_is_the_one_the_retrieval_filters_read() {
        // `retrieval-engine-rs/src/search/filters.rs` matches `filters.sources` on this
        // literal. Renaming either side without the other turns the corpus scope into a
        // filter that matches nothing.
        assert_eq!(PAYLOAD_KEY, "source");
    }

    #[test]
    fn a_source_is_written_exactly_as_the_document_has_it() {
        assert_eq!(stampable("quarry"), Some("quarry"));
        assert_eq!(stampable("chat-upload"), Some("chat-upload"));
        // Not normalised: the SQL predicate compares the raw column.
        assert_eq!(stampable("Quarry"), Some("Quarry"));
        assert_eq!(stampable(" quarry"), Some(" quarry"));
    }

    #[test]
    fn a_blank_source_is_no_source() {
        assert_eq!(stampable(""), None);
        assert_eq!(stampable("   "), None);
        assert_eq!(stampable("\t\n"), None);
    }
}
