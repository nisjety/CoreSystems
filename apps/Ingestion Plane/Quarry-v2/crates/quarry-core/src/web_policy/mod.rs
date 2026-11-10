//! Web policy (WS7.B): what a browser or scrape request may do, decided once
//! from the layers that bind it.
//!
//! The model is a lattice. Every layer (the operator baseline, the org layer,
//! a browser grant, the request's own ask, and the invariants and Zero Data
//! Retention floor) is a [`PolicyLayer`]. Layers combine with [`PolicyLayer::meet`],
//! which only ever tightens: an ability takes the more restrictive mode, a
//! ceiling takes the minimum, allow lists intersect and deny lists union. No
//! layer can express a loosening, so the resolved policy is never more
//! permissive than any input. [`resolve`] folds the layers and records which
//! layer decided each key, for the typed denial and for logs.
//!
//! This module is a pure library. It does no I/O and has no feature flag. The
//! edge wiring (WS7.C) and the agent gates (WS7.D) call it.
//!
//! Catalogue, action mapping and denial shapes live in the submodules.

pub mod action_class;
pub mod catalogue;
pub mod denial;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;

use crate::contracts::AgentAction;

pub use action_class::{action_class, action_tag, ActionClass};
pub use catalogue::{catalogue_json, AppliesTo, Baseline, CapSpec, Kind, CATALOGUE};
pub use denial::{DecisionSource, PolicyDenial};

/// Plain lowercase host names in a layer's rules: at most this many entries...
const MAX_RULE_ENTRIES: usize = 64;
/// ...and at most this many bytes in total across allow and deny.
const MAX_RULE_BYTES: usize = 2048;

/// Every capability key of the v1 catalogue. The string form is the wire name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapKey {
    ActScript,
    ActInput,
    ActInteract,
    ActUpload,
    ActDownload,
    ProfilePersistent,
    Recording,
    SearchPaidProvider,
    RequestCredentials,
    RobotsMaxMode,
    ProxyMaxTier,
    DomainsAllow,
    DomainsDeny,
}

impl CapKey {
    /// Every key, in catalogue order.
    pub const ALL: [CapKey; 13] = [
        CapKey::ActScript,
        CapKey::ActInput,
        CapKey::ActInteract,
        CapKey::ActUpload,
        CapKey::ActDownload,
        CapKey::ProfilePersistent,
        CapKey::Recording,
        CapKey::SearchPaidProvider,
        CapKey::RequestCredentials,
        CapKey::RobotsMaxMode,
        CapKey::ProxyMaxTier,
        CapKey::DomainsAllow,
        CapKey::DomainsDeny,
    ];

    /// The stable wire name, for example `browser.act.input`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ActScript => "browser.act.script",
            Self::ActInput => "browser.act.input",
            Self::ActInteract => "browser.act.interact",
            Self::ActUpload => "browser.act.upload",
            Self::ActDownload => "browser.act.download",
            Self::ProfilePersistent => "browser.profile.persistent",
            Self::Recording => "browser.recording",
            Self::SearchPaidProvider => "search.paid_provider",
            Self::RequestCredentials => "request.credentials",
            Self::RobotsMaxMode => "robots.max_mode",
            Self::ProxyMaxTier => "proxy.max_tier",
            Self::DomainsAllow => "domains.allow",
            Self::DomainsDeny => "domains.deny",
        }
    }

    /// The key named `raw`, or `None` when the catalogue has no such key.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.as_str() == raw)
    }

    /// The catalogue row for this key. Every key has exactly one row (a test
    /// checks it), so the lookup cannot miss.
    pub fn spec(self) -> &'static CapSpec {
        CATALOGUE
            .iter()
            .find(|spec| spec.key == self)
            .expect("every CapKey has exactly one catalogue row")
    }

    pub fn kind(self) -> Kind {
        self.spec().kind
    }
}

/// The mode of an ability or a protection in one layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Disabled,
    Allowed,
    /// Protections only: the behaviour is mandatory.
    Forced,
}

/// The robots.txt ceiling, least permissive first, so the meet is the minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RobotsMax {
    Strict,
    Respect,
    Ignore,
}

impl RobotsMax {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Respect => "respect",
            Self::Ignore => "ignore",
        }
    }
}

/// The proxy tier ceiling, least permissive first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMax {
    None,
    Pool,
}

impl ProxyMax {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Pool => "pool",
        }
    }
}

/// Who the policy binds: a person at a browser, or an autonomous agent.
/// Keys whose catalogue row says `applies_to: autonomous` are skipped for a human.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Principal {
    Human,
    Autonomous,
}

/// The value a key has when no layer says anything about it.
fn neutral(kind: Kind) -> Mode {
    match kind {
        Kind::Protection => Mode::Disabled,
        Kind::Ability | Kind::Ceiling | Kind::Rules => Mode::Allowed,
    }
}

