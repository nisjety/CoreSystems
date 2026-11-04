//! Which browser runs get the LLM planner (WS11.S2).
//!
//! The planner used to be switched by one process-wide variable
//! (`QUARRY_BROWSER_AGENT_LLM`). This module replaces that with a deployment
//! mode and an org flag:
//!
//! - `QUARRY_BROWSER_AGENT_PLANNER_MODE`: `off` (the default), `allowlist`
//!   (only the orgs in `QUARRY_BROWSER_AGENT_LLM_ORGS`), `org_opt_in` (an org
//!   that switched the planner on) or `default_on` (every org that has not
//!   switched it off).
//! - the org flag: model-gateway reads the org's quota row and writes
//!   `tool_input.planner_flag` (`on`, `off`, `unavailable`; absent = unset).
//!
//! Two rules do not bend. A ZDR run never selects the planner, and an org that
//! switched the planner off never gets it, whatever the mode or the legacy
//! variable says. When the flag could not be read, the modes that need it
//! (`org_opt_in`, `default_on`) pick the deterministic loop and say why.
//!
//! The legacy variable still works (it selects the planner for every org, as it
//! always did), logs one deprecation warning per process, and goes away at
//! rollout stage 4.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

/// `off`, `allowlist`, `org_opt_in` or `default_on`.
pub const MODE_ENV: &str = "QUARRY_BROWSER_AGENT_PLANNER_MODE";
/// Comma-separated org ids for `allowlist`.
pub const LLM_ORGS_ENV: &str = "QUARRY_BROWSER_AGENT_LLM_ORGS";
/// The legacy all-orgs switch (deprecated).
pub const LEGACY_ENV: &str = "QUARRY_BROWSER_AGENT_LLM";
/// The boot guard for `default_on`: a comma-separated list that must hold every
/// one of [`DEFAULT_ON_ACK_ITEMS`]. It is a ritual, not proof; the proof is in
/// the rollout runbook.
pub const DEFAULT_ON_ACK_ENV: &str = "QUARRY_BROWSER_AGENT_DEFAULT_ON_ACK";
/// What an operator acknowledges before `default_on` may take effect: browser
/// grants in production, run budgets, secret redaction at capture, and the
/// evaluation tiers.
pub const DEFAULT_ON_ACK_ITEMS: [&str; 4] = ["grants", "budgets", "redaction", "evals"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlannerMode {
    /// No org gets the planner. The default.
    #[default]
    Off,
    /// Only the orgs named in `QUARRY_BROWSER_AGENT_LLM_ORGS`.
    Allowlist,
    /// Orgs whose flag is `on`.
    OrgOptIn,
    /// Every org whose flag is not `off`.
    DefaultOn,
}

impl PlannerMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Allowlist => "allowlist",
            Self::OrgOptIn => "org_opt_in",
            Self::DefaultOn => "default_on",
        }
    }

    /// Case and `-` versus `_` do not matter; anything unrecognised is `off`
    /// (a typo never turns the planner on).
    #[must_use]
    pub fn from_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "allowlist" => Self::Allowlist,
            "org_opt_in" => Self::OrgOptIn,
            "default_on" => Self::DefaultOn,
            _ => Self::Off,
        }
    }
}

/// What the org's planner switch says (`tool_input.planner_flag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrgPlannerFlag {
    #[default]
    Unset,
    On,
    Off,
    /// The gateway could not read the org's switch.
    Unavailable,
}

impl OrgPlannerFlag {
    /// `on`, `off`, `unavailable`; anything else, or absent, is `Unset`.
    #[must_use]
    pub fn from_wire(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("on") => Self::On,
            Some("off") => Self::Off,
            Some("unavailable") => Self::Unavailable,
            _ => Self::Unset,
        }
    }
}

/// Why a run uses the deterministic loop. A closed list; it is logged, shown in
/// the run's usage line and used as a metric label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeterministicReason {
    /// The deployment mode is `off`.
    ModeOff,
    /// A ZDR run never selects the planner.
    Zdr,
    /// The org switched the planner off.
    OrgOff,
    /// `allowlist` and the org is not listed.
    NotAllowlisted,
    /// `org_opt_in` and the org has not switched it on.
    OrgNotOptedIn,
    /// The org's switch could not be read and the mode needs it.
    OrgPolicyUnavailable,
}

