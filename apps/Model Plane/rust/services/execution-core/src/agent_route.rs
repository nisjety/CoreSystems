//! The inference route of one governed agent run.
//!
//! A run's loop (and every delegated subagent's) can be pinned to the user's
//! ChatGPT subscription: `RunAgentRequest.provider_hint` (16) names the
//! canonical provider and `subscription_connection_id` (17) the connection.
//! This module is the whole of what that means here:
//!
//! * [`validate_run_route`] decides, from the request and the kill switch,
//!   whether such a run may start at all;
//! * [`ModelRoute::resolve`] turns a valid request into the one route every
//!   inference round of the run uses, built ONCE so a delegated child cannot
//!   re-derive it differently;
//! * [`ModelRoute::verify_response`] is the guard each round's answer must pass.
//!
//! # The failure this exists to make impossible
//!
//! The loop's model comes from `EXECUTION_AGENT_MODEL`, which compose defaults
//! to `model-router`: Azure, paid. A run that was meant for the subscription
//! must not be able to land there by any ordinary route:
//!
//! * the environment is not consulted for a pinned run (the request's model is
//!   used unchanged);
//! * the provider and connection ride on every `InferRequest`, so inference-core
//!   never falls through to another provider for it;
//! * a response that was nevertheless served by another provider or model ends
//!   the run: no further round, in this loop or in any other loop of the run;
//! * a half-set or malformed route is refused rather than ignored, because
//!   ignoring it is exactly the silent fallback.
//!
//! What this cannot do is protect a run from an `execution-core` that predates
//! the fields: that binary skips them and never runs this code. The kill switch
//! (`mp_contracts::agentic_subscription`) is the guard for that rollout order.
//!
//! Everything here is pure: the switches and the environment value arrive as
//! arguments, so a test never touches the process environment.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mp_contracts::agentic_subscription::{self as subscription, Switches};
use mp_contracts::model_plane::v1 as pb;
use tonic::Status;

use crate::auth::AuthenticatedUser;
use mp_contracts::retention::{resolve_turn, Resolution, RouteView};

/// A response that was served by something other than the route a run is
/// pinned to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RouteViolation {
    pub served_provider: String,
    pub served_model: String,
}

/// Where the inference rounds of one run may go.
///
/// Cloning shares the stop flag: the parent's loop and every delegated child's
/// hold the same flag, so the first round served by the wrong route stops them
/// all.
#[derive(Clone, Debug)]
pub(crate) struct ModelRoute {
    /// The model every round asks for. For a pinned run, exactly the request's.
    model: String,
    /// `Some` when the run is pinned to the subscription route.
    subscription_connection_id: Option<String>,
    /// `true` when the caller holds the waiver for this route (Z-18): the run is
    /// persistent for its user, on the one connection the waiver binds, with ZDR
    /// off for this run only. Every delegated child shares it.
    waived: bool,
    stopped: Arc<AtomicBool>,
}

/// The model an UNPINNED run drives. Autonomous, tool-using runs need a
/// tool-following model; a weak chat-tier model under-elects tools. When
/// `EXECUTION_AGENT_MODEL` is set (e.g. `model-router`, which selects the best
/// model per turn) it becomes the run's model; empty or unset, the requested
/// model is honoured unchanged.
///
/// Never called for a pinned run: that run's model is not the environment's to
/// choose.
#[must_use]
pub(crate) fn default_route_model(requested: &str, env_agent_model: Option<&str>) -> String {
    match env_agent_model.map(str::trim) {
        Some(value) if !value.is_empty() => value.to_owned(),
        _ => requested.to_owned(),
    }
}

/// Whether the request names a route at all: either field set. A request that
/// sets one half has asked for the route and is judged as having asked for it.
fn route_requested(req: &pb::RunAgentRequest) -> bool {
    !req.provider_hint.is_empty() || !req.subscription_connection_id.is_empty()
}

