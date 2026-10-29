//! The one case where a broker-validated browser run grant stands in for the
//! approval a high-risk capability `ask` would otherwise park the step for.
//!
//! `cap.browser.open` is high risk, so capability-core answers `ask` for every
//! `browser_agent` step. On the managed browser-run path the user has already
//! said yes, in a narrower form: they acquired a session grant with a bounded
//! domain list, started this run with their own bearer, and model-gateway
//! derived a run grant from it for exactly this run (same owner and tenant,
//! hosts no wider, ZDR inherited, expiry and revocation tied to the parent).
//! `ExecuteStep` then re-validated that grant at the broker with the user's own
//! delegated browser bearer before this module ever sees it.
//!
//! So the lift needs all of: the `browser_agent` tool; an `ask` from the
//! high-risk floor itself (`risk_level`) on `cap.browser.open`, whose signed
//! evidence verified; a broker-validated derived run grant owned by the user the
//! run acts for; and the tool input naming this run. Anything less keeps the
//! ask. It never lifts a deny, another capability, another browser tool, an ask
//! from an approval floor or a stored policy, or the hook and permission gates
//! that run after it. The agent loop and scheduled steps hold no validated grant,
//! so they can never lift.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use super::BROWSER_AGENT_TOOL;
use crate::capability_policy::{CapabilityDecision, CapabilityEvaluation};
use crate::tool_bridge::ValidatedBrowserGrant;

/// The capability every browser tool is bound to.
const BROWSER_CAPABILITY_ID: &str = "cap.browser.open";
/// The policy source of the high-risk floor, the only ask a grant satisfies.
const RISK_LEVEL_SOURCE: &str = "risk_level";
/// A grant derived from a session grant for one run.
const RUN_GRANT_KIND: &str = "run";

/// Why a high-risk ask was satisfied, as metadata only: nothing here is a URL,
/// a domain, a goal or a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GrantApproval {
    /// capability-core's audit id for the ask that was satisfied.
    pub(crate) decision_id: String,
    pub(crate) capability_version: String,
    /// The browser plan the run starts, as the tool input named it; may be empty.
    pub(crate) plan_id: String,
    /// The first 12 hex characters of sha256(grant id): enough to correlate,
    /// not the id itself.
    pub(crate) grant_ref: String,
    /// The broker holds the grant under Zero Data Retention.
    pub(crate) grant_zdr: bool,
}

/// `Some` only when every condition in the module doc holds.
pub(crate) fn satisfies_high_risk_ask(
    tool_name: &str,
    tool_input: &str,
    evaluation: &CapabilityEvaluation,
    browser_grant: Option<&ValidatedBrowserGrant>,
    user_id: &str,
    run_id: &str,
) -> Option<GrantApproval> {
    if tool_name != BROWSER_AGENT_TOOL
        || evaluation.decision != CapabilityDecision::Ask
        || evaluation.policy_source != RISK_LEVEL_SOURCE
        || evaluation.capability_id != BROWSER_CAPABILITY_ID
        || !evaluation.ask_evidence_verified
    {
        return None;
    }
    let grant = browser_grant?;
    let user_id = user_id.trim();
    if grant.kind() != RUN_GRANT_KIND
        || grant.parent_grant_id().trim().is_empty()
        || user_id.is_empty()
        || grant.owner_id() != user_id
    {
        return None;
    }
    let input: serde_json::Value = serde_json::from_str(tool_input).ok()?;
    let named_run = input.get("run_id").and_then(serde_json::Value::as_str)?;
    if run_id.trim().is_empty() || named_run != run_id {
        return None;
    }
    let plan_id = input
        .get("plan_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Some(GrantApproval {
        decision_id: evaluation.decision_id.clone(),
        capability_version: evaluation.capability_version.clone(),
        plan_id,
        grant_ref: grant_ref(grant.grant_id()),
        grant_zdr: grant.zdr(),
    })
}