impl DeterministicReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModeOff => "mode_off",
            Self::Zdr => "zdr",
            Self::OrgOff => "org_off",
            Self::NotAllowlisted => "not_allowlisted",
            Self::OrgNotOptedIn => "org_not_opted_in",
            Self::OrgPolicyUnavailable => "org_policy_unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannerDecision {
    Llm,
    Deterministic(DeterministicReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannerPolicy {
    /// The EFFECTIVE mode: what the boot guard left of the requested one.
    pub mode: PlannerMode,
    pub allow_orgs: BTreeSet<String>,
    /// `QUARRY_BROWSER_AGENT_LLM` was set: the planner for every org.
    pub legacy_all: bool,
    /// `default_on` was requested without the full acknowledgement, so the
    /// mode in force is `org_opt_in`.
    pub downgraded_from_default_on: bool,
}

/// Whether `value` (the `QUARRY_BROWSER_AGENT_DEFAULT_ON_ACK` list) names every
/// required item. Case, spaces and order do not matter; extra items are fine.
#[must_use]
pub fn default_on_ack_complete(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let given: BTreeSet<String> = value
        .split(',')
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .collect();
    DEFAULT_ON_ACK_ITEMS.iter().all(|item| given.contains(*item))
}

/// What the startup banner says about the limits in force. Numbers and names
/// only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BannerFacts {
    pub procedure: String,
    pub widened_actions: bool,
    pub default_steps: i32,
    pub ceiling_steps: i32,
    pub default_runtime_s: i32,
    pub ceiling_runtime_s: i32,
    pub planner_tokens_default: u64,
    pub planner_tokens_ceiling: u64,
    pub max_consecutive_failures: u32,
    pub max_total_failures: u32,
}

static LEGACY_WARNED: AtomicBool = AtomicBool::new(false);
static DOWNGRADE_LOGGED: AtomicBool = AtomicBool::new(false);

/// `true` the first time it is called in this process (the one error line for
/// a `default_on` request that the boot guard downgraded).
pub fn note_default_on_downgrade() -> bool {
    !DOWNGRADE_LOGGED.swap(true, Ordering::SeqCst)
}

/// `true` the first time it is called in this process, `false` after. The one
/// deprecation warning is logged on the `true`.
pub fn note_legacy_use() -> bool {
    !LEGACY_WARNED.swap(true, Ordering::SeqCst)
}

impl PlannerPolicy {
    /// Read the three variables from the process environment, per call. Using
    /// the legacy variable logs one deprecation warning per process.
    #[must_use]
    pub fn from_env() -> Self {
        let policy = Self::from_lookup(|name| std::env::var(name).ok());
        if policy.downgraded_from_default_on && note_default_on_downgrade() {
            tracing::error!(
                "QUARRY_BROWSER_AGENT_PLANNER_MODE=default_on needs QUARRY_BROWSER_AGENT_DEFAULT_ON_ACK \
                 to name grants, budgets, redaction and evals; running org_opt_in instead"
            );
        }
        if policy.legacy_all && note_legacy_use() {
            tracing::warn!(
                "QUARRY_BROWSER_AGENT_LLM is deprecated and selects the planner for EVERY org; \
                 use QUARRY_BROWSER_AGENT_PLANNER_MODE (allowlist, org_opt_in, default_on)"
            );
        }
        policy
    }

    #[must_use]
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let requested = lookup(MODE_ENV)
            .map(|v| PlannerMode::from_value(&v))
            .unwrap_or_default();
        // Boot guard: `default_on` takes effect only with the full acknowledgement.
        let downgraded_from_default_on = requested == PlannerMode::DefaultOn
            && !default_on_ack_complete(lookup(DEFAULT_ON_ACK_ENV).as_deref());
        let mode = if downgraded_from_default_on {
            PlannerMode::OrgOptIn
        } else {
            requested
        };
        let allow_orgs = lookup(LLM_ORGS_ENV)
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|org| !org.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let legacy_all = lookup(LEGACY_ENV)
            .is_some_and(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes"));
        Self {
            mode,
            allow_orgs,
            legacy_all,
            downgraded_from_default_on,
        }
    }