/// Whether a run may start on the route the request names.
///
/// A request that names none is accepted, as it always was, unless the caller
/// holds a waiver (which binds one route) or is a persistent token of a ZDR org
/// (Z-18). One that names the route must satisfy every condition below, and is
/// refused (never run on a default route instead) when it does not:
///
/// 1. the kill switch is on;
/// 2. the provider is the canonical registry id, not an alias;
/// 3. a connection is present;
/// 4. the model is concrete, not a `verevon-*` pseudo name or empty;
/// 5. the caller's retention allows this route (Z-18, the matrix): a waiver is
///    honoured for the route it binds and nothing else; a ZDR token and an
///    explicit ZDR request are refused on a connection, which has no ZDR
///    commitment; a persistent token of a ZDR org is refused without its waiver;
/// 6. the run is read-only (`plan_mode`), unless the second switch allows
///    effects. Until plan approval is connected a subscription run is
///    read-only by construction, not by the composer's good behaviour.
///
/// Refusals use the codes the gateway terminalizes as a confirmed dispatch
/// rejection (`PermissionDenied`, `InvalidArgument`), never one that leaves the
/// prepared run open.
///
/// # Errors
///
/// A [`Status`] naming the first condition that failed.
#[allow(clippy::result_large_err)]
pub(crate) fn validate_run_route(
    req: &pb::RunAgentRequest,
    switches: Switches,
    caller: &AuthenticatedUser,
) -> Result<(), Status> {
    if !route_requested(req) {
        // A run that names no route is unchanged, except that a waiver binds one
        // route: a waived caller on the default chain is refused (Z-18). A
        // persistent token of a ZDR org is refused here too
        // (`e_org_zdr_without_waiver_is_not_persistent`).
        return match resolve_turn(
            &[caller.retention_claims()],
            &RouteView {
                provider: "",
                connection: None,
                explicit_zdr: req.zdr,
            },
        ) {
            Resolution::Refuse(refusal) => Err(refusal.grpc_status(tonic::Code::PermissionDenied)),
            _ => Ok(()),
        };
    }
    if !switches.enabled {
        return Err(Status::permission_denied(format!(
            "{}: agentic runs on the ChatGPT subscription are not enabled",
            subscription::DISABLED_CODE
        )));
    }
    if req.provider_hint != subscription::PROVIDER_ID {
        return Err(Status::invalid_argument(format!(
            "provider_hint must be {}",
            subscription::PROVIDER_ID
        )));
    }
    if req.subscription_connection_id.trim().is_empty() {
        return Err(Status::invalid_argument(
            "subscription_connection_id is required with provider_hint",
        ));
    }
    if subscription::is_pseudo_model(&req.model) {
        return Err(Status::invalid_argument(
            "an explicit, concrete model is required on the subscription route",
        ));
    }
    // The retention decision for this route, the matrix's own (Z-18). A waiver is
    // honoured for the route it binds and nothing else; a ZDR token and an explicit
    // ZDR request are refused on a connection, which cannot give zero retention; a
    // persistent token of a ZDR org is refused without its waiver.
    let route = RouteView {
        provider: req.provider_hint.as_str(),
        connection: Some(req.subscription_connection_id.as_str()),
        explicit_zdr: req.zdr,
    };
    if let Resolution::Refuse(refusal) = resolve_turn(&[caller.retention_claims()], &route) {
        return Err(refusal.grpc_status(tonic::Code::PermissionDenied));
    }
    if !req.plan_mode && !switches.effects_permitted() {
        return Err(Status::permission_denied(
            "agentic runs on the ChatGPT subscription are read-only: plan_mode is required",
        ));
    }
    Ok(())
}

