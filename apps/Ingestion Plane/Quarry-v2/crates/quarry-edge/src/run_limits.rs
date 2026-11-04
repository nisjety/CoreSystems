//! Switches and bounds on live agent runs that the operator decides.
//!
//! Two kinds: how many runs may be live (global, per organization, and how many of
//! those may be a person's own browser sessions), and how long a run may sit idle or
//! live at all before the sweeper releases its browser (`RunLimits`); and whether an
//! agent action is refused while a person holds the browser
//! (`QUARRY_EDGE__TAKEOVER_ENFORCEMENT`).
//!
//! Every value is read once at boot. One that cannot be parsed stops the edge and
//! names its variable, rather than silently becoming "no limit".

use std::fmt;

pub const ENV_TAKEOVER_ENFORCEMENT: &str = "QUARRY_EDGE__TAKEOVER_ENFORCEMENT";

/// Parse the takeover-enforcement switch. Unset or empty is off (observe: log what
/// would be refused and refuse nothing). `1`/`true`/`on` enforce; `0`/`false`/`off`
/// observe; anything else is an error naming the variable, so a typo cannot
/// silently leave enforcement off.
pub fn parse_takeover_enforcement(raw: Option<&str>) -> Result<bool, String> {
    match raw.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("0") | Some("false") | Some("off") => Ok(false),
        Some("1") | Some("true") | Some("on") => Ok(true),
        Some(other) => Err(format!(
            "{ENV_TAKEOVER_ENFORCEMENT} must be 0, 1, on, off, true or false, got {other:?}"
        )),
    }
}

/// Whether an agent action is refused while a person holds the browser.
/// Off by default until the planner side maps the refusal (WS11); a value that
/// cannot be read is treated as off here and stops the edge at boot.
pub fn takeover_enforced() -> bool {
    parse_takeover_enforcement(std::env::var(ENV_TAKEOVER_ENFORCEMENT).ok().as_deref())
        .unwrap_or(false)
}

/// The switch for human-browse grants (WS8.B, D8-5). Off by default: a grant of
/// kind `human_browse` is refused while it is off. An unreadable value is off here
/// and stops the edge at boot (`RunLimits::from_lookup`).
pub const ENV_HUMAN_BROWSE: &str = "QUARRY_EDGE__HUMAN_BROWSE";

/// INNH-10: the switch for API-key agent runs. Off by default: an `api_key` principal token is
/// refused on EVERY route (`api_key_agent_runs_disabled`) while it is off, and the edge does not
/// boot with it on and no real grant validator (`grant_boot`). An unreadable value is off here
/// and stops the edge at boot (`RunLimits::from_lookup`).
pub const ENV_API_KEY_AGENT_RUNS: &str = "QUARRY_EDGE__API_KEY_AGENT_RUNS";

/// Parse the API-key switch: unset, empty, `0`, `false` or `off` is off; `1`, `true` or `on` is
/// on; anything else is an error naming the variable.
pub fn parse_api_key_agent_runs(raw: Option<&str>) -> Result<bool, String> {
    match raw.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("0") | Some("false") | Some("off") => Ok(false),
        Some("1") | Some("true") | Some("on") => Ok(true),
        Some(other) => Err(format!(
            "{ENV_API_KEY_AGENT_RUNS} must be 0, 1, on, off, true or false, got {other:?}"
        )),
    }
}

/// Whether API-key agent runs are accepted now. Read at each request, like
/// [`human_browse_enabled`], so a test or an operator changing the variable is seen by the
/// next request; unreadable is off.
pub fn api_key_agent_runs_enabled() -> bool {
    parse_api_key_agent_runs(std::env::var(ENV_API_KEY_AGENT_RUNS).ok().as_deref()).unwrap_or(false)
}

/// How a person's step is recorded on a non-ZDR run (WS8.B): `metadata` (the
/// default: redacted action, no observation), `full` (today's storage, for dev and
/// procedure recording) or `none` (no receipt). ZDR runs write nothing either way.
pub const ENV_HUMAN_RECEIPT_MODE: &str = "QUARRY_EDGE__HUMAN_RECEIPT_MODE";