/// Restrictiveness of a mode for a kind. A larger rank is more restrictive.
fn rank(kind: Kind, mode: Mode) -> u8 {
    match (kind, mode) {
        (Kind::Protection, Mode::Disabled) => 0,
        (Kind::Protection, Mode::Allowed) => 1,
        (Kind::Protection, Mode::Forced) => 2,
        (_, Mode::Disabled) => 1,
        (_, Mode::Allowed | Mode::Forced) => 0,
    }
}

/// Which modes a layer may state for a key. An ability cannot be forced on (a
/// layer cannot make a loosened ability mandatory), a protection cannot be
/// turned off, and a ceiling or rules key is not a mode key at all.
fn check_mode(kind: Kind, key: CapKey, mode: Mode) -> Result<(), LayerError> {
    match kind {
        Kind::Ability if mode == Mode::Forced => Err(LayerError::ForcedOnAbility(key)),
        Kind::Protection if mode == Mode::Disabled => Err(LayerError::DisabledOnProtection(key)),
        Kind::Ceiling | Kind::Rules => Err(LayerError::NotAMode(key)),
        Kind::Ability | Kind::Protection => Ok(()),
    }
}

/// Why a layer was refused at construction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LayerError {
    #[error("unknown capability key")]
    UnknownCapability(String),
    #[error("forced is not expressible on an ability: {0:?}")]
    ForcedOnAbility(CapKey),
    #[error("disabled is not expressible on a protection: {0:?}")]
    DisabledOnProtection(CapKey),
    #[error("not a mode key: {0:?}")]
    NotAMode(CapKey),
    #[error("domain rule is not a plain lowercase host name")]
    InvalidHost,
    #[error("domain rules exceed 64 entries or 2 KiB")]
    RulesTooLarge,
}

/// One layer of policy. Fields are private; build with [`PolicyLayer::new`] or
/// by deserialising (the wire form is validated the same way).
///
/// A key with no entry is neutral. Entries equal to the neutral value are not
/// stored, so two layers that mean the same thing compare equal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "LayerWire")]
pub struct PolicyLayer {
    modes: BTreeMap<CapKey, Mode>,
    robots_max: Option<RobotsMax>,
    proxy_max: Option<ProxyMax>,
    /// Each entry is one allow list. A host must match every list (intersection).
    allow_terms: Vec<BTreeSet<String>>,
    deny: BTreeSet<String>,
}

impl PolicyLayer {
    /// Build a layer, refusing anything a layer may not say.
    pub fn new(
        modes: BTreeMap<CapKey, Mode>,
        robots_max: Option<RobotsMax>,
        proxy_max: Option<ProxyMax>,
        allow_domains: Option<Vec<String>>,
        deny_domains: Vec<String>,
    ) -> Result<Self, LayerError> {
        let mut kept = BTreeMap::new();
        for (key, mode) in modes {
            check_mode(key.kind(), key, mode)?;
            if mode != neutral(key.kind()) {
                kept.insert(key, mode);
            }
        }
        let allow_term = match allow_domains {
            Some(list) => Some(normalise_hosts(&list)?),
            None => None,
        };
        let deny = normalise_hosts(&deny_domains)?;
        let entries = allow_term.as_ref().map_or(0, BTreeSet::len) + deny.len();
        let bytes: usize = allow_term
            .iter()
            .flatten()
            .chain(deny.iter())
            .map(String::len)
            .sum();
        if entries > MAX_RULE_ENTRIES || bytes > MAX_RULE_BYTES {
            return Err(LayerError::RulesTooLarge);
        }
        Ok(Self {
            modes: kept,
            robots_max,
            proxy_max,
            allow_terms: allow_term.into_iter().collect(),
            deny,
        })
    }

    /// The mode this layer states for `key`, if it states one.
    pub fn mode(&self, key: CapKey) -> Option<Mode> {
        self.modes.get(&key).copied()
    }

    pub fn robots_max(&self) -> Option<RobotsMax> {
        self.robots_max
    }

    pub fn proxy_max(&self) -> Option<ProxyMax> {
        self.proxy_max
    }

    pub fn allow_terms(&self) -> &[BTreeSet<String>] {
        &self.allow_terms
    }

    pub fn deny(&self) -> &BTreeSet<String> {
        &self.deny
    }

    /// WS7.G 6b: rebuild the layer a [`ResolvedPolicy::snapshot_json`] describes. Anything the
    /// current version does not know is refused, never guessed at: a snapshot is a security
    /// input, so a newer shape fails closed at the caller instead of running looser.
    pub fn from_snapshot_json(value: &serde_json::Value) -> Result<Self, SnapshotError> {
        let wire: SnapshotWire =
            serde_json::from_value(value.clone()).map_err(|_| SnapshotError::Malformed)?;
        if wire.v != SNAPSHOT_VERSION {
            return Err(SnapshotError::Version(wire.v));
        }
        if wire.allow.len() > MAX_SNAPSHOT_ALLOW_TERMS {
            return Err(SnapshotError::Layer(LayerError::RulesTooLarge));
        }
        let mut modes = BTreeMap::new();
        for (raw, mode) in wire.modes {
            let key = CapKey::parse(&raw)
                .ok_or_else(|| SnapshotError::Layer(LayerError::UnknownCapability(raw.clone())))?;
            modes.insert(key, mode);
        }
        let mut layer = Self::new(modes, wire.robots_max, wire.proxy_max, None, wire.deny)
            .map_err(SnapshotError::Layer)?;
        for term in wire.allow {
            let term_layer = Self::new(BTreeMap::new(), None, None, Some(term), Vec::new())
                .map_err(SnapshotError::Layer)?;
            layer = layer.meet(&term_layer);
        }
        Ok(layer)
    }

