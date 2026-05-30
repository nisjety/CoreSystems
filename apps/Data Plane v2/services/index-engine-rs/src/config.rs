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

    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_max_delivery_attempts")]
    pub max_delivery_attempts: u32,
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

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(envy::from_env::<Config>()?)
    }
}
