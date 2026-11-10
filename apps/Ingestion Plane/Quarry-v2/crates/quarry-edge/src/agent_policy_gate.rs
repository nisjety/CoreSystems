//! WS7.D: the agent path's web policy.
//!
//! `start_run` resolves the run's decision once: the caller's principal, the ZDR posture,
//! the operator baseline and the request's `policy` ask (`web_policy_edge::decide`). The
//! decision is kept on the lock-free `RunLive`, so every later surface reads the same one.
//!
//! The per-action gate is gate 8 of the admission chain (`admission::admit_action`, R-7):
//! after the cheap local checks and before the budget and the one-shot sensitive approval,
//! so a refusal consumes no step, no attempt, no receipt and no approval. REST `step`,
//! `interact`, the browser WebSocket and the tab routes all reach it through the same chain.
//!
//! A refusal is the typed 403 of `web_policy_edge::quarry_error`. Nothing here echoes a URL
//! or a script, and the counters and log lines carry the closed labels only.
//!
//! Organisation layer: the token's `web_policy` claim (WS7.J), resolved into the run's decision
//! at start. Grant layer (WS7.K): the broker grant's web-policy snapshot, read per step from the
//! grant gate 4 revalidates, with the step caller's own claim beside it ([`gate_policy_for_step`],
//! [`GrantSnapshot`]). They tighten the run's decision, never loosen it.

use std::sync::Arc;

use quarry_browser::{BrowserEgressPolicy, HostReputation, ReputationMode};
use quarry_core::contracts::AgentAction;
use quarry_core::web_policy::{
    resolve, CapKey, EgressNarrowing, Mode, OrgInput, PolicyDenial, PolicyInputs, PolicyLayer,
    Principal, ProxyMax, RequestAsk, ResolvedPolicy, RobotsMax,
};
use quarry_core::QuarryResult;
use quarry_runtime::GrantValidation;

use crate::agent_routes::RunLive;
use crate::policy_baseline::{Decision, Enforcement, PolicyBaseline};
use crate::state::AppState;
use crate::auth::Claims;
use crate::web_policy_edge::{self, quarry_error, record_denial};

/// Resolve the run's decision at start, before the browser is acquired.
///
/// `persist_profile` is the run's effective posture (a resumed run carries its checkpoint's
/// value), and the two artifact flags are the caller's driver requirements. A refusal here
/// is typed and consumes nothing: no lease, no slot, no checkpoint.
pub fn resolve_for_start(
    baseline: &PolicyBaseline,
    claims: &Claims,
    zdr: bool,
    ask: Option<&RequestAsk>,
    persist_profile: bool,
    artifact_uploads: bool,
    artifact_downloads: bool,
) -> QuarryResult<Decision> {
    let decision = web_policy_edge::decide(baseline, claims, zdr, ask)?;
    gate_decision(&decision, |policy| {
        if persist_profile {
            policy.require(CapKey::ProfilePersistent, "persist_profile")?;
        }
        if artifact_uploads {
            policy.require(CapKey::ActUpload, "upload_ref")?;
        }
        if artifact_downloads {
            policy.require(CapKey::ActDownload, "download_ref")?;
        }
        Ok(())
    })?;
    Ok(decision)
}

/// D2-051: the ask a resumed run is held to: the one it was started with (stored in its
/// checkpoint) and the one the resume request carries, as one. An ask can only tighten, so
/// the merge is the tightest of the two: capabilities disabled by either stay disabled, the
/// ceilings take the lower, the deny lists join, and a host must pass both allow lists. A
/// resume that carries no ask keeps the stored one whole; it never drops a restriction.
/// `None` only when neither side asked for anything.
pub fn merge_asks(stored: Option<&RequestAsk>, requested: Option<&RequestAsk>) -> Option<RequestAsk> {
    match (stored, requested) {
        (None, None) => None,
        (Some(only), None) | (None, Some(only)) => Some(only.clone()),
        (Some(a), Some(b)) => {
            let mut disable = a.disable.clone();
            for key in &b.disable {
                if !disable.contains(key) {
                    disable.push(key.clone());
                }
            }
            let mut deny_domains = a.deny_domains.clone();
            for host in &b.deny_domains {
                if !deny_domains.contains(host) {
                    deny_domains.push(host.clone());
                }
            }
            Some(RequestAsk {
                disable,
                robots_max: min_option(a.robots_max, b.robots_max),
                proxy_max: min_option(a.proxy_max, b.proxy_max),
                deny_domains,
                allow_domains: match (&a.allow_domains, &b.allow_domains) {
                    (None, None) => None,
                    (Some(only), None) | (None, Some(only)) => Some(only.clone()),
                    (Some(left), Some(right)) => Some(allow_both(left, right)),
                },
            })
        }
    }
}

/// The lower of two optional ceilings (the enums order least permissive first).
fn min_option<T: Ord + Copy>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (one, other) => one.or(other),
    }
}

/// Host `host` is `entry` or sits under it, on a dot boundary (the policy's own rule).
fn host_under(host: &str, entry: &str) -> bool {
    host == entry || host.strip_suffix(entry).is_some_and(|rest| rest.ends_with('.'))
}

/// One allow list for hosts that must pass both: each entry of one list that the other list
/// covers, so a parent domain narrowed by a subdomain keeps the subdomain. Empty when no host
/// passes both, which denies everything (never "allow all").
fn allow_both(left: &[String], right: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut keep = |host: &String| {
        if !out.contains(host) {
            out.push(host.clone());
        }
    };
    for host in left {
        if right.iter().any(|entry| host_under(host, entry)) {
            keep(host);
        }
    }
    for host in right {
        if left.iter().any(|entry| host_under(host, entry)) {
            keep(host);
        }
    }
    out
}

/// The egress boundary the run's browser is configured with: the run's own allow list,
/// narrowed by the policy. `constraints.allowed_domains` is never changed here, so the
/// broker grant equality checked by `validate_run_grant` still holds. An intersection that
/// is empty denies everything; it never falls back to "allow all".
pub fn egress_policy(decision: &Decision, allowed_domains: &[String]) -> BrowserEgressPolicy {
    // WS7.G: the hosts the policy denies are also the browser boundary's deny list, so a
    // sub-resource, frame or redirect to one of them is refused where it happens, even on
    // a run whose allow list is empty or names a parent domain. Empty when no rule denies.
    let denied = decision.policy().denied_hosts();
    let egress = egress_for_narrowing(
        decision.policy().narrow_egress(allowed_domains),
        allowed_domains,
        &denied,
    );
    if decision.policy().has_host_allowlist() {
        egress
    } else {
        egress.with_public_assets()
    }
}

/// The browser boundary for one narrowing result. `from_allowed_domains(&[])` is the
/// unrestricted policy, so a narrowed set with no host in it must never reach that
/// constructor: it is "nothing is allowed" (D2-020). `narrow_egress` itself answers
/// `DenyAll` for an empty result, and this keeps the boundary closed even if a future
/// change to it, or a narrowing from another source, ever produced an empty set.
fn egress_for_narrowing(
    narrowing: EgressNarrowing,
    allowed_domains: &[String],
    denied: &[String],
) -> BrowserEgressPolicy {
    match narrowing {
        EgressNarrowing::Unchanged => {
            BrowserEgressPolicy::from_allowed_domains(allowed_domains).with_denied_domains(denied)
        }
        EgressNarrowing::Narrowed(hosts) if hosts.is_empty() => BrowserEgressPolicy::deny_all(),
        EgressNarrowing::Narrowed(hosts) => {
            BrowserEgressPolicy::from_allowed_domains(&hosts).with_denied_domains(denied)
        }
        EgressNarrowing::DenyAll => BrowserEgressPolicy::deny_all(),
    }
}

/// The hosts the run's policy denies, for the run's per-action egress (WS7.G). A run built
/// outside `start_run` is held to the autonomous principal under the operator baseline, as
/// the other gates hold it; a baseline that cannot resolve yields no list here, and the
/// admission gates refuse that run anyway.
pub fn egress_denied(state: &AppState, live: &RunLive, zdr: bool) -> Vec<String> {
    match live.policy() {
        Some(decision) => decision.policy().denied_hosts(),
        None => state
            .policy
            .decide(Principal::Autonomous, zdr, None)
            .map(|decision| decision.policy().denied_hosts())
            .unwrap_or_default(),
    }
}

/// The egress boundary for a run that already exists (a tab opened mid-run): the run's
/// allow list narrowed and extended by its stored decision, as at start. Without a stored
/// decision the run's allow list stands alone, as it did before WS7.G.
pub fn live_egress_policy(
    state: &AppState,
    live: &RunLive,
    zdr: bool,
    allowed_domains: &[String],
) -> BrowserEgressPolicy {
    match live.policy() {
        Some(decision) => egress_policy(decision, allowed_domains),
        None => BrowserEgressPolicy::from_allowed_domains(allowed_domains)
            .with_denied_domains(&egress_denied(state, live, zdr)),
    }
}

/// The deployment's browser URL reputation mode (E3.8.7), read once from
/// `QUARRY_EDGE__REPUTATION_MODE`. Unset or unreadable is `Off`, and `Off` is a kill switch:
/// no org setting turns the hook on.
fn deployment_reputation_mode() -> ReputationMode {
    static MODE: std::sync::OnceLock<ReputationMode> = std::sync::OnceLock::new();
    *MODE.get_or_init(ReputationMode::from_env)
}

/// The strictness order of the modes: `Off` < `Warn` < `Block`.
fn reputation_rank(mode: ReputationMode) -> u8 {
    match mode {
        ReputationMode::Off => 0,
        ReputationMode::Warn => 1,
        ReputationMode::Block => 2,
    }
}

