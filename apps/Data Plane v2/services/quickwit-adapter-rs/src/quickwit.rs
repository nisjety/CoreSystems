use anyhow::Context;
use reqwest::{Client, StatusCode};

use crate::model::QuickwitDocument;

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
