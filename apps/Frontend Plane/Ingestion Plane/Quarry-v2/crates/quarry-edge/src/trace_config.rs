//! Replay ring bounds for the agent event trace (WS10.S3).
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
}