/// The reputation mode one org runs under: the stricter of the deployment mode and the org's
/// cap. The cap is tighten-only, so it can raise `Warn` to `Block` but never lower the
/// deployment mode, and a deployment that is `Off` stays `Off` whatever the org says. This is
/// deliberately not `ReputationMode::for_org`, which lets an org setting win.
pub fn effective_reputation_mode(
    deployment: ReputationMode,
    org_cap: Option<ReputationMode>,
) -> ReputationMode {
    if deployment == ReputationMode::Off {
        return ReputationMode::Off;
    }
    match org_cap {
        Some(cap) if reputation_rank(cap) > reputation_rank(deployment) => cap,
        _ => deployment,
    }
}

/// The org's reputation cap from the web-policy claim (key `browser.reputation`, tighten-only,
/// E3.8.7). The web-policy catalogue has no such key yet, so no org can set one and the
/// deployment mode applies alone. When the key exists this is the one place that reads it.
fn org_reputation_cap() -> Option<ReputationMode> {
    None
}

/// The host reputation source and mode a run's egress boundary is built with, or `None` when
/// the mode is `Off` (nothing is attached, no overhead) or the engine offers no source.
/// `org_cap` is the org's tighten-only cap, if any. The source is the shared security engine's
/// blocklist, so a hot reload reaches every live run. A ZDR run behaves the same: verdicts are
/// not persisted.
pub fn reputation_for(
    state: &AppState,
    org_cap: Option<ReputationMode>,
) -> Option<(Arc<dyn HostReputation>, ReputationMode)> {
    let mode = effective_reputation_mode(deployment_reputation_mode(), org_cap);
    if mode == ReputationMode::Off {
        return None;
    }
    let source = state.security.host_reputation()?;
    Some((source, mode))
}

/// Attach the host reputation hook to a run's egress boundary when the mode is on (E3.8.7). The
/// hook is consulted by the browser boundary after the grant and before the DNS guard, for
/// every navigation, redirect hop, click, frame and sub-resource. With the mode `Off` the
/// policy is returned unchanged. There is no second enforcement path: the D7 preflight stays
/// the scrape path's check and this is the browser path's.
pub fn attach_reputation(state: &AppState, policy: BrowserEgressPolicy) -> BrowserEgressPolicy {
    match reputation_for(state, org_reputation_cap()) {
        Some((source, mode)) => policy.with_reputation(source, mode),
        None => policy,
    }
}

/// Gate a URL the run is about to use, before it is used (the recovery navigation of a
/// resumed run). Host rules only.
pub fn gate_url(decision: &Decision, url: &str) -> QuarryResult<()> {
    gate_decision(decision, |policy| policy.check_url(url))
}

/// Gate 8 of the admission chain: the action's capability and, for a navigation, its host.
/// Read, observe and navigate classes are never refused by a capability.
pub fn gate_policy(
    state: &AppState,
    live: &RunLive,
    zdr: bool,
    action: &AgentAction,
) -> QuarryResult<()> {
    gate_live(state, live, zdr, |policy| check_action(policy, action))
}

/// The host rules for a URL a run has reached or is about to reach: a page the browser
/// landed on after an action, or the URL of a tab a call opens. The caller returns the
/// error; nothing is persisted for a refused page here.
pub fn gate_visit(state: &AppState, live: &RunLive, zdr: bool, url: &str) -> QuarryResult<()> {
    gate_live(state, live, zdr, |policy| policy.check_url(url))
}

/// The per-action check against one resolved policy.
pub fn check_action(policy: &ResolvedPolicy, action: &AgentAction) -> Result<(), PolicyDenial> {
    policy.gate_action(action)?;
    if let AgentAction::Navigate { url } = action {
        policy.check_url(url)?;
    }
    Ok(())
}

/// Resolve an attachment's own tighten-only ask as an enforced independent layer.
/// Unlike an operator baseline running in observe mode, an explicit restriction
/// accepted from the caller must never silently become advisory.
pub(crate) fn attachment_policy(ask: &RequestAsk) -> QuarryResult<ResolvedPolicy> {
    let layer = ask.into_layer().map_err(web_policy_edge::ask_error)?;
    resolve(&PolicyInputs {
        principal: Principal::Autonomous,
        zdr: false, // ZDR attachments are rejected before this is called.
        baseline: PolicyLayer::default(),
        org: OrgInput::Absent,
        grant: None,
        request: Some(layer),
    }).map_err(|denial| quarry_error(&denial))
}

/// Enforce the explicit attachment restriction in addition to the run's normal
/// admission chain; an operator's observe mode must not weaken this promise.
pub(crate) fn gate_attachment_action(policy: &ResolvedPolicy, action: &AgentAction) -> QuarryResult<()> {
    check_action(policy, action).map_err(|denial| {
        record_denial(&denial, true);
        quarry_error(&denial)
    })
}

pub(crate) fn gate_attachment_url(policy: &ResolvedPolicy, url: &str) -> QuarryResult<()> {
    policy.check_url(url).map_err(|denial| {
        record_denial(&denial, true);
        quarry_error(&denial)
    })
}

/// The tab boundary meets this with the parent session's own egress policy.
pub(crate) fn attachment_egress_policy(
    policy: &ResolvedPolicy,
    allowed_domains: &[String],
    ask: &RequestAsk,
) -> BrowserEgressPolicy {
    // A narrower request may name a child of a grant root. Filtering only the
    // grant roots would deny that legitimate intersection. Candidate roots can
    // come from either side, but each must satisfy both the grant and policy.
    let mut candidates: Vec<String> = allowed_domains.iter()
        .chain(ask.allow_domains.iter().flatten())
        .map(|host| host.trim().to_ascii_lowercase())
        .filter(|host| !host.is_empty())
        .collect();
    candidates.retain(|host| {
        allowed_domains.iter().any(|grant| {
            let grant = grant.trim().to_ascii_lowercase();
            !grant.is_empty() && (host == &grant || host.strip_suffix(&grant).is_some_and(|prefix| prefix.ends_with('.')))
        }) && policy.check_host(host).is_ok()
    });
    candidates.sort();
    candidates.dedup();
    if candidates.is_empty() {
        BrowserEgressPolicy::deny_all()
    } else {
        let egress = BrowserEgressPolicy::from_allowed_domains(&candidates)
            .with_denied_domains(&policy.denied_hosts());
        if policy.has_host_allowlist() {
            egress
        } else {
            egress.with_public_assets()
        }
    }
}

fn gate_live(
    state: &AppState,
    live: &RunLive,
    zdr: bool,
    check: impl Fn(&ResolvedPolicy) -> Result<(), PolicyDenial>,
) -> QuarryResult<()> {
    match live.policy() {
        Some(decision) => gate_decision(decision, check),
        None => {
            // A run with no stored decision (one built outside `start_run`) is held to the
            // autonomous principal, which is never less restrictive than the human one,
            // under the operator baseline.
            let decision = state
                .policy
                .decide(Principal::Autonomous, zdr, None)
                .map_err(|denial| quarry_error(&denial))?;
            gate_decision(&decision, check)
        }
    }
}

/// Run one check against the decision. Under `observe` the shadow (what `enforce` would
/// do) is checked first and counted; the run's own decision decides.
fn gate_decision(
    decision: &Decision,
    check: impl Fn(&ResolvedPolicy) -> Result<(), PolicyDenial>,
) -> QuarryResult<()> {
    if let Some(shadow) = decision.shadow() {
        if let Err(denial) = check(shadow) {
            record_denial(&denial, false);
        }
    }
    check(decision.policy()).map_err(|denial| {
        record_denial(&denial, true);
        quarry_error(&denial)
    })
}

// ---- WS7.K: the broker grant's web-policy snapshot, read as the grant layer ----------------
//
// The broker stores on a grant the org web policy of the principal it verified (the `web_policy`
// claim of the user's broker token), and a derived run grant may only tighten its parent's. Quarry
// reads that snapshot per step, from the grant it revalidates at gate 4, as the grant layer of
// the policy (`DecisionSource::Grant`). The step also carries the org layer of the step caller's
// own token. Both are checked beside the run's decision, never instead of it, so the effect is
// the meet: a looser claim never loosens a tighter snapshot, and a tighter claim (or a tighter
// snapshot) tightens a running run on its next step. Nothing here can widen what the run's own
// decision allows.
//
// Binding: the snapshot is the claim, carried by the grant, so it binds exactly when the claim
// does: `QUARRY_EDGE__WEB_POLICY_CLAIM_ENFORCEMENT` (default `observe`) bounded by
// `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT`. Under `observe` a refusal is only counted
// (`web_policy_would_deny_total`), under `off` the step layers are not read. No new flag.

/// The only snapshot version this build reads: the claim's, since the broker copies the claim it
/// verified. Any other version fails closed.
const GRANT_SNAPSHOT_VERSION: u64 = 1;

/// The top-level fields of a version 1 snapshot (the claim's). Anything else is an unknown key.
const KNOWN_SNAPSHOT_FIELDS: [&str; 6] = ["v", "rev", "caps", "robots_max", "proxy_max", "domains"];

/// The grant layer, as one validation of the run's grant states it.
#[derive(Debug, Clone, PartialEq)]
pub enum GrantSnapshot {
    /// The grant carries no snapshot (a broker that predates WS7.K, or a grant minted from a
    /// token without a claim): no grant layer.
    Absent,
    /// The snapshot, as a layer, and the issuer's revision it was taken at.
    Present { layer: PolicyLayer, rev: u64 },
    /// The grant carries a snapshot this build cannot read. Refused under `enforce`, counted
    /// under `observe`; never run as if it were absent.
    Unreadable,
}