fn grant_ref(grant_id: &str) -> String {
    Sha256::digest(grant_id.as_bytes()).iter().take(6).fold(
        String::with_capacity(12),
        |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = "user_test";
    const RUN: &str = "run_test";

    fn ask() -> CapabilityEvaluation {
        CapabilityEvaluation {
            decision: CapabilityDecision::Ask,
            decision_id: "pdec_1".to_owned(),
            capability_version: "1.0.0".to_owned(),
            evidence_verified: true,
            policy_source: "risk_level".to_owned(),
            side_effect_class: String::new(),
            approval_floor: String::new(),
            capability_id: "cap.browser.open".to_owned(),
            reason: "high risk".to_owned(),
            ask_evidence_verified: true,
        }
    }

    fn run_grant() -> ValidatedBrowserGrant {
        ValidatedBrowserGrant::for_test("grant-run", "run", "grant-session", USER, false)
    }

    fn input() -> String {
        serde_json::json!({
            "grant_id": "grant-run",
            "run_id": RUN,
            "plan_id": "plan_x",
            "start_url": "https://example.com/",
        })
        .to_string()
    }

    fn lift(
        tool: &str,
        tool_input: &str,
        evaluation: &CapabilityEvaluation,
        grant: Option<&ValidatedBrowserGrant>,
        user: &str,
    ) -> Option<GrantApproval> {
        satisfies_high_risk_ask(tool, tool_input, evaluation, grant, user, RUN)
    }

    #[test]
    fn a_validated_run_grant_of_the_runs_user_satisfies_the_high_risk_ask() {
        let grant = run_grant();
        let approval = lift("browser_agent", &input(), &ask(), Some(&grant), USER)
            .expect("every condition holds");
        assert_eq!(approval.decision_id, "pdec_1");
        assert_eq!(approval.capability_version, "1.0.0");
        assert_eq!(approval.plan_id, "plan_x");
        assert_eq!(approval.grant_ref.len(), 12);
        assert!(approval.grant_ref.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!approval.grant_ref.contains("grant"));
        assert!(!approval.grant_zdr);

        let zdr = ValidatedBrowserGrant::for_test("grant-run", "run", "grant-session", USER, true);
        assert!(
            lift("browser_agent", &input(), &ask(), Some(&zdr), USER)
                .expect("a ZDR grant lifts too")
                .grant_zdr
        );
    }

    #[test]
    fn only_browser_agent_is_lifted() {
        let grant = run_grant();
        for tool in ["browser.act", "browser.observe", "shell", ""] {
            assert!(
                lift(tool, &input(), &ask(), Some(&grant), USER).is_none(),
                "{tool:?}"
            );
        }
    }

    #[test]
    fn only_the_high_risk_floor_is_lifted() {
        let grant = run_grant();
        for source in ["approval_floor", "tool_policy_ask", "capability_gate", ""] {
            let evaluation = CapabilityEvaluation {
                policy_source: source.to_owned(),
                ..ask()
            };
            assert!(
                lift("browser_agent", &input(), &evaluation, Some(&grant), USER).is_none(),
                "{source:?}"
            );
        }
    }

    #[test]
    fn allow_and_deny_are_not_asks() {
        let grant = run_grant();
        for decision in [CapabilityDecision::Allow, CapabilityDecision::Deny] {
            let evaluation = CapabilityEvaluation { decision, ..ask() };
            assert!(lift("browser_agent", &input(), &evaluation, Some(&grant), USER).is_none());
        }
    }

    #[test]
    fn only_the_browser_capability_is_lifted() {
        let grant = run_grant();
        for capability in ["cap.command.shell", "cap.browser", ""] {
            let evaluation = CapabilityEvaluation {
                capability_id: capability.to_owned(),
                ..ask()
            };
            assert!(
                lift("browser_agent", &input(), &evaluation, Some(&grant), USER).is_none(),
                "{capability:?}"
            );
        }
    }

    #[test]
    fn an_ask_whose_evidence_did_not_verify_is_not_lifted() {
        let grant = run_grant();
        let evaluation = CapabilityEvaluation {
            ask_evidence_verified: false,
            ..ask()
        };
        assert!(lift("browser_agent", &input(), &evaluation, Some(&grant), USER).is_none());
    }

    #[test]
    fn no_validated_grant_no_lift() {
        assert!(lift("browser_agent", &input(), &ask(), None, USER).is_none());
    }

    #[test]
    fn only_a_derived_run_grant_lifts() {
        for kind in ["session", "sensitive_approval", "", "Run"] {
            let grant =
                ValidatedBrowserGrant::for_test("grant-run", kind, "grant-session", USER, false);
            assert!(
                lift("browser_agent", &input(), &ask(), Some(&grant), USER).is_none(),
                "{kind:?}"
            );
        }
        for parent in ["", "   "] {
            let grant = ValidatedBrowserGrant::for_test("grant-run", "run", parent, USER, false);
            assert!(
                lift("browser_agent", &input(), &ask(), Some(&grant), USER).is_none(),
                "{parent:?}"
            );
        }
    }

    #[test]
    fn the_grant_must_belong_to_the_user_the_run_acts_for() {
        let grant = run_grant();
        assert!(lift(
            "browser_agent",
            &input(),
            &ask(),
            Some(&grant),
            "user_other"
        )
        .is_none());
        assert!(lift("browser_agent", &input(), &ask(), Some(&grant), "").is_none());
        assert!(lift("browser_agent", &input(), &ask(), Some(&grant), "  ").is_none());
        let unowned =
            ValidatedBrowserGrant::for_test("grant-run", "run", "grant-session", "", false);
        assert!(lift("browser_agent", &input(), &ask(), Some(&unowned), "").is_none());
    }

    #[test]
    fn the_tool_input_must_name_this_run() {
        let grant = run_grant();
        let missing = serde_json::json!({ "grant_id": "grant-run" }).to_string();
        let other =
            serde_json::json!({ "grant_id": "grant-run", "run_id": "run_other" }).to_string();
        let not_a_string = serde_json::json!({ "grant_id": "grant-run", "run_id": 7 }).to_string();
        for tool_input in [
            missing.as_str(),
            other.as_str(),
            not_a_string.as_str(),
            "not json",
        ] {
            assert!(
                lift("browser_agent", tool_input, &ask(), Some(&grant), USER).is_none(),
                "{tool_input}"
            );
        }
        // An empty authorized run never matches, even an empty named one.
        let empty = serde_json::json!({ "grant_id": "grant-run", "run_id": "" }).to_string();
        assert!(
            satisfies_high_risk_ask("browser_agent", &empty, &ask(), Some(&grant), USER, "")
                .is_none()
        );
    }
}