    fn mode_or_neutral(&self, key: CapKey) -> Mode {
        self.mode(key).unwrap_or_else(|| neutral(key.kind()))
    }

    /// The layer that is at least as restrictive as both inputs. Commutative,
    /// associative and idempotent (checked by the property tests).
    pub fn meet(&self, other: &Self) -> Self {
        let mut modes = BTreeMap::new();
        for key in CapKey::ALL {
            let kind = key.kind();
            if matches!(kind, Kind::Ceiling | Kind::Rules) {
                continue;
            }
            let a = self.mode_or_neutral(key);
            let b = other.mode_or_neutral(key);
            let mode = if rank(kind, a) >= rank(kind, b) { a } else { b };
            if mode != neutral(kind) {
                modes.insert(key, mode);
            }
        }
        let mut allow_terms: Vec<BTreeSet<String>> = self
            .allow_terms
            .iter()
            .chain(other.allow_terms.iter())
            .cloned()
            .collect();
        allow_terms.sort();
        allow_terms.dedup();
        Self {
            modes,
            robots_max: min_opt(self.robots_max, other.robots_max),
            proxy_max: min_opt(self.proxy_max, other.proxy_max),
            allow_terms,
            deny: self.deny.union(&other.deny).cloned().collect(),
        }
    }

    /// The layer as one principal sees it: an autonomous-only key is dropped
    /// for a human, and so is the allow list.
    fn for_principal(&self, principal: Principal) -> Self {
        match principal {
            Principal::Autonomous => self.clone(),
            Principal::Human => Self {
                modes: self
                    .modes
                    .iter()
                    .filter(|(key, _)| key.spec().applies_to == AppliesTo::All)
                    .map(|(key, mode)| (*key, *mode))
                    .collect(),
                robots_max: self.robots_max,
                proxy_max: self.proxy_max,
                allow_terms: Vec::new(),
                deny: self.deny.clone(),
            },
        }
    }
}

fn min_opt<T: Ord + Copy>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// Lowercase ASCII host names only: no wildcard, no IP literal, no port, no
/// trailing dot. The last label may not be all digits, which rules out IPv4.
fn valid_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    if !host
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
    {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.iter().any(|label| {
        label.is_empty() || label.len() > 63 || label.starts_with('-') || label.ends_with('-')
    }) {
        return false;
    }
    let last = labels.last().copied().unwrap_or("");
    !last.bytes().all(|b| b.is_ascii_digit())
}

fn normalise_hosts(hosts: &[String]) -> Result<BTreeSet<String>, LayerError> {
    let mut out = BTreeSet::new();
    for host in hosts {
        if !valid_host(host) {
            return Err(LayerError::InvalidHost);
        }
        out.insert(host.clone());
    }
    Ok(out)
}

