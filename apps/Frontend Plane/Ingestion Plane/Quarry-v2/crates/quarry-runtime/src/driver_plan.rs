use quarry_core::output::DriverKind;
use quarry_tls::TlsProfile;
use serde::{Deserialize, Serialize};

use crate::escalation::{escalation_allowed, EscalationMode, EscalationTrigger};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UrlType {
    Json,
    PlainText,
    #[default]
    Default,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverSignals {
    pub actions: Vec<String>,
    pub screenshot: bool,
    pub pdf: bool,
    /// Deprecated hint from the caller that earlier fetches of this URL were
    /// blocked. Under `QUARRY_BLOCK_ESCALATION=legacy` it raises the driver
    /// tier (1: TLS driver, 2 or more: browser); under `honest` it is accepted
    /// and ignored, because a block never selects a different identity.
    pub prior_block_signals: u8,
    pub profile_required: bool,
    pub url_type: UrlType,
}

impl Default for DriverSignals {
    fn default() -> Self {
        Self {
            actions: Vec::new(),
            screenshot: false,
            pdf: false,
            prior_block_signals: 0,
            profile_required: false,
            url_type: UrlType::Default,
        }
    }
}

pub fn plan_from_signals(signals: DriverSignals) -> DriverPlan {
    plan_from_signals_with(EscalationMode::current(), signals)
}

/// [`plan_from_signals`] with the escalation mode given, so a test never
/// depends on the process environment.
pub fn plan_from_signals_with(mode: EscalationMode, signals: DriverSignals) -> DriverPlan {
    // A prior-block hint is a caller-supplied reason to change identity; it only
    // counts where the mode allows it.
    let hinted = signals.prior_block_signals > 0
        && escalation_allowed(
            mode,
            EscalationTrigger::CallerSignals(signals.prior_block_signals),
        );
    let needs_browser = !signals.actions.is_empty()
        || signals.screenshot
        || signals.pdf
        || (hinted && signals.prior_block_signals >= 2)
        || signals.profile_required;

    if needs_browser {
        DriverPlan::browser_fetch("signals require browser")
    } else if hinted && signals.prior_block_signals == 1 {
        DriverPlan::tls_fetch(quarry_tls::TlsProfile::default(), "prior block signal")
    } else {
        DriverPlan::static_fetch("no signals")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverPlan {
    pub driver: DriverKind,
    pub fallback_chain: Vec<DriverKind>,
    pub tls_profile: Option<TlsProfile>,
    pub js_required: bool,
    pub reason: String,
}

impl DriverPlan {
    pub fn static_fetch(reason: impl Into<String>) -> Self {
        Self {
            driver: DriverKind::Static,
            fallback_chain: vec![DriverKind::Tls, DriverKind::Browser],
            tls_profile: None,
            js_required: false,
            reason: reason.into(),
        }
    }

    pub fn tls_fetch(profile: TlsProfile, reason: impl Into<String>) -> Self {
        Self {
            driver: DriverKind::Tls,
            fallback_chain: vec![DriverKind::Browser],
            tls_profile: Some(profile),
            js_required: false,
            reason: reason.into(),
        }
    }

    pub fn browser_fetch(reason: impl Into<String>) -> Self {
        Self {
            driver: DriverKind::Browser,
            fallback_chain: vec![],
            tls_profile: None,
            js_required: true,
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_prior(prior_block_signals: u8) -> DriverSignals {
        DriverSignals {
            prior_block_signals,
            ..DriverSignals::default()
        }
    }

    #[test]
    fn prior_block_signals_legacy_tiers_unchanged() {
        let none = plan_from_signals_with(EscalationMode::Legacy, with_prior(0));
        assert_eq!(none.driver, DriverKind::Static);
        let one = plan_from_signals_with(EscalationMode::Legacy, with_prior(1));
        assert_eq!(one.driver, DriverKind::Tls);
        assert_eq!(one.reason, "prior block signal");
        for n in [2u8, 3, 255] {
            let plan = plan_from_signals_with(EscalationMode::Legacy, with_prior(n));
            assert_eq!(plan.driver, DriverKind::Browser, "prior_block_signals {n}");
        }
    }

    #[test]
    fn prior_block_signals_ignored_in_honest_mode() {
        for n in [0u8, 1, 2, 3, 255] {
            let plan = plan_from_signals_with(EscalationMode::Honest, with_prior(n));
            assert_eq!(plan.driver, DriverKind::Static, "prior_block_signals {n}");
            assert_eq!(plan.reason, "no signals");
            assert!(plan.tls_profile.is_none());
            assert!(!plan.js_required);
        }
    }

    #[test]
    fn non_block_signals_still_choose_the_browser_in_both_modes() {
        for mode in [EscalationMode::Legacy, EscalationMode::Honest] {
            for signals in [
                DriverSignals {
                    screenshot: true,
                    ..DriverSignals::default()
                },
                DriverSignals {
                    pdf: true,
                    ..DriverSignals::default()
                },
                DriverSignals {
                    profile_required: true,
                    ..DriverSignals::default()
                },
                DriverSignals {
                    actions: vec!["click".into()],
                    ..DriverSignals::default()
                },
                // Honest ignores the hint but still honours the other signals.
                DriverSignals {
                    screenshot: true,
                    prior_block_signals: 2,
                    ..DriverSignals::default()
                },
            ] {
                let plan = plan_from_signals_with(mode, signals);
                assert_eq!(plan.driver, DriverKind::Browser, "{mode:?}");
            }
        }
    }
}
