//! The v1 web-policy catalogue: every capability key, its kind, who it binds,
//! its baseline and the site that consumes it.
//!
//! The table [`CATALOGUE`] is the source of truth. `contracts/web-policy-catalogue.v1.json`
//! is generated from it by [`catalogue_json`] (regenerate with
//! `UPDATE_CONTRACT_GOLDEN=1 cargo test -p quarry-core --test web_policy`), and a
//! test fails when the checked-in file or its `.sha256` pin differs. Key strings
//! become public once the claim carries them, so review them with Control before
//! WS7.H.

use serde_json::{json, Value};

use super::{CapKey, Mode, ProxyMax, RobotsMax};

/// What a key is, which decides how a layer may state it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A capability on or off. A layer may disable it, never force it on.
    Ability,
    /// A behaviour that a layer may force on. A layer may not turn it off.
    Protection,
    /// A maximum on an ordered scale. The meet is the minimum.
    Ceiling,
    /// Allow and deny host lists.
    Rules,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ability => "ability",
            Self::Protection => "protection",
            Self::Ceiling => "ceiling",
            Self::Rules => "rules",
        }
    }
}

/// Whom a key binds. An autonomous-only key is skipped for a human principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppliesTo {
    All,
    Autonomous,
}

impl AppliesTo {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Autonomous => "autonomous",
        }
    }
}

/// The baseline of a key, in the type its kind uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Baseline {
    Mode(Mode),
    Robots(RobotsMax),
    Proxy(ProxyMax),
    Rules,
}

impl Baseline {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mode(Mode::Disabled) => "disabled",
            Self::Mode(Mode::Allowed) => "allowed",
            Self::Mode(Mode::Forced) => "forced",
            Self::Robots(value) => value.as_str(),
            Self::Proxy(value) => value.as_str(),
            Self::Rules => "rules",
        }
    }
}

/// One catalogue row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapSpec {
    pub key: CapKey,
    pub kind: Kind,
    pub applies_to: AppliesTo,
    pub baseline: Baseline,
    /// False only for an invariant: no layer can move the key off its baseline.
    pub unlockable: bool,
    /// Zero Data Retention forces the key disabled.
    pub zdr_implies_disabled: bool,
    /// Listed and inert: no call site consults the key yet. A new consumer
    /// must go through `require` and be listed here first.
    pub reserved: bool,
    /// The work item that introduced the row.
    pub since: &'static str,
    /// The call site that enforces the key, or `None` when reserved.
    pub site: Option<&'static str>,
}

const SINCE: &str = "WS7.B";