/// Host `host` is `entry` or sits under it, on a dot boundary.
fn host_matches(host: &str, entry: &str) -> bool {
    host == entry || host.strip_suffix(entry).is_some_and(|rest| rest.ends_with('.'))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct LayerWire {
    modes: BTreeMap<String, Mode>,
    robots_max: Option<RobotsMax>,
    proxy_max: Option<ProxyMax>,
    domains: DomainsWire,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct DomainsWire {
    allow: Option<Vec<String>>,
    deny: Vec<String>,
}

impl TryFrom<LayerWire> for PolicyLayer {
    type Error = LayerError;

    fn try_from(wire: LayerWire) -> Result<Self, LayerError> {
        let mut modes = BTreeMap::new();
        for (raw, mode) in wire.modes {
            let key = CapKey::parse(&raw).ok_or_else(|| LayerError::UnknownCapability(raw.clone()))?;
            modes.insert(key, mode);
        }
        PolicyLayer::new(
            modes,
            wire.robots_max,
            wire.proxy_max,
            wire.domains.allow,
            wire.domains.deny,
        )
    }
}

/// The version of the snapshot a durable job carries (WS7.G 6b).
pub const SNAPSHOT_VERSION: u64 = 1;
/// A snapshot states at most this many allow lists (one per layer that has one).
const MAX_SNAPSHOT_ALLOW_TERMS: usize = 16;

/// Why a snapshot was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotError {
    #[error("the policy snapshot is not the shape this version reads")]
    Malformed,
    #[error("unsupported policy snapshot version {0}")]
    Version(u64),
    #[error("the policy snapshot states something a layer may not: {0}")]
    Layer(LayerError),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotWire {
    v: u64,
    #[serde(default)]
    modes: BTreeMap<String, Mode>,
    #[serde(default)]
    robots_max: Option<RobotsMax>,
    #[serde(default)]
    proxy_max: Option<ProxyMax>,
    #[serde(default)]
    allow: Vec<Vec<String>>,
    #[serde(default)]
    deny: Vec<String>,
    /// Informational (the org policy revision the snapshot was resolved under).
    #[serde(default)]
    #[allow(dead_code)]
    org_rev: Option<u64>,
}

/// What a request asks for on top of its policy. It can only tighten: it names
/// capabilities to disable, ceilings to lower and hosts to deny or allow. It
/// has no field that could state a mode other than disabled. The wire form
/// omits empty `disable` and `deny_domains` lists and absent options, so the
/// empty ask is `{}` (the golden is `contracts/request-ask.v1.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RequestAsk {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disable: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robots_max: Option<RobotsMax>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_max: Option<ProxyMax>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deny_domains: Vec<String>,
    /// `None` is no allow list and is not written. `Some(vec![])` allows nothing
    /// and is written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_domains: Option<Vec<String>>,
}

impl RequestAsk {
    /// The ask as a layer. An unknown key or a key the request may not touch
    /// (a protection, a ceiling) is refused.
    pub fn into_layer(&self) -> Result<PolicyLayer, LayerError> {
        let mut modes = BTreeMap::new();
        for raw in &self.disable {
            let key = CapKey::parse(raw).ok_or_else(|| LayerError::UnknownCapability(raw.clone()))?;
            modes.insert(key, Mode::Disabled);
        }
        PolicyLayer::new(
            modes,
            self.robots_max,
            self.proxy_max,
            self.allow_domains.clone(),
            self.deny_domains.clone(),
        )
    }
}

/// The org layer as the edge received it.
#[derive(Debug, Clone)]
pub enum OrgInput {
    /// No org policy applies to this caller.
    Absent,
    Present { layer: PolicyLayer, rev: u64 },
    /// The org policy could not be read. The request is refused, never run
    /// on the baseline alone.
    Unreadable,
}

/// Everything [`resolve`] folds. The caller builds the layers; this module
/// never reads a claim or a database.
#[derive(Debug, Clone)]
pub struct PolicyInputs {
    pub principal: Principal,
    /// The Zero Data Retention floor. It comes from the caller's claim and is
    /// not a layer, so no layer can lower it.
    pub zdr: bool,
    /// The operator baseline (`QUARRY_EDGE__WEB_POLICY_BASELINE_JSON`).
    pub baseline: PolicyLayer,
    pub org: OrgInput,
    pub grant: Option<PolicyLayer>,
    pub request: Option<PolicyLayer>,
}

/// The layers in decision-source order. Ties go to the earlier one.
fn layers(inputs: &PolicyInputs) -> Result<Vec<(DecisionSource, PolicyLayer)>, PolicyDenial> {
    let org = match &inputs.org {
        OrgInput::Absent => PolicyLayer::default(),
        OrgInput::Present { layer, .. } => layer.clone(),
        OrgInput::Unreadable => return Err(PolicyDenial::PolicyUnreadable),
    };
    Ok(vec![
        (DecisionSource::Invariant, invariant_layer()),
        (DecisionSource::Zdr, zdr_layer(inputs.zdr)),
        (DecisionSource::OrgPolicy, org),
        (DecisionSource::Grant, inputs.grant.clone().unwrap_or_default()),
        (
            DecisionSource::Baseline,
            catalogue_defaults().meet(&inputs.baseline),
        ),
        (DecisionSource::Request, inputs.request.clone().unwrap_or_default()),
    ]
    .into_iter()
    .map(|(source, layer)| (source, layer.for_principal(inputs.principal)))
    .collect())
}

/// The catalogue's own baseline values, as a layer.
fn catalogue_defaults() -> PolicyLayer {
    let mut modes = BTreeMap::new();
    let mut robots = None;
    let mut proxy = None;
    for spec in CATALOGUE {
        match spec.baseline {
            Baseline::Mode(mode) => {
                modes.insert(spec.key, mode);
            }
            Baseline::Robots(value) => robots = Some(value),
            Baseline::Proxy(value) => proxy = Some(value),
            Baseline::Rules => {}
        }
    }
    PolicyLayer::new(modes, robots, proxy, None, Vec::new())
        .expect("catalogue baseline values are valid layer values")
}

/// Keys whose catalogue row is not unlockable, at their baseline mode. v1 has
/// one such key, `browser.act.script`. Only the lattice is built in v1, so an
/// unlock path does not exist yet (see the depth file).
fn invariant_layer() -> PolicyLayer {
    let modes = CATALOGUE
        .iter()
        .filter(|spec| !spec.unlockable)
        .filter_map(|spec| match spec.baseline {
            Baseline::Mode(mode) => Some((spec.key, mode)),
            Baseline::Robots(_) | Baseline::Proxy(_) | Baseline::Rules => None,
        })
        .collect();
    PolicyLayer::new(modes, None, None, None, Vec::new())
        .expect("invariant baseline values are valid layer values")
}

/// Zero Data Retention forces off the keys whose catalogue row says so.
fn zdr_layer(zdr: bool) -> PolicyLayer {
    if !zdr {
        return PolicyLayer::default();
    }
    let modes = CATALOGUE
        .iter()
        .filter(|spec| spec.zdr_implies_disabled)
        .map(|spec| (spec.key, Mode::Disabled))
        .collect();
    PolicyLayer::new(modes, None, None, None, Vec::new())
        .expect("zdr layer values are valid layer values")
}

/// Whether ZDR forces the key off (profile persistence, paid search, recording).
pub fn zdr_forces_disabled(key: CapKey) -> bool {
    key.spec().zdr_implies_disabled
}

/// Fold every layer into the decision for one request.
pub fn resolve(inputs: &PolicyInputs) -> Result<ResolvedPolicy, PolicyDenial> {
    let layers = layers(inputs)?;
    let org_rev = match &inputs.org {
        OrgInput::Present { rev, .. } => Some(*rev),
        OrgInput::Absent | OrgInput::Unreadable => None,
    };
    let resolved = layers
        .iter()
        .fold(PolicyLayer::default(), |acc, (_, layer)| acc.meet(layer));

    let mut modes = BTreeMap::new();
    for key in CapKey::ALL {
        if !matches!(key.kind(), Kind::Ability | Kind::Protection) {
            continue;
        }
        if inputs.principal == Principal::Human && key.spec().applies_to == AppliesTo::Autonomous {
            continue;
        }
        let value = resolved.mode_or_neutral(key);
        let source = layers
            .iter()
            .find(|(_, layer)| layer.mode(key) == Some(value))
            .map_or(DecisionSource::Baseline, |(source, _)| *source);
        modes.insert(key, (value, source));
    }

    // A ceiling no layer states falls back to its most restrictive value. The
    // catalogue baseline always states one, so this is defensive only.
    let robots = resolved.robots_max.unwrap_or(RobotsMax::Strict);
    let robots_source = layers
        .iter()
        .find(|(_, layer)| layer.robots_max == Some(robots))
        .map_or(DecisionSource::Baseline, |(source, _)| *source);
    let proxy = resolved.proxy_max.unwrap_or(ProxyMax::None);
    let proxy_source = layers
        .iter()
        .find(|(_, layer)| layer.proxy_max == Some(proxy))
        .map_or(DecisionSource::Baseline, |(source, _)| *source);

    let allow_terms: Vec<(DecisionSource, BTreeSet<String>)> = layers
        .iter()
        .flat_map(|(source, layer)| layer.allow_terms.iter().map(move |term| (*source, term.clone())))
        .collect();
    let mut deny: Vec<(DecisionSource, String)> = Vec::new();
    for (source, layer) in &layers {
        for host in &layer.deny {
            if !deny.iter().any(|(_, seen)| seen == host) {
                deny.push((*source, host.clone()));
            }
        }
    }

    Ok(ResolvedPolicy {
        principal: inputs.principal,
        zdr: inputs.zdr,
        modes,
        robots_max: (robots, robots_source),
        proxy_max: (proxy, proxy_source),
        allow_terms,
        deny,
        org_rev,
    })
}

/// Result of narrowing an egress host list by the policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressNarrowing {
    Unchanged,
    Narrowed(Vec<String>),
    DenyAll,
}

