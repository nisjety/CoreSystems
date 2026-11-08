//! INNH-10: what `POST /v1/agent/runs` does for an `api_key` principal.
//!
//! The route fence (`crate::api_key`) decides which routes a key may call. This module decides
//! what a key may START: no resume, no profile, a grant that was issued to this very key, on
//! hosts the grant names, under a budget the grant carries, and never an open-web or
//! human-browse grant. The per-step half (the grant still belongs to the key, no sensitive
//! actions) is in `admission`.
//!
//! Every function here is pure or logs only ids, so the table is unit-tested without a router.

use super::*;

use crate::api_key::{reason, refusal_error};

/// The 403 for a refused key start, with the reason in `error.details.reason`. Logs the reason
/// and the key id (never a host, a grant's hosts or a token).
pub(super) fn refuse(claims: &crate::auth::Claims, request_id: &str, reason: &'static str) -> ApiErr {
    tracing::warn!(
        reason,
        key_id = claims.key_id().unwrap_or(""),
        org_id = %claims.org_id,
        "agent.run.refused"
    );
    (
        StatusCode::FORBIDDEN,
        Json(Envelope::<()>::err(request_id, refusal_error(reason))),
    )
}

/// The rules on the request itself, before any grant is read or any slot is taken.
///
/// - the switch is on and the token carries `browser:agent` (the fence already said so; a
///   second check, because the start must not depend on how it was reached);
/// - no `resume_run_id`: a resume reads a durable checkpoint, and a key run keeps none that a
///   key could present again (`api_key_resume_refused`);
/// - no `profile_id`, no `persist_profile`: a key run always gets a fresh, run-scoped profile
///   (`api_key_profile_refused`);
/// - a `grant_id`, in every environment, whatever `REQUIRE_BROWSER_GRANTS` says
///   (`api_key_grant_required`);
/// - run budgets are enforced (`QUARRY_EDGE__BUDGET_ENFORCEMENT=on`): the grant's budget is
///   what bounds a key's spend, and in `shadow` or `off` nothing would stop a run at it
///   (`api_key_budget_not_enforced`).
pub(super) fn check_start_request(
    claims: &crate::auth::Claims,
    body: &StartRunBody,
    enabled: bool,
    budget_enforced: bool,
) -> Result<(), &'static str> {
    if !enabled {
        return Err(reason::DISABLED);
    }
    if !claims.has_scope(crate::auth::SCOPE_BROWSER_AGENT) {
        return Err(reason::SCOPE_MISSING);
    }
    if body.resume_run_id.is_some() {
        return Err(reason::RESUME_REFUSED);
    }
    if body.profile_id.is_some() || body.persist_profile {
        return Err(reason::PROFILE_REFUSED);
    }
    if body
        .grant_id
        .as_deref()
        .map_or(true, |grant_id| grant_id.trim().is_empty())
    {
        return Err(reason::GRANT_REQUIRED);
    }
    if !budget_enforced {
        return Err(reason::BUDGET_NOT_ENFORCED);
    }
    Ok(())
}

/// Binds a validated broker grant to a key's run, or says why not. Returns the grant's
/// canonical hosts, which ALWAYS replace the request's `allowed_domains` (in every environment,
/// where for a person they replace them only when grants are required or the grant carries a
/// policy).
///
/// The grant must be:
/// - an allow-list session grant: kind `""` or `session`, policy mode `""` or `allowlist`.
///   `human_browse` and `open_web` are `api_key_open_web_refused`; every other kind or mode
///   (a run, a sensitive approval, a derived attach grant, an unknown mode) is
///   `api_key_grant_kind_refused`;
/// - issued to THIS key and THIS key's owner: `grant.key_id` equals the token's `key_id` and
///   `grant.owner_id` equals its `acting_user_id`. An empty `key_id` (a grant of a person, an
///   older broker, the gRPC validator until the proto carries the field, the no-op validator)
///   never matches (`api_key_grant_owner_mismatch`). Unlike a service principal, which is not
///   bound to the grant's owner, a key is bound to both;
/// - carrying a non-empty, canonical host list (`api_key_grant_hosts_required`);
/// - carrying a budget claim that bounds something (`api_key_grant_budget_required`).
pub(super) fn bind_key_grant(
    grant: &quarry_runtime::GrantValidation,
    claims: &crate::auth::Claims,
) -> Result<Vec<String>, &'static str> {
    if grant.kind == super::human_browse::GRANT_KIND_HUMAN_BROWSE || grant.policy_mode == "open_web" {
        return Err(reason::OPEN_WEB_REFUSED);
    }
    if !matches!(grant.kind.as_str(), "" | "session")
        || !matches!(grant.policy_mode.as_str(), "" | "allowlist")
        || grant.parent_grant_id.is_some()
        || !grant.resource_id.is_empty()
    {
        return Err(reason::GRANT_KIND_REFUSED);
    }
    let (Some(key_id), Some(owner)) = (claims.key_id(), claims.key_owner_user_id()) else {
        return Err(reason::GRANT_OWNER_MISMATCH);
    };
    if grant.key_id.is_empty() || grant.key_id != key_id || grant.owner_id != owner {
        return Err(reason::GRANT_OWNER_MISMATCH);
    }
    let hosts = BrowserEgressPolicy::canonical_broker_domains(&grant.allowed_domains)
        .map_err(|_| reason::GRANT_HOSTS_REQUIRED)?;
    if !grant.budget.as_ref().is_some_and(|claim| claim.is_set()) {
        return Err(reason::GRANT_BUDGET_REQUIRED);
    }
    Ok(hosts)
}

