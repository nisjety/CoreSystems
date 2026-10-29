use anyhow::Context;
use reqwest::{Client, StatusCode};
use serde_json::json;

use crate::model::KeywordDocument;

/// Thin HTTP client over Meilisearch's index/settings/document API — the
/// write side used by this adapter.
///
/// retrieval-engine-rs's `search/keyword.rs` has its own, separate,
/// query-only Meilisearch client. The two are deliberately independent HTTP
/// clients against the same service rather than a shared crate — the same
/// split `quickwit-adapter-rs::quickwit::QuickwitClient` (ingest) and
/// retrieval-engine's own Quickwit query path already use for the sparse
/// arm — because their failure/timeout/retry needs differ: an ingest failure
/// can retry via NATS redelivery, while a query must fail fast so a live
/// retrieval request never stalls on a stuck HTTP call.
#[derive(Clone)]
pub struct MeilisearchClient {
    http: Client,
    base_url: String,
    index_uid: String,
    api_key: String,
}

impl MeilisearchClient {
    pub fn new(
        base_url: impl Into<String>,
        index_uid: impl Into<String>,
        api_key: impl Into<String>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            index_uid: index_uid.into(),
            api_key: api_key.into(),
        })
    }

    fn authed(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.is_empty() {
            builder
        } else {
            builder.bearer_auth(&self.api_key)
        }
    }

    /// Idempotent: creates the index (if absent) with `id` as its primary
    /// key, then converges `filterableAttributes`/`searchableAttributes`/
    /// `sortableAttributes` every startup — so a settings change (e.g. adding
    /// a new filterable field) rolls out on the next deploy with no manual
    /// admin step, mirroring `QuickwitClient::ensure_index`'s role.
    ///
    /// `org_id` is filterable and MUST stay filterable for the lifetime of
    /// this index: it is the sole isolation boundary the query-side arm
    /// relies on (see retrieval-engine-rs's `search/keyword.rs` org-filter
    /// construction). `document_id` is filterable so a delete-by-document
    /// (the erasure path) can target it directly without a full reindex.
    pub async fn ensure_index(&self) -> anyhow::Result<()> {
        let create_url = format!("{}/indexes", self.base_url);
        let create = self
            .authed(self.http.post(&create_url))
            .json(&json!({ "uid": self.index_uid, "primaryKey": "id" }))
            .send()
            .await
            .context("create Meilisearch index request")?;
        // Meilisearch queues index creation and answers 202 either way; a
        // repeat call on an existing index instead answers 4xx with
        // `index_already_exists`, which is equally fine here — anything else
        // is a real failure.
        if !create.status().is_success() {
            let body = create.text().await.unwrap_or_default();
            if !body.contains("index_already_exists") {
                anyhow::bail!("Meilisearch index create failed: {body}");
            }
        }

        let settings_url = format!("{}/indexes/{}/settings", self.base_url, self.index_uid);
        let settings = self
            .authed(self.http.patch(&settings_url))
            .json(&json!({
                "filterableAttributes": ["org_id", "document_id"],
                "searchableAttributes": ["title", "body", "document_id", "knowledge_id", "source"],
                "sortableAttributes": ["updated_at"],
            }))
            .send()
            .await
            .context("update Meilisearch index settings request")?;
        if !settings.status().is_success() {
            anyhow::bail!(
                "Meilisearch settings update failed: {} {}",
                settings.status(),
                settings.text().await.unwrap_or_default()
            );
        }
        Ok(())
    }

    /// Upsert (create-or-replace by primary key). Empty input is a no-op,
    /// mirroring `QuickwitClient::ingest`'s empty-batch guard.
    pub async fn upsert(&self, docs: &[KeywordDocument]) -> anyhow::Result<()> {
        if docs.is_empty() {
            return Ok(());
        }
        let url = format!("{}/indexes/{}/documents", self.base_url, self.index_uid);
        let response = self
            .authed(self.http.post(&url))
            .json(docs)
            .send()
            .await
            .context("Meilisearch upsert request")?;
        if response.status().is_success() {
            return Ok(());
        }
        anyhow::bail!(
            "Meilisearch upsert failed: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
    }

    /// Delete every document for one `(org_id, document_id)` pair. Used by
    /// the erasure path (`dataplane.documents.deleted`) and by the
    /// clear-before-reindex step on a content update — mirroring
    /// `QuickwitClient::delete_by_query`'s role for the Quickwit arm.
    ///
    /// The filter is a Meilisearch filter EXPRESSION (not free text), built
    /// with quoted+escaped literals via [`quote_filter_value`] so an org_id
    /// or document_id containing a `"` or `\` can never break out of its
    /// clause and widen the match.
    pub async fn delete_by_document(&self, org_id: &str, document_id: &str) -> anyhow::Result<()> {
        let filter = format!(
            "org_id = {} AND document_id = {}",
            quote_filter_value(org_id),
            quote_filter_value(document_id)
        );
        let url = format!(
            "{}/indexes/{}/documents/delete",
            self.base_url, self.index_uid
        );
        let response = self
            .authed(self.http.post(&url))
            .json(&json!({ "filter": filter }))
            .send()
            .await
            .context("Meilisearch delete-by-filter request")?;
        if response.status().is_success() {
            return Ok(());
        }
        anyhow::bail!(
            "Meilisearch delete-by-filter failed: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
    }

    pub async fn healthy(&self) -> bool {
        let url = format!("{}/health", self.base_url);
        matches!(self.http.get(&url).send().await, Ok(r) if r.status() == StatusCode::OK)
    }
}

/// Escapes a value for embedding inside a Meilisearch filter expression: a
/// double-quoted string literal with `\` and `"` escaped. Mirrors
/// `quickwit-adapter-rs::quickwit::quote_query_value`'s role for Quickwit's
/// query-string filters — the same defense against a value breaking out of
/// its clause (org_id/document_id are system-generated in practice, but a
/// filter builder must not silently assume that).
pub fn quote_filter_value(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_value_escapes_quotes_and_backslashes() {
        assert_eq!(quote_filter_value("org-1"), "\"org-1\"");
        assert_eq!(quote_filter_value(r#"org"1\evil"#), r#""org\"1\\evil""#);
    }

    #[test]
    fn filter_value_escapes_every_embedded_quote() {
        // A naive, unescaped build would let this value close its own quote
        // early and append a second `OR` clause. Every quote embedded in the
        // input must come out escaped (`\"`), so none remain bare and able to
        // terminate the wrapper early.
        let hostile = "value\" OR org_id=\"other";
        let quoted = quote_filter_value(hostile);
        assert_eq!(hostile.matches('"').count(), quoted.matches("\\\"").count());
        assert!(quoted.starts_with('"') && quoted.ends_with('"'));
    }
}
