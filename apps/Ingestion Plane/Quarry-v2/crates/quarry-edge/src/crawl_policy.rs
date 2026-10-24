//! WS14.S3: the crawl scope switch and the scope the handoff stamps on a crawl.
//!
//! `QUARRY_EDGE__CRAWL_SCOPE_MODE` is one of:
//!
//! * `off`: the crawl runs as it did before scope existed. The handoff stamps
//!   `scope.legacy = true` and the orchestrator sends no `crawl` object to
//!   `run_page`. This is the rollback position.
//! * `observe` (the default): every page is scoped and counted, and nothing is
//!   dropped. `links` stay as sent; the report says what `enforce` would drop.
//! * `enforce`: the links that the scope refuses are removed before the
//!   orchestrator enqueues the children.
//!
//! The value is read once at boot. An unparsable value fails boot (the same
//! latch idiom as `limits::crawl_limits`), so a typo cannot silently turn the
//! scope off or on.

use std::sync::OnceLock;

use quarry_core::crawl_scope::{CrawlScope, RobotsMode, ScopeMode};

/// The environment name. Read by `main` at boot and by the handoff at request time.
pub const ENV_CRAWL_SCOPE_MODE: &str = "QUARRY_EDGE__CRAWL_SCOPE_MODE";

/// What the edge does with a crawl's scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrawlScopeSwitch {
    /// No scope. Stamps `legacy` and the orchestrator sends no context.
    Off,
    /// Scope and count; drop nothing. The default.
    #[default]
    Observe,
    /// Scope, count and drop.
    Enforce,
}

impl CrawlScopeSwitch {
    /// Parse the environment value. An absent or blank value is `observe`.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "observe" => Ok(Self::Observe),
            "off" => Ok(Self::Off),
            "enforce" => Ok(Self::Enforce),
            other => Err(format!(
                "{ENV_CRAWL_SCOPE_MODE} must be off, observe or enforce (got {} characters)",
                other.chars().count()
            )),
        }
    }

    /// Read the switch through a lookup, so tests need no process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        match lookup(ENV_CRAWL_SCOPE_MODE) {
            Some(value) => Self::parse(&value),
            None => Ok(Self::Observe),
        }
    }

    /// Read the switch from the process environment.
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// The label the boot log and the handoff echo use.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::Enforce => "enforce",
        }
    }

    /// The switch the process runs with, read once. Boot validates the
    /// environment first ([`CrawlScopeSwitch::from_env`] in `main`), so a value
    /// that reaches here was valid at start-up.
    pub fn current() -> Self {
        static SWITCH: OnceLock<CrawlScopeSwitch> = OnceLock::new();
        *SWITCH.get_or_init(|| Self::from_env().unwrap_or_default())
    }
}

/// The scope a handoff stamps into the orchestrator payload (`scope`,
/// `scope_mode`, `robots`) and echoes back as `HandoffAck.effective`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffScope {
    pub scope: CrawlScope,
    pub scope_mode: ScopeMode,
    pub robots: RobotsMode,
    /// True under `off`: the orchestrator treats the crawl as a legacy one.
    pub legacy: bool,
    /// Human-readable notes about the scope the caller asked for (for example
    /// that external links are followed). Empty for the default scope.
    pub warnings: Vec<&'static str>,
}

/// Build the stamped scope from the switch and the caller's two switches.
/// Defaults: same host only, external links off, subdomains off, the whole
/// domain, robots respected.
pub fn handoff_scope(
    switch: CrawlScopeSwitch,
    allow_subdomains: bool,
    allow_external_links: bool,
) -> HandoffScope {
    let mut warnings = Vec::new();
    if allow_subdomains {
        warnings.push("subdomains_followed");
    }
    if allow_external_links {
        warnings.push("external_links_followed");
    }
    let scope = CrawlScope {
        allow_subdomains,
        allow_external_links,
        legacy: matches!(switch, CrawlScopeSwitch::Off),
        ..CrawlScope::default()
    };
    HandoffScope {
        scope,
        scope_mode: match switch {
            CrawlScopeSwitch::Enforce => ScopeMode::Enforce,
            CrawlScopeSwitch::Observe | CrawlScopeSwitch::Off => ScopeMode::Observe,
        },
        robots: RobotsMode::Respect,
        legacy: matches!(switch, CrawlScopeSwitch::Off),
        warnings,
    }
}

