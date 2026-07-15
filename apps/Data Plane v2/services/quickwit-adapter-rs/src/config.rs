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
    #[serde(default)]
    pub auth_public_key_file: Option<String>,
    #[serde(default)]
    pub auth_public_key_pem: Option<String>,
    pub auth_audience: String,
    pub auth_issuer: String,
    #[serde(default)]
    pub documents_event_public_key_path: String,
    #[serde(default)]
    pub index_event_public_key_path: String,
    #[serde(default)]
    pub embedding_event_public_key_path: String,
    #[serde(default)]
    pub wiki_event_public_key_path: String,
    #[serde(default = "default_event_auth_audience")]
    pub event_auth_audience: String,
    #[serde(default = "default_admin_job_rate_seconds")]
    pub admin_job_rate_seconds: u64,
    #[serde(default = "default_admin_job_lease_seconds")]
    pub admin_job_lease_seconds: u64,
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

fn default_admin_job_rate_seconds() -> u64 {
    60
}

fn default_admin_job_lease_seconds() -> u64 {
    300
}

fn default_event_auth_audience() -> String {
    "dataplane-events".into()
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let config = envy::from_env::<Config>()?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.auth_audience.trim().is_empty(),
            "AUTH_AUDIENCE is required"
        );
        anyhow::ensure!(
            !self.auth_issuer.trim().is_empty(),
            "AUTH_ISSUER is required"
        );
        let file_configured = self
            .auth_public_key_file
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        let pem_configured = self
            .auth_public_key_pem
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        anyhow::ensure!(
            file_configured ^ pem_configured,
            "exactly one of AUTH_PUBLIC_KEY_FILE or AUTH_PUBLIC_KEY_PEM is required"
        );
        anyhow::ensure!(
            !self.rebuild_on_start,
            "REBUILD_ON_START is disabled; use the scoped admin API"
        );
        anyhow::ensure!(
            self.admin_job_rate_seconds >= 1,
            "ADMIN_JOB_RATE_SECONDS must be at least 1"
        );
        anyhow::ensure!(
            self.admin_job_lease_seconds >= 30,
            "ADMIN_JOB_LEASE_SECONDS must be at least 30"
        );
        Ok(())
    }

    pub fn auth_public_key(&self) -> anyhow::Result<Vec<u8>> {
        if let Some(path) = self.auth_public_key_file.as_deref() {
            return std::fs::read(path)
                .map_err(anyhow::Error::from)
                .map_err(|error| error.context(format!("read AUTH_PUBLIC_KEY_FILE {path}")));
        }
        self.auth_public_key_pem
            .as_deref()
            .map(|pem| pem.as_bytes().to_vec())
            .ok_or_else(|| anyhow::anyhow!("admin JWT public key is not configured"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> Config {
        Config {
            database_url: "postgres://test.invalid/test".into(),
            nats_url: "nats://test.invalid:4222".into(),
            quickwit_url: default_quickwit_url(),
            quickwit_index_id: default_quickwit_index_id(),
            quickwit_index_config_path: default_index_config_path(),
            admin_port: default_admin_port(),
            batch_size: default_batch_size(),
            rebuild_on_start: false,
            auth_public_key_file: Some("/run/secrets/test-public-key.pem".into()),
            auth_public_key_pem: None,
            auth_audience: "data-plane".into(),
            auth_issuer: "https://control.example/api/convex-auth".into(),
            documents_event_public_key_path: "/run/event-keys/documents-events.pub".into(),
            index_event_public_key_path: "/run/event-keys/index-events.pub".into(),
            embedding_event_public_key_path: "/run/event-keys/embedding-events.pub".into(),
            wiki_event_public_key_path: "/run/event-keys/wiki-events.pub".into(),
            event_auth_audience: default_event_auth_audience(),
            admin_job_rate_seconds: default_admin_job_rate_seconds(),
            admin_job_lease_seconds: default_admin_job_lease_seconds(),
        }
    }

    #[test]
    fn startup_rebuild_is_rejected_before_connecting_to_dependencies() {
        let mut config = valid_config();
        config.rebuild_on_start = true;
        assert!(config.validate().is_err());
    }

    #[test]
    fn exactly_one_verification_key_source_is_required() {
        let mut config = valid_config();
        config.auth_public_key_file = None;
        assert!(config.validate().is_err());

        config.auth_public_key_file = Some("/run/secrets/test-public-key.pem".into());
        config.auth_public_key_pem = Some("test-public-key".into());
        assert!(config.validate().is_err());
    }

    #[test]
    fn signed_event_key_paths_are_explicit_configuration() {
        let config = valid_config();
        assert!(!config.documents_event_public_key_path.is_empty());
        assert!(!config.index_event_public_key_path.is_empty());
        assert!(!config.embedding_event_public_key_path.is_empty());
        assert!(!config.wiki_event_public_key_path.is_empty());
        assert_eq!(config.event_auth_audience, "dataplane-events");
    }
}