/// The decision for one request. Built by [`resolve`]; never deserialised.
#[derive(Debug, Clone)]
pub struct ResolvedPolicy {
    principal: Principal,
    zdr: bool,
    modes: BTreeMap<CapKey, (Mode, DecisionSource)>,
    robots_max: (RobotsMax, DecisionSource),
    proxy_max: (ProxyMax, DecisionSource),
    allow_terms: Vec<(DecisionSource, BTreeSet<String>)>,
    deny: Vec<(DecisionSource, String)>,
    org_rev: Option<u64>,
}

impl ResolvedPolicy {
    pub fn principal(&self) -> Principal {
        self.principal
    }

    pub fn zdr(&self) -> bool {
        self.zdr
    }

    /// The resolved mode of a key and the layer that decided it. `None` for a
    /// key that does not apply to this principal.
    pub fn mode(&self, key: CapKey) -> Option<(Mode, DecisionSource)> {
        self.modes.get(&key).copied()
    }

    /// False only when the key resolved to disabled. Keys that do not apply to
    /// this principal are allowed.
    pub fn allows(&self, key: CapKey) -> bool {
        self.modes
            .get(&key)
            .map_or(true, |(mode, _)| *mode != Mode::Disabled)
    }

    /// Refuse `key` unless it is allowed. `denied_action` is the action's wire tag.
    pub fn require(&self, key: CapKey, denied_action: &str) -> Result<(), PolicyDenial> {
        match self.modes.get(&key) {
            Some((Mode::Disabled, source)) => Err(PolicyDenial::PolicyDenied {
                capability: key,
                decision_source: *source,
                policy_rev: if *source == DecisionSource::OrgPolicy {
                    self.org_rev
                } else {
                    None
                },
                denied_action: denied_action.to_string(),
            }),
            _ => Ok(()),
        }
    }

