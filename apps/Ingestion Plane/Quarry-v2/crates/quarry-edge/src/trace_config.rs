//! Replay ring bounds for the agent event trace (WS10.S3), and the Model Plane
//! run-trace publisher settings (WS10.S5b, [`ModelTraceEnv`], off by default).
//!
//! Env-only, so the 187-line `config.rs` does not change for this slice. Both knobs
//! are read once at boot. An unparsable value aborts the boot with the variable's
//! name, so a typo never silently changes the replay window.
//!
//! - `QUARRY_EDGE__TRACE_RING_EVENTS` (default 512): events kept per open run. `0`
//!   disables the ring; a resume that missed events then reports `replay_gap`.
//! - `QUARRY_EDGE__TRACE_RING_MAX_BYTES` (default 1 MiB): estimated bytes per open run.
//!   `0` is refused as a value because it would drop every event: use the events knob.

use quarry_runtime::RingLimits;

pub const ENV_RING_EVENTS: &str = "QUARRY_EDGE__TRACE_RING_EVENTS";
pub const ENV_RING_MAX_BYTES: &str = "QUARRY_EDGE__TRACE_RING_MAX_BYTES";

pub const DEFAULT_RING_EVENTS: usize = 512;
pub const DEFAULT_RING_MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceConfig {
    pub ring: RingLimits,
}

impl TraceConfig {
    /// Read the two knobs from the process environment.
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Read the two knobs through `lookup` (tests pass a map, boot passes the environment).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let max_events = parse_count(
            ENV_RING_EVENTS,
            lookup(ENV_RING_EVENTS),
            DEFAULT_RING_EVENTS,
        )?;
        let max_bytes = parse_count(
            ENV_RING_MAX_BYTES,
            lookup(ENV_RING_MAX_BYTES),
            DEFAULT_RING_MAX_BYTES,
        )?;
        if max_bytes == 0 {
            return Err(format!(
                "{ENV_RING_MAX_BYTES} must be at least 1; use {ENV_RING_EVENTS}=0 to disable the ring"
            ));
        }
        Ok(Self {
            ring: RingLimits {
                max_events,
                max_bytes,
            },
        })
    }
}

// --- WS10.S5b: the Model Plane run-trace publisher -------------------------------
//
// - `QUARRY_EDGE__MODEL_TRACE_NATS_URL`: the Model NATS URL. Unset or empty (the
//   default) = no publisher, so the trace stays dark.
// - `QUARRY_EDGE__MODEL_TRACE_NATS_USER` (default `quarry-trace-producer`): the
//   publish-only principal.
// - `QUARRY_EDGE__MODEL_TRACE_NATS_PASSWORD`: required once the URL is set; a URL
//   without it aborts the boot. Never logged.
// - `QUARRY_EDGE__MODEL_TRACE_QUEUE` (default 2048, 1..=65536): the in-process queue
//   bound; a full queue drops and counts.
// - `QUARRY_EDGE__MODEL_TRACE_SUBJECT_PREFIX` (default `mp.v1.trace.run`): dot-separated
//   tokens; the subject is `<prefix>.<model_run_id>`.

pub const ENV_MODEL_TRACE_NATS_URL: &str = "QUARRY_EDGE__MODEL_TRACE_NATS_URL";
pub const ENV_MODEL_TRACE_NATS_USER: &str = "QUARRY_EDGE__MODEL_TRACE_NATS_USER";
pub const ENV_MODEL_TRACE_NATS_PASSWORD: &str = "QUARRY_EDGE__MODEL_TRACE_NATS_PASSWORD";
pub const ENV_MODEL_TRACE_QUEUE: &str = "QUARRY_EDGE__MODEL_TRACE_QUEUE";
pub const ENV_MODEL_TRACE_SUBJECT_PREFIX: &str = "QUARRY_EDGE__MODEL_TRACE_SUBJECT_PREFIX";

pub const DEFAULT_MODEL_TRACE_USER: &str = "quarry-trace-producer";
pub const DEFAULT_MODEL_TRACE_QUEUE: usize =
    quarry_runtime::model_trace_publisher::DEFAULT_QUEUE_CAP;
pub const MAX_MODEL_TRACE_QUEUE: usize = 65_536;

/// Reads the Model trace publisher's settings at boot.
pub struct ModelTraceEnv;

impl ModelTraceEnv {
    /// Read the publisher settings from the process environment. `Ok(None)` = off.
    pub fn from_env() -> Result<Option<quarry_runtime::ModelTraceConfig>, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Read the publisher settings through `lookup` (tests pass a map, boot passes the
    /// environment). An error names the variable and never carries the password.
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<quarry_runtime::ModelTraceConfig>, String> {
        let non_empty = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        let Some(url) = non_empty(ENV_MODEL_TRACE_NATS_URL) else {
            return Ok(None);
        };
        let user = non_empty(ENV_MODEL_TRACE_NATS_USER)
            .unwrap_or_else(|| DEFAULT_MODEL_TRACE_USER.to_owned());
        // The password is taken as given (not trimmed), but a blank one counts as unset.
        let password = lookup(ENV_MODEL_TRACE_NATS_PASSWORD)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                format!(
                    "{ENV_MODEL_TRACE_NATS_PASSWORD} must be set when {ENV_MODEL_TRACE_NATS_URL} is set; \
                     unset {ENV_MODEL_TRACE_NATS_URL} to turn the Model trace publisher off"
                )
            })?;
        let queue_cap = parse_count(
            ENV_MODEL_TRACE_QUEUE,
            lookup(ENV_MODEL_TRACE_QUEUE),
            DEFAULT_MODEL_TRACE_QUEUE,
        )?;
        if !(1..=MAX_MODEL_TRACE_QUEUE).contains(&queue_cap) {
            return Err(format!(
                "{ENV_MODEL_TRACE_QUEUE} must be between 1 and {MAX_MODEL_TRACE_QUEUE}"
            ));
        }
        let subject_prefix = non_empty(ENV_MODEL_TRACE_SUBJECT_PREFIX).unwrap_or_else(|| {
            quarry_runtime::model_trace_publisher::DEFAULT_SUBJECT_PREFIX.to_owned()
        });
        if !valid_subject_prefix(&subject_prefix) {
            return Err(format!(
                "{ENV_MODEL_TRACE_SUBJECT_PREFIX} must be dot-separated tokens of letters, digits, '-' or '_'"
            ));
        }
        let mut config =
            quarry_runtime::ModelTraceConfig::new(url).with_user_password(user, password);
        config.queue_cap = queue_cap;
        config.subject_prefix = subject_prefix;
        Ok(Some(config))
    }
}

