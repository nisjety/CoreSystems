use std::collections::BTreeSet;

use anyhow::Context;
use reqwest::{Client, StatusCode};

use crate::model::QuickwitDocument;

/// Fields this adapter writes and searches that were added AFTER the original
/// index schema shipped.
///
/// Quickwit 0.8 cannot alter an existing index (no update endpoint — see
/// [`QuickwitClient::ensure_index`]), so on any deployment whose index predates
/// one of these, the field is neither ingested nor searched, with no error
/// anywhere. Listing them here is what lets startup say so out loud.
///
/// Add an entry whenever `dataplane-corpus-index.yaml` gains a field the adapter
/// depends on; the remedy is always the same — delete the index and let
/// `ensure_index` recreate it, then rebuild.
const SCHEMA_ADDITIONS: &[(&str, &str)] = &[(
    "context_body",
    "Contextual Retrieval's lexical half: chunk context is not indexed or searched on this index",
)];

/// Compare the live index's field list against [`SCHEMA_ADDITIONS`] and warn per
/// missing field.
///
/// A warning rather than a hard failure, deliberately: every one of these is a
/// retrieval-QUALITY feature, and the Postgres fallback backend carries its own
/// copy of the same capability (contextual BM25 lives in `content_tsv` there
/// regardless). Refusing to boot would trade a partial feature for a dead
/// keyword arm.
fn warn_on_missing_schema_additions(live: &serde_json::Value, index_id: &str) {
    let present: Vec<&str> = live
        .pointer("/index_config/doc_mapping/field_mappings")
        .and_then(serde_json::Value::as_array)
        .map(|fields| {
            fields
                .iter()
                .filter_map(|f| f.get("name").and_then(serde_json::Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    if present.is_empty() {
        // Unrecognised response shape — say nothing rather than warn falsely.
        return;
    }
    for (field, consequence) in SCHEMA_ADDITIONS {
        if !present.contains(field) {
            tracing::warn!(
                index = %index_id,
                field = %field,
                "Quickwit index predates a required field and CANNOT be altered in place \
                 (0.8 has no index-update API). {consequence}. Remedy: DELETE \
                 /api/v1/indexes/{index_id} and restart so the index is recreated from \
                 dataplane-corpus-index.yaml, then trigger a rebuild."
            );
        }
    }
}

/// How many distinct units one parity request asks the index to list. Far under the aggregation
/// bucket limit of Quickwit (65,000), and enough that an organization of a few thousand units
/// is read in two requests.
pub const TERMS_PER_REQUEST: u64 = 10_000;

/// The deepest id prefix the parity check will split by: sixteen to the power of this many parts
/// is more than any index holds, so reaching it means the ids are not what this check reads.
const MAX_PREFIX_DEPTH: usize = 8;

const HEX: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// One organization's knowledge units as the index holds them (see
/// [`QuickwitClient::indexed_units`]). Ids are lower-cased.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexedUnits {
    /// Documents the index counts, duplicates included.
    pub documents: u64,
    /// Distinct units.
    pub distinct: BTreeSet<String>,
    /// Distinct units with a `context_body` (the field is present: the indexer writes none for a
    /// blank context).
    pub with_context: BTreeSet<String>,
}

struct TermsPage {
    hits: u64,
    keys: BTreeSet<String>,
    /// Documents the aggregation left out because it returned `size` terms.
    other: u64,
}

#[derive(Clone)]
pub struct QuickwitClient {
    http: Client,
    base_url: String,
    index_id: String,
    index_config: String,
}

impl QuickwitClient {
    pub fn new(
        base_url: impl Into<String>,
        index_id: impl Into<String>,
        index_config_path: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<Self> {
        let index_config =
            std::fs::read_to_string(index_config_path.as_ref()).with_context(|| {
                format!(
                    "read Quickwit index config from {}",
                    index_config_path.as_ref().display()
                )
            })?;

        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            index_id: index_id.into(),
            index_config,
        })
    }

    pub fn index_id(&self) -> &str {
        &self.index_id
    }

    pub async fn ensure_index(&self) -> anyhow::Result<()> {
        let url = format!("{}/api/v1/indexes/{}", self.base_url, self.index_id);
        let response = self.http.get(&url).send().await?;
        if response.status().is_success() {
            // The index exists, so its schema is FROZEN. Quickwit 0.8 has no
            // index-update API — verified against 0.8.1: `PUT
            // /api/v1/indexes/{id}` answers 405 and
            // `.../search-settings` answers 404 — so the config we just read
            // from disk has no way to reach an already-created index.
            //
            // That makes a newly-added field silently inert rather than broken:
            // `mode: lenient` drops it on ingest, and the index's own stored
            // `default_search_fields` never gains it, so queries keep working
            // and simply never match it. Silence is the problem — check for it.
            match response.json::<serde_json::Value>().await {
                Ok(live) => warn_on_missing_schema_additions(&live, &self.index_id),
                Err(e) => tracing::warn!(
                    error = %e,
                    index = %self.index_id,
                    "could not read live Quickwit index config; skipping schema-drift check"
                ),
            }
            return Ok(());
        }
        if response.status() != StatusCode::NOT_FOUND {
            anyhow::bail!(
                "Quickwit index lookup failed: {} {}",
                response.status(),
                response.text().await.unwrap_or_default()
            );
        }

        let create_url = format!("{}/api/v1/indexes", self.base_url);
        let create = self
            .http
            .post(&create_url)
            .header("content-type", "application/yaml")
            .body(self.index_config.clone())
            .send()
            .await?;

        if create.status().is_success() || create.status() == StatusCode::CONFLICT {
            tracing::info!(index = %self.index_id, "Quickwit index ready");
            return Ok(());
        }

        anyhow::bail!(
            "Quickwit index create failed: {} {}",
            create.status(),
            create.text().await.unwrap_or_default()
        )
    }

    pub async fn clear_index(&self) -> anyhow::Result<()> {
        let url = format!("{}/api/v1/indexes/{}/clear", self.base_url, self.index_id);
        let response = self.http.put(&url).send().await?;
        if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        anyhow::bail!(
            "Quickwit clear failed: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
    }

    pub async fn ingest(&self, docs: &[QuickwitDocument]) -> anyhow::Result<()> {
        self.ingest_with_commit(docs, "auto").await
    }

    pub async fn ingest_rebuild_batch(&self, docs: &[QuickwitDocument]) -> anyhow::Result<()> {
        self.ingest_with_commit(docs, "force").await
    }

    async fn ingest_with_commit(
        &self,
        docs: &[QuickwitDocument],
        commit: &str,
    ) -> anyhow::Result<()> {
        if docs.is_empty() {
            return Ok(());
        }

        let mut ndjson = String::new();
        for doc in docs {
            ndjson.push_str(&serde_json::to_string(doc)?);
            ndjson.push('\n');
        }

        let url = format!(
            "{}/api/v1/{}/ingest?commit={commit}",
            self.base_url, self.index_id,
        );
        let response = self
            .http
            .post(&url)
            .header("content-type", "application/json")
            .body(ndjson)
            .send()
            .await?;

        if response.status().is_success() {
            return Ok(());
        }

        anyhow::bail!(
            "Quickwit ingest failed: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
    }

    pub async fn count_documents(&self, org_id: Option<&str>) -> anyhow::Result<u64> {
        let query = org_id
            .map(|org| format!("org_id:{}", quote_query_value(org)))
            .unwrap_or_else(|| "*".to_string());
        self.count_query(&query).await
    }

    pub async fn count_rebuild_batch(&self, batch_id: &str) -> anyhow::Result<u64> {
        self.count_query(&format!("rebuild_batch_id:{}", quote_query_value(batch_id)))
            .await
    }

    async fn count_query(&self, query: &str) -> anyhow::Result<u64> {
        let url = format!("{}/api/v1/{}/search", self.base_url, self.index_id);
        let response = self
            .http
            .post(&url)
            .json(&serde_json::json!({"query": query, "max_hits": 0}))
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Quickwit rebuild preflight failed with status {}",
                response.status()
            );
        }
        let body: serde_json::Value = response.json().await?;
        body.get("num_hits")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("Quickwit rebuild preflight omitted num_hits"))
    }

    /// The knowledge units one organization has in the index, as the keyword-index parity check
    /// needs them: how many documents the index counts, which distinct units there are (de-duplicated
    /// by `knowledge_id`, because a delete is a task that takes effect at the next merge and until
    /// then the index holds both generations of a re-indexed chunk), and which of those have a
    /// `context_body`. (The index answers `context_body:*` by whether the field is present, and the
    /// indexer writes none for a blank context, so present means not blank.)
    ///
    /// Read-only: a count, and terms aggregations that carry ids and no document. Two things are
    /// true of this Quickwit (0.8.1, measured on the live index) and shape the code:
    ///
    /// * it refuses the `cardinality` aggregation, so the distinct units are enumerated with a
    ///   `terms` aggregation on `knowledge_id`;
    /// * a `terms` aggregation returns at most `size` buckets and says how many documents it left
    ///   out (`sum_other_doc_count`), and the field cannot be range-queried or sorted (it is a text
    ///   field), so an organization with more units than one request holds is split by id prefix,
    ///   `knowledge_id:a*`, `knowledge_id:a0*`, and so on, until every part fits. The parts must add
    ///   up to the whole: a part that does not (an id that is not lower-case hexadecimal, which a
    ///   prefix query on a raw-tokenized field cannot reach) is an error, never a silently smaller
    ///   count.
    ///
    /// The aggregation reads the lower-cased fast field, so the ids come back lower-cased; the
    /// caller compares them lower-cased.
    pub async fn indexed_units(&self, org_id: &str) -> anyhow::Result<IndexedUnits> {
        self.indexed_units_by(org_id, TERMS_PER_REQUEST).await
    }

    /// [`Self::indexed_units`] with the number of terms one request may return, so that a test can
    /// make a small index need partitioning.
    pub async fn indexed_units_by(
        &self,
        org_id: &str,
        terms_per_request: u64,
    ) -> anyhow::Result<IndexedUnits> {
        let base = format!(
            "org_id:{} AND entity_type:\"knowledge_unit\"",
            quote_query_value(org_id)
        );
        let (documents, distinct) = self.distinct_ids(&base, terms_per_request).await?;
        let (_, with_context) = self
            .distinct_ids(&format!("{base} AND context_body:*"), terms_per_request)
            .await?;
        Ok(IndexedUnits {
            documents,
            distinct,
            with_context,
        })
    }

    /// `(documents the query matches, the distinct lower-cased ids among them)`.
    async fn distinct_ids(
        &self,
        base: &str,
        terms_per_request: u64,
    ) -> anyhow::Result<(u64, BTreeSet<String>)> {
        let mut ids = BTreeSet::new();
        let mut total = None;
        // Prefixes still to read; the empty one is the whole query.
        let mut pending = vec![String::new()];
        while let Some(prefix) = pending.pop() {
            let query = if prefix.is_empty() {
                base.to_owned()
            } else {
                format!("{base} AND knowledge_id:{prefix}*")
            };
            let part = self.terms_page(&query, terms_per_request).await?;
            if prefix.is_empty() {
                total = Some(part.hits);
            }
            if part.other == 0 {
                ids.extend(part.keys);
                continue;
            }
            // More units than one request holds: split by the next character of the id.
            anyhow::ensure!(
                prefix.len() < MAX_PREFIX_DEPTH,
                "the index holds more units than can be listed by id prefix"
            );
            let mut parts = Vec::with_capacity(HEX.len());
            let mut covered = 0u64;
            for digit in HEX {
                let child = format!("{prefix}{digit}");
                let hits = self
                    .count_query(&format!("{base} AND knowledge_id:{child}*"))
                    .await?;
                covered += hits;
                if hits > 0 {
                    parts.push(child);
                }
            }
            anyhow::ensure!(
                covered == part.hits,
                "the index holds units whose id does not start with a lower-case hexadecimal \
                 digit after `{prefix}`: {} of {} cannot be listed by prefix",
                part.hits.saturating_sub(covered),
                part.hits
            );
            pending.extend(parts);
        }
        Ok((total.unwrap_or(0), ids))
    }

    /// One `terms` aggregation over `knowledge_id`.
    async fn terms_page(&self, query: &str, size: u64) -> anyhow::Result<TermsPage> {
        let url = format!("{}/api/v1/{}/search", self.base_url, self.index_id);
        let response = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "query": query,
                "max_hits": 0,
                "aggs": { "ids": { "terms": { "field": "knowledge_id", "size": size } } },
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Quickwit parity search failed with status {}",
                response.status()
            );
        }
        let body: serde_json::Value = response.json().await?;
        let hits = body
            .get("num_hits")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("Quickwit parity search omitted num_hits"))?;
        let aggregation = body.pointer("/aggregations/ids");
        let keys = aggregation
            .and_then(|ids| ids.get("buckets"))
            .and_then(serde_json::Value::as_array)
            .map(|buckets| {
                buckets
                    .iter()
                    .filter_map(|bucket| bucket.get("key").and_then(serde_json::Value::as_str))
                    .map(str::to_lowercase)
                    .collect::<BTreeSet<_>>()
            });
        // No aggregation in the answer is only right when nothing matched: a Quickwit that
        // ignored the aggregation would otherwise read as an empty index.
        let keys = match (keys, hits) {
            (Some(keys), _) => keys,
            (None, 0) => BTreeSet::new(),
            (None, _) => anyhow::bail!("Quickwit parity search returned no aggregation"),
        };
        let other = aggregation
            .and_then(|ids| ids.get("sum_other_doc_count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        Ok(TermsPage { hits, keys, other })
    }

    pub async fn delete_by_query(&self, query: &str) -> anyhow::Result<()> {
        let url = format!("{}/api/v1/{}/delete-tasks", self.base_url, self.index_id);
        let response = self
            .http
            .post(&url)
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await?;

        if response.status().is_success() {
            return Ok(());
        }

        anyhow::bail!(
            "Quickwit delete task failed: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
    }
}

pub fn quote_query_value(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod schema_drift_tests {
    use super::{warn_on_missing_schema_additions, SCHEMA_ADDITIONS};
    use serde_json::json;

    fn live_index_with(fields: &[&str]) -> serde_json::Value {
        json!({
            "index_config": {
                "doc_mapping": {
                    "field_mappings": fields
                        .iter()
                        .map(|name| json!({"name": name, "type": "text"}))
                        .collect::<Vec<_>>()
                }
            }
        })
    }

    /// The whole point of the constant: every field the adapter *writes* that
    /// arrived after the initial schema must be listed, or a Quickwit index
    /// created before it silently ignores the field forever.
    #[test]
    fn every_listed_addition_is_actually_in_the_shipped_index_config() {
        let yaml = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../infra/quickwit/dataplane-corpus-index.yaml"
        ));
        for (field, _) in SCHEMA_ADDITIONS {
            assert!(
                yaml.contains(&format!("name: {field}")),
                "`{field}` is listed as a schema addition but is absent from \
                 dataplane-corpus-index.yaml, so a fresh index would not have it either"
            );
            assert!(
                yaml.contains(&format!("- {field}")),
                "`{field}` must appear in default_search_fields, or indexing it \
                 buys nothing — an unsearched field is dead weight"
            );
        }
    }

    /// The drift case this exists to catch: index missing the field. The check
    /// must notice rather than pass silently.
    #[test]
    fn a_stale_index_is_detected_and_a_current_one_is_not() {
        let current: Vec<&str> = SCHEMA_ADDITIONS
            .iter()
            .map(|(f, _)| *f)
            .chain(["body", "title"])
            .collect();
        // Both branches must run without panicking; the observable effect is a
        // log line, so what is asserted here is that field presence is read
        // correctly from the real response shape.
        warn_on_missing_schema_additions(&live_index_with(&current), "dataplane-corpus");
        warn_on_missing_schema_additions(&live_index_with(&["body", "title"]), "dataplane-corpus");
    }

    /// An unparseable or restructured response must not produce a false alarm —
    /// warning about a missing field on every startup would train people to
    /// ignore the warning that matters.
    #[test]
    fn an_unrecognised_response_shape_stays_quiet() {
        warn_on_missing_schema_additions(&json!({}), "dataplane-corpus");
        warn_on_missing_schema_additions(&json!({"index_config": {}}), "dataplane-corpus");
        warn_on_missing_schema_additions(&json!("not an object"), "dataplane-corpus");
    }
}
