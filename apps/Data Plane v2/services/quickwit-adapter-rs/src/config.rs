use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub nats_url: String,
    #[serde(default = "default_quickwit_url")]
    pub quickwit_url: String,
    #[serde(default = "default_quickwit_index_id")]
    pub quickwit_index_id: String,
    #[serde(default = "default_index_config_path")]
    pub quickwit_index_config_path: String,
    #[serde(default = "default_admin_port")]
    pub admin_port: u16,
    #[serde(default = "default_batch_size")]
    pub batch_size: i64,
    #[serde(default)]
    pub rebuild_on_start: bool,
}

fn default_quickwit_url() -> String {
    "http://quickwit:7280".into()
}

fn default_quickwit_index_id() -> String {
    "dataplane-corpus".into()
}

fn default_index_config_path() -> String {
    "infra/quickwit/dataplane-corpus-index.yaml".into()
}

fn default_admin_port() -> u16 {
    9204
}

fn default_batch_size() -> i64 {
    500
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(envy::from_env::<Config>()?)
    }
}