    /// Gate one agent action. `evaluate` is refused unconditionally: it is the
    /// invariant key, so no layer can allow it.
    pub fn gate_action(&self, action: &AgentAction) -> Result<(), PolicyDenial> {
        let class = action_class(action);
        if class == ActionClass::Script {
            return Err(PolicyDenial::RawScriptDenied);
        }
        match class.cap_key() {
            Some(key) => self.require(key, action_tag(action)),
            None => Ok(()),
        }
    }

    /// Refuse a robots mode above the resolved ceiling.
    pub fn require_robots(&self, requested: RobotsMax) -> Result<(), PolicyDenial> {
        let (max, _) = self.robots_max;
        if requested > max {
            return Err(PolicyDenial::CeilingExceeded {
                capability: CapKey::RobotsMaxMode,
                max: max.as_str().to_string(),
                requested: requested.as_str().to_string(),
            });
        }
        Ok(())
    }

    /// Refuse a proxy tier above the resolved ceiling.
    pub fn require_proxy(&self, requested: ProxyMax) -> Result<(), PolicyDenial> {
        let (max, _) = self.proxy_max;
        if requested > max {
            return Err(PolicyDenial::CeilingExceeded {
                capability: CapKey::ProxyMaxTier,
                max: max.as_str().to_string(),
                requested: requested.as_str().to_string(),
            });
        }
        Ok(())
    }

    /// The resolved robots ceiling and the layer that set it.
    pub fn robots_max(&self) -> (RobotsMax, DecisionSource) {
        self.robots_max
    }

    /// The resolved proxy ceiling and the layer that set it.
    pub fn proxy_max(&self) -> (ProxyMax, DecisionSource) {
        self.proxy_max
    }

    /// Check a URL against the domain rules. The URL is parsed here and never
    /// echoed; the host is reported only when ZDR is off.
    pub fn check_url(&self, url: &str) -> Result<(), PolicyDenial> {
        let host = Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
            .unwrap_or_default();
        self.check_host(&host)
    }

    /// Check a lowercase host. Deny always applies; allow lists apply to an
    /// autonomous principal only.
    pub fn check_host(&self, host: &str) -> Result<(), PolicyDenial> {
        let shown = if self.zdr { None } else { Some(host.to_string()) };
        if let Some((source, _)) = self.deny.iter().find(|(_, entry)| host_matches(host, entry)) {
            return Err(PolicyDenial::DomainDenied {
                host: shown,
                decision_source: *source,
            });
        }
        if self.principal == Principal::Autonomous {
            for (source, term) in &self.allow_terms {
                if !term.iter().any(|entry| host_matches(host, entry)) {
                    return Err(PolicyDenial::DomainNotAllowed {
                        host: shown,
                        decision_source: *source,
                    });
                }
            }
        }
        Ok(())
    }

    /// The hosts any layer denies, canonical, sorted and without duplicates. The browser
    /// egress boundary takes this list as its own deny list (WS7.G), so a sub-resource,
    /// frame or redirect to a denied host is refused where it happens.
    pub fn denied_hosts(&self) -> Vec<String> {
        let mut hosts: Vec<String> = self.deny.iter().map(|(_, host)| host.clone()).collect();
        hosts.sort();
        hosts.dedup();
        hosts
    }

    /// Whether a policy layer explicitly limits network requests by host.
    pub fn has_host_allowlist(&self) -> bool {
        !self.allow_terms.is_empty()
    }

    /// WS7.G 6b: the policy as a snapshot a durable job carries from the handoff to every
    /// page run. It states only what a layer can state, so [`PolicyLayer::from_snapshot_json`]
    /// turns it back into a layer that the page's own policy meets (tighten-only). Modes equal
    /// to the neutral value are left out. It carries host names and closed labels only: never a
    /// URL, a path, a query or caller data. Key order is not significant; the edge and the
    /// orchestrator hash the canonical (sorted, compact) form.
    pub fn snapshot_json(&self) -> serde_json::Value {
        let modes: serde_json::Map<String, serde_json::Value> = self
            .modes
            .iter()
            .filter(|(key, (mode, _))| *mode != neutral(key.kind()))
            .map(|(key, (mode, _))| (key.as_str().to_string(), json!(mode_str(*mode))))
            .collect();
        let mut allow: Vec<Vec<String>> = self
            .allow_terms
            .iter()
            .map(|(_, term)| term.iter().cloned().collect())
            .collect();
        allow.sort();
        allow.dedup();
        let mut snapshot = json!({
            "v": SNAPSHOT_VERSION,
            "modes": modes,
            "robots_max": self.robots_max.0.as_str(),
            "proxy_max": self.proxy_max.0.as_str(),
            "allow": allow,
            "deny": self.denied_hosts(),
        });
        if let (Some(rev), Some(object)) = (self.org_rev, snapshot.as_object_mut()) {
            object.insert("org_rev".to_string(), json!(rev));
        }
        snapshot
    }