impl GrantSnapshot {
    /// Read the snapshot the broker stored on `grant`. Unknown capability keys and fields are
    /// ignored and counted, as for the claim (D2-053); anything else this build will not apply
    /// (another version, a malformed shape, a rule a layer may not state) is `Unreadable`.
    pub fn from_grant(grant: &GrantValidation) -> Self {
        let Some(raw) = grant.web_policy.as_ref().filter(|raw| !raw.is_null()) else {
            return Self::Absent;
        };
        match parse_grant_snapshot(raw) {
            Ok((layer, rev, unknown_keys)) => {
                if unknown_keys > 0 {
                    metrics::counter!("web_policy_grant_snapshot_unknown_key_total")
                        .increment(unknown_keys as u64);
                }
                Self::Present { layer, rev }
            }
            Err(()) => {
                metrics::counter!("web_policy_grant_snapshot_unreadable_total").increment(1);
                Self::Unreadable
            }
        }
    }
}

/// What one step applies on top of the run's decision: the grant's snapshot from this step's
/// revalidation, and the org layer of the step caller's token ([`step_claim`]).
#[derive(Debug, Clone, Copy)]
pub struct StepLayers<'a> {
    pub grant: &'a GrantSnapshot,
    pub claim: &'a OrgInput,
}

/// The org layer of the step caller's token, as the baseline reads a claim (claim mode `off`
/// reads none; a missing claim is unreadable only when the baseline requires one).
pub fn step_claim(baseline: &PolicyBaseline, claims: &Claims) -> OrgInput {
    baseline.org_input(claims.web_policy_claim())
}

/// How the step layers bind: the claim mode, bounded by the baseline's mode.
fn step_layer_mode(baseline: &PolicyBaseline) -> Enforcement {
    match (baseline.enforcement(), baseline.claim_mode()) {
        (Enforcement::Off, _) | (_, Enforcement::Off) => Enforcement::Off,
        (Enforcement::Enforce, Enforcement::Enforce) => Enforcement::Enforce,
        _ => Enforcement::Observe,
    }
}

/// The policy of the step layers alone: the invariants, the ZDR floor and the catalogue
/// defaults, with the step claim as the org layer and the snapshot as the grant layer.
/// `Ok(None)` when the step states neither (nothing to check beyond the run's decision).
pub fn step_policy(
    principal: Principal,
    zdr: bool,
    layers: &StepLayers<'_>,
) -> Result<Option<ResolvedPolicy>, PolicyDenial> {
    let grant = match layers.grant {
        GrantSnapshot::Absent => None,
        GrantSnapshot::Present { layer, .. } => Some(layer.clone()),
        GrantSnapshot::Unreadable => return Err(PolicyDenial::PolicyUnreadable),
    };
    if grant.is_none() && matches!(layers.claim, OrgInput::Absent) {
        return Ok(None);
    }
    resolve(&PolicyInputs {
        principal,
        zdr,
        baseline: PolicyLayer::default(),
        org: layers.claim.clone(),
        grant,
        request: None,
    })
    .map(Some)
}

/// Check one step against its step layers. Under `enforce` a refusal is the typed 403 and is
/// counted as enforced; under `observe` it is counted as a would-be denial and the step goes on;
/// under `off` nothing is read.
pub fn gate_step_layers(
    baseline: &PolicyBaseline,
    principal: Principal,
    zdr: bool,
    layers: &StepLayers<'_>,
    check: impl Fn(&ResolvedPolicy) -> Result<(), PolicyDenial>,
) -> QuarryResult<()> {
    let mode = step_layer_mode(baseline);
    if mode == Enforcement::Off {
        return Ok(());
    }
    let outcome = step_policy(principal, zdr, layers).and_then(|policy| match policy {
        Some(policy) => check(&policy),
        None => Ok(()),
    });
    match outcome {
        Ok(()) => Ok(()),
        Err(denial) if mode == Enforcement::Enforce => {
            record_denial(&denial, true);
            Err(quarry_error(&denial))
        }
        Err(denial) => {
            record_denial(&denial, false);
            Ok(())
        }
    }
}

/// Gate 8 with the step layers: the run's decision first ([`gate_policy`]), then the grant
/// snapshot and the step claim. A refusal by either consumes nothing, as gate 8 always did.
pub fn gate_policy_for_step(
    state: &AppState,
    live: &RunLive,
    zdr: bool,
    action: &AgentAction,
    layers: &StepLayers<'_>,
) -> QuarryResult<()> {
    gate_policy(state, live, zdr, action)?;
    gate_step_layers(&state.policy, run_principal(live), zdr, layers, |policy| {
        check_action(policy, action)
    })
}

/// [`gate_visit`] with the step layers: the host rules of the run, then of the snapshot and the
/// step claim.
pub fn gate_visit_for_step(
    state: &AppState,
    live: &RunLive,
    zdr: bool,
    url: &str,
    layers: &StepLayers<'_>,
) -> QuarryResult<()> {
    gate_visit(state, live, zdr, url)?;
    gate_step_layers(&state.policy, run_principal(live), zdr, layers, |policy| {
        policy.check_url(url)
    })
}

/// The principal the run's decision was resolved for; a run with no stored decision is held to
/// the autonomous principal, as [`gate_live`] holds it.
fn run_principal(live: &RunLive) -> Principal {
    live.policy()
        .map_or(Principal::Autonomous, |decision| decision.policy().principal())
}

/// Parse a version 1 snapshot (the claim's shape). `Err(())` for anything this build will not
/// apply; the caller counts it and refuses, and the snapshot is never logged.
fn parse_grant_snapshot(raw: &serde_json::Value) -> Result<(PolicyLayer, u64, usize), ()> {
    use serde_json::Value;

    let obj = raw.as_object().ok_or(())?;
    if obj.get("v").and_then(Value::as_u64) != Some(GRANT_SNAPSHOT_VERSION) {
        return Err(());
    }
    let rev = snapshot_rev(obj.get("rev").and_then(Value::as_str).ok_or(())?)?;
    let mut unknown_keys = obj
        .keys()
        .filter(|key| !KNOWN_SNAPSHOT_FIELDS.contains(&key.as_str()))
        .count();

    let mut robots: Option<RobotsMax> = snapshot_ceiling(obj.get("robots_max"))?;
    let mut proxy: Option<ProxyMax> = snapshot_ceiling(obj.get("proxy_max"))?;
    let mut modes = std::collections::BTreeMap::new();
    match obj.get("caps") {
        None | Some(Value::Null) => {}
        Some(caps) => {
            for (name, value) in caps.as_object().ok_or(())? {
                match CapKey::parse(name) {
                    // A key this build does not know: ignored and counted, as for the claim.
                    None => unknown_keys += 1,
                    Some(CapKey::RobotsMaxMode) => {
                        robots = min_option(robots, snapshot_ceiling(Some(value))?)
                    }
                    Some(CapKey::ProxyMaxTier) => {
                        proxy = min_option(proxy, snapshot_ceiling(Some(value))?)
                    }
                    // Domain rules travel in `domains`, never in `caps`.
                    Some(CapKey::DomainsAllow | CapKey::DomainsDeny) => return Err(()),
                    Some(key) => {
                        let mode: Mode = serde_json::from_value(value.clone()).map_err(|_| ())?;
                        modes.insert(key, mode);
                    }
                }
            }
        }
    }

    let (allow, deny) = match obj.get("domains") {
        None | Some(Value::Null) => (None, Vec::new()),
        Some(domains) => {
            let domains = domains.as_object().ok_or(())?;
            unknown_keys += domains
                .keys()
                .filter(|key| !matches!(key.as_str(), "allow" | "deny"))
                .count();
            let allow = match domains.get("allow") {
                None | Some(Value::Null) => None,
                // As in the claim: an empty allow list is no allow rule, not "allow nothing".
                Some(list) => Some(snapshot_hosts(list)?).filter(|hosts| !hosts.is_empty()),
            };
            let deny = match domains.get("deny") {
                None | Some(Value::Null) => Vec::new(),
                Some(list) => snapshot_hosts(list)?,
            };
            (allow, deny)
        }
    };

    let layer = PolicyLayer::new(modes, robots, proxy, allow, deny).map_err(|_| ())?;
    Ok((layer, rev, unknown_keys))
}

/// The snapshot `rev`: `"0"` or 1 to 12 hex digits, as the claim carries it.
fn snapshot_rev(raw: &str) -> Result<u64, ()> {
    if raw.is_empty() || raw.len() > 12 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(());
    }
    u64::from_str_radix(raw, 16).map_err(|_| ())
}

/// A robots or proxy ceiling by its wire name; absent or null is no ceiling.
fn snapshot_ceiling<T: serde::de::DeserializeOwned>(
    value: Option<&serde_json::Value>,
) -> Result<Option<T>, ()> {
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone()).map(Some).map_err(|_| ()),
    }
}

