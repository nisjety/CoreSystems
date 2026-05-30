use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_database_url")]
    pub database_url: String,

    #[serde(default = "default_nats_url")]
    pub nats_url: String,

    #[serde(default = "default_admin_port")]
    pub admin_port: u16,

    #[serde(default = "default_grpc_port")]
    pub grpc_port: u16,

    #[serde(default = "default_azure_endpoint")]
    pub azure_openai_endpoint: String,

    #[serde(default)]
    pub azure_openai_api_key: String,

    #[serde(default = "default_extraction_deployment")]
    pub azure_openai_extraction_deployment: String,

    #[serde(default = "default_max_entities_per_chunk")]
    pub max_entities_per_chunk: usize,

    #[serde(default = "default_community_min_size")]
    #[allow(dead_code)] // consumed by detect_communities once that path is wired
    pub community_min_size: usize,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        envy::from_env().map_err(|e| anyhow::anyhow!("config: {e}"))
    }
}

fn default_database_url() -> String {
    "postgres://dataplane:dataplane@localhost:5442/dataplane".into()
}

fn default_nats_url() -> String {
    "nats://localhost:4232".into()
}

fn default_admin_port() -> u16 {
    9203
}

fn default_grpc_port() -> u16 {
    // Matches the Model Plane gateway's DATAPLANE_GRAPH_ADDR default (:50053).
    50053
}

fn default_azure_endpoint() -> String {
    String::new()
}

fn default_extraction_deployment() -> String {
    "gpt-4o".into()
}

fn default_max_entities_per_chunk() -> usize {
    20
}

fn default_community_min_size() -> usize {
    3
}