    /// Keep only the egress hosts the policy still allows.
    pub fn narrow_egress(&self, hosts: &[String]) -> EgressNarrowing {
        let kept: Vec<String> = hosts
            .iter()
            .filter(|host| self.check_host(&host.to_ascii_lowercase()).is_ok())
            .cloned()
            .collect();
        if kept.len() == hosts.len() {
            EgressNarrowing::Unchanged
        } else if kept.is_empty() {
            EgressNarrowing::DenyAll
        } else {
            EgressNarrowing::Narrowed(kept)
        }
    }

    /// A blake3 digest of the effective policy, for logs. It carries no URL and
    /// no caller data.
    pub fn digest(&self) -> String {
        let modes: BTreeMap<String, &'static str> = self
            .modes
            .iter()
            .map(|(key, (mode, _))| (key.as_str().to_string(), mode_str(*mode)))
            .collect();
        let mut allow: Vec<Vec<String>> = self
            .allow_terms
            .iter()
            .map(|(_, term)| term.iter().cloned().collect())
            .collect();
        allow.sort();
        let mut deny: Vec<String> = self.deny.iter().map(|(_, host)| host.clone()).collect();
        deny.sort();
        deny.dedup();
        let canonical = json!({
            "principal": match self.principal {
                Principal::Human => "human",
                Principal::Autonomous => "autonomous",
            },
            "zdr": self.zdr,
            "modes": modes,
            "robots_max": self.robots_max.0.as_str(),
            "proxy_max": self.proxy_max.0.as_str(),
            "allow": allow,
            "deny": deny,
            "org_rev": self.org_rev,
        });
        let bytes = serde_json::to_vec(&canonical).expect("canonical policy json");
        blake3::hash(&bytes).to_hex().to_string()
    }
}