fn snapshot_hosts(list: &serde_json::Value) -> Result<Vec<String>, ()> {
    list.as_array()
        .ok_or(())?
        .iter()
        .map(|item| item.as_str().map(str::to_owned).ok_or(()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use quarry_core::web_policy::{
        resolve, OrgInput, PolicyInputs, PolicyLayer, ProxyMax, RobotsMax,
    };

    fn resolved(principal: Principal, zdr: bool, ask: RequestAsk) -> ResolvedPolicy {
        resolve(&PolicyInputs {
            principal,
            zdr,
            baseline: PolicyLayer::default(),
            org: OrgInput::Absent,
            grant: None,
            request: Some(ask.into_layer().expect("a valid ask")),
        })
        .expect("the policy resolves")
    }

    fn disable(keys: &[&str]) -> RequestAsk {
        RequestAsk {
            disable: keys.iter().map(|key| (*key).to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn read_and_navigate_classes_are_never_denied_by_capabilities() {
        let everything_off = disable(&[
            "browser.act.input",
            "browser.act.interact",
            "browser.act.upload",
            "browser.act.download",
            "browser.profile.persistent",
        ]);
        let policy = resolved(Principal::Autonomous, false, everything_off);

        for action in [
            AgentAction::Navigate {
                url: "https://example.com/".into(),
            },
            AgentAction::Back,
            AgentAction::Forward,
            AgentAction::Wait { ms: 0 },
            AgentAction::GetContent,
            AgentAction::Pdf,
        ] {
            assert!(
                check_action(&policy, &action).is_ok(),
                "a read or navigate action is not gated by a capability"
            );
        }
        // The same policy does refuse an interaction, so the test above is not vacuous.
        assert!(check_action(
            &policy,
            &AgentAction::Click {
                selector: "#go".into()
            }
        )
        .is_err());
    }

    #[test]
    fn human_principal_skips_autonomous_only_keys() {
        // `browser.act.input` applies to autonomous principals only: a person typing is
        // not an agent typing, so the same ask does not refuse a human.
        let typed = AgentAction::Type {
            selector: "#q".into(),
            text: "x".into(),
        };
        let human = resolved(Principal::Human, false, disable(&["browser.act.input"]));
        assert!(check_action(&human, &typed).is_ok());

        let autonomous = resolved(Principal::Autonomous, false, disable(&["browser.act.input"]));
        assert!(check_action(&autonomous, &typed).is_err());
    }

    #[test]
    fn service_principal_input_denied_when_the_layer_disables_input() {
        // The organisation layer is absent in this slice (WS7.H, WS7.J), so the disabling
        // layer here is the request's own ask. The refusal names the layer that decided it.
        let policy = resolved(Principal::Autonomous, false, disable(&["browser.act.input"]));
        let denial = check_action(
            &policy,
            &AgentAction::TypeRef {
                snapshot_id: "snap".into(),
                generation: 1,
                ref_id: "e1".into(),
                text: "x".into(),
            },
        )
        .expect_err("input is disabled for an autonomous principal");
        match denial {
            PolicyDenial::PolicyDenied {
                capability,
                decision_source,
                ..
            } => {
                assert_eq!(capability, CapKey::ActInput);
                assert_eq!(decision_source.as_str(), "request");
            }
            other => panic!("expected a typed policy denial, got {other:?}"),
        }
    }

    #[test]
    fn denied_hosts_are_refused_even_with_no_allow_list_and_under_an_allowed_parent() {
        let decision = PolicyBaseline::default()
            .decide(
                Principal::Autonomous,
                false,
                Some(
                    RequestAsk {
                        deny_domains: vec!["ads.example.com".into()],
                        ..Default::default()
                    }
                    .into_layer()
                    .expect("a valid ask"),
                ),
            )
            .expect("the policy resolves");
        // An unrestricted run (no allow list) still refuses the denied host.
        let open = egress_policy(&decision, &[]);
        assert!(open.allows_url("https://news.example.org/"));
        assert!(!open.allows_url("https://ads.example.com/pixel.gif"));
        assert!(!open.allows_url("https://cdn.ads.example.com/pixel.gif"));
        // A denied subdomain of an allowed parent is refused.
        let parent = egress_policy(&decision, &["example.com".to_string()]);
        assert!(parent.allows_url("https://www.example.com/"));
        assert!(!parent.allows_url("https://ads.example.com/"));
    }

    fn strs(list: &[&str]) -> Vec<String> {
        list.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn merging_asks_keeps_every_restriction_of_both_and_nothing_when_neither_asked() {
        assert_eq!(merge_asks(None, None), None);
        let stored = RequestAsk {
            disable: strs(&["browser.act.upload"]),
            robots_max: Some(RobotsMax::Respect),
            proxy_max: Some(ProxyMax::Pool),
            deny_domains: strs(&["ads.example"]),
            allow_domains: None,
        };
        // A resume with no ask of its own keeps the stored one whole.
        assert_eq!(merge_asks(Some(&stored), None), Some(stored.clone()));
        assert_eq!(merge_asks(None, Some(&stored)), Some(stored.clone()));

        let requested = RequestAsk {
            disable: strs(&["browser.act.input", "browser.act.upload"]),
            robots_max: Some(RobotsMax::Strict),
            proxy_max: None,
            deny_domains: strs(&["tracker.example", "ads.example"]),
            allow_domains: None,
        };
        let merged = merge_asks(Some(&stored), Some(&requested)).expect("an ask");
        assert_eq!(merged.disable, strs(&["browser.act.upload", "browser.act.input"]));
        assert_eq!(merged.robots_max, Some(RobotsMax::Strict), "the lower ceiling");
        assert_eq!(merged.proxy_max, Some(ProxyMax::Pool), "a ceiling only one side set");
        assert_eq!(merged.deny_domains, strs(&["ads.example", "tracker.example"]));
        assert_eq!(merged.allow_domains, None);
        // The merged ask is still a valid layer.
        merged.into_layer().expect("a valid merged ask");
    }

    #[test]
    fn a_resume_cannot_loosen_a_ceiling_the_start_set() {
        let stored = RequestAsk {
            robots_max: Some(RobotsMax::Strict),
            proxy_max: Some(ProxyMax::None),
            ..Default::default()
        };
        let looser = RequestAsk {
            robots_max: Some(RobotsMax::Ignore),
            proxy_max: Some(ProxyMax::Pool),
            ..Default::default()
        };
        let merged = merge_asks(Some(&stored), Some(&looser)).expect("an ask");
        assert_eq!(merged.robots_max, Some(RobotsMax::Strict));
        assert_eq!(merged.proxy_max, Some(ProxyMax::None));
    }

    #[test]
    fn allow_lists_merge_to_the_hosts_that_pass_both() {
        let ask = |hosts: &[&str]| RequestAsk {
            allow_domains: Some(strs(hosts)),
            ..Default::default()
        };
        let merged = |a: &[&str], b: &[&str]| {
            merge_asks(Some(&ask(a)), Some(&ask(b)))
                .and_then(|merged| merged.allow_domains)
                .expect("an allow list")
        };
        // A subdomain narrows its parent, whichever side names it.
        assert_eq!(merged(&["example.com"], &["docs.example.com"]), strs(&["docs.example.com"]));
        assert_eq!(merged(&["docs.example.com"], &["example.com"]), strs(&["docs.example.com"]));
        // Hosts only one side allows drop out.
        assert_eq!(
            merged(&["example.com", "other.test"], &["example.com"]),
            strs(&["example.com"])
        );
        // A suffix that is not on a dot boundary is not under the entry.
        assert_eq!(merged(&["notexample.com"], &["example.com"]), Vec::<String>::new());
        // Disjoint lists pass nothing, which is deny-all rather than allow-all.
        let empty = merged(&["a.test"], &["b.test"]);
        assert!(empty.is_empty());
        let decision = PolicyBaseline::default()
            .decide(
                Principal::Autonomous,
                false,
                Some(
                    RequestAsk {
                        allow_domains: Some(empty),
                        ..Default::default()
                    }
                    .into_layer()
                    .expect("a valid ask"),
                ),
            )
            .expect("the policy resolves");
        assert!(!egress_policy(&decision, &["a.test".to_string()]).allows_url("https://a.test/"));
        assert!(decision.policy().check_url("https://a.test/").is_err());
    }

    #[test]
    fn a_merged_deny_list_reaches_the_browser_egress_of_an_unconstrained_run() {
        // D2-052 after a resume: the start asked to deny ads, the resume asked to deny a
        // tracker; the browser boundary refuses a sub-resource on either, with no allow list.
        let stored = RequestAsk {
            deny_domains: strs(&["ads.example"]),
            ..Default::default()
        };
        let resume = RequestAsk {
            deny_domains: strs(&["tracker.example"]),
            ..Default::default()
        };
        let merged = merge_asks(Some(&stored), Some(&resume)).expect("an ask");
        let decision = PolicyBaseline::default()
            .decide(Principal::Autonomous, false, Some(merged.into_layer().expect("a valid ask")))
            .expect("the policy resolves");
        let egress = egress_policy(&decision, &[]);
        assert!(egress.allows_url("https://news.example.org/"));
        assert!(!egress.allows_url("https://ads.example/pixel.png"));
        assert!(!egress.allows_url("https://cdn.tracker.example/t.js"));
    }

    #[test]
    fn a_policy_without_deny_rules_builds_the_egress_policy_it_always_did() {
        let decision = PolicyBaseline::default()
            .decide(Principal::Autonomous, false, None)
            .expect("the policy resolves");
        assert_eq!(
            egress_policy(&decision, &["example.com".to_string()]),
            BrowserEgressPolicy::from_allowed_domains(&["example.com".to_string()]),
            "an empty deny list leaves the policy byte-identical"
        );
        assert_eq!(
            egress_policy(&decision, &[]),
            BrowserEgressPolicy::from_allowed_domains(&[])
        );
    }

    #[test]
    fn an_empty_narrowed_set_denies_all_instead_of_becoming_unrestricted() {
        // D2-020: `from_allowed_domains(&[])` is the unrestricted policy, so an empty
        // `Narrowed` set must not be built through it.
        let denied = vec!["ads.example".to_string()];
        let egress = egress_for_narrowing(
            EgressNarrowing::Narrowed(Vec::new()),
            &["example.com".to_string()],
            &denied,
        );
        assert!(!egress.is_unconstrained());
        assert!(!egress.allows_url("https://example.com/"));
        assert!(!egress.allows_url("https://anywhere.test/"));
        assert_eq!(egress, BrowserEgressPolicy::deny_all());
        // The same input as `DenyAll` is the same boundary.
        assert_eq!(
            egress_for_narrowing(EgressNarrowing::DenyAll, &[], &[]),
            BrowserEgressPolicy::deny_all()
        );
        // A non-empty narrowing is still a narrowing, with the deny list riding along.
        let kept = egress_for_narrowing(
            EgressNarrowing::Narrowed(vec!["example.com".to_string()]),
            &["example.com".to_string(), "other.test".to_string()],
            &denied,
        );
        assert!(kept.allows_url("https://example.com/"));
        assert!(!kept.allows_url("https://other.test/"));
    }

    #[test]
    fn narrow_egress_never_answers_an_empty_narrowed_set() {
        // The invariant the boundary above relies on: whatever the policy denies, the
        // narrowing is `Unchanged`, a non-empty `Narrowed` set or `DenyAll`.
        let deny = |hosts: &[&str]| {
            PolicyBaseline::default()
                .decide(
                    Principal::Autonomous,
                    false,
                    Some(
                        RequestAsk {
                            deny_domains: hosts.iter().map(|h| (*h).to_string()).collect(),
                            ..Default::default()
                        }
                        .into_layer()
                        .expect("a valid ask"),
                    ),
                )
                .expect("the policy resolves")
        };
        let hosts = |list: &[&str]| list.iter().map(|h| (*h).to_string()).collect::<Vec<_>>();
        let decisions = [deny(&[]), deny(&["a.test"]), deny(&["a.test", "b.test", "c.test"])];
        let inputs = [
            hosts(&[]),
            hosts(&["a.test"]),
            hosts(&["a.test", "b.test"]),
            hosts(&["a.test", "b.test", "c.test", "d.test"]),
            hosts(&["x.a.test", "y.test"]),
            hosts(&["A.TEST"]),
        ];
        for decision in &decisions {
            for input in &inputs {
                match decision.policy().narrow_egress(input) {
                    EgressNarrowing::Narrowed(kept) => {
                        assert!(!kept.is_empty(), "an empty narrowed set for {input:?}")
                    }
                    EgressNarrowing::DenyAll => {
                        assert!(!input.is_empty(), "DenyAll for an empty input")
                    }
                    EgressNarrowing::Unchanged => {}
                }
            }
        }
    }

    #[test]
    fn an_empty_intersection_denies_all_instead_of_allowing_all() {
        // The run may use example.com; the policy denies example.com: nothing is left.
        let decision = PolicyBaseline::default()
            .decide(
                Principal::Autonomous,
                false,
                Some(
                    RequestAsk {
                        deny_domains: vec!["example.com".into()],
                        ..Default::default()
                    }
                    .into_layer()
                    .expect("a valid ask"),
                ),
            )
            .expect("the policy resolves");
        let egress = egress_policy(&decision, &["example.com".to_string()]);
        assert!(!egress.allows_url("https://example.com/"));

        // A partial intersection keeps the host that is still allowed.
        let narrowed = egress_policy(
            &decision,
            &["example.com".to_string(), "other.test".to_string()],
        );
        assert!(narrowed.allows_url("https://other.test/"));
        assert!(!narrowed.allows_url("https://example.com/"));
    }

    // ---- WS7.K: the grant snapshot as the grant layer ------------------------------------------

    use serde_json::json;

    fn grant_with(snapshot: Option<serde_json::Value>) -> GrantValidation {
        let mut grant = json!({ "grant_id": "g", "active": true });
        if let Some(snapshot) = snapshot {
            grant["web_policy"] = snapshot;
        }
        serde_json::from_value(grant).expect("grant json")
    }

    fn snapshot(caps: serde_json::Value, deny: &[&str]) -> GrantSnapshot {
        GrantSnapshot::from_grant(&grant_with(Some(json!({
            "v": 1,
            "rev": "00000000002a",
            "caps": caps,
            "domains": { "allow": [], "deny": deny },
        }))))
    }

    fn claim_disabling(keys: &[CapKey]) -> OrgInput {
        let modes = keys.iter().map(|key| (*key, Mode::Disabled)).collect();
        OrgInput::Present {
            layer: PolicyLayer::new(modes, None, None, None, Vec::new()).expect("a valid layer"),
            rev: 7,
        }
    }

    /// Both web-policy modes at `enforce`, so the step layers bind.
    fn enforcing() -> PolicyBaseline {
        PolicyBaseline::from_lookup(|name| {
            (name == crate::policy_baseline::CLAIM_ENFORCEMENT_ENV).then(|| "enforce".to_owned())
        })
        .expect("a valid baseline")
    }

    fn claim_mode(mode: &'static str) -> PolicyBaseline {
        PolicyBaseline::from_lookup(move |name| {
            (name == crate::policy_baseline::CLAIM_ENFORCEMENT_ENV).then(|| mode.to_owned())
        })
        .expect("a valid baseline")
    }

    fn typed() -> AgentAction {
        AgentAction::Type {
            selector: "#q".into(),
            text: "x".into(),
        }
    }

    fn click() -> AgentAction {
        AgentAction::Click {
            selector: "#go".into(),
        }
    }

    fn source_of(denial: PolicyDenial) -> &'static str {
        match denial {
            PolicyDenial::PolicyDenied {
                decision_source, ..
            } => decision_source.as_str(),
            other => panic!("expected a typed policy denial, got {other:?}"),
        }
    }

    #[test]
    fn a_grant_without_a_snapshot_is_no_grant_layer_and_an_unknown_one_is_unreadable() {
        assert_eq!(GrantSnapshot::from_grant(&grant_with(None)), GrantSnapshot::Absent);
        for unreadable in [
            json!({ "v": 2, "rev": "0" }),
            json!({ "v": 1 }),
            json!({ "v": 1, "rev": "not-hex" }),
            json!({ "v": 1, "rev": "0", "caps": { "browser.act.upload": "sometimes" } }),
            json!({ "v": 1, "rev": "0", "domains": { "deny": "ads.example" } }),
            json!("a string"),
        ] {
            assert_eq!(
                GrantSnapshot::from_grant(&grant_with(Some(unreadable.clone()))),
                GrantSnapshot::Unreadable,
                "{unreadable}"
            );
        }
        // An unknown capability key is ignored (and counted), never applied, as for the claim.
        match snapshot(json!({ "browser.act.teleport": "disabled" }), &[]) {
            GrantSnapshot::Present { layer, rev } => {
                assert_eq!(layer, PolicyLayer::default());
                assert_eq!(rev, 0x2a);
            }
            other => panic!("expected a snapshot, got {other:?}"),
        }
        // With neither a snapshot nor a step claim there is nothing to check.
        let none = StepLayers {
            grant: &GrantSnapshot::Absent,
            claim: &OrgInput::Absent,
        };
        assert!(step_policy(Principal::Autonomous, false, &none)
            .expect("resolves")
            .is_none());
    }

    #[test]
    fn grant_snapshot_is_met_with_the_claim_per_step() {
        let grant = snapshot(json!({ "browser.act.upload": "disabled" }), &["ads.example"]);
        let claim = claim_disabling(&[CapKey::ActInput]);
        let layers = StepLayers {
            grant: &grant,
            claim: &claim,
        };
        let policy = step_policy(Principal::Autonomous, false, &layers)
            .expect("resolves")
            .expect("a step policy");
        assert!(!policy.allows(CapKey::ActUpload), "the snapshot's rule holds");
        assert!(!policy.allows(CapKey::ActInput), "the claim's rule holds");
        assert!(policy.allows(CapKey::ActInteract), "what neither states stays allowed");
        assert_eq!(source_of(policy.require(CapKey::ActUpload, "upload").unwrap_err()), "grant");
        assert_eq!(source_of(policy.require(CapKey::ActInput, "type").unwrap_err()), "org_policy");
        assert!(policy.check_url("https://ads.example/pixel.gif").is_err());
        assert!(policy.check_url("https://news.example.org/").is_ok());

        let baseline = enforcing();
        let gate = |action: &AgentAction| {
            gate_step_layers(&baseline, Principal::Autonomous, false, &layers, |policy| {
                check_action(policy, action)
            })
        };
        assert!(gate(&typed()).is_err(), "the claim refuses input");
        assert!(gate(&click()).is_ok());
        assert!(gate(&AgentAction::Navigate {
            url: "https://cdn.ads.example/".into()
        })
        .is_err(), "the snapshot's deny list refuses the host");
    }

    #[test]
    fn looser_claim_does_not_loosen_a_tighter_grant_snapshot() {
        let grant = snapshot(json!({ "browser.act.interact": "disabled" }), &["ads.example"]);
        // The run's own decision allows everything the catalogue allows.
        let decision = PolicyBaseline::default()
            .decide(Principal::Autonomous, false, None)
            .expect("the policy resolves");
        assert!(check_action(decision.policy(), &click()).is_ok());

        // The step caller's claim states nothing, or the empty pre-WS7.I claim, or is absent.
        let empty = OrgInput::Present {
            layer: PolicyLayer::default(),
            rev: 0,
        };
        let baseline = enforcing();
        for claim in [OrgInput::Absent, empty] {
            let layers = StepLayers {
                grant: &grant,
                claim: &claim,
            };
            let refused = gate_step_layers(&baseline, Principal::Autonomous, false, &layers, |policy| {
                check_action(policy, &click())
            });
            assert!(refused.is_err(), "the snapshot still refuses interaction under {claim:?}");
            assert!(gate_step_layers(&baseline, Principal::Autonomous, false, &layers, |policy| {
                policy.check_url("https://ads.example/")
            })
            .is_err());
        }
    }

    #[test]
    fn tighter_claim_tightens_a_running_run_on_the_next_step() {
        let decision = PolicyBaseline::default()
            .decide(Principal::Autonomous, false, None)
            .expect("the policy resolves");
        assert!(check_action(decision.policy(), &typed()).is_ok(), "the run started allowing input");
        let baseline = enforcing();

        // Step 1: the grant carries no snapshot and the caller's claim states nothing.
        let first = StepLayers {
            grant: &GrantSnapshot::Absent,
            claim: &OrgInput::Absent,
        };
        assert!(gate_step_layers(&baseline, Principal::Autonomous, false, &first, |policy| {
            check_action(policy, &typed())
        })
        .is_ok());

        // Step 2: the organisation tightened its policy; the step caller's fresh claim says so.
        let tighter = claim_disabling(&[CapKey::ActInput]);
        let second = StepLayers {
            grant: &GrantSnapshot::Absent,
            claim: &tighter,
        };
        let error = gate_step_layers(&baseline, Principal::Autonomous, false, &second, |policy| {
            check_action(policy, &typed())
        })
        .expect_err("the running run is tightened on its next step");
        assert_eq!(error.code, quarry_core::error::ErrorCode::Forbidden);

        // Under the default claim mode (`observe`) the same step is only counted.
        assert!(gate_step_layers(
            &PolicyBaseline::default(),
            Principal::Autonomous,
            false,
            &second,
            |policy| check_action(policy, &typed())
        )
        .is_ok());
    }

    #[test]
    fn an_unreadable_snapshot_fails_closed_under_enforce_and_the_claim_mode_bounds_the_layers() {
        let unreadable = GrantSnapshot::from_grant(&grant_with(Some(json!({ "v": 9, "rev": "0" }))));
        let layers = StepLayers {
            grant: &unreadable,
            claim: &OrgInput::Absent,
        };
        let check = |policy: &ResolvedPolicy| check_action(policy, &click());
        assert!(gate_step_layers(&enforcing(), Principal::Autonomous, false, &layers, check).is_err());
        // `observe` counts it; `off` never reads it.
        assert!(gate_step_layers(&claim_mode("observe"), Principal::Autonomous, false, &layers, check).is_ok());
        assert!(gate_step_layers(&claim_mode("off"), Principal::Autonomous, false, &layers, check).is_ok());

        // A readable refusing snapshot is not read under `off` either.
        let refusing = snapshot(json!({ "browser.act.interact": "disabled" }), &[]);
        let layers = StepLayers {
            grant: &refusing,
            claim: &OrgInput::Absent,
        };
        assert!(gate_step_layers(&claim_mode("off"), Principal::Autonomous, false, &layers, check).is_ok());
        assert!(gate_step_layers(&enforcing(), Principal::Autonomous, false, &layers, check).is_err());
    }

    #[test]
    fn a_human_is_not_held_to_autonomous_only_keys_of_the_snapshot() {
        // `browser.act.input` applies to autonomous principals only, in the snapshot as anywhere.
        let grant = snapshot(json!({ "browser.act.input": "disabled" }), &[]);
        let layers = StepLayers {
            grant: &grant,
            claim: &OrgInput::Absent,
        };
        let baseline = enforcing();
        let check = |policy: &ResolvedPolicy| check_action(policy, &typed());
        assert!(gate_step_layers(&baseline, Principal::Human, false, &layers, check).is_ok());
        assert!(gate_step_layers(&baseline, Principal::Autonomous, false, &layers, check).is_err());
    }
}

// ---- E3.2-e1: per-origin consent, the Quarry half (origin-extension grants) ----------------
//
// A person can let a running run reach origins outside its own allow list (an SSO host, a CDN,
// a partner site) by approving an origin extension. The broker (Model Plane) records the
// decision as a child grant of the run grant, kind `origin_extension`; Quarry validates that
// grant and only then widens the run's browser egress (`configure_egress_policy`). Quarry never
// widens from anything the caller says, and it never widens past the web policy: any deny (the
// run's decision, the run grant's snapshot, the open-web denied list, the org's consent caps)
// beats the approval, and the whole extension is refused rather than applied in part.
//
// The grant contract (MP-ASKS.md, "W3-E3.2-e1"): `kind = "origin_extension"`, `parent_grant_id`
// = the live run grant, `owner_id` = the run's actor, `resource_id` = the start host (the origin
// the person was on), `allowed_domains` = the start host plus at most eight hosts the person
// listed, all canonical, `allowed_actions` = exactly one entry `extend_origin:<scope>` with scope
// `turn` | `conversation` | `always`, no frame/dialog/artifact scope, allow-list mode with no
// denied list, `zdr` equal to the run's. A ZDR run takes a turn-scoped extension only.
//
// Flag: `QUARRY_EDGE__ORIGIN_CONSENT`, default off (OD13: the consent wording ships off until
// counsel has reviewed it). Off, every extension is refused before any broker call.

/// The flag that admits origin extensions. Default off.
pub const ORIGIN_CONSENT_FLAG: &str = "QUARRY_EDGE__ORIGIN_CONSENT";

/// The grant kind the broker issues for a person's per-origin consent decision.
pub const ORIGIN_EXTENSION_KIND: &str = "origin_extension";

/// The one `allowed_actions` entry an origin-extension grant carries, before its scope.
pub const ORIGIN_EXTENSION_ACTION_PREFIX: &str = "extend_origin:";

/// At most this many hosts a person lists, beside the start host (OD13 / DEC-19).
pub const MAX_ORIGIN_EXTENSION_HOSTS: usize = 8;

/// Org default (E3.2-f, claim caps): `disabled` refuses every extension, `allowed` states nothing.
pub const ORIGIN_CONSENT_CAP_KEY: &str = "browser.consent.origin";

/// Org default (E3.2-f, claim caps): the widest scope an extension may have.
pub const ORIGIN_CONSENT_MAX_SCOPE_CAP_KEY: &str = "browser.consent.max_scope";

/// Whether `QUARRY_EDGE__ORIGIN_CONSENT` is on in this process. Read per call so the flag needs no
/// AppState plumbing; tests pass the value explicitly instead.
pub fn origin_consent_enabled() -> bool {
    std::env::var(ORIGIN_CONSENT_FLAG)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// How long a person's consent decision lasts. Ordered narrowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConsentScope {
    Turn,
    Conversation,
    Always,
}

impl ConsentScope {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "turn" => Some(Self::Turn),
            "conversation" => Some(Self::Conversation),
            "always" => Some(Self::Always),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Conversation => "conversation",
            Self::Always => "always",
        }
    }
}

/// A validated origin extension: what the run may now also reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginExtension {
    pub grant_id: String,
    /// The origin the person was on when they approved.
    pub start_host: String,
    /// The hosts the person listed, without the start host. At most
    /// [`MAX_ORIGIN_EXTENSION_HOSTS`].
    pub hosts: Vec<String>,
    pub scope: ConsentScope,
}

impl OriginExtension {
    /// The start host and the listed hosts, the set the egress boundary gains.
    pub fn all_hosts(&self) -> Vec<String> {
        let mut all = Vec::with_capacity(self.hosts.len() + 1);
        all.push(self.start_host.clone());
        all.extend(self.hosts.iter().cloned());
        all
    }
}

/// Why an origin extension is refused, as a closed label (logs, counters, error details). It
/// never carries a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginExtensionRefusal {
    /// `QUARRY_EDGE__ORIGIN_CONSENT` is off.
    FlagOff,
    /// Not a live origin-extension grant of this run, its owner and its ZDR posture.
    GrantInvalid,
    /// A host that is not canonical, or the start host missing from the list.
    HostInvalid,
    /// More than [`MAX_ORIGIN_EXTENSION_HOSTS`] listed hosts.
    TooManyHosts,
    /// A ZDR run takes a turn-scoped extension only.
    ZdrScope,
    /// The org's `browser.consent.origin` is `disabled`.
    OrgDisabled,
    /// The extension's scope is wider than the org's `browser.consent.max_scope`.
    OrgScope,
    /// The org's consent caps carry a value this build does not know (fail closed).
    OrgUnreadable,
    /// A host the web policy, the grant snapshot or the run's denied list refuses.
    Denied,
    /// A key run, which has no person to consent.
    ApiKeyRun,
}

impl OriginExtensionRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FlagOff => "origin_consent_disabled",
            Self::GrantInvalid => "origin_extension_grant_invalid",
            Self::HostInvalid => "origin_extension_host_invalid",
            Self::TooManyHosts => "origin_extension_too_many_hosts",
            Self::ZdrScope => "origin_extension_zdr_scope",
            Self::OrgDisabled => "org_origin_consent_disabled",
            Self::OrgScope => "org_origin_consent_scope",
            Self::OrgUnreadable => "org_origin_consent_unreadable",
            Self::Denied => "origin_extension_denied",
            Self::ApiKeyRun => "origin_extension_api_key_run",
        }
    }

    /// The typed error: 403 when the deployment has the feature off, `SecurityBlocked`
    /// otherwise. The details carry the closed label only.
    pub fn to_error(self) -> quarry_core::error::QuarryError {
        use quarry_core::error::{ErrorCode, QuarryError};
        let (code, message) = match self {
            Self::FlagOff => (
                ErrorCode::Forbidden,
                "origin consent is not enabled on this deployment (QUARRY_EDGE__ORIGIN_CONSENT is off)",
            ),
            Self::ApiKeyRun => (
                ErrorCode::Forbidden,
                "an API-key run cannot be extended to other origins",
            ),
            Self::Denied => (
                ErrorCode::SecurityBlocked,
                "the web policy denies an origin in this extension; a deny beats the approval",
            ),
            _ => (
                ErrorCode::SecurityBlocked,
                "origin extension grant does not authorize this run",
            ),
        };
        QuarryError::new(code, message)
            .with_details(serde_json::json!({ "reason": self.as_str() }))
    }
}