    /// The one-line startup banner: the mode in force, how big the allowlist
    /// is (never who is on it), the procedure and the limits. No org ids, no
    /// credentials, no environment values.
    #[must_use]
    pub fn banner(&self, facts: &BannerFacts) -> String {
        format!(
            "browser planner: mode={} downgraded_from_default_on={} allowlist_size={} legacy_all_orgs_env={} \
             procedure={} widened_actions={} steps={}/{} runtime_s={}/{} planner_tokens={}/{} failures={}/{}",
            self.mode.as_str(),
            self.downgraded_from_default_on,
            self.allow_orgs.len(),
            self.legacy_all,
            facts.procedure,
            facts.widened_actions,
            facts.default_steps,
            facts.ceiling_steps,
            facts.default_runtime_s,
            facts.ceiling_runtime_s,
            facts.planner_tokens_default,
            facts.planner_tokens_ceiling,
            facts.max_consecutive_failures,
            facts.max_total_failures,
        )
    }

    /// Decide for one run. Order: a deployment with the planner off (and no
    /// legacy variable) says `mode_off` first, so the default deployment stays
    /// quiet; then ZDR; then the org's own `off`; then the mode.
    #[must_use]
    pub fn resolve(&self, org_id: &str, zdr: bool, flag: OrgPlannerFlag) -> PlannerDecision {
        use DeterministicReason as R;
        if self.mode == PlannerMode::Off && !self.legacy_all {
            return PlannerDecision::Deterministic(R::ModeOff);
        }
        if zdr {
            return PlannerDecision::Deterministic(R::Zdr);
        }
        if flag == OrgPlannerFlag::Off {
            return PlannerDecision::Deterministic(R::OrgOff);
        }
        if self.legacy_all {
            return PlannerDecision::Llm;
        }
        match self.mode {
            PlannerMode::Off => PlannerDecision::Deterministic(R::ModeOff),
            PlannerMode::Allowlist => {
                if self.allow_orgs.contains(org_id.trim()) {
                    PlannerDecision::Llm
                } else {
                    PlannerDecision::Deterministic(R::NotAllowlisted)
                }
            }
            PlannerMode::OrgOptIn => match flag {
                OrgPlannerFlag::On => PlannerDecision::Llm,
                OrgPlannerFlag::Unavailable => {
                    PlannerDecision::Deterministic(R::OrgPolicyUnavailable)
                }
                _ => PlannerDecision::Deterministic(R::OrgNotOptedIn),
            },
            PlannerMode::DefaultOn => match flag {
                OrgPlannerFlag::Unavailable => {
                    PlannerDecision::Deterministic(R::OrgPolicyUnavailable)
                }
                _ => PlannerDecision::Llm,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use DeterministicReason as R;
    use OrgPlannerFlag as F;

    fn policy(mode: PlannerMode, orgs: &[&str], legacy_all: bool) -> PlannerPolicy {
        PlannerPolicy {
            mode,
            allow_orgs: orgs.iter().map(|o| (*o).to_owned()).collect(),
            legacy_all,
            downgraded_from_default_on: false,
        }
    }

    const LLM: PlannerDecision = PlannerDecision::Llm;
    fn det(reason: DeterministicReason) -> PlannerDecision {
        PlannerDecision::Deterministic(reason)
    }

    #[test]
    fn planner_policy_resolves_each_mode_flag_zdr_combination() {
        // (mode, listed org?, flag, zdr) -> decision, for every combination.
        let flags = [F::Unset, F::On, F::Off, F::Unavailable];
        for zdr in [false, true] {
            for listed in [false, true] {
                let orgs: &[&str] = if listed { &["org_a"] } else { &["org_z"] };
                for flag in flags {
                    let at = |mode: PlannerMode| policy(mode, orgs, false).resolve("org_a", zdr, flag);
                    // off: always deterministic, whatever else is true.
                    assert_eq!(at(PlannerMode::Off), det(R::ModeOff), "off {zdr} {listed} {flag:?}");
                    // Every other mode: ZDR, then org off, then the mode's own rule.
                    let guarded = |mode_decision: PlannerDecision| {
                        if zdr {
                            det(R::Zdr)
                        } else if flag == F::Off {
                            det(R::OrgOff)
                        } else {
                            mode_decision
                        }
                    };
                    assert_eq!(
                        at(PlannerMode::Allowlist),
                        guarded(if listed { LLM } else { det(R::NotAllowlisted) }),
                        "allowlist {zdr} {listed} {flag:?}"
                    );
                    assert_eq!(
                        at(PlannerMode::OrgOptIn),
                        guarded(match flag {
                            F::On => LLM,
                            F::Unavailable => det(R::OrgPolicyUnavailable),
                            _ => det(R::OrgNotOptedIn),
                        }),
                        "org_opt_in {zdr} {listed} {flag:?}"
                    );
                    assert_eq!(
                        at(PlannerMode::DefaultOn),
                        guarded(if flag == F::Unavailable {
                            det(R::OrgPolicyUnavailable)
                        } else {
                            LLM
                        }),
                        "default_on {zdr} {listed} {flag:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn org_off_flag_beats_allowlist_and_default_on() {
        let listed = policy(PlannerMode::Allowlist, &["org_a"], false);
        assert_eq!(listed.resolve("org_a", false, F::Unset), LLM);
        assert_eq!(listed.resolve("org_a", false, F::Off), det(R::OrgOff));
        let all = policy(PlannerMode::DefaultOn, &[], false);
        assert_eq!(all.resolve("org_a", false, F::Off), det(R::OrgOff));
        // The legacy variable does not beat an org that said no.
        let legacy = policy(PlannerMode::Off, &[], true);
        assert_eq!(legacy.resolve("org_a", false, F::Off), det(R::OrgOff));
    }

    #[test]
    fn zdr_never_selects_the_planner() {
        for mode in [
            PlannerMode::Off,
            PlannerMode::Allowlist,
            PlannerMode::OrgOptIn,
            PlannerMode::DefaultOn,
        ] {
            for legacy_all in [false, true] {
                for flag in [F::Unset, F::On, F::Off, F::Unavailable] {
                    let decision = policy(mode, &["org_a"], legacy_all).resolve("org_a", true, flag);
                    assert!(
                        matches!(decision, PlannerDecision::Deterministic(_)),
                        "{mode:?} legacy={legacy_all} {flag:?} selected the planner for ZDR"
                    );
                }
            }
        }
        let on = policy(PlannerMode::DefaultOn, &[], false);
        assert_eq!(on.resolve("org_a", true, F::On), det(R::Zdr));
    }

    #[test]
    fn legacy_env_selects_all_orgs_and_warns_once() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        let legacy = PlannerPolicy::from_lookup(env(&[(LEGACY_ENV, "1")]));
        assert!(legacy.legacy_all);
        assert_eq!(legacy.mode, PlannerMode::Off, "the legacy variable leaves the mode alone");
        for org in ["org_a", "org_b", "org_c"] {
            assert_eq!(legacy.resolve(org, false, F::Unset), LLM, "{org}");
            assert_eq!(legacy.resolve(org, false, F::Unavailable), LLM, "{org}");
        }
        for off_spelling in ["0", "false", "no", ""] {
            let lookup = move |name: &str| (name == LEGACY_ENV).then(|| off_spelling.to_owned());
            assert!(!PlannerPolicy::from_lookup(lookup).legacy_all, "{off_spelling:?}");
        }
        // One warning per process, however many runs read the variable.
        let first = note_legacy_use();
        let second = note_legacy_use();
        let third = note_legacy_use();
        assert!(!second && !third, "only the first call may warn");
        // (`first` may already be false if another test in this process warned.)
        let _ = first;
    }

    #[test]
    fn mode_and_allowlist_come_from_the_environment_and_typos_stay_off() {
        let lookup = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(PlannerPolicy::from_lookup(lookup(&[])), PlannerPolicy::default());
        let set = PlannerPolicy::from_lookup(lookup(&[
            (MODE_ENV, " Org-Opt-In "),
            (LLM_ORGS_ENV, "org_a, org_b ,,org_c"),
        ]));
        assert_eq!(set.mode, PlannerMode::OrgOptIn);
        assert_eq!(set.allow_orgs.len(), 3);
        assert!(set.allow_orgs.contains("org_b"));
        for typo in ["on", "default-on-please", "ALLOW", "1"] {
            assert_eq!(PlannerMode::from_value(typo), PlannerMode::Off, "{typo}");
        }
        assert_eq!(PlannerMode::from_value("default_on"), PlannerMode::DefaultOn);
        assert_eq!(PlannerMode::from_value("allowlist"), PlannerMode::Allowlist);
    }

    fn lookup_of(
        pairs: &'static [(&'static str, &'static str)],
    ) -> impl Fn(&str) -> Option<String> {
        move |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn default_on_without_ack_downgrades_to_org_opt_in() {
        for ack in [None, Some(""), Some("grants"), Some("grants,budgets,redaction"), Some("yes")] {
            let lookup = move |name: &str| match name {
                MODE_ENV => Some("default_on".to_owned()),
                DEFAULT_ON_ACK_ENV => ack.map(str::to_owned),
                _ => None,
            };
            let policy = PlannerPolicy::from_lookup(lookup);
            assert_eq!(policy.mode, PlannerMode::OrgOptIn, "{ack:?}");
            assert!(policy.downgraded_from_default_on, "{ack:?}");
            // The downgrade is real: an org that never opted in gets nothing.
            assert_eq!(
                policy.resolve("org_a", false, F::Unset),
                det(R::OrgNotOptedIn),
                "{ack:?}"
            );
            assert_eq!(policy.resolve("org_a", false, F::On), LLM);
        }
        // Other modes are not touched by the guard.
        let allow = PlannerPolicy::from_lookup(lookup_of(&[(MODE_ENV, "allowlist")]));
        assert_eq!(allow.mode, PlannerMode::Allowlist);
        assert!(!allow.downgraded_from_default_on);
    }

    #[test]
    fn default_on_with_full_ack_stays_default_on() {
        for ack in [
            "grants,budgets,redaction,evals",
            " Evals , GRANTS,redaction ,budgets ",
            "grants,budgets,redaction,evals,extra",
        ] {
            let owned = ack.to_owned();
            let lookup = move |name: &str| match name {
                MODE_ENV => Some("default_on".to_owned()),
                DEFAULT_ON_ACK_ENV => Some(owned.clone()),
                _ => None,
            };
            let policy = PlannerPolicy::from_lookup(lookup);
            assert_eq!(policy.mode, PlannerMode::DefaultOn, "{ack:?}");
            assert!(!policy.downgraded_from_default_on);
            assert_eq!(policy.resolve("org_a", false, F::Unset), LLM);
        }
        assert!(!default_on_ack_complete(None));
        assert!(default_on_ack_complete(Some("grants,budgets,redaction,evals")));
    }

    #[test]
    fn banner_never_logs_org_ids_or_secrets() {
        let lookup = lookup_of(&[
            (MODE_ENV, "allowlist"),
            (LLM_ORGS_ENV, "org_acme_corp,org_secret_name"),
            (DEFAULT_ON_ACK_ENV, "grants,budgets,redaction,evals"),
            ("INFERENCE_CORE_URL", "https://user:hunter2@inference.internal"),
        ]);
        let policy = PlannerPolicy::from_lookup(lookup);
        let facts = BannerFacts {
            procedure: "browse/1".to_owned(),
            widened_actions: false,
            default_steps: 20,
            ceiling_steps: 100,
            default_runtime_s: 120,
            ceiling_runtime_s: 900,
            planner_tokens_default: 200_000,
            planner_tokens_ceiling: 1_000_000,
            max_consecutive_failures: 3,
            max_total_failures: 8,
        };
        let banner = policy.banner(&facts);
        assert!(!banner.contains('\n'), "one line: {banner}");
        for forbidden in ["org_acme_corp", "org_secret_name", "hunter2", "inference.internal", "http"] {
            assert!(!banner.contains(forbidden), "{forbidden} in {banner}");
        }
        assert!(banner.contains("mode=allowlist"));
        assert!(banner.contains("allowlist_size=2"));
        assert!(banner.contains("procedure=browse/1"));
        assert!(banner.contains("steps=20/100"));
        assert!(banner.contains("planner_tokens=200000/1000000"));
    }

    #[test]
    fn the_planner_flag_reads_the_gateways_three_values() {
        assert_eq!(OrgPlannerFlag::from_wire(None), F::Unset);
        assert_eq!(OrgPlannerFlag::from_wire(Some("on")), F::On);
        assert_eq!(OrgPlannerFlag::from_wire(Some(" OFF ")), F::Off);
        assert_eq!(OrgPlannerFlag::from_wire(Some("unavailable")), F::Unavailable);
        assert_eq!(OrgPlannerFlag::from_wire(Some("1")), F::Unset);
        assert_eq!(OrgPlannerFlag::from_wire(Some("true")), F::Unset);
    }

    #[test]
    fn reasons_are_a_closed_low_cardinality_list() {
        let all = [
            R::ModeOff,
            R::Zdr,
            R::OrgOff,
            R::NotAllowlisted,
            R::OrgNotOptedIn,
            R::OrgPolicyUnavailable,
        ];
        let labels: BTreeSet<&str> = all.iter().map(|r| r.as_str()).collect();
        assert_eq!(labels.len(), all.len());
        assert!(labels.iter().all(|l| l.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')));
    }
}