/// Parse the human-browse switch: unset, empty, `0`, `false` or `off` is off;
/// `1`, `true` or `on` is on; anything else is an error naming the variable.
pub fn parse_human_browse(raw: Option<&str>) -> Result<bool, String> {
    match raw.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("0") | Some("false") | Some("off") => Ok(false),
        Some("1") | Some("true") | Some("on") => Ok(true),
        Some(other) => Err(format!(
            "{ENV_HUMAN_BROWSE} must be 0, 1, on, off, true or false, got {other:?}"
        )),
    }
}

/// The recording mode of a person's step (see `ENV_HUMAN_RECEIPT_MODE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanReceiptMode {
    Metadata,
    Full,
    None,
}

/// Parse the human receipt mode; unset or empty is `metadata`.
pub fn parse_human_receipt_mode(raw: Option<&str>) -> Result<HumanReceiptMode, String> {
    match raw.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("metadata") => Ok(HumanReceiptMode::Metadata),
        Some("full") => Ok(HumanReceiptMode::Full),
        Some("none") => Ok(HumanReceiptMode::None),
        Some(other) => Err(format!(
            "{ENV_HUMAN_RECEIPT_MODE} must be metadata, full or none, got {other:?}"
        )),
    }
}

/// The switch for agent attachments (WS8.F). Unset or `0` is off; anything else that
/// is not a boolean word is an error at boot (`RunLimits::from_lookup`). Read at each
/// start, so the next request sees a change; an unreadable value is off here.
pub const ENV_ATTACH: &str = "QUARRY_EDGE__ATTACH";

/// Parse the attach switch with the same words as the other switches.
pub fn parse_attach(raw: Option<&str>) -> Result<bool, String> {
    match raw.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("0") | Some("false") | Some("off") => Ok(false),
        Some("1") | Some("true") | Some("on") => Ok(true),
        Some(other) => Err(format!("{ENV_ATTACH} must be 0, 1, on, off, true or false, got {other:?}")),
    }
}

/// Whether an agent may attach to a live run now (WS8.F). Unreadable is off.
pub fn attach_enabled() -> bool {
    parse_attach(std::env::var(ENV_ATTACH).ok().as_deref()).unwrap_or(false)
}

/// The attachment idle limit now (WS8.F): 180 seconds unless the variable says otherwise.
pub fn attach_idle_ttl_s() -> u64 {
    std::env::var("QUARRY_EDGE__ATTACH_IDLE_TTL_S")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value >= 1)
        .unwrap_or(180)
}

/// Whether human-browse grants are accepted now. Read at each start, so a test or an
/// operator changing the variable is seen by the next start; unreadable is off.
pub fn human_browse_enabled() -> bool {
    parse_human_browse(std::env::var(ENV_HUMAN_BROWSE).ok().as_deref()).unwrap_or(false)
}

/// The receipt mode for a person's step now; unreadable is `metadata`.
pub fn human_receipt_mode() -> HumanReceiptMode {
    parse_human_receipt_mode(std::env::var(ENV_HUMAN_RECEIPT_MODE).ok().as_deref())
        .unwrap_or(HumanReceiptMode::Metadata)
}

/// Ceiling on an agent run's absolute lifetime, whatever it asks for.
pub const AGENT_MAX_TTL_CEILING_S: u64 = 3600;

