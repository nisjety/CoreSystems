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
        let allow_insecure = env::var("ALLOW_INSECURE_DEV_DEFAULTS").ok();
        let isolated_insecure = isolated_insecure_from(
            allow_insecure.as_deref(),
            env::var("ISOLATED_E2E").ok().as_deref(),
        );
        if internal_token.is_none() && !isolated_insecure {
            return Err(AppError::Config(
                "AUTOCOMPLETE_INTERNAL_TOKEN is required outside isolated E2E".to_string(),
            ));
        }
        if placeholder_token_refused(internal_token.as_deref(), allow_insecure.as_deref()) {
            return Err(AppError::Config(
                "AUTOCOMPLETE_INTERNAL_TOKEN is a change-me placeholder; set the real shared token \
                 (or ALLOW_INSECURE_DEV_DEFAULTS=1 to override)"
                    .to_string(),
            ));
        }

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
        let nats_enabled = env_bool("NATS_ENABLED", nats_url_explicit.is_some());
        let nats_url = nats_url_explicit.unwrap_or_else(|| "nats://127.0.0.1:4222".to_string());

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

/// Reads one bypass switch exactly, as Model Plane, the BFF and `routes.rs` read it: only the
/// literal `1` turns it on. `true`, `yes`, `on` and padded values leave it off.
fn switch_is_literal_one(value: Option<&str>) -> bool {
    value == Some("1")
}

/// The isolated-E2E bypass needs both gates, each read exactly: `ALLOW_INSECURE_DEV_DEFAULTS=1`
/// and `ISOLATED_E2E=1`. The boot check here and the request-time check in `routes.rs` must
/// accept the same spellings, or a boot could pass that every authorized route then refuses.
fn isolated_insecure_from(allow_insecure: Option<&str>, isolated_e2e: Option<&str>) -> bool {
    switch_is_literal_one(allow_insecure) && switch_is_literal_one(isolated_e2e)
}

/// A `change-me*` token is a copied example placeholder. The BFF refuses that whole family for its
/// internal key, and integration-corev2 refuses it for its service tokens, trimmed and in any case.
/// Boot refuses it the same way unless `ALLOW_INSECURE_DEV_DEFAULTS` is exactly `1`; otherwise the
/// placeholder would authorize every non-health request as its bearer token.
fn placeholder_token_refused(internal_token: Option<&str>, allow_insecure: Option<&str>) -> bool {
    internal_token.is_some_and(|token| token.trim().to_ascii_lowercase().starts_with("change-me"))
        && !switch_is_literal_one(allow_insecure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insecure_switch_opens_only_for_the_exact_literal_one() {
        assert!(switch_is_literal_one(Some("1")));
        for value in [
            None,
            Some(""),
            Some("0"),
            Some("true"),
            Some("TRUE"),
            Some("yes"),
            Some("on"),
            Some(" 1"),
            Some("1 "),
            Some("01"),
        ] {
            assert!(!switch_is_literal_one(value), "{value:?}");
        }
    }

    #[test]
    fn isolated_insecure_needs_both_gates_as_the_literal_one() {
        assert!(isolated_insecure_from(Some("1"), Some("1")));
        for (allow_insecure, isolated_e2e) in [
            (Some("true"), Some("1")),
            (Some("1"), Some("true")),
            (Some("yes"), Some("on")),
            (Some("TRUE"), Some("TRUE")),
            (Some("1"), None),
            (None, Some("1")),
            (None, None),
        ] {
            assert!(
                !isolated_insecure_from(allow_insecure, isolated_e2e),
                "{allow_insecure:?} {isolated_e2e:?}"
            );
        }
    }

    #[test]
    fn change_me_placeholder_is_refused_unless_the_insecure_switch_is_exactly_one() {
        for token in [
            "change-me",
            "change-me-internal-service-secret",
            "CHANGE-ME-internal-service-secret",
            "  change-me  ",
        ] {
            for allow_insecure in [
                None,
                Some(""),
                Some("0"),
                Some("true"),
                Some("TRUE"),
                Some("yes"),
                Some("on"),
                Some(" 1"),
                Some("1 "),
                Some("01"),
            ] {
                assert!(
                    placeholder_token_refused(Some(token), allow_insecure),
                    "{token:?} {allow_insecure:?}"
                );
            }
            assert!(
                !placeholder_token_refused(Some(token), Some("1")),
                "{token:?}"
            );
        }
    }

    #[test]
    fn tokens_without_the_change_me_prefix_are_not_refused_as_placeholders() {
        for token in [
            "shared-token-fixture-aaaaaaaaaaaaaaaaaaaa",
            "changed-me-later",
        ] {
            assert!(!placeholder_token_refused(Some(token), None), "{token:?}");
        }
        assert!(!placeholder_token_refused(None, None));
        assert!(!placeholder_token_refused(None, Some("0")));
    }
}
