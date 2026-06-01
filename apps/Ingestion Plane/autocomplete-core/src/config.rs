use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use crate::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct Settings {
    pub http_addr: SocketAddr,
    pub metadata_db_path: PathBuf,
    pub internal_token: Option<String>,
    pub sonic: SonicSettings,
    pub nats: NatsSettings,
}

#[derive(Debug, Clone)]
pub struct SonicSettings {
    pub enabled: bool,
    pub addr: String,
    pub password: String,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct NatsSettings {
    pub enabled: bool,
    pub url: String,
    pub stream_name: String,
    pub durable_name: String,
    pub subject_filter: String,
    pub auth_token: Option<String>,
}

impl Settings {
    pub fn from_env() -> AppResult<Self> {
        let http_addr = env::var("AUTOCOMPLETE_HTTP_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:3219".to_string())
            .parse()
            .map_err(|e| AppError::Config(format!("invalid AUTOCOMPLETE_HTTP_ADDR: {e}")))?;

        let metadata_db_path = env::var("AUTOCOMPLETE_METADATA_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data/autocomplete-core.sqlite3"));

        let internal_token = env::var("AUTOCOMPLETE_INTERNAL_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty());

        let sonic_addr = env::var("SONIC_ADDR").unwrap_or_else(|_| "127.0.0.1:1491".to_string());
        let sonic_password = env::var("SONIC_PASSWORD").unwrap_or_default();
        let sonic_enabled = env_bool("SONIC_ENABLED", !sonic_password.is_empty());

        let nats_url_explicit = env::var("NATS_URL")
            .or_else(|_| env::var("QUARRY_EDGE__NATS_URL"))
            .ok();
        // Enable NATS by default when an explicit URL is configured, matching
        // the same convention as SONIC_ENABLED (enabled iff credentials present).
        // Previously hard-coded to false, which silently disabled the consumer
        // and left the Sonic index permanently empty even when NATS_URL was set.
        let nats_enabled = env_bool(
            "NATS_ENABLED",
            nats_url_explicit.is_some(),
        );
        let nats_url = nats_url_explicit
            .unwrap_or_else(|| "nats://127.0.0.1:4222".to_string());

        Ok(Self {
            http_addr,
            metadata_db_path,
            internal_token,
            sonic: SonicSettings {
                enabled: sonic_enabled,
                addr: sonic_addr,
                password: sonic_password,
                timeout: Duration::from_secs(env_u64("SONIC_TIMEOUT_SECS", 3)),
            },
            nats: NatsSettings {
                enabled: nats_enabled,
                url: nats_url,
                stream_name: env::var("NATS_STREAM")
                    .unwrap_or_else(|_| "QUARRY_EVENTS".to_string()),
                durable_name: env::var("NATS_DURABLE")
                    .unwrap_or_else(|_| "autocomplete-core".to_string()),
                subject_filter: env::var("NATS_SUBJECT_FILTER")
                    .unwrap_or_else(|_| "quarry.events.*".to_string()),
                auth_token: env::var("NATS_AUTH_TOKEN").ok(),
            },
        })
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}