/// `agent.run.started` for a key run: ids and the class only, no hosts (ZDR or not).
pub(super) fn log_started(entry: &RunEntry, class: &'static str) {
    tracing::info!(
        run_id = %entry.run_id,
        org_id = %entry.org_id,
        auth_method = "api_key",
        key_id = entry.key_id.as_deref().unwrap_or(""),
        grant_id = entry.grant_id.as_deref().unwrap_or(""),
        zdr = entry.zdr.is_active(),
        class,
        "agent.run.started"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_runtime::GrantValidation;
    use quarry_runtime::grant_validator::GrantBudget;

    fn key_claims() -> crate::auth::Claims {
        let mut claims = crate::test_support::claims_for_org("org");
        claims.principal_type = Some("api_key".into());
        claims.sub = "apikey:key-1".into();
        claims.user_id = "apikey:key-1".into();
        claims.key_id = Some("key-1".into());
        claims.acting_user_id = Some("user-owner".into());
        claims.scopes = vec![crate::auth::SCOPE_BROWSER_AGENT.into()];
        claims
    }

    fn key_grant() -> GrantValidation {
        serde_json::from_value(serde_json::json!({
            "grant_id": "g1",
            "active": true,
            "kind": "session",
            "owner_id": "user-owner",
            "key_id": "key-1",
            "allowed_domains": ["example.com"],
            "budget": { "micro_usd": 1_000_000, "max_steps": 50, "max_runtime_s": 600 },
        }))
        .expect("a grant")
    }

    fn body(json: serde_json::Value) -> StartRunBody {
        serde_json::from_value(json).expect("a start body")
    }

    #[test]
    fn a_grant_issued_to_this_key_binds_and_yields_its_hosts() {
        assert_eq!(
            bind_key_grant(&key_grant(), &key_claims()),
            Ok(vec!["example.com".to_owned()])
        );
        // The kind and mode may be empty (a broker that predates them) or spelled out.
        let mut grant = key_grant();
        grant.kind = String::new();
        grant.policy_mode = "allowlist".into();
        assert!(bind_key_grant(&grant, &key_claims()).is_ok());
    }

    #[test]
    fn a_grant_that_is_not_this_keys_is_refused_in_every_direction() {
        let mismatch = Err(reason::GRANT_OWNER_MISMATCH);
        // Another key of the same owner.
        let mut grant = key_grant();
        grant.key_id = "key-2".into();
        assert_eq!(bind_key_grant(&grant, &key_claims()), mismatch);
        // The owner's own grant, issued to the person: no key id.
        let mut grant = key_grant();
        grant.key_id = String::new();
        assert_eq!(bind_key_grant(&grant, &key_claims()), mismatch);
        // Another owner.
        let mut grant = key_grant();
        grant.owner_id = "user-other".into();
        assert_eq!(bind_key_grant(&grant, &key_claims()), mismatch);
        // An empty owner (older broker, no-op validator).
        let mut grant = key_grant();
        grant.owner_id = String::new();
        assert_eq!(bind_key_grant(&grant, &key_claims()), mismatch);
        // A person's token never binds a key grant: the key principal is the only one that does.
        let person = crate::test_support::claims_for_org("org");
        assert_eq!(bind_key_grant(&key_grant(), &person), mismatch);
    }

    #[test]
    fn open_web_and_human_browse_are_refused_before_anything_else() {
        let mut grant = key_grant();
        grant.kind = "human_browse".into();
        assert_eq!(bind_key_grant(&grant, &key_claims()), Err(reason::OPEN_WEB_REFUSED));
        let mut grant = key_grant();
        grant.policy_mode = "open_web".into();
        grant.allowed_domains = Vec::new();
        assert_eq!(bind_key_grant(&grant, &key_claims()), Err(reason::OPEN_WEB_REFUSED));
    }

    #[test]
    fn only_a_plain_session_allowlist_grant_backs_a_key_run() {
        for kind in ["run", "sensitive_approval", "attach", "anything"] {
            let mut grant = key_grant();
            grant.kind = kind.into();
            assert_eq!(
                bind_key_grant(&grant, &key_claims()),
                Err(reason::GRANT_KIND_REFUSED),
                "kind {kind}"
            );
        }
        let mut grant = key_grant();
        grant.policy_mode = "something_new".into();
        assert_eq!(bind_key_grant(&grant, &key_claims()), Err(reason::GRANT_KIND_REFUSED));
        let mut grant = key_grant();
        grant.parent_grant_id = Some("parent".into());
        assert_eq!(bind_key_grant(&grant, &key_claims()), Err(reason::GRANT_KIND_REFUSED));
        let mut grant = key_grant();
        grant.resource_id = "run_1".into();
        assert_eq!(bind_key_grant(&grant, &key_claims()), Err(reason::GRANT_KIND_REFUSED));
    }

    #[test]
    fn the_grant_must_carry_canonical_hosts_and_a_budget() {
        let mut grant = key_grant();
        grant.allowed_domains = Vec::new();
        assert_eq!(bind_key_grant(&grant, &key_claims()), Err(reason::GRANT_HOSTS_REQUIRED));
        // Not canonical: unsorted, upper case, a wildcard.
        for domains in [vec!["b.com", "a.com"], vec!["EXAMPLE.com"], vec!["*.example.com"]] {
            let mut grant = key_grant();
            grant.allowed_domains = domains.iter().map(|d| (*d).to_owned()).collect();
            assert_eq!(
                bind_key_grant(&grant, &key_claims()),
                Err(reason::GRANT_HOSTS_REQUIRED),
                "{domains:?}"
            );
        }
        let mut grant = key_grant();
        grant.budget = None;
        assert_eq!(bind_key_grant(&grant, &key_claims()), Err(reason::GRANT_BUDGET_REQUIRED));
        // A claim that bounds nothing is no claim.
        grant.budget = Some(GrantBudget::default());
        assert_eq!(bind_key_grant(&grant, &key_claims()), Err(reason::GRANT_BUDGET_REQUIRED));
    }

    #[test]
    fn a_key_is_an_agent_whatever_it_sends_and_its_identity_is_the_key() {
        let claims = key_claims();
        for hint in [None, Some(InitiatorHint::Human), Some(InitiatorHint::Agent)] {
            let initiator = derive_initiator_ref(&claims, hint, &AdvisoryAgent::default());
            assert_eq!(initiator.class, StepInitiator::Agent, "hint {hint:?}");
            let agent = initiator.agent.expect("an agent identity");
            assert_eq!(agent.basis, AgentIdentityBasis::ApiKey);
            assert_eq!(agent.principal_id.as_deref(), Some("apikey:key-1"));
            assert!(agent.is_verified());
        }
        // A person with the same hint stays a person, and is not verified as an agent.
        let person = crate::test_support::claims_for_org("org");
        assert_eq!(
            derive_initiator_ref(&person, Some(InitiatorHint::Human), &AdvisoryAgent::default()).class,
            StepInitiator::Human
        );
    }

    #[test]
    fn a_key_is_an_autonomous_principal_for_the_web_policy() {
        assert!(matches!(
            crate::web_policy_edge::principal(&key_claims()),
            quarry_core::web_policy::Principal::Autonomous
        ));
        assert!(matches!(
            crate::web_policy_edge::principal(&crate::test_support::claims_for_org("org")),
            quarry_core::web_policy::Principal::Human
        ));
    }

    #[test]
    fn the_start_request_rules_are_checked_in_order() {
        let claims = key_claims();
        let ok = body(serde_json::json!({ "grant_id": "g1" }));
        assert_eq!(check_start_request(&claims, &ok, true, true), Ok(()));
        assert_eq!(check_start_request(&claims, &ok, false, true), Err(reason::DISABLED));
        let mut no_scope = key_claims();
        no_scope.scopes.clear();
        assert_eq!(check_start_request(&no_scope, &ok, true, true), Err(reason::SCOPE_MISSING));
        let no_grant = body(serde_json::json!({}));
        assert_eq!(check_start_request(&claims, &no_grant, true, true), Err(reason::GRANT_REQUIRED));
        let blank = body(serde_json::json!({ "grant_id": "  " }));
        assert_eq!(check_start_request(&claims, &blank, true, true), Err(reason::GRANT_REQUIRED));
        let resume = body(serde_json::json!({ "grant_id": "g1", "resume_run_id": "run_1" }));
        assert_eq!(check_start_request(&claims, &resume, true, true), Err(reason::RESUME_REFUSED));
        let profile = body(serde_json::json!({ "grant_id": "g1", "profile_id": "prof_1" }));
        assert_eq!(check_start_request(&claims, &profile, true, true), Err(reason::PROFILE_REFUSED));
        let persist = body(serde_json::json!({ "grant_id": "g1", "persist_profile": true }));
        assert_eq!(check_start_request(&claims, &persist, true, true), Err(reason::PROFILE_REFUSED));
        // Budget enforcement short of `on` refuses: the claim would bound nothing.
        assert_eq!(
            check_start_request(&claims, &ok, true, false),
            Err(reason::BUDGET_NOT_ENFORCED)
        );
    }
}
