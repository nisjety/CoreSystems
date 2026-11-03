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

use quarry_browser::BrowserEgressPolicy;
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
    egress_for_narrowing(
        decision.policy().narrow_egress(allowed_domains),
        allowed_domains,
        &denied,
    )
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
