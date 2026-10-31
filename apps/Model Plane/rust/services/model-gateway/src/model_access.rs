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

use std::collections::HashMap;
use std::time::Duration;

use axum::{http::StatusCode, Json};
use mp_contracts::model_entitlement::{Policy, Reason, Refusal};
use mp_contracts::model_plane::v1::{
    inference_core_client::InferenceCoreClient, GetModelAccessRequest, GetModelAccessResponse,
    ModelPolicyDoc, PrivacyTier, ReasoningEffort, SurfaceDefault,
};
use serde_json::{json, Map, Value};
use tonic::{metadata::MetadataMap, transport::Channel, Status};

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

/// The fields a `POST /v1/models/access` body may carry. Any other is refused
/// before inference-core is asked: a misspelt field would leave a draft unjudged,
/// and the organisation is the verified caller's, never one the body names.
const QUESTION_FIELDS: [&str; 4] = ["draft", "include_catalogue", "model", "surface"];

/// The fields of a draft policy, in the shape org-core stores (ADR-0005).
const DRAFT_FIELDS: [&str; 6] = [
    "allow_mode",
    "allow",
    "deny",
    "max_effort",
    "min_privacy_tier",
    "defaults",
];

/// The fields of one surface's default.
const DEFAULT_FIELDS: [&str; 2] = ["model", "effort"];

/// A typed refusal of the access route, `{error: {code, message, details?}}`.
/// `details` names the field that failed, never the caller's value.
fn typed_error(
    status: StatusCode,
    code: &str,
    message: &str,
    details: Option<Value>,
) -> (StatusCode, Json<Value>) {
    let mut error = json!({ "code": code, "message": message });
    if let Some(details) = details {
        error["details"] = details;
    }
    (status, Json(json!({ "error": error })))
}

/// A question the route cannot read: a 400 that names the field, not its value.
fn malformed(message: &str, field: Option<&str>) -> (StatusCode, Json<Value>) {
    typed_error(
        StatusCode::BAD_REQUEST,
        "invalid_model_access_request",
        message,
        field.map(|field| json!({ "field": field })),
    )
}

/// `value` as an object, refused when it is not one or carries a key outside
/// `allowed`. `what` names the object in the message.
fn object_with_fields<'a>(
    value: &'a Value,
    allowed: &[&str],
    what: &str,
) -> Result<&'a Map<String, Value>, (StatusCode, Json<Value>)> {
    let object = value
        .as_object()
        .ok_or_else(|| malformed(&format!("{what} must be a JSON object"), None))?;
    match object.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(field) => Err(malformed(
            "the route does not take this field",
            Some(field.as_str()),
        )),
        None => Ok(object),
    }
}

/// The text of an optional field, trimmed: absent or null is empty, and anything
/// that is not a string is refused.
fn text_of(object: &Map<String, Value>, field: &str) -> Result<String, (StatusCode, Json<Value>)> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.trim().to_owned()),
        Some(_) => Err(malformed("a field has the wrong type", Some(field))),
    }
}

/// The list of text an optional field holds: absent or null is empty.
fn list_of(
    object: &Map<String, Value>,
    field: &str,
) -> Result<Vec<String>, (StatusCode, Json<Value>)> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => Ok(text.clone()),
                _ => Err(malformed("a field has the wrong type", Some(field))),
            })
            .collect(),
        Some(_) => Err(malformed("a field has the wrong type", Some(field))),
    }
}

/// The privacy tier a label names. Org-core stores the lower-case labels that
/// `privacy_tier_label` writes back; empty (and `unspecified`) is no minimum. Any
/// other label is `None`: a typo is refused, never read as no minimum.
#[must_use]
pub fn privacy_tier_from_label(label: &str) -> Option<PrivacyTier> {
    match label.trim().to_ascii_lowercase().as_str() {
        "" | "unspecified" => Some(PrivacyTier::Unspecified),
        "global" => Some(PrivacyTier::Global),
        "eu_resident" => Some(PrivacyTier::EuResident),
        "zdr_contractual" => Some(PrivacyTier::ZdrContractual),
        "sovereign" => Some(PrivacyTier::Sovereign),
        _ => None,
    }
}

