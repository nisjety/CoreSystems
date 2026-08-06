use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub nats_url: String,

    #[serde(default = "default_admin_port")]
    pub admin_port: u16,

    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,
    #[serde(default = "default_chunk_overlap")]
    pub chunk_overlap: usize,
    /// Token budget for each chunk's parent context window (P2-2). Unset
    /// (the default) disables parent-window attachment entirely — nothing
    /// downstream consumes it yet, so this stays opt-in.
    #[serde(default)]
    pub parent_chunk_size: Option<usize>,

    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_max_delivery_attempts")]
    pub max_delivery_attempts: u32,

    #[serde(default)]
    pub documents_event_public_key_path: String,
    #[serde(default)]
    pub index_event_private_key_path: String,
    #[serde(default = "default_event_auth_audience")]
    pub event_auth_audience: String,
}

fn default_admin_port() -> u16 {
    9201
}
fn default_chunk_size() -> usize {
    512
}
fn default_chunk_overlap() -> usize {
    64
}
fn default_batch_size() -> usize {
    10
}
fn default_max_delivery_attempts() -> u32 {
    5
}
fn default_event_auth_audience() -> String {
    "dataplane-events".into()
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(envy::from_env::<Config>()?)
    }
}