/// Host suffixes no consent can reach: loopback and private-use names (same list as user-core's
/// browser_consent validation).
const NON_GRANTABLE_SUFFIXES: [&str; 4] = ["localhost", "local", "internal", "home.arpa"];

/// A host is canonical when it is already in the form the policy matches on: lower case ASCII
/// (punycode for an internationalised name), at least two labels, no trailing dot, no scheme,
/// port, path, userinfo or wildcard, not an IP literal (a numeric last label refuses dotted
/// IPv4), and not a loopback or private-use name. A non-canonical host is refused, never
/// rewritten.
pub fn is_canonical_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || !host.contains('.') {
        return false;
    }
    let labels_ok = host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    });
    if !labels_ok {
        return false;
    }
    let last = host.rsplit('.').next().unwrap_or_default();
    if last.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    !NON_GRANTABLE_SUFFIXES
        .iter()
        .any(|suffix| host_under(host, suffix))
}

/// The run an extension must belong to.
#[derive(Debug, Clone, Copy)]
pub struct ExtensionRun<'a> {
    /// The run's live grant: the extension's parent.
    pub parent_grant_id: &'a str,
    /// The run's actor: the extension's owner.
    pub owner_id: &'a str,
    pub zdr: bool,
}

/// Read an origin-extension grant against the run it claims to extend. Fails closed: any field
/// that does not match the contract refuses the whole grant.
pub fn origin_extension_from_grant(
    grant: &GrantValidation,
    run: &ExtensionRun<'_>,
) -> Result<OriginExtension, OriginExtensionRefusal> {
    if !grant.is_usable()
        || grant.kind != ORIGIN_EXTENSION_KIND
        || grant.parent_grant_id.as_deref() != Some(run.parent_grant_id)
        || run.parent_grant_id.is_empty()
        || run.owner_id.is_empty()
        || grant.owner_id != run.owner_id
        || grant.zdr != run.zdr
        || !grant.key_id.is_empty()
        || !grant.allowed_frame_ids.is_empty()
        || !grant.allowed_dialog_ids.is_empty()
        || !grant.allowed_artifact_ids.is_empty()
        || !matches!(grant.policy_mode.as_str(), "" | "allowlist")
        || !grant.denied_domains.is_empty()
    {
        return Err(OriginExtensionRefusal::GrantInvalid);
    }
    let scope = match grant.allowed_actions.as_slice() {
        [only] => only
            .strip_prefix(ORIGIN_EXTENSION_ACTION_PREFIX)
            .and_then(ConsentScope::parse)
            .ok_or(OriginExtensionRefusal::GrantInvalid)?,
        _ => return Err(OriginExtensionRefusal::GrantInvalid),
    };
    if run.zdr && scope != ConsentScope::Turn {
        return Err(OriginExtensionRefusal::ZdrScope);
    }

    let start_host = grant.resource_id.as_str();
    if !is_canonical_host(start_host) {
        return Err(OriginExtensionRefusal::HostInvalid);
    }
    let mut hosts: Vec<String> = Vec::new();
    let mut saw_start = false;
    for host in &grant.allowed_domains {
        if !is_canonical_host(host) {
            return Err(OriginExtensionRefusal::HostInvalid);
        }
        if host == start_host {
            if saw_start {
                return Err(OriginExtensionRefusal::HostInvalid);
            }
            saw_start = true;
        } else if hosts.contains(host) {
            return Err(OriginExtensionRefusal::HostInvalid);
        } else {
            hosts.push(host.clone());
        }
    }
    if !saw_start {
        return Err(OriginExtensionRefusal::HostInvalid);
    }
    if hosts.len() > MAX_ORIGIN_EXTENSION_HOSTS {
        return Err(OriginExtensionRefusal::TooManyHosts);
    }
    Ok(OriginExtension {
        grant_id: grant.grant_id.clone(),
        start_host: start_host.to_owned(),
        hosts,
        scope,
    })
}