/// One surface's default, as the policy document carries it.
fn surface_default_of(entry: &Value) -> Result<SurfaceDefault, (StatusCode, Json<Value>)> {
    let entry = object_with_fields(entry, &DEFAULT_FIELDS, "a surface default")?;
    Ok(SurfaceDefault {
        model: text_of(entry, "model")?,
        effort: text_of(entry, "effort")?,
    })
}

/// A draft policy as the `ModelPolicyDoc` that inference-core judges in place of
/// the stored one. An absent field is the empty one: no restriction, no minimum.
///
/// # Errors
/// A typed 400 for a key that is not a draft field, a value of the wrong type, or
/// a privacy tier label this build does not know.
pub fn draft_from_json(value: &Value) -> Result<ModelPolicyDoc, (StatusCode, Json<Value>)> {
    let draft = object_with_fields(value, &DRAFT_FIELDS, "the draft")?;
    let tier_label = text_of(draft, "min_privacy_tier")?;
    let min_privacy_tier = privacy_tier_from_label(&tier_label).ok_or_else(|| {
        typed_error(
            StatusCode::BAD_REQUEST,
            "unknown_privacy_tier",
            "the draft names a privacy tier this build does not know",
            Some(json!({ "field": "min_privacy_tier" })),
        )
    })?;
    let defaults = match draft.get("defaults") {
        None | Some(Value::Null) => HashMap::new(),
        Some(Value::Object(surfaces)) => surfaces
            .iter()
            .map(|(surface, entry)| {
                surface_default_of(entry).map(|default| (surface.clone(), default))
            })
            .collect::<Result<HashMap<_, _>, _>>()?,
        Some(_) => return Err(malformed("a field has the wrong type", Some("defaults"))),
    };
    Ok(ModelPolicyDoc {
        allow_mode: text_of(draft, "allow_mode")?,
        allow: list_of(draft, "allow")?,
        deny: list_of(draft, "deny")?,
        max_effort: text_of(draft, "max_effort")?,
        min_privacy_tier: min_privacy_tier as i32,
        defaults,
    })
}

/// The question a `POST /v1/models/access` body asks, for the organisation the
/// verified caller belongs to. The body names the model, the surface, whether to
/// answer per chat model, and the draft, and nothing else.
///
/// # Errors
/// A typed 400 for a key the route does not take or a value of the wrong type.
pub fn question_from_json(
    body: &Value,
    org_id: &str,
) -> Result<GetModelAccessRequest, (StatusCode, Json<Value>)> {
    let question = object_with_fields(body, &QUESTION_FIELDS, "the request body")?;
    let include_catalogue = match question.get("include_catalogue") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => {
            return Err(malformed(
                "a field has the wrong type",
                Some("include_catalogue"),
            ))
        }
    };
    let draft = match question.get("draft") {
        None | Some(Value::Null) => None,
        Some(draft) => Some(draft_from_json(draft)?),
    };
    Ok(GetModelAccessRequest {
        org_id: org_id.to_owned(),
        model: text_of(question, "model")?,
        surface: text_of(question, "surface")?,
        include_catalogue,
        draft,
        ..GetModelAccessRequest::default()
    })
}

/// The answer as the route's JSON: the verdict, the facts of the policy and, when
/// they were asked for, one verdict per chat model with its attestation. It
/// carries no content.
#[must_use]
pub fn answer_json(answer: &GetModelAccessResponse) -> Value {
    json!({
        "state": answer.state,
        "enforced": answer.enforced,
        "revision": answer.revision,
        "role": answer.role,
        "restricted": answer.restricted,
        "model_allowed": answer.model_allowed,
        "reason": answer.reason,
        "default_model": answer.default_model,
        "default_effort": answer.default_effort,
        "max_effort": answer.max_effort,
        "min_privacy_tier": crate::http_routes::privacy_tier_label(answer.min_privacy_tier),
        "can_manage": answer.can_manage,
        "catalogue": answer
            .catalogue
            .iter()
            .map(|entry| {
                json!({
                    "id": entry.id,
                    "provider": entry.provider,
                    "entitled": entry.entitled,
                    "reason": entry.reason,
                    "zdr_attested": entry.zdr_attested,
                })
            })
            .collect::<Vec<_>>(),
    })
}

