//! Gateway admission under the organisation's model policy (ADR-0005, WS10).
//!
//! inference-core is the policy enforcement point: it refuses a model the caller
//! may not use, and it is the only Model Plane service that reads the policy.
//! What it cannot do is refuse before the gateway has opened a durable turn, so a
//! refused turn would still leave a thread, a user message and a failed run
//! behind. [`admit`] asks inference-core (`GetModelAccess`, with the caller's own
//! credential) first, on every invoke transport, and a refusal ends the turn
//! before any session-core call. It reads the answer and decides nothing itself:
//! the matcher is `mp_contracts::model_entitlement`, applied in inference-core.
//!
//! The answer is applied only when inference-core says it is enforced. Under
//! `MODEL_ENTITLEMENT_MODE=off` and `shadow`, for a service caller, and for a
//! caller with no policy it is not, and then admission changes nothing about the
//! turn: no refusal, no substituted model, no lowered effort.
//!
//! # When inference-core cannot say
//!
//! A typed refusal in the status (`model_entitlement_unavailable`: the answer
//! could not be read and inference-core fails closed) is the turn's refusal. Any
//! other failure of the question, including a build of inference-core that does
//! not know it, is not a verdict: the turn goes on and the same policy is
//! applied where it always is, at inference-core's own pre-check, which refuses
//! under `enforce` whatever the gateway asked. What that costs is only the rows a
//! refused turn leaves behind, never a model call.
//!
//! Everything here is content-free: the question carries an organisation id, a
//! model id and a provider hint, and a refusal names a model id and a reason.

use std::time::Duration;

use axum::{http::StatusCode, Json};
use mp_contracts::model_entitlement::{Policy, Reason, Refusal};
use mp_contracts::model_plane::v1::{
    inference_core_client::InferenceCoreClient, GetModelAccessRequest, GetModelAccessResponse,
    ReasoningEffort,
};
use tonic::{metadata::MetadataMap, transport::Channel};

/// The product surface whose defaults the gateway asks for.
pub const CHAT_SURFACE: &str = "chat";

/// How long the question may take. It is on the path of every turn, and
/// inference-core's own answer is cached, so a slow one is a sick one.
const ASK_TIMEOUT: Duration = Duration::from_secs(3);

/// What admission decided for a turn it let through. The default changes nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Admitted {
    /// The model the turn runs on when the caller named none and the policy sets
    /// a default for the chat surface. `None` leaves the model as it was.
    pub model: Option<String>,
    /// The highest reasoning effort the policy allows, as a canonical effort
    /// name. Empty means no ceiling.
    pub max_effort: String,
}

impl Admitted {
    /// Lowers a requested effort profile to the policy's ceiling. Never raises
    /// one, and leaves a request that names none, or none this build knows, alone.
    pub fn clamp_effort(&self, requested: &mut Option<String>, ladder_on: bool) {
        let clamped = requested
            .as_deref()
            .and_then(|profile| clamp_profile(profile, &self.max_effort, ladder_on));
        if clamped.is_some() {
            *requested = clamped;
        }
    }
}

/// Asks inference-core whether the caller may use `model` and returns what the
/// turn runs under, or the typed refusal that ends it.
///
/// `auth` is the metadata that carries the caller's verified inference-core
/// credential, built by each transport the way it builds every other call.
/// `model` is as the caller named it: empty, or `default`, means none, and the
/// policy's chat default stands in for it when there is one.
///
/// # Errors
/// The refusal when the policy is enforced and does not allow the model, or
/// inference-core could not read the policy and fails closed.
pub async fn admit(
    client: &InferenceCoreClient<Channel>,
    auth: &MetadataMap,
    org_id: &str,
    model: &str,
    provider_hint: &str,
) -> Result<Admitted, Refusal> {
    let outcome = admit_inner(client, auth, org_id, model, provider_hint).await;
    let label = match &outcome {
        Ok(Some(_)) => "admitted",
        Ok(None) => "unchecked",
        Err(refusal) if refusal.reason().is_some() => "refused",
        Err(_) => "unavailable",
    };
    metrics::counter!("gateway_model_admission_total", "outcome" => label).increment(1);
    outcome.map(Option::unwrap_or_default)
}