/// The org's consent defaults (E3.2-f), read from the run grant's web-policy snapshot (the claim
/// the broker verified). Absent keys state nothing. A value this build does not know fails closed.
pub fn org_consent_refusal(
    snapshot: Option<&serde_json::Value>,
    scope: ConsentScope,
) -> Option<OriginExtensionRefusal> {
    let caps = snapshot
        .filter(|raw| !raw.is_null())
        .and_then(|raw| raw.get("caps"))
        .filter(|caps| !caps.is_null())?;
    let Some(caps) = caps.as_object() else {
        return Some(OriginExtensionRefusal::OrgUnreadable);
    };
    match caps.get(ORIGIN_CONSENT_CAP_KEY) {
        None | Some(serde_json::Value::Null) => {}
        Some(value) => match value.as_str() {
            Some("allowed") => {}
            Some("disabled") => return Some(OriginExtensionRefusal::OrgDisabled),
            _ => return Some(OriginExtensionRefusal::OrgUnreadable),
        },
    }
    match caps.get(ORIGIN_CONSENT_MAX_SCOPE_CAP_KEY) {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => match value.as_str().and_then(ConsentScope::parse) {
            Some(max) if scope > max => Some(OriginExtensionRefusal::OrgScope),
            Some(_) => None,
            None => Some(OriginExtensionRefusal::OrgUnreadable),
        },
    }
}