/// The failure of the question as the route's answer. A typed entitlement refusal
/// keeps its catalogue status (403, or 503 for an unreadable policy); PERMISSION_DENIED
/// is a typed 403 for a caller who may not manage the policy; anything else is a typed
/// 502. The upstream message is never echoed: only its code is logged.
#[must_use]
pub fn route_error(status: &Status) -> (StatusCode, Json<Value>) {
    if let Some(refusal) = Refusal::from_status(status) {
        return http(&refusal);
    }
    if status.code() == tonic::Code::PermissionDenied {
        return typed_error(
            StatusCode::FORBIDDEN,
            "model_access_forbidden",
            "Only those who manage the organisation's model policy can ask for a draft or for the whole catalogue.",
            None,
        );
    }
    tracing::warn!(
        grpc_code = ?status.code(),
        "model access: inference-core could not answer the question"
    );
    typed_error(
        StatusCode::BAD_GATEWAY,
        "model_access_unavailable",
        "The model policy could not be read right now. Try again shortly.",
        None,
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

    #[test]
    fn each_privacy_tier_label_names_its_enum_and_an_unknown_one_names_none() {
        let cases = [
            ("", PrivacyTier::Unspecified),
            ("unspecified", PrivacyTier::Unspecified),
            ("global", PrivacyTier::Global),
            ("eu_resident", PrivacyTier::EuResident),
            ("zdr_contractual", PrivacyTier::ZdrContractual),
            ("sovereign", PrivacyTier::Sovereign),
            ("  Sovereign ", PrivacyTier::Sovereign),
        ];
        for (label, tier) in cases {
            assert_eq!(privacy_tier_from_label(label), Some(tier), "{label:?}");
        }
        for label in ["gold", "zdr", "eu-resident", "norway"] {
            assert_eq!(privacy_tier_from_label(label), None, "{label:?}");
        }
    }

    #[test]
    fn a_draft_maps_every_field_and_each_surface_default_onto_the_policy_document() {
        let draft = draft_from_json(&json!({
            "allow_mode": "listed",
            "allow": ["gpt-open", "provider:openai"],
            "deny": ["gpt-blocked"],
            "max_effort": "medium",
            "min_privacy_tier": "zdr_contractual",
            "defaults": {"chat": {"model": "gpt-open", "effort": "low"}},
        }))
        .expect("a well-formed draft");
        assert_eq!(draft.allow_mode, "listed");
        assert_eq!(draft.allow, ["gpt-open", "provider:openai"]);
        assert_eq!(draft.deny, ["gpt-blocked"]);
        assert_eq!(draft.max_effort, "medium");
        assert_eq!(draft.min_privacy_tier, PrivacyTier::ZdrContractual as i32);
        assert_eq!(draft.defaults.len(), 1);
        assert_eq!(
            draft.defaults["chat"],
            SurfaceDefault {
                model: "gpt-open".to_owned(),
                effort: "low".to_owned(),
            }
        );

        // An empty draft is the unrestricted policy: nothing listed, no minimum.
        assert_eq!(
            draft_from_json(&json!({})).expect("an empty draft"),
            ModelPolicyDoc::default()
        );
    }

    #[test]
    fn an_unknown_privacy_tier_label_is_a_typed_400_and_never_no_minimum() {
        let (status, Json(body)) =
            draft_from_json(&json!({"min_privacy_tier": "gold"})).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "unknown_privacy_tier");
        assert_eq!(body["error"]["details"]["field"], "min_privacy_tier");
    }

    #[test]
    fn a_key_the_route_does_not_take_is_refused_before_inference_core_is_asked() {
        // The organisation is the caller's, and a misspelt key would leave a draft
        // unjudged: both are refused at the top level and inside the draft.
        for body in [
            json!({"org_id": "org-other"}),
            json!({"provider_hint": "openai"}),
            json!({"include_catalogue": true, "catalogue": true}),
        ] {
            let (status, Json(error)) = question_from_json(&body, "org-claims").unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(
                error["error"]["code"], "invalid_model_access_request",
                "{body}"
            );
        }
        let (_, Json(error)) = question_from_json(
            &json!({"draft": {"allow_mode": "all", "allowed": ["gpt-x"]}}),
            "org-claims",
        )
        .unwrap_err();
        assert_eq!(error["error"]["details"]["field"], "allowed");
        assert!(question_from_json(&json!({"draft": "all"}), "org-claims").is_err());
        assert!(question_from_json(&json!([]), "org-claims").is_err());
    }

    #[test]
    fn the_organisation_is_the_callers_and_the_body_names_the_rest() {
        let question = question_from_json(
            &json!({
                "model": "  gpt-open ",
                "surface": "chat",
                "include_catalogue": true,
                "draft": {"allow_mode": "all"},
            }),
            "org-from-claims",
        )
        .expect("a question");
        assert_eq!(question.org_id, "org-from-claims");
        assert_eq!(question.model, "gpt-open");
        assert_eq!(question.surface, "chat");
        assert!(question.include_catalogue);
        assert_eq!(question.draft.expect("the draft").allow_mode, "all");

        // An empty body asks about the caller alone: no draft, no catalogue.
        assert_eq!(
            question_from_json(&json!({}), "org-from-claims").expect("an empty question"),
            GetModelAccessRequest {
                org_id: "org-from-claims".to_owned(),
                ..GetModelAccessRequest::default()
            }
        );
    }

    #[test]
    fn a_mistyped_field_is_a_typed_400() {
        for body in [
            json!({"include_catalogue": "yes"}),
            json!({"model": 7}),
            json!({"draft": {"allow": "gpt-open"}}),
            json!({"draft": {"allow": [1]}}),
            json!({"draft": {"defaults": "chat"}}),
            json!({"draft": {"defaults": {"chat": {"model": true}}}}),
        ] {
            let (status, Json(error)) = question_from_json(&body, "org-1").unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(
                error["error"]["code"], "invalid_model_access_request",
                "{body}"
            );
        }
    }

    #[test]
    fn the_answer_names_each_fact_and_each_verdict_with_its_attestation() {
        use mp_contracts::model_plane::v1::ModelAccessEntry;
        let answer = GetModelAccessResponse {
            state: "role".to_owned(),
            enforced: true,
            revision: 7,
            role: "member".to_owned(),
            restricted: true,
            model_allowed: false,
            reason: "not_in_allow_list".to_owned(),
            default_model: "gpt-open".to_owned(),
            default_effort: "low".to_owned(),
            max_effort: "medium".to_owned(),
            min_privacy_tier: PrivacyTier::Sovereign as i32,
            can_manage: true,
            catalogue: vec![ModelAccessEntry {
                id: "gpt-attested".to_owned(),
                provider: "azure".to_owned(),
                entitled: false,
                reason: "not_in_allow_list".to_owned(),
                zdr_attested: true,
            }],
            ..GetModelAccessResponse::default()
        };
        assert_eq!(
            answer_json(&answer),
            json!({
                "state": "role",
                "enforced": true,
                "revision": 7,
                "role": "member",
                "restricted": true,
                "model_allowed": false,
                "reason": "not_in_allow_list",
                "default_model": "gpt-open",
                "default_effort": "low",
                "max_effort": "medium",
                "min_privacy_tier": "sovereign",
                "can_manage": true,
                "catalogue": [{
                    "id": "gpt-attested",
                    "provider": "azure",
                    "entitled": false,
                    "reason": "not_in_allow_list",
                    "zdr_attested": true,
                }],
            })
        );
    }

    #[test]
    fn a_refusal_keeps_its_catalogue_status_and_no_upstream_detail_is_echoed() {
        // The policy cannot be read: the catalogue's 503 and its typed body.
        let (status, Json(body)) = route_error(&Refusal::unavailable().grpc_status());
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "model_entitlement_unavailable");

        // An entitlement refusal keeps its own status and code.
        let refused = Refusal::not_entitled("gpt-x", Reason::ExplicitlyDenied);
        let (status, Json(body)) = route_error(&refused.grpc_status());
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "model_not_entitled");

        // Refused as not a manager: a typed 403, never the upstream's words.
        let (status, Json(body)) =
            route_error(&Status::permission_denied("secret upstream detail"));
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "model_access_forbidden");
        assert!(!body.to_string().contains("secret upstream detail"));

        // Anything else is a typed 502 that names no upstream detail either.
        let (status, Json(body)) = route_error(&Status::internal("pool exhausted at db-7"));
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], "model_access_unavailable");
        assert!(!body.to_string().contains("db-7"));
    }
}
