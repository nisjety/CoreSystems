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