/// Deny beats approval: every host of the extension must pass the run's resolved policy, the
/// step layer (the run grant's snapshot) when there is one, and the run's own denied list (an
/// open-web grant's). One refused host refuses the extension.
pub fn origin_extension_denied(
    run_policy: &ResolvedPolicy,
    step_layer: Option<&ResolvedPolicy>,
    run_denied: &[String],
    extension: &OriginExtension,
) -> bool {
    extension.all_hosts().iter().any(|host| {
        run_policy.check_host(host).is_err()
            || step_layer.is_some_and(|layer| layer.check_host(host).is_err())
            || run_denied.iter().any(|entry| host_under(host, entry))
    })
}

/// The run's allow list with the extension's hosts added, or `None` when the run's list is
/// empty (an unconstrained boundary needs no widening, and adding hosts to it would narrow it).
pub fn extended_allowed_domains(
    allowed_domains: &[String],
    extension: &OriginExtension,
) -> Option<Vec<String>> {
    if allowed_domains.is_empty() {
        return None;
    }
    let mut out = allowed_domains.to_vec();
    for host in extension.all_hosts() {
        if !out.contains(&host) {
            out.push(host);
        }
    }
    Some(out)
}

/// The full Quarry-side check of an extension against a live run: the org's consent caps from
/// the run grant's snapshot, then deny-beats-approval against the run's decision (or the
/// autonomous fallback a run without one is held to), the snapshot as a layer, and the run's
/// denied list. Nothing is consumed or installed here.
pub fn check_origin_extension(
    state: &AppState,
    live: &RunLive,
    zdr: bool,
    run_denied: &[String],
    run_grant: &GrantValidation,
    extension: &OriginExtension,
) -> Result<(), OriginExtensionRefusal> {
    if let Some(refusal) = org_consent_refusal(run_grant.web_policy.as_ref(), extension.scope) {
        return Err(refusal);
    }
    let fallback;
    let run_policy = match live.policy() {
        Some(decision) => decision.policy(),
        None => {
            fallback = state
                .policy
                .decide(Principal::Autonomous, zdr, None)
                .map_err(|_| OriginExtensionRefusal::Denied)?;
            fallback.policy()
        }
    };
    let snapshot = GrantSnapshot::from_grant(run_grant);
    let step_layer = step_policy(
        run_principal(live),
        zdr,
        &StepLayers {
            grant: &snapshot,
            claim: &OrgInput::Absent,
        },
    )
    .map_err(|_| OriginExtensionRefusal::Denied)?;
    if origin_extension_denied(run_policy, step_layer.as_ref(), run_denied, extension) {
        return Err(OriginExtensionRefusal::Denied);
    }
    Ok(())
}

/// The egress boundary of a run once the extension applies: the run's allow list plus the
/// extension's hosts, narrowed and deny-listed by the run's decision exactly as at start
/// ([`live_egress_policy`]). An unconstrained run keeps its boundary.
pub fn extended_egress_policy(
    state: &AppState,
    live: &RunLive,
    zdr: bool,
    allowed_domains: &[String],
    extension: &OriginExtension,
) -> BrowserEgressPolicy {
    match extended_allowed_domains(allowed_domains, extension) {
        Some(extended) => live_egress_policy(state, live, zdr, &extended),
        None => live_egress_policy(state, live, zdr, allowed_domains),
    }
}

#[cfg(test)]
mod origin_consent_tests {
    use super::*;
    use serde_json::json;

    const RUN_GRANT: &str = "grant_run_1";
    const OWNER: &str = "user_1";