/// Caps on live runs and the lifetimes the sweeper enforces. `0` for a cap means
/// "no cap".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLimits {
    /// Run the sweeper at all (`false` is the immediate escape).
    pub sweeper: bool,
    /// Log `would_expire` and release nothing. The first deploy runs this way.
    pub sweeper_dry_run: bool,
    pub sweep_interval_s: u64,
    /// An agent run with no activity for this long is released.
    pub agent_idle_ttl_s: u64,
    /// An agent run that did not say how long it needs lives at most this long;
    /// one that did lives its `max_runtime_s` plus the grace. Never above
    /// [`AGENT_MAX_TTL_CEILING_S`].
    pub agent_max_ttl_s: u64,
    pub agent_ttl_grace_s: u64,
    /// A person's browser session with no activity and no viewer for this long.
    pub interactive_idle_ttl_s: u64,
    /// Absolute backstop for a person's session; applies even with a viewer open.
    pub interactive_max_ttl_s: u64,
    /// Idle limit while a person holds the browser (they may be reading).
    pub takeover_idle_ttl_s: u64,
    pub max_concurrent_runs: u32,
    pub max_runs_per_org: u32,
    /// How many of an organization's runs may be a person's own sessions, so open
    /// tabs cannot crowd out its agent runs.
    pub max_interactive_runs_per_org: u32,
    /// INNH-10: how many runs one API key may have live (`QUARRY_EDGE__MAX_CONCURRENT_RUNS_PER_KEY`).
    /// Counted per key id inside the organization; at least 1, `0` (no cap) only for the
    /// unlimited registry of tests and tools.
    pub max_runs_per_key: u32,
    /// What a refused start tells the caller to wait.
    pub cap_retry_after_s: u64,
    /// WS8.F: whether an agent may attach to a person's live run (`QUARRY_EDGE__ATTACH`).
    /// Off in every environment until the WS8.H evidence; a value that does not parse fails at boot.
    pub attach_enabled: bool,
    /// WS8.F: an attachment with no activity for this long is released (`QUARRY_EDGE__ATTACH_IDLE_TTL_S`).
    pub attach_idle_ttl_s: u64,
}

impl Default for RunLimits {
    /// The documented defaults. Placeholders until Chromium memory is recorded at
    /// 8, 16 and 32 contexts.
    fn default() -> Self {
        Self {
            sweeper: true,
            sweeper_dry_run: false,
            sweep_interval_s: 15,
            agent_idle_ttl_s: 300,
            agent_max_ttl_s: 1800,
            agent_ttl_grace_s: 30,
            interactive_idle_ttl_s: 900,
            interactive_max_ttl_s: 14_400,
            takeover_idle_ttl_s: 1800,
            max_concurrent_runs: 32,
            max_runs_per_org: 8,
            max_interactive_runs_per_org: 6,
            max_runs_per_key: 2,
            cap_retry_after_s: 10,
            attach_enabled: false,
            attach_idle_ttl_s: 180,
        }
    }
}

