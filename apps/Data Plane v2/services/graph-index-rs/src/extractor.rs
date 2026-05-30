use anyhow::Context;
use reqwest::Client;

use crate::config::Config;
use crate::model::ExtractionResult;

pub struct GraphExtractor {
    client: Client,
    endpoint: String,
    api_key: String,
    deployment: String,
    max_entities: usize,
}

impl GraphExtractor {
    pub fn new(cfg: &Config) -> Self {
        Self {
            client: Client::new(),
            endpoint: cfg.azure_openai_endpoint.clone(),
            api_key: cfg.azure_openai_api_key.clone(),
            deployment: cfg.azure_openai_extraction_deployment.clone(),
            max_entities: cfg.max_entities_per_chunk,
        }
    }

    pub async fn extract(&self, text: &str, org_id: &str) -> anyhow::Result<ExtractionResult> {
        let prompt = format!(
            r#"Extract entities, relationships, and claims from the following text.
Return JSON with this schema:
{{
  "entities": [{{ "entity_type": "...", "entity_text": "...", "confidence": 0.0-1.0 }}],
  "relationships": [{{ "source_entity": "...", "target_entity": "...", "relation_type": "...", "confidence": 0.0-1.0 }}],
  "claims": [{{ "claim_text": "...", "related_entities": ["..."], "confidence": 0.0-1.0 }}]
}}
Max {max} entities. Only return valid JSON.

Text:
{text}"#,
            max = self.max_entities,
            text = text
        );

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version=2024-02-01",
            self.endpoint, self.deployment
        );

        let body = serde_json::json!({
            "messages": [
                {"role": "system", "content": "You are a knowledge graph extraction engine. Extract entities, relationships, and claims. Return only valid JSON."},
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.1,
            "max_tokens": 4096,
            "response_format": {"type": "json_object"}
        });

        let resp = self
            .client
            .post(&url)
            .header("api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .context("extraction API call")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("extraction API {status}: {text}");
        }

        let json: serde_json::Value = resp.json().await?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("{}");

        let result: ExtractionResult =
            serde_json::from_str(content).context("parse extraction result")?;

        tracing::info!(
            org_id,
            entities = result.entities.len(),
            relationships = result.relationships.len(),
            claims = result.claims.len(),
            "extraction complete"
        );

        Ok(result)
    }
}