fn mode_str(mode: Mode) -> &'static str {
    match mode {
        Mode::Disabled => "disabled",
        Mode::Allowed => "allowed",
        Mode::Forced => "forced",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The v1 catalogue has no protection key, so the protection rule is checked
    // on the kind directly here; the integration tests cover the ability rule.
    #[test]
    fn disabled_on_a_protection_is_rejected() {
        assert_eq!(
            check_mode(Kind::Protection, CapKey::ActScript, Mode::Disabled),
            Err(LayerError::DisabledOnProtection(CapKey::ActScript))
        );
        assert!(check_mode(Kind::Protection, CapKey::ActScript, Mode::Forced).is_ok());
        assert!(check_mode(Kind::Ability, CapKey::ActInput, Mode::Disabled).is_ok());
    }

    #[test]
    fn every_key_has_one_row_and_a_unique_name() {
        let mut names: Vec<&str> = CapKey::ALL.iter().map(|key| key.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), CapKey::ALL.len());
        assert_eq!(CATALOGUE.len(), CapKey::ALL.len());
        for key in CapKey::ALL {
            assert_eq!(key.spec().key, key);
        }
    }

    // ---- WS7.G 6b: the snapshot a durable job carries ----

    fn layer(json: &str) -> PolicyLayer {
        serde_json::from_str(json).expect("a valid layer document")
    }

    fn inputs(principal: Principal, baseline: PolicyLayer, request: Option<PolicyLayer>) -> PolicyInputs {
        PolicyInputs {
            principal,
            zdr: false,
            baseline,
            org: OrgInput::Absent,
            grant: None,
            request,
        }
    }

    const STRICT_BASELINE: &str = r#"{
        "modes": {"search.paid_provider": "disabled", "browser.act.input": "disabled"},
        "robots_max": "respect",
        "domains": {"allow": ["example.com", "docs.example.org"], "deny": ["bad.example"]}
    }"#;

    #[test]
    fn a_snapshot_rebuilt_as_a_layer_resolves_to_the_policy_it_was_taken_from() {
        let handoff = resolve(&inputs(Principal::Autonomous, layer(STRICT_BASELINE), None)).unwrap();
        let snapshot = handoff.snapshot_json();
        assert_eq!(snapshot["v"], 1);
        assert_eq!(snapshot["deny"], serde_json::json!(["bad.example"]));
        let rebuilt = PolicyLayer::from_snapshot_json(&snapshot).unwrap();
        // A page with no baseline of its own, meeting the snapshot, decides what the handoff did.
        let page = resolve(&inputs(Principal::Autonomous, PolicyLayer::default(), Some(rebuilt))).unwrap();
        assert_eq!(page.digest(), handoff.digest());
        assert!(page.check_url("https://bad.example/x").is_err());
        assert!(page.check_url("https://other.test/x").is_err(), "outside the allow list");
        assert!(page.check_url("https://www.example.com/x").is_ok());
        assert!(!page.allows(CapKey::SearchPaidProvider));
        assert!(!page.allows(CapKey::ActInput));
    }

    #[test]
    fn a_snapshot_can_only_tighten_the_page_policy() {
        // The handoff was lenient; the page's own baseline is strict. The snapshot adds nothing
        // and removes nothing: the strict baseline still binds.
        let lenient = resolve(&inputs(Principal::Autonomous, PolicyLayer::default(), None)).unwrap();
        let snapshot = PolicyLayer::from_snapshot_json(&lenient.snapshot_json()).unwrap();
        let page = resolve(&inputs(Principal::Autonomous, layer(STRICT_BASELINE), Some(snapshot))).unwrap();
        let strict_alone = resolve(&inputs(Principal::Autonomous, layer(STRICT_BASELINE), None)).unwrap();
        assert_eq!(page.digest(), strict_alone.digest());

        // The handoff was strict; the page's baseline is empty. The snapshot binds.
        let strict = resolve(&inputs(Principal::Autonomous, layer(STRICT_BASELINE), None)).unwrap();
        let snapshot = PolicyLayer::from_snapshot_json(&strict.snapshot_json()).unwrap();
        let page = resolve(&inputs(
            Principal::Autonomous,
            layer(r#"{"domains": {"deny": ["other.bad"]}}"#),
            Some(snapshot),
        ))
        .unwrap();
        assert!(page.check_url("https://bad.example/").is_err(), "the snapshot's deny");
        assert!(page.check_url("https://other.bad/").is_err(), "the page's own deny");
        assert!(!page.allows(CapKey::SearchPaidProvider));
    }

    #[test]
    fn the_default_snapshot_leaves_the_default_policy_unchanged() {
        let default = resolve(&inputs(Principal::Autonomous, PolicyLayer::default(), None)).unwrap();
        let snapshot = PolicyLayer::from_snapshot_json(&default.snapshot_json()).unwrap();
        let page = resolve(&inputs(Principal::Autonomous, PolicyLayer::default(), Some(snapshot))).unwrap();
        assert_eq!(page.digest(), default.digest());
    }

    #[test]
    fn a_snapshot_the_version_cannot_read_is_refused() {
        let ok = serde_json::json!({"v": 1});
        assert!(PolicyLayer::from_snapshot_json(&ok).is_ok());
        assert_eq!(
            PolicyLayer::from_snapshot_json(&serde_json::json!({"v": 2})),
            Err(SnapshotError::Version(2))
        );
        for bad in [
            serde_json::json!("not an object"),
            serde_json::json!({}),
            serde_json::json!({"v": 1, "extra": true}),
            serde_json::json!({"v": 1, "robots_max": "anything"}),
            serde_json::json!({"v": 1, "deny": "bad.example"}),
        ] {
            assert_eq!(PolicyLayer::from_snapshot_json(&bad), Err(SnapshotError::Malformed), "{bad}");
        }
        assert!(matches!(
            PolicyLayer::from_snapshot_json(&serde_json::json!({"v": 1, "modes": {"nope.key": "disabled"}})),
            Err(SnapshotError::Layer(LayerError::UnknownCapability(_)))
        ));
        assert!(matches!(
            PolicyLayer::from_snapshot_json(&serde_json::json!({"v": 1, "modes": {"browser.act.input": "forced"}})),
            Err(SnapshotError::Layer(LayerError::ForcedOnAbility(_)))
        ));
        assert!(matches!(
            PolicyLayer::from_snapshot_json(&serde_json::json!({"v": 1, "deny": ["Bad.Example"]})),
            Err(SnapshotError::Layer(LayerError::InvalidHost))
        ));
        assert!(matches!(
            PolicyLayer::from_snapshot_json(&serde_json::json!({"v": 1, "deny": ["10.0.0.1"]})),
            Err(SnapshotError::Layer(LayerError::InvalidHost))
        ));
        let many_terms: Vec<serde_json::Value> = (0..17).map(|_| serde_json::json!(["a.example"])).collect();
        assert!(matches!(
            PolicyLayer::from_snapshot_json(&serde_json::json!({"v": 1, "allow": many_terms})),
            Err(SnapshotError::Layer(LayerError::RulesTooLarge))
        ));
    }

    #[test]
    fn the_snapshot_holds_host_names_and_closed_labels_only() {
        let handoff = resolve(&inputs(Principal::Autonomous, layer(STRICT_BASELINE), None)).unwrap();
        let text = handoff.snapshot_json().to_string();
        assert!(!text.contains("http"), "no URL: {text}");
        assert!(!text.contains('/'), "no path: {text}");
        // Key order is the writer's business; the hash is over the canonical form.
        assert!(text.contains("bad.example"));
    }
}
