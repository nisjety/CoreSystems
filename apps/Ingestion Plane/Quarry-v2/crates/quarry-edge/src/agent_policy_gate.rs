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
//! Organisation layer: absent until WS7.H and WS7.J. The baseline and the request layer
//! are the only operator and caller inputs here.

use quarry_browser::BrowserEgressPolicy;
use quarry_core::contracts::AgentAction;
use quarry_core::web_policy::{
    CapKey, EgressNarrowing, PolicyDenial, Principal, RequestAsk, ResolvedPolicy,
};
use quarry_core::QuarryResult;

use crate::agent_routes::RunLive;
use crate::policy_baseline::{Decision, PolicyBaseline};
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

#[cfg(test)]
mod tests {
    use super::*;

    use quarry_core::web_policy::{resolve, OrgInput, PolicyInputs, PolicyLayer};

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
}