impl ModelRoute {
    /// The route of this run, once, after [`validate_run_route`] has passed.
    ///
    /// `env_agent_model` is the value of `EXECUTION_AGENT_MODEL`, passed in
    /// rather than read here. It is used only for an unpinned run.
    ///
    /// # Errors
    ///
    /// Whatever [`validate_run_route`] refuses.
    #[allow(clippy::result_large_err)]
    pub(crate) fn resolve(
        req: &pb::RunAgentRequest,
        switches: Switches,
        env_agent_model: Option<&str>,
        caller: &AuthenticatedUser,
    ) -> Result<Self, Status> {
        validate_run_route(req, switches, caller)?;
        // Validation has already matched the caller's waiver to this route, or
        // refused the run, so a waiver the caller holds here is the route's own.
        let waived = route_requested(req) && caller.zdr_waiver.is_some();
        let (model, subscription_connection_id) = if route_requested(req) {
            // The request's model, byte for byte: it is the model the response
            // guard compares against and the one Integration Core's catalog is
            // checked for.
            (
                req.model.clone(),
                Some(req.subscription_connection_id.clone()),
            )
        } else {
            (default_route_model(&req.model, env_agent_model), None)
        };
        Ok(Self {
            model,
            subscription_connection_id,
            waived,
            stopped: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Whether the run's route is the one the caller's waiver binds (Z-18). Only a
    /// route the request names can be waived.
    pub(crate) fn is_waived(&self) -> bool {
        self.waived
    }

    /// The model each round asks for.
    pub(crate) fn model(&self) -> &str {
        &self.model
    }

    /// The `InferRequest.provider_hint` of each round: the canonical
    /// subscription id for a pinned run, empty (no provider named) otherwise.
    pub(crate) fn provider_hint(&self) -> &str {
        if self.subscription_connection_id.is_some() {
            subscription::PROVIDER_ID
        } else {
            ""
        }
    }

    /// The `InferRequest.subscription_connection_id` of each round: empty for
    /// an unpinned run.
    pub(crate) fn subscription_connection_id(&self) -> &str {
        self.subscription_connection_id.as_deref().unwrap_or("")
    }

    /// Whether the run is pinned to the subscription route.
    pub(crate) fn is_pinned(&self) -> bool {
        self.subscription_connection_id.is_some()
    }

    /// The guard every round's response passes. An unpinned run accepts any
    /// provider (today's behaviour); a pinned run accepts only the
    /// subscription provider serving exactly the requested model.
    ///
    /// The same condition the inline chat loop applies to its own subscription
    /// rounds (model-gateway `tool_loop`), so the two loops cannot disagree
    /// about what a subscription turn is.
    ///
    /// # Errors
    ///
    /// The provider and model that actually served the round.
    pub(crate) fn verify_response(
        &self,
        provider_used: &str,
        model_used: &str,
    ) -> Result<(), RouteViolation> {
        if self.is_pinned()
            && (provider_used != subscription::PROVIDER_ID || model_used != self.model)
        {
            return Err(RouteViolation {
                served_provider: provider_used.to_owned(),
                served_model: model_used.to_owned(),
            });
        }
        Ok(())
    }

    /// Stop the run: no loop of it starts another round.
    pub(crate) fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    /// Whether any loop of the run has stopped it.
    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: Switches = Switches {
        enabled: true,
        allow_effects: false,
    };

    /// A persistent caller of an ordinary org: no waiver, no ZDR claim.
    fn persistent_caller() -> AuthenticatedUser {
        AuthenticatedUser::for_test("org-1", "user-1")
    }

    /// The caller of a ZDR org's persistent token, bound to `connection` by the
    /// waiver Auth Core stamps for that connection (Z-18).
    fn waived_caller(connection: &str) -> AuthenticatedUser {
        AuthenticatedUser::for_test_with_waiver("org-1", "user-1", connection, "ack-1")
    }

    /// The matrix refusal a status carries, read back from its catalogue kind.
    fn refusal_of(status: &Status) -> Option<mp_contracts::zdr_refusal::Refusal> {
        mp_contracts::zdr_refusal::Refusal::from_status(status)
    }

    fn plain_request() -> pb::RunAgentRequest {
        pb::RunAgentRequest {
            run_id: "run-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            ..Default::default()
        }
    }

    fn subscription_request() -> pb::RunAgentRequest {
        pb::RunAgentRequest {
            model: "gpt-5.6-terra".to_owned(),
            provider_hint: "openai-codex-subscription".to_owned(),
            subscription_connection_id: "conn_1".to_owned(),
            plan_mode: true,
            ..plain_request()
        }
    }

    #[test]
    fn a_run_without_route_fields_is_unchanged() {
        // Whatever the switches say, a run that names no route resolves as it
        // always did: the environment model if set, else the requested model,
        // and no provider or connection on any round.
        for switches in [Switches::OFF, ON, Switches::from_values(Some("1"), Some("1"))] {
            let route = ModelRoute::resolve(&plain_request(), switches, None, &persistent_caller())
                .expect("valid");
            assert_eq!(route.model(), "gpt-4o-mini");
            assert_eq!(route.provider_hint(), "");
            assert_eq!(route.subscription_connection_id(), "");
            assert!(!route.is_pinned());

            let overridden = ModelRoute::resolve(
                &plain_request(),
                switches,
                Some("model-router"),
                &persistent_caller(),
            )
            .expect("valid");
            assert_eq!(
                overridden.model(),
                "model-router",
                "EXECUTION_AGENT_MODEL is still honoured for an unpinned run"
            );
            assert_eq!(overridden.provider_hint(), "");
        }
    }

    #[test]
    fn an_empty_or_blank_environment_model_is_the_same_as_unset() {
        for env in [None, Some(""), Some("   ")] {
            assert_eq!(default_route_model("gpt-4o-mini", env), "gpt-4o-mini");
        }
        assert_eq!(
            default_route_model("gpt-4o-mini", Some("  verevon-balance ")),
            "verevon-balance"
        );
    }

    #[test]
    fn a_pinned_run_ignores_the_environment_model() {
        // The compose default. If this ever returned "model-router" the loop
        // would run on Azure while the user believed it was on their seat.
        for env in [None, Some("model-router"), Some("verevon-balance"), Some("")] {
            let route = ModelRoute::resolve(&subscription_request(), ON, env, &persistent_caller())
                .expect("valid pinned run");
            assert_eq!(route.model(), "gpt-5.6-terra", "env {env:?} must not matter");
            assert_eq!(route.provider_hint(), "openai-codex-subscription");
            assert_eq!(route.subscription_connection_id(), "conn_1");
            assert!(route.is_pinned());
        }
    }

    #[test]
    fn a_route_is_refused_with_the_switch_off_never_run_on_the_default() {
        for switches in [
            Switches::OFF,
            Switches::from_values(None, Some("1")),
            Switches::from_values(Some("0"), Some("1")),
        ] {
            let error = ModelRoute::resolve(
                &subscription_request(),
                switches,
                Some("model-router"),
                &persistent_caller(),
            )
            .expect_err("a disabled route must not start");
            assert_eq!(error.code(), tonic::Code::PermissionDenied);
            assert!(
                error.message().contains(subscription::DISABLED_CODE),
                "{}",
                error.message()
            );
        }
        // And the same request with the switch on is fine.
        assert!(
            ModelRoute::resolve(&subscription_request(), ON, None, &persistent_caller()).is_ok()
        );
    }

    #[test]
    fn every_other_condition_is_enforced() {
        type Mutation = fn(&mut pb::RunAgentRequest);
        let cases: [(&str, Mutation, tonic::Code); 9] = [
            (
                "an alias is not the canonical id",
                |req| req.provider_hint = "chatgpt-codex".to_owned(),
                tonic::Code::InvalidArgument,
            ),
            (
                "another provider",
                |req| req.provider_hint = "azure-openai".to_owned(),
                tonic::Code::InvalidArgument,
            ),
            (
                "a connection without a provider",
                |req| req.provider_hint.clear(),
                tonic::Code::InvalidArgument,
            ),
            (
                "a provider without a connection",
                |req| req.subscription_connection_id.clear(),
                tonic::Code::InvalidArgument,
            ),
            (
                "a blank connection",
                |req| req.subscription_connection_id = "  ".to_owned(),
                tonic::Code::InvalidArgument,
            ),
            (
                "no model",
                |req| req.model.clear(),
                tonic::Code::InvalidArgument,
            ),
            (
                "a verevon pseudo model",
                |req| req.model = "verevon-balance".to_owned(),
                tonic::Code::InvalidArgument,
            ),
            (
                "a zero-data-retention run",
                |req| req.zdr = true,
                tonic::Code::PermissionDenied,
            ),
            (
                "a run that may act",
                |req| req.plan_mode = false,
                tonic::Code::PermissionDenied,
            ),
        ];
        for (label, mutate, expected) in cases {
            let mut req = subscription_request();
            mutate(&mut req);
            let error = validate_run_route(&req, ON, &persistent_caller()).expect_err(label);
            assert_eq!(error.code(), expected, "{label}: {}", error.message());
            // Refused, not downgraded: resolve fails too, so no route is ever
            // built from a request validation refused.
            assert!(
                ModelRoute::resolve(&req, ON, Some("model-router"), &persistent_caller()).is_err(),
                "{label}"
            );
        }
    }

    #[test]
    fn pseudo_model_names_are_all_refused() {
        for pseudo in ["verevon", "verevon-auto", "Verevon-Genius", "auto", "default", " "] {
            let mut req = subscription_request();
            req.model = pseudo.to_owned();
            assert_eq!(
                validate_run_route(&req, ON, &persistent_caller())
                    .expect_err(pseudo)
                    .code(),
                tonic::Code::InvalidArgument,
                "{pseudo:?}"
            );
        }
    }

    #[test]
    fn effects_need_both_switches_and_plan_mode_is_the_default() {
        let mut acting = subscription_request();
        acting.plan_mode = false;
        let caller = persistent_caller();
        assert!(validate_run_route(&acting, ON, &caller).is_err());
        assert!(
            validate_run_route(&acting, Switches::from_values(None, Some("1")), &caller).is_err(),
            "the effects switch grants nothing on its own"
        );
        assert!(validate_run_route(
            &acting,
            Switches::from_values(Some("1"), Some("1")),
            &caller
        )
        .is_ok());
        // Plan mode never needed the second switch.
        assert!(validate_run_route(&subscription_request(), ON, &caller).is_ok());
    }

    #[test]
    fn a_pinned_run_accepts_only_the_subscription_provider_serving_the_requested_model() {
        let route = ModelRoute::resolve(&subscription_request(), ON, None, &persistent_caller())
            .expect("valid");
        assert_eq!(
            route.verify_response("openai-codex-subscription", "gpt-5.6-terra"),
            Ok(())
        );
        for (provider, model) in [
            ("azure-openai", "gpt-5.6-terra"),
            ("azure-openai", "model-router"),
            ("", "gpt-5.6-terra"),
            ("openai-codex-subscription", ""),
            ("openai-codex-subscription", "gpt-5.6-luna"),
            ("codex-subscription", "gpt-5.6-terra"),
            ("OpenAI-Codex-Subscription", "gpt-5.6-terra"),
        ] {
            let violation = route
                .verify_response(provider, model)
                .expect_err("a response from another route must not pass");
            assert_eq!(violation.served_provider, provider);
            assert_eq!(violation.served_model, model);
        }
    }

    #[test]
    fn an_unpinned_run_accepts_any_provider_as_before() {
        let route = ModelRoute::resolve(
            &plain_request(),
            Switches::OFF,
            Some("model-router"),
            &persistent_caller(),
        )
        .expect("valid");
        for (provider, model) in [("azure-openai", "gpt-5-mini"), ("", ""), ("anthropic", "x")] {
            assert_eq!(route.verify_response(provider, model), Ok(()));
        }
    }

    #[test]
    fn the_stop_flag_is_shared_by_every_clone() {
        let route = ModelRoute::resolve(&subscription_request(), ON, None, &persistent_caller())
            .expect("valid");
        let child = route.clone();
        assert!(!route.is_stopped() && !child.is_stopped());
        child.stop();
        assert!(route.is_stopped(), "a child's stop reaches the parent's loop");
        // A different run's route is independent.
        let other = ModelRoute::resolve(&subscription_request(), ON, None, &persistent_caller())
            .expect("valid");
        assert!(!other.is_stopped());
    }

    // --- Z-18: the caller's waiver is judged on the route it binds ----------

    /// The status of a refused route, with the matrix code it carries (Z-18).
    fn refused(
        req: &pb::RunAgentRequest,
        caller: &AuthenticatedUser,
    ) -> (tonic::Code, Option<mp_contracts::zdr_refusal::Refusal>) {
        let status = validate_run_route(req, ON, caller).expect_err("the route is refused");
        (status.code(), refusal_of(&status))
    }

    /// A ZDR org's persistent token, waived for the connection the run names, runs
    /// on that connection, and the route it builds is marked waived so every
    /// delegated child and every retrieval of the run inherits it.
    #[test]
    fn a_waived_run_on_its_own_route_is_admitted_and_marked_waived() {
        let route =
            ModelRoute::resolve(&subscription_request(), ON, None, &waived_caller("conn_1"))
                .expect("the waived route is admitted");
        assert!(route.is_waived(), "the route carries the waiver");
        assert_eq!(route.subscription_connection_id(), "conn_1");
        assert!(route.clone().is_waived(), "a child shares the waived route");
        // An ordinary persistent run is never marked.
        let plain = ModelRoute::resolve(&subscription_request(), ON, None, &persistent_caller())
            .expect("valid");
        assert!(!plain.is_waived());
    }

    /// A waiver for one connection never admits a run on another (matrix row
    /// `e_waiver_other_connection`).
    #[test]
    fn a_waiver_for_another_connection_is_refused_with_the_route_mismatch_code() {
        let (code, refusal) = refused(&subscription_request(), &waived_caller("conn_2"));
        assert_eq!(code, tonic::Code::PermissionDenied);
        assert_eq!(
            refusal.map(|r| r.code()),
            Some(mp_contracts::zdr_refusal::code::WAIVER_ROUTE_MISMATCH)
        );
    }

    /// A waiver binds the subscription route only: a run that names no route, or
    /// another provider, is refused rather than served on the default chain
    /// (`e_waiver_default_route`, `e_waiver_platform_provider`).
    #[test]
    fn a_waiver_never_admits_the_default_route_or_another_provider() {
        let plain = pb::RunAgentRequest {
            run_id: "run-1".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            ..Default::default()
        };
        let (code, refusal) = refused(&plain, &waived_caller("conn_1"));
        assert_eq!(code, tonic::Code::PermissionDenied);
        assert_eq!(
            refusal.map(|r| r.code()),
            Some(mp_contracts::zdr_refusal::code::WAIVER_ROUTE_MISMATCH)
        );

        let mut other = subscription_request();
        other.provider_hint = "azure-openai".to_owned();
        assert_eq!(
            validate_run_route(&other, ON, &waived_caller("conn_1"))
                .expect_err("another provider is refused")
                .code(),
            tonic::Code::InvalidArgument,
            "the canonical-provider check comes first, as before"
        );
    }

    /// A waiver with an explicit ZDR request on the connection is refused: the
    /// waiver is for a persistent turn, and a request for ZDR is not one
    /// (`e_waiver_with_explicit_zdr`).
    #[test]
    fn a_waiver_with_an_explicit_zdr_request_is_refused() {
        let mut explicit = subscription_request();
        explicit.zdr = true;
        let (code, refusal) = refused(&explicit, &waived_caller("conn_1"));
        assert_eq!(code, tonic::Code::PermissionDenied);
        assert_eq!(
            refusal.map(|r| r.code()),
            Some(mp_contracts::zdr_refusal::code::WAIVER_ROUTE_MISMATCH)
        );
    }

    /// A persistent token of a ZDR org with no waiver is never served on a route
    /// (`e_org_zdr_without_waiver_is_not_persistent`, `e_temporary_chat_refuses_subscription_org_off`
    /// for the explicit request).
    #[test]
    fn a_persistent_token_of_a_zdr_org_without_a_waiver_is_refused_on_the_route() {
        let org_zdr = AuthenticatedUser {
            org_zdr: Some(true),
            ..persistent_caller()
        };
        let (code, refusal) = refused(&subscription_request(), &org_zdr);
        assert_eq!(code, tonic::Code::PermissionDenied);
        assert_eq!(
            refusal.map(|r| r.code()),
            Some(mp_contracts::zdr_refusal::code::POSTURE_MISMATCH)
        );
    }

    /// A ZDR token never reaches the subscription route, and the refusal is the
    /// typed one (`e_zdr_token_never_reaches_subscription`).
    #[test]
    fn a_zdr_token_is_refused_on_the_subscription_route_with_a_typed_code() {
        let zdr = AuthenticatedUser {
            zdr: true,
            org_zdr: Some(true),
            ..persistent_caller()
        };
        let (code, refusal) = refused(&subscription_request(), &zdr);
        assert_eq!(code, tonic::Code::PermissionDenied);
        assert_eq!(
            refusal.map(|r| r.code()),
            Some(mp_contracts::zdr_refusal::code::CONNECTION_NOT_ACKNOWLEDGED)
        );
    }

    /// The kill switch still stops a waived run first, exactly as any other
    /// subscription run: a waiver grants nothing the switch withheld.
    #[test]
    fn the_kill_switch_stops_a_waived_run_before_its_waiver_is_judged() {
        let error = validate_run_route(
            &subscription_request(),
            Switches::OFF,
            &waived_caller("conn_1"),
        )
        .expect_err("a disabled route must not start");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert!(error.message().contains(subscription::DISABLED_CODE));
    }
}