/// The v1 catalogue, in [`CapKey::ALL`] order.
pub const CATALOGUE: &[CapSpec] = &[
    CapSpec {
        key: CapKey::ActScript,
        kind: Kind::Ability,
        applies_to: AppliesTo::All,
        baseline: Baseline::Mode(Mode::Disabled),
        unlockable: false,
        zdr_implies_disabled: false,
        reserved: false,
        since: SINCE,
        site: Some("quarry-edge agent gate: raw script refusal (WS2.A, WS7.D)"),
    },
    CapSpec {
        key: CapKey::ActInput,
        kind: Kind::Ability,
        applies_to: AppliesTo::Autonomous,
        baseline: Baseline::Mode(Mode::Allowed),
        unlockable: true,
        zdr_implies_disabled: false,
        reserved: false,
        since: SINCE,
        site: Some("quarry-edge agent step gate (WS7.D)"),
    },
    CapSpec {
        key: CapKey::ActInteract,
        kind: Kind::Ability,
        applies_to: AppliesTo::Autonomous,
        baseline: Baseline::Mode(Mode::Allowed),
        unlockable: true,
        zdr_implies_disabled: false,
        reserved: false,
        since: SINCE,
        site: Some("quarry-edge agent step gate (WS7.D)"),
    },
    CapSpec {
        key: CapKey::ActUpload,
        kind: Kind::Ability,
        applies_to: AppliesTo::All,
        baseline: Baseline::Mode(Mode::Allowed),
        unlockable: true,
        zdr_implies_disabled: false,
        reserved: false,
        since: SINCE,
        site: Some("quarry-edge agent step gate, one-shot approval still applies (WS7.D)"),
    },
    CapSpec {
        key: CapKey::ActDownload,
        kind: Kind::Ability,
        applies_to: AppliesTo::All,
        baseline: Baseline::Mode(Mode::Allowed),
        unlockable: true,
        zdr_implies_disabled: false,
        reserved: false,
        since: SINCE,
        site: Some("quarry-edge agent step gate, one-shot approval still applies (WS7.D)"),
    },
    CapSpec {
        key: CapKey::ProfilePersistent,
        kind: Kind::Ability,
        applies_to: AppliesTo::All,
        baseline: Baseline::Mode(Mode::Allowed),
        unlockable: true,
        zdr_implies_disabled: true,
        reserved: false,
        since: SINCE,
        site: Some("quarry-edge profile routes (WS7.E)"),
    },
    CapSpec {
        key: CapKey::ConsentReject,
        kind: Kind::Ability,
        applies_to: AppliesTo::All,
        baseline: Baseline::Mode(Mode::Disabled),
        unlockable: true,
        zdr_implies_disabled: false,
        reserved: false,
        since: "W5-FENCE-1",
        site: Some(
            "Verevon desktop browser: SPA reads the effective view and passes consentReject to the adapter (D-W5-4)",
        ),
    },
    CapSpec {
        key: CapKey::ReputationBlock,
        kind: Kind::Protection,
        applies_to: AppliesTo::All,
        baseline: Baseline::Mode(Mode::Allowed),
        unlockable: false,
        zdr_implies_disabled: false,
        reserved: false,
        since: "W3-E3.8.7",
        site: Some(
            "quarry-edge agent egress: forces host reputation to block for the org's runs (tighten-only over QUARRY_EDGE__REPUTATION_MODE)",
        ),
    },
    CapSpec {
        key: CapKey::Recording,
        kind: Kind::Ability,
        applies_to: AppliesTo::All,
        baseline: Baseline::Mode(Mode::Disabled),
        unlockable: true,
        zdr_implies_disabled: true,
        reserved: true,
        since: SINCE,
        site: None,
    },
    CapSpec {
        key: CapKey::SearchPaidProvider,
        kind: Kind::Ability,
        applies_to: AppliesTo::All,
        baseline: Baseline::Mode(Mode::Allowed),
        unlockable: true,
        zdr_implies_disabled: true,
        reserved: false,
        since: SINCE,
        site: Some("quarry-edge search routes (WS7.E)"),
    },
    CapSpec {
        key: CapKey::RequestCredentials,
        kind: Kind::Ability,
        applies_to: AppliesTo::All,
        baseline: Baseline::Mode(Mode::Disabled),
        unlockable: true,
        zdr_implies_disabled: false,
        reserved: true,
        since: SINCE,
        site: None,
    },
    CapSpec {
        key: CapKey::RobotsMaxMode,
        kind: Kind::Ceiling,
        applies_to: AppliesTo::All,
        baseline: Baseline::Robots(RobotsMax::Respect),
        unlockable: true,
        zdr_implies_disabled: false,
        reserved: true,
        since: SINCE,
        site: None,
    },
    CapSpec {
        key: CapKey::ProxyMaxTier,
        kind: Kind::Ceiling,
        applies_to: AppliesTo::All,
        baseline: Baseline::Proxy(ProxyMax::None),
        unlockable: true,
        zdr_implies_disabled: false,
        reserved: true,
        since: SINCE,
        site: None,
    },
    CapSpec {
        key: CapKey::DomainsAllow,
        kind: Kind::Rules,
        applies_to: AppliesTo::Autonomous,
        baseline: Baseline::Rules,
        unlockable: true,
        zdr_implies_disabled: false,
        reserved: false,
        since: SINCE,
        site: Some("quarry-edge navigation gate (WS7.C, WS7.D)"),
    },
    CapSpec {
        key: CapKey::DomainsDeny,
        kind: Kind::Rules,
        applies_to: AppliesTo::All,
        baseline: Baseline::Rules,
        unlockable: true,
        zdr_implies_disabled: false,
        reserved: false,
        since: SINCE,
        site: Some("quarry-edge scrape and navigation gate (WS7.C, WS7.D)"),
    },
];

/// The catalogue as the checked-in golden holds it. Generated, never hand-edited.
pub fn catalogue_json() -> Value {
    let keys: Vec<Value> = CATALOGUE
        .iter()
        .map(|spec| {
            json!({
                "key": spec.key.as_str(),
                "kind": spec.kind.as_str(),
                "applies_to": spec.applies_to.as_str(),
                "baseline": spec.baseline.as_str(),
                "unlockable": spec.unlockable,
                "zdr_implies_disabled": spec.zdr_implies_disabled,
                "reserved": spec.reserved,
                "since": spec.since,
                "site": spec.site,
            })
        })
        .collect();
    json!({ "version": 1, "keys": keys })
}