/// [`admit`] without the count: `None` when no verdict was available.
async fn admit_inner(
    client: &InferenceCoreClient<Channel>,
    auth: &MetadataMap,
    org_id: &str,
    model: &str,
    provider_hint: &str,
) -> Result<Option<Admitted>, Refusal> {
    let Some(answer) = ask(client, auth, org_id, model, provider_hint).await? else {
        return Ok(None);
    };
    let mut admitted = judge(&answer, model)?;
    let default = answer.default_model.trim();
    if answer.enforced && is_unnamed(model) && !default.is_empty() {
        // The default is a model like any other: a policy that names one the
        // caller may not use is refused here, not at inference-core after the
        // turn's rows exist.
        if let Some(answer) = ask(client, auth, org_id, default, provider_hint).await? {
            admitted = judge(&answer, default)?;
        }
        admitted.model = Some(default.to_owned());
    }
    Ok(Some(admitted))
}

/// Whether the caller named no model. `default` is what the gRPC transport
/// writes for an empty one.
fn is_unnamed(model: &str) -> bool {
    let model = model.trim();
    model.is_empty() || model.eq_ignore_ascii_case("default")
}

/// The question, and the answer when there is one to read.
async fn ask(
    client: &InferenceCoreClient<Channel>,
    auth: &MetadataMap,
    org_id: &str,
    model: &str,
    provider_hint: &str,
) -> Result<Option<GetModelAccessResponse>, Refusal> {
    let mut request = tonic::Request::new(GetModelAccessRequest {
        org_id: org_id.to_owned(),
        model: model.trim().to_owned(),
        provider_hint: provider_hint.trim().to_owned(),
        surface: CHAT_SURFACE.to_owned(),
        ..GetModelAccessRequest::default()
    });
    *request.metadata_mut() = auth.clone();
    request.set_timeout(ASK_TIMEOUT);
    match client.clone().get_model_access(request).await {
        Ok(answer) => Ok(Some(answer.into_inner())),
        Err(status) => match Refusal::from_status(&status) {
            Some(refusal) => Err(refusal),
            None => {
                tracing::warn!(
                    grpc_code = ?status.code(),
                    "model admission: inference-core gave no verdict; the turn goes on to its pre-check"
                );
                Ok(None)
            }
        },
    }
}

/// The verdict on the model the caller named, for a policy that is applied.
fn judge(answer: &GetModelAccessResponse, model: &str) -> Result<Admitted, Refusal> {
    if !answer.enforced {
        return Ok(Admitted::default());
    }
    if !answer.model_allowed {
        // The reason vocabulary is closed. A name from a newer inference-core
        // still refuses, as the plainest reason.
        let reason = Reason::parse(&answer.reason).unwrap_or(Reason::NotInAllowList);
        return Err(Refusal::not_entitled(model, reason));
    }
    Ok(Admitted {
        model: None,
        max_effort: answer.max_effort.trim().to_owned(),
    })
}

/// The refusal as the unary and SSE-open HTTP answer: the catalogue's status
/// (403, or 503 when the policy could not be read) and its nested typed body.
#[must_use]
pub fn http(refusal: &Refusal) -> (StatusCode, Json<serde_json::Value>) {
    let status = StatusCode::from_u16(refusal.http_status()).unwrap_or(StatusCode::FORBIDDEN);
    (status, Json(refusal.json_body()))
}