/// Dot-separated, non-empty tokens with no wildcard, whitespace or other punctuation.
fn valid_subject_prefix(prefix: &str) -> bool {
    !prefix.is_empty()
        && prefix.split('.').all(|token| {
            !token.is_empty()
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

fn parse_count(name: &str, raw: Option<String>, default: usize) -> Result<usize, String> {
    match raw.map(|value| value.trim().to_owned()) {
        None => Ok(default),
        Some(value) if value.is_empty() => Ok(default),
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| format!("{name} must be a non-negative integer")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name: &str| {
            map.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn defaults_are_512_events_and_one_mebibyte() {
        let config = TraceConfig::from_lookup(lookup(&[])).unwrap();
        assert_eq!(config.ring.max_events, 512);
        assert_eq!(config.ring.max_bytes, 1024 * 1024);
    }

    #[test]
    fn zero_events_disables_the_ring_and_is_accepted() {
        let config =
            TraceConfig::from_lookup(lookup(&[(ENV_RING_EVENTS, "0")])).unwrap();
        assert_eq!(config.ring.max_events, 0);
    }

    #[test]
    fn zero_bytes_is_refused_and_names_the_events_knob() {
        let error =
            TraceConfig::from_lookup(lookup(&[(ENV_RING_MAX_BYTES, "0")])).unwrap_err();
        assert!(error.contains(ENV_RING_MAX_BYTES));
        assert!(error.contains(ENV_RING_EVENTS));
    }

    #[test]
    fn an_unparsable_value_aborts_and_names_the_variable() {
        let error =
            TraceConfig::from_lookup(lookup(&[(ENV_RING_EVENTS, "many")])).unwrap_err();
        assert!(error.starts_with(ENV_RING_EVENTS));
        let error =
            TraceConfig::from_lookup(lookup(&[(ENV_RING_MAX_BYTES, "-1")])).unwrap_err();
        assert!(error.starts_with(ENV_RING_MAX_BYTES));
    }

    #[test]
    fn a_blank_value_is_the_default() {
        let config = TraceConfig::from_lookup(lookup(&[
            (ENV_RING_EVENTS, "  "),
            (ENV_RING_MAX_BYTES, ""),
        ]))
        .unwrap();
        assert_eq!(config.ring.max_events, DEFAULT_RING_EVENTS);
        assert_eq!(config.ring.max_bytes, DEFAULT_RING_MAX_BYTES);
    }

    #[test]
    fn model_trace_is_off_without_a_url() {
        assert!(ModelTraceEnv::from_lookup(lookup(&[])).unwrap().is_none());
        assert!(
            ModelTraceEnv::from_lookup(lookup(&[(ENV_MODEL_TRACE_NATS_URL, " ")]))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn model_trace_url_without_a_password_aborts_without_echoing_secrets() {
        let error = ModelTraceEnv::from_lookup(lookup(&[(
            ENV_MODEL_TRACE_NATS_URL,
            "nats://model-nats:4222",
        )]))
        .unwrap_err();
        assert!(error.starts_with(ENV_MODEL_TRACE_NATS_PASSWORD));
    }

    #[test]
    fn model_trace_defaults_and_bounds() {
        let config = ModelTraceEnv::from_lookup(lookup(&[
            (ENV_MODEL_TRACE_NATS_URL, "nats://model-nats:4222"),
            (ENV_MODEL_TRACE_NATS_PASSWORD, "placeholder"),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(config.user.as_deref(), Some(DEFAULT_MODEL_TRACE_USER));
        assert_eq!(config.queue_cap, DEFAULT_MODEL_TRACE_QUEUE);
        assert_eq!(config.subject_prefix, "mp.v1.trace.run");
        for (name, value) in [
            (ENV_MODEL_TRACE_QUEUE, "0"),
            (ENV_MODEL_TRACE_QUEUE, "65537"),
            (ENV_MODEL_TRACE_QUEUE, "many"),
            (ENV_MODEL_TRACE_SUBJECT_PREFIX, "mp..trace"),
            (ENV_MODEL_TRACE_SUBJECT_PREFIX, "mp.v1.>"),
        ] {
            let error = ModelTraceEnv::from_lookup(lookup(&[
                (ENV_MODEL_TRACE_NATS_URL, "nats://model-nats:4222"),
                (ENV_MODEL_TRACE_NATS_PASSWORD, "placeholder"),
                (name, value),
            ]))
            .unwrap_err();
            assert!(error.starts_with(name), "{name}={value}: {error}");
        }
    }
}