    fn run(zdr: bool) -> ExtensionRun<'static> {
        ExtensionRun {
            parent_grant_id: RUN_GRANT,
            owner_id: OWNER,
            zdr,
        }
    }

    fn grant(scope: &str, start: &str, hosts: &[&str]) -> GrantValidation {
        let mut domains = vec![start.to_owned()];
        domains.extend(hosts.iter().map(|host| (*host).to_owned()));
        serde_json::from_value(json!({
            "grant_id": "grant_ext_1",
            "active": true,
            "kind": ORIGIN_EXTENSION_KIND,
            "parent_grant_id": RUN_GRANT,
            "owner_id": OWNER,
            "resource_id": start,
            "allowed_domains": domains,
            "allowed_actions": [format!("{ORIGIN_EXTENSION_ACTION_PREFIX}{scope}")],
        }))
        .expect("grant json")
    }

    #[test]
    fn the_flag_defaults_off() {
        if std::env::var(ORIGIN_CONSENT_FLAG).is_err() {
            assert!(!origin_consent_enabled());
        }
        assert_eq!(OriginExtensionRefusal::FlagOff.to_error().code.http_status(), 403);
    }

    #[test]
    fn a_well_formed_extension_is_read() {
        let extension = origin_extension_from_grant(
            &grant("conversation", "shop.example.com", &["login.example.net", "cdn.example.org"]),
            &run(false),
        )
        .expect("valid");
        assert_eq!(extension.start_host, "shop.example.com");
        assert_eq!(extension.hosts, vec!["login.example.net", "cdn.example.org"]);
        assert_eq!(extension.scope, ConsentScope::Conversation);
        assert_eq!(extension.all_hosts().len(), 3);
    }

    #[test]
    fn eight_listed_hosts_pass_and_nine_do_not() {
        let eight: Vec<String> = (0..8).map(|i| format!("h{i}.example.com")).collect();
        let eight: Vec<&str> = eight.iter().map(String::as_str).collect();
        assert!(origin_extension_from_grant(&grant("turn", "start.example.com", &eight), &run(false)).is_ok());

        let nine: Vec<String> = (0..9).map(|i| format!("h{i}.example.com")).collect();
        let nine: Vec<&str> = nine.iter().map(String::as_str).collect();
        assert_eq!(
            origin_extension_from_grant(&grant("turn", "start.example.com", &nine), &run(false)),
            Err(OriginExtensionRefusal::TooManyHosts)
        );
    }

    #[test]
    fn the_grant_must_be_a_child_of_this_run_for_this_owner() {
        let mut other_parent = grant("turn", "a.example.com", &[]);
        other_parent.parent_grant_id = Some("grant_run_2".into());
        let mut other_owner = grant("turn", "a.example.com", &[]);
        other_owner.owner_id = "user_2".into();
        let mut wrong_kind = grant("turn", "a.example.com", &[]);
        wrong_kind.kind = "sensitive_approval".into();
        let mut inactive = grant("turn", "a.example.com", &[]);
        inactive.active = false;
        let mut key = grant("turn", "a.example.com", &[]);
        key.key_id = "key_1".into();
        let mut open_web = grant("turn", "a.example.com", &[]);
        open_web.policy_mode = "open_web".into();
        let mut two_actions = grant("turn", "a.example.com", &[]);
        two_actions.allowed_actions.push("upload".into());
        let mut bad_scope = grant("turn", "a.example.com", &[]);
        bad_scope.allowed_actions = vec!["extend_origin:once".into()];
        let mut framed = grant("turn", "a.example.com", &[]);
        framed.allowed_frame_ids = vec!["main".into()];
        for bad in [
            other_parent,
            other_owner,
            wrong_kind,
            inactive,
            key,
            open_web,
            two_actions,
            bad_scope,
            framed,
        ] {
            assert_eq!(
                origin_extension_from_grant(&bad, &run(false)),
                Err(OriginExtensionRefusal::GrantInvalid)
            );
        }
    }

    #[test]
    fn zdr_takes_turn_scope_only_and_the_postures_must_match() {
        let mut zdr_turn = grant("turn", "a.example.com", &[]);
        zdr_turn.zdr = true;
        assert!(origin_extension_from_grant(&zdr_turn, &run(true)).is_ok());

        for scope in ["conversation", "always"] {
            let mut zdr_wide = grant(scope, "a.example.com", &[]);
            zdr_wide.zdr = true;
            assert_eq!(
                origin_extension_from_grant(&zdr_wide, &run(true)),
                Err(OriginExtensionRefusal::ZdrScope)
            );
        }
        // A non-ZDR grant never backs a ZDR run, nor the reverse.
        assert_eq!(
            origin_extension_from_grant(&grant("turn", "a.example.com", &[]), &run(true)),
            Err(OriginExtensionRefusal::GrantInvalid)
        );
        assert_eq!(
            origin_extension_from_grant(&zdr_turn, &run(false)),
            Err(OriginExtensionRefusal::GrantInvalid)
        );
    }

    #[test]
    fn hosts_must_be_canonical() {
        for host in [
            "Example.com",
            "example.com.",
            "https://example.com",
            "example.com:443",
            "*.example.com",
            "10.0.0.1",
            "localhost",
            "printer.local",
            "svc.internal",
            "router.home.arpa",
            "-bad.example.com",
            "example",
            "",
        ] {
            assert!(!is_canonical_host(host), "{host:?} is not canonical");
        }
        for host in ["example.com", "xn--bcher-kva.example", "a-b.c1.example.org"] {
            assert!(is_canonical_host(host), "{host:?} is canonical");
        }
        assert_eq!(
            origin_extension_from_grant(&grant("turn", "a.example.com", &["B.example.com"]), &run(false)),
            Err(OriginExtensionRefusal::HostInvalid)
        );
        // The start host must be in the list, once; a listed host appears once.
        let mut no_start = grant("turn", "a.example.com", &["b.example.com"]);
        no_start.allowed_domains.remove(0);
        assert_eq!(
            origin_extension_from_grant(&no_start, &run(false)),
            Err(OriginExtensionRefusal::HostInvalid)
        );
        assert_eq!(
            origin_extension_from_grant(
                &grant("turn", "a.example.com", &["b.example.com", "b.example.com"]),
                &run(false)
            ),
            Err(OriginExtensionRefusal::HostInvalid)
        );
    }

    #[test]
    fn org_consent_caps_tighten_only() {
        let snap = |caps: serde_json::Value| json!({ "v": 1, "rev": "1", "caps": caps });
        assert_eq!(org_consent_refusal(None, ConsentScope::Always), None);
        assert_eq!(org_consent_refusal(Some(&snap(json!({}))), ConsentScope::Always), None);
        assert_eq!(
            org_consent_refusal(
                Some(&snap(json!({ "browser.consent.origin": "disabled" }))),
                ConsentScope::Turn
            ),
            Some(OriginExtensionRefusal::OrgDisabled)
        );
        let max_conversation = snap(json!({ "browser.consent.max_scope": "conversation" }));
        assert_eq!(org_consent_refusal(Some(&max_conversation), ConsentScope::Conversation), None);
        assert_eq!(
            org_consent_refusal(Some(&max_conversation), ConsentScope::Always),
            Some(OriginExtensionRefusal::OrgScope)
        );
        for unknown in [
            json!({ "browser.consent.origin": "forced" }),
            json!({ "browser.consent.max_scope": "once" }),
            json!({ "browser.consent.max_scope": 3 }),
        ] {
            assert_eq!(
                org_consent_refusal(Some(&snap(unknown)), ConsentScope::Turn),
                Some(OriginExtensionRefusal::OrgUnreadable)
            );
        }
    }

    fn policy_denying(deny: &[&str]) -> ResolvedPolicy {
        PolicyBaseline::default()
            .decide(
                Principal::Human,
                false,
                Some(
                    RequestAsk {
                        deny_domains: deny.iter().map(|host| (*host).to_owned()).collect(),
                        ..Default::default()
                    }
                    .into_layer()
                    .expect("a valid ask"),
                ),
            )
            .expect("a decision")
            .policy()
            .clone()
    }

    #[test]
    fn any_deny_beats_the_approval() {
        let extension = origin_extension_from_grant(
            &grant("turn", "shop.example.com", &["login.example.net", "ads.tracker.com"]),
            &run(false),
        )
        .expect("valid");

        let allows = policy_denying(&["other.example"]);
        assert!(!origin_extension_denied(&allows, None, &[], &extension));
        // The run's decision denies a parent of one host.
        assert!(origin_extension_denied(&policy_denying(&["tracker.com"]), None, &[], &extension));
        // The step layer (the grant snapshot) denies one.
        assert!(origin_extension_denied(
            &allows,
            Some(&policy_denying(&["login.example.net"])),
            &[],
            &extension
        ));
        // The run's own denied list (an open-web grant's) denies the start host.
        assert!(origin_extension_denied(&allows, None, &["example.com".into()], &extension));
    }

    #[test]
    fn the_allow_list_gains_the_extension_and_an_unconstrained_one_stays_so() {
        let extension = origin_extension_from_grant(
            &grant("turn", "shop.example.com", &["login.example.net"]),
            &run(false),
        )
        .expect("valid");
        let extended = extended_allowed_domains(&["shop.example.com".into()], &extension)
            .expect("a constrained run is widened");
        assert_eq!(extended, vec!["shop.example.com", "login.example.net"]);
        let egress = BrowserEgressPolicy::from_allowed_domains(&extended);
        assert!(egress.allows_url("https://login.example.net/sso"));
        assert!(!egress.allows_url("https://elsewhere.example/"));
        assert_eq!(extended_allowed_domains(&[], &extension), None);
    }
}

#[cfg(test)]
mod attachment_policy_tests {
    use super::*;
    #[test]
    fn attachment_ask_restricts_actions_and_the_browser_network_boundary() {
        let ask = RequestAsk {
            disable: vec!["browser.act.interact".into()],
            allow_domains: Some(vec!["docs.example.com".into()]),
            deny_domains: vec!["ads.docs.example.com".into()],
            ..Default::default()
        };
        let policy = attachment_policy(&ask).unwrap();
        assert!(gate_attachment_action(&policy, &AgentAction::Click { selector: "button".into() }).is_err());
        assert!(gate_attachment_action(&policy, &AgentAction::Wait { ms: 0 }).is_ok());
        let boundary = attachment_egress_policy(&policy, &["example.com".into()], &ask);
        assert!(boundary.allows_url("https://docs.example.com/"));
        assert!(!boundary.allows_url("https://ads.docs.example.com/"));
        assert!(!boundary.allows_url("https://example.com/"));
        assert!(attachment_policy(&RequestAsk { disable: vec!["unknown.capability".into()], ..Default::default() }).is_err());
    }

    #[test]
    fn attachment_intersection_never_expands_the_grant() {
        let ask = RequestAsk { allow_domains: Some(vec!["example.com".into()]), ..Default::default() };
        let policy = attachment_policy(&ask).unwrap();
        let narrow = attachment_egress_policy(&policy, &["docs.example.com".into()], &ask);
        assert!(narrow.allows_url("https://docs.example.com/"));
        assert!(!narrow.allows_url("https://example.com/"));
        assert!(!narrow.allows_url("https://other.example.com/"));
        let disjoint = attachment_egress_policy(&policy, &["other.test".into()], &ask);
        assert!(!disjoint.allows_url("https://example.com/"));
        assert!(!disjoint.allows_url("https://other.test/"));
    }
}