/// `profile` lowered to `max_effort`, or `None` when it is already within it,
/// names no effort, or there is no ceiling.
///
/// The ceiling is on the canonical ladder, and a profile is a client's word for
/// a point on it (`quick` is low, `deep` is high), so the profile is read onto
/// the ladder first and the answer is written back in what the gateway's own
/// derivation understands: the canonical name with the ladder on, and with it off
/// the legacy profile, because the legacy budget knows only `quick` and `deep`.
/// A profile the legacy budget would not turn into thinking is left alone, so
/// the clamp can only ever lower a request.
#[must_use]
pub fn clamp_profile(profile: &str, max_effort: &str, ladder_on: bool) -> Option<String> {
    if max_effort.trim().is_empty() {
        return None;
    }
    if !ladder_on && crate::thinking::budget_tokens(profile, i32::MAX) == 0 {
        return None;
    }
    let asked = level_name(crate::thinking::effort_level(profile))?;
    let allowed = Policy {
        max_effort: max_effort.trim().to_owned(),
        ..Policy::default()
    }
    .clamp_effort(asked);
    if allowed == asked {
        return None;
    }
    Some(if ladder_on {
        allowed
    } else {
        legacy_profile(&allowed).to_owned()
    })
}

/// The canonical name of a level, or `None` for no effort.
fn level_name(level: ReasoningEffort) -> Option<&'static str> {
    match level {
        ReasoningEffort::Unspecified => None,
        ReasoningEffort::Minimal => Some("minimal"),
        ReasoningEffort::Low => Some("low"),
        ReasoningEffort::Medium => Some("medium"),
        ReasoningEffort::High => Some("high"),
        ReasoningEffort::Xhigh => Some("xhigh"),
        ReasoningEffort::Max => Some("max"),
    }
}

/// The legacy profile that is the strongest one not above a canonical ceiling.
fn legacy_profile(ceiling: &str) -> &'static str {
    match ceiling {
        "minimal" => "standard",
        "low" | "medium" => "quick",
        _ => "deep",
    }
}

