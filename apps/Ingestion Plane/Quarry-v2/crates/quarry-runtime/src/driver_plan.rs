use quarry_core::output::DriverKind;
use quarry_tls::TlsProfile;
use serde::{Deserialize, Serialize};

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
    let needs_browser = !signals.actions.is_empty()
        || signals.screenshot
        || signals.pdf
        || signals.prior_block_signals >= 2
        || signals.profile_required;

    if needs_browser {
        DriverPlan::browser_fetch("signals require browser")
    } else if signals.prior_block_signals == 1 {
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
    pub stealth_level: u8,
    pub js_required: bool,
    pub reason: String,
}

impl DriverPlan {
    pub fn static_fetch(reason: impl Into<String>) -> Self {
        Self {
            driver: DriverKind::Static,
            fallback_chain: vec![DriverKind::Tls, DriverKind::Browser],
            tls_profile: None,
            stealth_level: 0,
            js_required: false,
            reason: reason.into(),
        }
    }

    pub fn tls_fetch(profile: TlsProfile, reason: impl Into<String>) -> Self {
        Self {
            driver: DriverKind::Tls,
            fallback_chain: vec![DriverKind::Browser],
            tls_profile: Some(profile),
            stealth_level: 1,
            js_required: false,
            reason: reason.into(),
        }
    }

    pub fn browser_fetch(reason: impl Into<String>) -> Self {
        Self {
            driver: DriverKind::Browser,
            fallback_chain: vec![],
            tls_profile: None,
            stealth_level: 2,
            js_required: true,
            reason: reason.into(),
        }
    }
}
