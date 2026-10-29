use serde::Deserialize;

/// Runtime configuration for `meilisearch-adapter`, the write-side companion
/// to retrieval-engine-rs's `search/keyword.rs` query arm. This service owns
/// exactly one job: keep the Meilisearch `dataplane-knowledge` index
/// converged with the `knowledge_units`/`documents` corpus — created,
/// content-updated, and deleted — so the keyword arm never serves stale or
/// leaked content. See `stream.rs`'s module docs for why that is scoped to
/// the knowledge/document lifecycle only (not wiki pages or source-object
/// metadata) and why there is no admin rebuild HTTP API in this v1, unlike
/// its Quickwit sibling.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub nats_url: String,
    #[serde(default = "default_meilisearch_url")]
    pub meilisearch_url: String,
    /// Meilisearch master/admin key. Required in every real deployment
    /// (Compose enforces presence via `${MEILISEARCH_MASTER_KEY:?...}`);
    /// empty here only in a unit-test context that never dials a real
    /// Meilisearch instance. `MeilisearchClient` sends it as a bearer token.
    #[serde(default)]
    pub meilisearch_api_key: String,
    #[serde(default = "default_index_uid")]
    pub meilisearch_index_uid: String,
    #[serde(default = "default_admin_port")]
    pub admin_port: u16,
    #[serde(default)]
    pub documents_event_public_key_path: String,
    #[serde(default)]
    pub index_event_public_key_path: String,
    #[serde(default)]
    pub embedding_event_public_key_path: String,
    #[serde(default = "default_event_auth_audience")]
    pub event_auth_audience: String,
}

fn default_meilisearch_url() -> String {
    "http://meilisearch:7700".into()
}

fn default_index_uid() -> String {
    "dataplane-knowledge".into()
}

fn default_admin_port() -> u16 {
    9205
}

fn default_event_auth_audience() -> String {
    "dataplane-events".into()
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(envy::from_env::<Config>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_point_at_the_compose_internal_meilisearch_service() {
        assert_eq!(default_meilisearch_url(), "http://meilisearch:7700");
        assert_eq!(default_index_uid(), "dataplane-knowledge");
        assert_eq!(default_admin_port(), 9205);
    }

    #[test]
    fn event_audience_matches_the_shared_dataplane_events_contract() {
        // Every signed-event producer/consumer in this plane shares one
        // audience string; a typo here would make every envelope verify-fail.
        assert_eq!(default_event_auth_audience(), "dataplane-events");
    }

    #[test]
    fn config_deserializes_from_minimal_env_with_defaults_applied() {
        let cfg: Config = envy::from_iter(vec![
            (
                "DATABASE_URL".to_string(),
                "postgres://test.invalid/test".to_string(),
            ),
            (
                "NATS_URL".to_string(),
                "nats://test.invalid:4222".to_string(),
            ),
        ])
        .expect("minimal env must deserialize using field defaults");
        assert_eq!(cfg.meilisearch_url, "http://meilisearch:7700");
        assert_eq!(cfg.meilisearch_index_uid, "dataplane-knowledge");
        assert_eq!(cfg.admin_port, 9205);
        assert!(cfg.meilisearch_api_key.is_empty());
    }
}