/// The models the organisation's policy allows, as the comma-separated list
/// `OrgPolicy.allowed_models` carries, for the caller who manages the policy.
/// Empty when nothing is restricted (the field's own "empty means all"), and
/// `None` when it cannot be read: the field then stays zero-valued, as it was
/// before the policy had a store.
pub async fn allowed_models(
    client: &InferenceCoreClient<Channel>,
    auth: &MetadataMap,
    org_id: &str,
) -> Option<String> {
    let mut request = tonic::Request::new(GetModelAccessRequest {
        org_id: org_id.to_owned(),
        surface: CHAT_SURFACE.to_owned(),
        include_catalogue: true,
        ..GetModelAccessRequest::default()
    });
    *request.metadata_mut() = auth.clone();
    request.set_timeout(ASK_TIMEOUT);
    let answer = client
        .clone()
        .get_model_access(request)
        .await
        .ok()?
        .into_inner();
    if !answer.enforced || !answer.restricted {
        return Some(String::new());
    }
    Some(
        answer
            .catalogue
            .iter()
            .filter(|entry| entry.entitled)
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>()
            .join(","),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(enforced: bool, allowed: bool, reason: &str) -> GetModelAccessResponse {
        GetModelAccessResponse {
            enforced,
            model_allowed: allowed,
            reason: reason.to_owned(),
            max_effort: "medium".to_owned(),
            ..GetModelAccessResponse::default()
        }
    }

    #[test]
    fn a_policy_that_is_not_applied_changes_nothing() {
        // Shadow counts what it would refuse and refuses nothing: the verdict is
        // the policy's, but `enforced` is false.
        let admitted = judge(&answer(false, false, "not_in_allow_list"), "gpt-x").expect("admitted");
        assert_eq!(admitted, Admitted::default());
    }

    #[test]
    fn an_enforced_denial_is_the_catalogue_refusal_with_its_reason() {
        let refusal = judge(&answer(true, false, "explicitly_denied"), "gpt-x").unwrap_err();
        assert_eq!(refusal.code(), "model_not_entitled");
        assert_eq!(refusal.reason(), Some(Reason::ExplicitlyDenied));
        assert_eq!(refusal.model(), "gpt-x");
        let (status, Json(body)) = http(&refusal);
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["details"]["reason"], "explicitly_denied");
    }

    #[test]
    fn a_reason_from_a_newer_inference_core_still_refuses() {
        let refusal = judge(&answer(true, false, "a_reason_added_later"), "gpt-x").unwrap_err();
        assert_eq!(refusal.reason(), Some(Reason::NotInAllowList));
    }

    #[test]
    fn the_unavailable_refusal_is_a_503() {
        let (status, Json(body)) = http(&Refusal::unavailable());
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "model_entitlement_unavailable");
    }

    #[test]
    fn an_allowed_model_carries_the_ceiling_and_no_substitute() {
        let admitted = judge(&answer(true, true, ""), "gpt-x").expect("admitted");
        assert_eq!(admitted.max_effort, "medium");
        assert_eq!(admitted.model, None);
    }

    #[test]
    fn no_model_means_empty_or_default() {
        for named in ["", "  ", "default", "DEFAULT"] {
            assert!(is_unnamed(named), "{named:?}");
        }
        for named in ["gpt-x", "verevon-auto", "defaults"] {
            assert!(!is_unnamed(named), "{named:?}");
        }
    }

    #[test]
    fn the_clamp_lowers_a_canonical_effort_to_the_ceiling_and_never_raises_one() {
        let cases = [
            ("max", "medium", Some("medium")),
            ("xhigh", "high", Some("high")),
            ("high", "high", None),
            ("low", "high", None),
            ("minimal", "low", None),
            ("high", "minimal", Some("minimal")),
            ("", "low", None),
            ("standard", "low", None),
            ("max", "", None),
        ];
        for (asked, ceiling, expected) in cases {
            assert_eq!(
                clamp_profile(asked, ceiling, true).as_deref(),
                expected,
                "{asked:?} under {ceiling:?}"
            );
        }
    }

    #[test]
    fn a_client_profile_is_read_onto_the_ladder_before_it_is_clamped() {
        // `deep` is high and `quick` is low: a ceiling of medium lowers the first
        // and leaves the second.
        assert_eq!(clamp_profile("deep", "medium", true).as_deref(), Some("medium"));
        assert_eq!(clamp_profile("quick", "medium", true), None);
        assert_eq!(clamp_profile("extended", "low", true).as_deref(), Some("low"));
    }

    #[test]
    fn with_the_ladder_off_the_clamp_speaks_the_legacy_budget() {
        // The legacy budget knows `quick` and `deep` only, so a lowered request
        // is written as one of them, and a profile it never turned into thinking
        // is not made into some by the clamp.
        assert_eq!(clamp_profile("deep", "medium", false).as_deref(), Some("quick"));
        assert_eq!(clamp_profile("deep", "low", false).as_deref(), Some("quick"));
        assert_eq!(clamp_profile("deep", "minimal", false).as_deref(), Some("standard"));
        assert_eq!(clamp_profile("deep", "high", false), None);
        assert_eq!(clamp_profile("quick", "medium", false), None);
        assert_eq!(clamp_profile("max", "low", false), None);
        assert_eq!(clamp_profile("standard", "low", false), None);
    }

    #[test]
    fn clamp_effort_rewrites_only_what_it_lowers() {
        let admitted = Admitted {
            model: None,
            max_effort: "low".to_owned(),
        };
        let mut requested = Some("deep".to_owned());
        admitted.clamp_effort(&mut requested, true);
        assert_eq!(requested.as_deref(), Some("low"));

        let mut none = None;
        admitted.clamp_effort(&mut none, true);
        assert_eq!(none, None);

        let mut within = Some("quick".to_owned());
        admitted.clamp_effort(&mut within, true);
        assert_eq!(within.as_deref(), Some("quick"), "the profile the client sent stays");

        let mut open = Some("max".to_owned());
        Admitted::default().clamp_effort(&mut open, true);
        assert_eq!(open.as_deref(), Some("max"), "no ceiling, no change");
    }
}