impl HandoffScope {
    /// The wire name of the scope mode.
    pub fn scope_mode_label(&self) -> &'static str {
        match self.scope_mode {
            ScopeMode::Enforce => "enforce",
            ScopeMode::Observe => "observe",
        }
    }

    /// The wire name of the robots mode.
    pub fn robots_label(&self) -> &'static str {
        match self.robots {
            RobotsMode::Respect => "respect",
            RobotsMode::Ignore => "ignore",
        }
    }

    /// The `scope` object of the orchestrator payload and of `effective`.
    pub fn scope_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.scope).unwrap_or(serde_json::Value::Null)
    }

    /// The `effective` object of the handoff acknowledgement: the bounds the
    /// crawl runs with and the scope it runs under.
    pub fn effective(&self, max_pages: u32, max_depth: u32) -> serde_json::Value {
        serde_json::json!({
            "scope": self.scope_value(),
            "scope_mode": self.scope_mode_label(),
            "robots": self.robots_label(),
            "max_pages": max_pages,
            "max_depth": max_depth,
            "legacy": self.legacy,
            "warnings": self.warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(value: Option<&'static str>) -> impl Fn(&str) -> Option<String> {
        move |name: &str| {
            assert_eq!(name, ENV_CRAWL_SCOPE_MODE);
            value.map(str::to_owned)
        }
    }

    #[test]
    fn absent_and_blank_are_observe() {
        assert_eq!(CrawlScopeSwitch::from_lookup(lookup(None)), Ok(CrawlScopeSwitch::Observe));
        assert_eq!(CrawlScopeSwitch::from_lookup(lookup(Some("  "))), Ok(CrawlScopeSwitch::Observe));
    }

    #[test]
    fn three_values_parse_case_insensitively() {
        assert_eq!(CrawlScopeSwitch::parse("OFF"), Ok(CrawlScopeSwitch::Off));
        assert_eq!(CrawlScopeSwitch::parse(" observe "), Ok(CrawlScopeSwitch::Observe));
        assert_eq!(CrawlScopeSwitch::parse("Enforce"), Ok(CrawlScopeSwitch::Enforce));
    }

    #[test]
    fn unknown_value_fails_boot_and_does_not_echo_it() {
        let err = CrawlScopeSwitch::parse("enforced-ish-secretvalue").unwrap_err();
        assert!(err.contains(ENV_CRAWL_SCOPE_MODE));
        assert!(!err.contains("secretvalue"), "the refusal must not echo the value: {err}");
    }

    #[test]
    fn default_is_observe() {
        assert_eq!(CrawlScopeSwitch::default(), CrawlScopeSwitch::Observe);
    }

    #[test]
    fn off_stamps_legacy_and_observe_mode() {
        let h = handoff_scope(CrawlScopeSwitch::Off, false, false);
        assert!(h.legacy);
        assert!(h.scope.legacy);
        assert_eq!(h.scope_mode, ScopeMode::Observe);
    }

    #[test]
    fn observe_and_enforce_are_not_legacy() {
        let observe = handoff_scope(CrawlScopeSwitch::Observe, false, false);
        assert!(!observe.legacy);
        assert_eq!(observe.scope_mode, ScopeMode::Observe);
        let enforce = handoff_scope(CrawlScopeSwitch::Enforce, false, false);
        assert!(!enforce.legacy);
        assert_eq!(enforce.scope_mode, ScopeMode::Enforce);
    }

    #[test]
    fn caller_switches_pass_through_and_widen_warnings() {
        let h = handoff_scope(CrawlScopeSwitch::Enforce, true, true);
        assert!(h.scope.allow_subdomains);
        assert!(h.scope.allow_external_links);
        assert_eq!(h.warnings, vec!["subdomains_followed", "external_links_followed"]);
        let default = handoff_scope(CrawlScopeSwitch::Enforce, false, false);
        assert!(default.warnings.is_empty());
    }

    #[test]
    fn labels_carry_the_wire_names() {
        let h = handoff_scope(CrawlScopeSwitch::Enforce, false, false);
        assert_eq!(h.scope_mode_label(), "enforce");
        assert_eq!(h.robots_label(), "respect");
        assert_eq!(h.scope_value()["crawl_entire_domain"], true);
        assert_eq!(h.scope_value()["legacy"], false);
    }

    #[test]
    fn effective_echoes_the_resolved_bounds() {
        let e = handoff_scope(CrawlScopeSwitch::Observe, false, true).effective(50, 2);
        assert_eq!(e["max_pages"], 50);
        assert_eq!(e["max_depth"], 2);
        assert_eq!(e["scope_mode"], "observe");
        assert_eq!(e["legacy"], false);
        assert_eq!(e["warnings"][0], "external_links_followed");
    }

    #[test]
    fn label_matches_the_wire_value() {
        for s in [CrawlScopeSwitch::Off, CrawlScopeSwitch::Observe, CrawlScopeSwitch::Enforce] {
            assert_eq!(CrawlScopeSwitch::parse(s.label()), Ok(s));
        }
    }
}