impl RunLimits {
    /// No caps and no sweeper: what a registry built without limits (tests, tools)
    /// behaves like.
    pub fn unlimited() -> Self {
        Self {
            sweeper: false,
            max_concurrent_runs: 0,
            max_runs_per_org: 0,
            max_interactive_runs_per_org: 0,
            max_runs_per_key: 0,
            ..Self::default()
        }
    }

    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Read every value through `get` (the environment in production).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        // The human-browse switches fail loud at boot, like the bounds below.
        parse_human_browse(get(ENV_HUMAN_BROWSE).as_deref())?;
        parse_human_receipt_mode(get(ENV_HUMAN_RECEIPT_MODE).as_deref())?;
        parse_api_key_agent_runs(get(ENV_API_KEY_AGENT_RUNS).as_deref())?;
        let defaults = Self::default();
        let flag = |name: &str, default: bool| -> Result<bool, String> {
            let full = format!("QUARRY_EDGE__{name}");
            match get(&full).map(|v| v.trim().to_ascii_lowercase()).as_deref() {
                None | Some("") => Ok(default),
                Some("1") | Some("true") | Some("on") => Ok(true),
                Some("0") | Some("false") | Some("off") => Ok(false),
                Some(other) => Err(format!("{full} must be 0, 1, on, off, true or false, got {other:?}")),
            }
        };
        let number = |name: &str, default: u64, min: u64| -> Result<u64, String> {
            let full = format!("QUARRY_EDGE__{name}");
            match get(&full).map(|v| v.trim().to_owned()).as_deref() {
                None | Some("") => Ok(default),
                Some(raw) => {
                    let value: u64 = raw
                        .parse()
                        .map_err(|_| format!("{full} must be a whole number, got {raw:?}"))?;
                    if value < min {
                        return Err(format!("{full} must be at least {min}, got {value}"));
                    }
                    Ok(value)
                }
            }
        };
        let cap = |name: &str, default: u32| -> Result<u32, String> {
            let value = number(name, u64::from(default), 0)?;
            u32::try_from(value).map_err(|_| format!("QUARRY_EDGE__{name} is too large: {value}"))
        };
        let limits = Self {
            sweeper: flag("RUN_SWEEPER", defaults.sweeper)?,
            sweeper_dry_run: flag("RUN_SWEEPER_DRY_RUN", defaults.sweeper_dry_run)?,
            sweep_interval_s: number("RUN_SWEEP_INTERVAL_S", defaults.sweep_interval_s, 1)?,
            agent_idle_ttl_s: number("AGENT_RUN_IDLE_TTL_S", defaults.agent_idle_ttl_s, 1)?,
            agent_max_ttl_s: number("AGENT_RUN_MAX_TTL_S", defaults.agent_max_ttl_s, 1)?
                .min(AGENT_MAX_TTL_CEILING_S),
            agent_ttl_grace_s: number("AGENT_RUN_TTL_GRACE_S", defaults.agent_ttl_grace_s, 0)?,
            interactive_idle_ttl_s: number(
                "INTERACTIVE_RUN_IDLE_TTL_S",
                defaults.interactive_idle_ttl_s,
                1,
            )?,
            interactive_max_ttl_s: number(
                "INTERACTIVE_RUN_MAX_TTL_S",
                defaults.interactive_max_ttl_s,
                1,
            )?,
            takeover_idle_ttl_s: number("TAKEOVER_IDLE_TTL_S", defaults.takeover_idle_ttl_s, 1)?,
            max_concurrent_runs: cap("MAX_CONCURRENT_RUNS", defaults.max_concurrent_runs)?,
            max_runs_per_org: cap("MAX_CONCURRENT_RUNS_PER_ORG", defaults.max_runs_per_org)?,
            max_interactive_runs_per_org: cap(
                "MAX_INTERACTIVE_RUNS_PER_ORG",
                defaults.max_interactive_runs_per_org,
            )?,
            max_runs_per_key: {
                let value = number(
                    "MAX_CONCURRENT_RUNS_PER_KEY",
                    u64::from(defaults.max_runs_per_key),
                    1,
                )?;
                u32::try_from(value)
                    .map_err(|_| format!("QUARRY_EDGE__MAX_CONCURRENT_RUNS_PER_KEY is too large: {value}"))?
            },
            cap_retry_after_s: number("RUN_CAP_RETRY_AFTER_S", defaults.cap_retry_after_s, 1)?,
            attach_enabled: flag("ATTACH", defaults.attach_enabled)?,
            attach_idle_ttl_s: number("ATTACH_IDLE_TTL_S", defaults.attach_idle_ttl_s, 1)?,
        };
        Ok(limits)
    }

    /// Settings that contradict each other or the driver's own reaper. Logged as
    /// warnings until the numbers are final; `driver_idle_s` and `driver_max_age_s`
    /// are the browser driver's backstops, which must sit above every run limit.
    pub fn ordering_warnings(&self, driver_idle_s: u64, driver_max_age_s: u64) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.interactive_idle_ttl_s > self.interactive_max_ttl_s {
            warnings.push("INTERACTIVE_RUN_IDLE_TTL_S is longer than INTERACTIVE_RUN_MAX_TTL_S".into());
        }
        if self.takeover_idle_ttl_s > self.interactive_max_ttl_s {
            warnings.push("TAKEOVER_IDLE_TTL_S is longer than INTERACTIVE_RUN_MAX_TTL_S".into());
        }
        if self.agent_idle_ttl_s > self.agent_max_ttl_s {
            warnings.push("AGENT_RUN_IDLE_TTL_S is longer than AGENT_RUN_MAX_TTL_S".into());
        }
        let longest_run = self.interactive_max_ttl_s.max(self.agent_max_ttl_s);
        if driver_max_age_s <= longest_run {
            warnings.push(format!(
                "the browser driver's max session age ({driver_max_age_s} s) must exceed the longest run limit ({longest_run} s), or the driver reaps a run first"
            ));
        }
        let longest_idle = self
            .takeover_idle_ttl_s
            .max(self.interactive_idle_ttl_s)
            .max(self.agent_idle_ttl_s);
        if driver_idle_s <= longest_idle {
            warnings.push(format!(
                "the browser driver's idle limit ({driver_idle_s} s) must exceed the longest run idle limit ({longest_idle} s)"
            ));
        }
        warnings
    }
}

