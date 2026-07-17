//! HTTP client for graph-index-rs's `POST /v1/graph/traverse` — the Neo4j-backed
//! native multi-hop traversal (with its own transparent Postgres-BFS fallback
//! server-side). This is how the fused graph arm reaches DEEP multi-hop
//! neighbourhoods: Neo4j stays encapsulated inside graph-index-rs (no direct
//! Bolt from this service — plane rule), and this service re-grounds the
//! returned entity ids through its own org-visible Postgres chunk mapping.
//!
//! Auth: the caller's ALREADY-VERIFIED bearer is forwarded; graph-index
//! re-verifies it and pins the org itself (defense in depth — two independent
//! verifications of the same principal). No service-key fallback: without a
//! bearer the arm uses the in-process SQL grounding instead.

use anyhow::Context;

/// Thin client over graph-index's traverse endpoint. Cheap to clone.
#[derive(Clone)]
pub struct GraphTraverseClient {
    http: reqwest::Client,
    base_url: String,
}

impl GraphTraverseClient {
    /// `None` when `base_url` is empty (feature off — e.g. unit tests, bare
    /// metal). The timeout is deliberately short: this runs inside the
    /// retrieval hot path (concurrent with the other arms) and is non-fatal,
    /// so a slow graph-index must degrade, never stall the query.
    pub fn from_config(base_url: &str, timeout_ms: u64) -> Option<Self> {
        let base_url = base_url.trim().trim_end_matches('/');
        if base_url.is_empty() {
            return None;
        }
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(timeout_ms.max(100)))
            .build()
            .ok()?;
        Some(Self {
            http,
            base_url: base_url.to_string(),
        })
    }

    /// Multi-hop traversal from `seed_ids` within `org_id`. Returns
    /// `(entity_id, hops)` pairs for the reached entities. The org in the body
    /// must match the bearer's verified org or graph-index rejects with 403 —
    /// callers always pass the same org the bearer was verified for.
    pub async fn traverse(
        &self,
        bearer: &str,
        org_id: &str,
        seed_ids: &[String],
        max_hops: u8,
        max_entities: u32,
    ) -> anyhow::Result<Vec<(String, u8)>> {
        let url = format!("{}/v1/graph/traverse", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(bearer)
            .json(&serde_json::json!({
                "org_id": org_id,
                "seed_entity_ids": seed_ids,
                "max_hops": max_hops,
                "max_entities": max_entities,
            }))
            .send()
            .await
            .context("graph traverse request failed")?;
        let status = resp.status();
        if !status.is_success() {
            // Never echo the body — mirrors the rerank client's no-body-leak rule.
            anyhow::bail!("graph traverse returned {status}");
        }
        let body: serde_json::Value = resp.json().await.context("graph traverse decode")?;
        Ok(parse_traverse_entities(&body))
    }
}

/// Pulls `(entity_id, hops)` out of a traverse response. Tolerant by design:
/// entities missing an id are skipped, missing hops default to 1 (the server's
/// Postgres-fallback convention).
fn parse_traverse_entities(body: &serde_json::Value) -> Vec<(String, u8)> {
    body["entities"]
        .as_array()
        .map(|entities| {
            entities
                .iter()
                .filter_map(|e| {
                    let id = e["entity_id"].as_str()?;
                    if id.is_empty() {
                        return None;
                    }
                    let hops = e["hops"].as_u64().unwrap_or(1).clamp(1, u8::MAX as u64) as u8;
                    Some((id.to_string(), hops))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_disabled_on_empty_url_and_trims_slash() {
        assert!(GraphTraverseClient::from_config("", 1000).is_none());
        assert!(GraphTraverseClient::from_config("   ", 1000).is_none());
        let c = GraphTraverseClient::from_config("http://graph-index:9203/", 1000).unwrap();
        assert_eq!(c.base_url, "http://graph-index:9203");
    }

    #[test]
    fn parse_traverse_entities_reads_ids_and_hops() {
        let body = serde_json::json!({
            "entities": [
                {"entity_id": "e1", "hops": 1, "entity_text": "Ada"},
                {"entity_id": "e2", "hops": 3},
                {"entity_id": "e3"},            // missing hops → 1
                {"hops": 2},                     // missing id → skipped
                {"entity_id": "", "hops": 2},    // empty id → skipped
            ],
            "backend": "neo4j",
        });
        assert_eq!(
            parse_traverse_entities(&body),
            vec![
                ("e1".to_string(), 1),
                ("e2".to_string(), 3),
                ("e3".to_string(), 1),
            ]
        );
    }

    #[test]
    fn parse_traverse_entities_empty_on_malformed_body() {
        assert!(parse_traverse_entities(&serde_json::json!({})).is_empty());
        assert!(parse_traverse_entities(&serde_json::json!({"entities": "nope"})).is_empty());
    }
}