impl fmt::Display for RunLimits {
    /// One boot log line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "global={} org={} interactive={} sweeper={} dry_run={} interval_s={} agent_idle_s={} agent_max_s={} interactive_idle_s={} interactive_max_s={} takeover_idle_s={} per_key={}",
            self.max_concurrent_runs,
            self.max_runs_per_org,
            self.max_interactive_runs_per_org,
            if self.sweeper { "on" } else { "off" },
            if self.sweeper_dry_run { "on" } else { "off" },
            self.sweep_interval_s,
            self.agent_idle_ttl_s,
            self.agent_max_ttl_s,
            self.interactive_idle_ttl_s,
            self.interactive_max_ttl_s,
            self.takeover_idle_ttl_s,
            self.max_runs_per_key,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn from(pairs: &[(&str, &str)]) -> Result<RunLimits, String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (format!("QUARRY_EDGE__{k}"), (*v).to_owned()))
            .collect();
        RunLimits::from_lookup(|name| map.get(name).cloned())
    }

    #[test]
    fn takeover_enforcement_is_off_unless_asked() {
        assert_eq!(parse_takeover_enforcement(None), Ok(false));
        assert_eq!(parse_takeover_enforcement(Some("")), Ok(false));
        assert_eq!(parse_takeover_enforcement(Some("0")), Ok(false));
        assert_eq!(parse_takeover_enforcement(Some("OFF")), Ok(false));
        assert_eq!(parse_takeover_enforcement(Some("1")), Ok(true));
        assert_eq!(parse_takeover_enforcement(Some(" On ")), Ok(true));
    }

    #[test]
    fn a_misspelled_switch_is_an_error_naming_the_variable() {
        let error = parse_takeover_enforcement(Some("enforce")).unwrap_err();
        assert!(error.contains(ENV_TAKEOVER_ENFORCEMENT), "{error}");
    }

    #[test]
    fn defaults_match_the_documented_table() {
        let limits = from(&[]).unwrap();
        assert_eq!(limits, RunLimits::default());
        assert!(limits.sweeper);
        assert!(!limits.sweeper_dry_run);
        assert_eq!(limits.sweep_interval_s, 15);
        assert_eq!(limits.agent_idle_ttl_s, 300);
        assert_eq!(limits.agent_max_ttl_s, 1800);
        assert_eq!(limits.agent_ttl_grace_s, 30);
        assert_eq!(limits.interactive_idle_ttl_s, 900);
        assert_eq!(limits.interactive_max_ttl_s, 14_400);
        assert_eq!(limits.takeover_idle_ttl_s, 1800);
        assert_eq!(limits.max_concurrent_runs, 32);
        assert_eq!(limits.max_runs_per_org, 8);
        assert_eq!(limits.max_interactive_runs_per_org, 6);
        assert_eq!(limits.cap_retry_after_s, 10);
        assert_eq!(limits.max_runs_per_key, 2);
        assert!(limits.ordering_warnings(3600, 21_600).is_empty());
    }

    #[test]
    fn from_env_rejects_unparsable_and_names_the_variable() {
        for (name, value) in [
            ("MAX_CONCURRENT_RUNS", "many"),
            ("RUN_SWEEP_INTERVAL_S", "0"),
            ("AGENT_RUN_IDLE_TTL_S", "-3"),
            ("RUN_SWEEPER", "maybe"),
            ("TAKEOVER_IDLE_TTL_S", "1.5"),
        ] {
            let error = from(&[(name, value)]).unwrap_err();
            assert!(error.contains(&format!("QUARRY_EDGE__{name}")), "{error}");
        }
    }

    #[test]
    fn zero_disables_a_cap_and_the_agent_ceiling_holds() {
        let limits = from(&[
            ("MAX_CONCURRENT_RUNS", "0"),
            ("MAX_CONCURRENT_RUNS_PER_ORG", "0"),
            ("AGENT_RUN_MAX_TTL_S", "99999"),
        ])
        .unwrap();
        assert_eq!(limits.max_concurrent_runs, 0);
        assert_eq!(limits.max_runs_per_org, 0);
        assert_eq!(limits.agent_max_ttl_s, AGENT_MAX_TTL_CEILING_S);
    }

    #[test]
    fn ordering_warnings_flag_a_driver_that_would_reap_first() {
        let limits = RunLimits::default();
        assert_eq!(limits.ordering_warnings(3600, 21_600).len(), 0);
        let warnings = limits.ordering_warnings(600, 7200);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }

    #[test]
    fn api_key_switch_is_off_unless_asked_and_a_typo_stops_the_edge() {
        assert_eq!(parse_api_key_agent_runs(None), Ok(false));
        assert_eq!(parse_api_key_agent_runs(Some("")), Ok(false));
        assert_eq!(parse_api_key_agent_runs(Some("0")), Ok(false));
        assert_eq!(parse_api_key_agent_runs(Some("False")), Ok(false));
        assert_eq!(parse_api_key_agent_runs(Some("off")), Ok(false));
        assert_eq!(parse_api_key_agent_runs(Some("1")), Ok(true));
        assert_eq!(parse_api_key_agent_runs(Some(" TRUE ")), Ok(true));
        assert_eq!(parse_api_key_agent_runs(Some("on")), Ok(true));
        let error = parse_api_key_agent_runs(Some("enabled")).unwrap_err();
        assert!(error.contains(ENV_API_KEY_AGENT_RUNS), "{error}");
        // The edge stops at boot on a value it cannot read, like the other switches.
        let error = from(&[("API_KEY_AGENT_RUNS", "maybe")]).unwrap_err();
        assert!(error.contains(ENV_API_KEY_AGENT_RUNS), "{error}");
        assert!(from(&[("API_KEY_AGENT_RUNS", "1")]).is_ok());
    }

    #[test]
    fn per_key_cap_defaults_to_two_and_is_at_least_one() {
        assert_eq!(from(&[]).unwrap().max_runs_per_key, 2);
        assert_eq!(from(&[("MAX_CONCURRENT_RUNS_PER_KEY", "5")]).unwrap().max_runs_per_key, 5);
        let error = from(&[("MAX_CONCURRENT_RUNS_PER_KEY", "0")]).unwrap_err();
        assert!(error.contains("QUARRY_EDGE__MAX_CONCURRENT_RUNS_PER_KEY"), "{error}");
        let error = from(&[("MAX_CONCURRENT_RUNS_PER_KEY", "many")]).unwrap_err();
        assert!(error.contains("QUARRY_EDGE__MAX_CONCURRENT_RUNS_PER_KEY"), "{error}");
        assert_eq!(RunLimits::unlimited().max_runs_per_key, 0);
    }

    #[test]
    fn boot_line_names_the_effective_numbers() {
        let line = RunLimits::default().to_string();
        assert!(line.contains("global=32 org=8 interactive=6 sweeper=on dry_run=off"), "{line}");
    }
}
