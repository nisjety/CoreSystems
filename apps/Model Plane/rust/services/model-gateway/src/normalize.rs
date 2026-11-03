//! Request normalization and validation for model-gateway.
//!
//! Trims whitespace, validates content length and model names against an allowlist.

use axum::{http::StatusCode, Json};
use serde_json::json;

use crate::http_routes::InvokeRequest;

/// Maximum allowed content size in bytes (100 KB).
const MAX_CONTENT_BYTES: usize = 100 * 1024;

/// A turn that moves the branch of a message already in its thread (WS17 S9). Each variant names the
/// message the user acted on; the gateway resolves the rest from the thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionTurn {
    /// Regenerate the answer `message_id`: its question stays, and the answer is produced again beside it.
    Regenerate { message_id: String },
    /// Edit the user turn `message_id`: the edited text is appended beside it.
    Edit { message_id: String },
}

/// Normalized request after validation.
#[derive(Debug)]
pub struct NormalizedRequest {
    pub content: String,
    pub model: String,
    /// A provider routing hint selected by the authenticated caller. No
    /// credential is carried here.
    pub provider_hint: String,
    /// Opaque Integration Core subscription reference. This is validated again
    /// by Integration Core with the caller's verified org/user scope.
    pub subscription_connection_id: String,
    #[allow(dead_code)]
    pub session_key: Option<String>,
    #[allow(dead_code)]
    pub thread_id: Option<String>,
    /// Server-injected Control decision for a newly-created scoped thread.
    /// Session Core is still the trust boundary and verifies its signature.
    pub space_context: Option<crate::session_flow::ThreadSpaceContext>,
    pub space_append_context: Option<crate::session_flow::ThreadSpaceContext>,
    pub structured_output_schema: Option<String>,
    pub zdr: bool,
    /// Caller-selected minimum privacy tier (Venice-style). `None` is
    /// `UNSPECIFIED` and keeps today's behavior byte-identical; `Some(tier)`
    /// is threaded onto every downstream InferRequest and enforced by
    /// inference-core's chain (fail-closed, never a silent downgrade).
    pub min_privacy_tier: Option<mp_contracts::model_plane::v1::PrivacyTier>,
    pub max_cost_usd: Option<f64>,
    pub max_tokens: Option<u32>,
}

/// Wire numeric (`model_plane.v1.PrivacyTier`) of the caller-selected minimum
/// tier, 0 (= UNSPECIFIED) when none was expressed. This is the exact value
/// inference-core validates/enforces downstream, so every transport threads
/// this one function's result rather than re-deriving its own encoding.
#[must_use]
pub fn min_privacy_tier_wire(req: &NormalizedRequest) -> i32 {
    req.min_privacy_tier.map_or(0, |tier| tier as i32)
}

/// Per-turn retrieval-source filter (Perplexity-Focus style, chat-wave2
/// scope spec). Orthogonal to `conversation_only`: that flag is a
/// privacy/retention commitment that excludes both families outright, while
/// this axis only narrows which family a turn that IS grounded may draw on.
/// `Auto` reproduces today's behavior byte-identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalScope {
    Auto,
    KnowledgeOnly,
    WebOnly,
}

impl RetrievalScope {
    #[must_use]
    pub fn is_auto(self) -> bool {
        matches!(self, RetrievalScope::Auto)
    }
}

/// Normalizes the wire `retrieval_scope` string
/// (`InvokeRequest::retrieval_scope`). Unknown or absent values fail safe to
/// `Auto` — an unrecognized value from a newer client must never silently
/// narrow what an older gateway would have granted.
#[must_use]
pub fn retrieval_scope(raw: Option<&str>) -> RetrievalScope {
    match raw {
        Some("knowledge_only") => RetrievalScope::KnowledgeOnly,
        Some("web_only") => RetrievalScope::WebOnly,
        _ => RetrievalScope::Auto,
    }
}

/// Load the model allowlist from `ALLOWED_MODELS` env var (comma-separated).
/// Returns `None` if the env var is not set (all models allowed).
fn load_allowed_models() -> Option<Vec<String>> {
    std::env::var("ALLOWED_MODELS").ok().map(|v| {
        v.split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    })
}

/// The typed refusal for a model the operator's deployment allowlist
/// (`ALLOWED_MODELS`) leaves out: a 403 whose body is the model-entitlement
/// catalogue's `model_not_entitled` with reason `deployment_allowlist`, not the
/// untyped 400 it used to be. `None` when no allowlist is set, the list is
/// empty, the model is unspecified (inference-core resolves it to a provider
/// default downstream, so there is nothing to check yet) or the model is on the
/// list. The body names the model id and the reason, never the list itself.
fn deployment_allowlist_refusal(
    allowed: Option<&[String]>,
    model: &str,
) -> Option<(StatusCode, Json<serde_json::Value>)> {
    let allowed = allowed?;
    if model.is_empty() || allowed.is_empty() || allowed.iter().any(|entry| entry == model) {
        return None;
    }
    let refusal = mp_contracts::model_entitlement::Refusal::not_entitled(
        model,
        mp_contracts::model_entitlement::Reason::DeploymentAllowlist,
    );
    let status = StatusCode::from_u16(refusal.http_status()).unwrap_or(StatusCode::FORBIDDEN);
    Some((status, Json(refusal.json_body())))
}

/// Load the default model from `DEFAULT_MODEL` env var. Shared with the SSE
/// stream path so it resolves an unspecified model identically to the unary
/// path. When unset, returns an EMPTY string on purpose: inference-core's
/// fallback chain then resolves "Verevon Auto" to the configured provider's own
/// default (per-provider), so chat works against any single provider without an
/// operator pinning a model. (The previous literal "default" was not a real
/// model id and made every unpinned request fail provider lookup.)
pub(crate) fn load_default_model() -> String {
    std::env::var("DEFAULT_MODEL")
        .ok()
        .map(|m| m.trim().to_owned())
        .unwrap_or_default()
}

/// The refusal a subscription turn has always got for anything beyond
/// text-only, non-ZDR chat. Unchanged byte for byte, so a client (or a test)
/// that matches on it keeps working when the agentic exception is off.
const SUBSCRIPTION_STRICT_REFUSAL: &str = "openai-codex-subscription supports text-only, non-ZDR chat without image generation, browsing, deep research, plan mode, or agentic tool specs";

/// The same refusal where plan mode and the agentic feature ARE admitted, so
/// it must not list them as unsupported.
const SUBSCRIPTION_AGENTIC_REFUSAL: &str = "openai-codex-subscription supports text-only, non-ZDR chat without image generation, browsing, deep research, or client-declared tool specs";

/// A 400 with the usual `{"error": message}` body, plus a stable `code` when
/// the caller has one worth matching on.
fn refusal(message: &str, code: Option<&str>) -> (StatusCode, Json<serde_json::Value>) {
    let mut body = json!({ "error": message });
    if let (Some(code), Some(map)) = (code, body.as_object_mut()) {
        map.insert("code".to_owned(), json!(code));
    }
    (StatusCode::BAD_REQUEST, Json(body))
}

/// Whether `provider` names the ChatGPT subscription route, by its canonical
/// registry id or either alias inference-core also accepts.
#[must_use]
pub fn is_codex_subscription_hint(provider: &str) -> bool {
    let provider = provider.trim();
    provider.eq_ignore_ascii_case(mp_contracts::agentic_subscription::PROVIDER_ID)
        || provider.eq_ignore_ascii_case("codex-subscription")
        || provider.eq_ignore_ascii_case("chatgpt-codex")
}

/// Whether the streaming endpoint may admit an agentic run on a subscription
/// turn: the gateway's reader of the `AGENTIC_SUBSCRIPTION_ENABLED` kill switch
/// (and `AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS`), whose parsing lives in
/// [`mp_contracts::agentic_subscription`] so execution-core and inference-core
/// read it the same way.
///
/// Off by default, and a pure value: the function that decides takes it as an
/// argument, so a test never has to touch the process environment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AgenticSubscriptionPolicy {
    switches: mp_contracts::agentic_subscription::Switches,
}

impl AgenticSubscriptionPolicy {
    /// Both switches off: the strict rule.
    pub const OFF: Self = Self {
        switches: mp_contracts::agentic_subscription::Switches::OFF,
    };

    /// The process environment right now.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            switches: mp_contracts::agentic_subscription::Switches::from_env(),
        }
    }

    /// A policy with the given switches.
    #[must_use]
    pub const fn new(enabled: bool, allow_effects: bool) -> Self {
        Self {
            switches: mp_contracts::agentic_subscription::Switches {
                enabled,
                allow_effects,
            },
        }
    }

    /// Whether a subscription turn may carry the agentic feature at all.
    #[must_use]
    pub fn enabled(self) -> bool {
        self.switches.enabled
    }

    /// Whether such a run may act (run without plan mode). False whenever
    /// [`Self::enabled`] is false.
    #[must_use]
    pub fn effects_permitted(self) -> bool {
        self.switches.effects_permitted()
    }
}

/// The branch move a request asks for (WS17 S9), or `None` for an ordinary send. A blank id is absent.
/// A request naming both a regenerate and an edit, or a move with no thread to move in, is malformed. An
/// agentic turn cannot move a branch: its run is an agent loop, not a plain answer.
pub fn version_turn(
    req: &InvokeRequest,
) -> Result<Option<VersionTurn>, (StatusCode, Json<serde_json::Value>)> {
    let present = |raw: Option<&str>| {
        raw.map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
    };
    let turn = match (
        present(req.regenerate_of_message_id.as_deref()),
        present(req.edit_of_message_id.as_deref()),
    ) {
        (None, None) => return Ok(None),
        (Some(message_id), None) => VersionTurn::Regenerate { message_id },
        (None, Some(message_id)) => VersionTurn::Edit { message_id },
        (Some(_), Some(_)) => {
            return Err(refusal(
                "a turn is a regenerate or an edit, not both",
                Some("version_turn_invalid"),
            ));
        }
    };
    if present(req.thread_id.as_deref()).is_none() {
        return Err(refusal(
            "a regenerate or an edit names the thread it moves",
            Some("version_turn_invalid"),
        ));
    }
    if req.execute_granted || req.features.iter().any(|feature| feature == "agentic") {
        return Err(refusal(
            "a Do turn cannot move a message's branch",
            Some("feature_not_supported"),
        ));
    }
    Ok(Some(turn))
}

/// Normalize and validate an incoming invoke request.
///
/// Returns a `NormalizedRequest` on success, or an error response tuple on failure.
///
/// # Errors
/// Returns [`StatusCode::BAD_REQUEST`] when the request fails validation
/// (empty content, unknown modality, invalid parameters, etc.).
pub fn normalize(
    req: &InvokeRequest,
) -> Result<NormalizedRequest, (StatusCode, Json<serde_json::Value>)> {
    normalize_with(req, None)
}

/// [`normalize`] for the streaming endpoint (`/v1/invoke/stream`), which is the
/// only transport with a path to `execution-core`'s governed agent loop.
///
/// Identical except for one thing: with `policy` on, a subscription turn
/// (`provider: openai-codex-subscription`) may carry the `agentic` feature,
/// read-only and on an explicit model (see [`AgenticSubscriptionPolicy`]).
/// Every other rejection of a subscription turn is unchanged. With the policy
/// off the answer is the same as [`normalize`]'s, plus the stable error code
/// [`mp_contracts::agentic_subscription::DISABLED_CODE`] on the request that
/// asked for the agentic run.
///
/// # Errors
/// As [`normalize`].
pub fn normalize_stream(
    req: &InvokeRequest,
    policy: AgenticSubscriptionPolicy,
) -> Result<NormalizedRequest, (StatusCode, Json<serde_json::Value>)> {
    normalize_with(req, Some(policy))
}

fn normalize_with(
    req: &InvokeRequest,
    stream_policy: Option<AgenticSubscriptionPolicy>,
) -> Result<NormalizedRequest, (StatusCode, Json<serde_json::Value>)> {
    let content = req.content.trim().to_owned();

    // Validate content is not empty
    if content.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content must not be empty" })),
        ));
    }

    // Validate content length
    if content.len() > MAX_CONTENT_BYTES {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!(
                    "content exceeds maximum size of {} bytes (got {} bytes)",
                    MAX_CONTENT_BYTES,
                    content.len()
                )
            })),
        ));
    }

    // A branch move is validated on every path. The streaming endpoint does not make one yet, so a stream
    // turn that asks for one is refused, rather than landing as an ordinary send that repeats the question.
    let version_turn_requested = version_turn(req)?.is_some();
    if stream_policy.is_some() && version_turn_requested {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": "a regenerate or an edit is not served on the streaming endpoint yet",
                "code": "history_versions_unavailable",
            })),
        ));
    }

    // Resolve model name
    let default_model = load_default_model();
    let model = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map_or(default_model, str::to_owned);

    let provider_hint = req
        .provider
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let subscription_connection_id = req
        .subscription_connection_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let is_codex_subscription = is_codex_subscription_hint(&provider_hint);
    if is_codex_subscription && subscription_connection_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "subscription_connection_id is required for openai-codex-subscription",
            })),
        ));
    }
    if !is_codex_subscription && !subscription_connection_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "subscription_connection_id requires provider openai-codex-subscription",
            })),
        ));
    }
    // Subscription tool decisions are constrained proposals executed only by
    // the gateway. The separate orchestration, vision and browsing paths still
    // lack this connection-aware contract and remain unavailable.
    //
    // The one exception is the agentic run on `execution-core`'s governed loop,
    // which the streaming endpoint may admit (see `AgenticSubscriptionPolicy`).
    if is_codex_subscription {
        // A ZDR turn on the user's own subscription is refused typed, with the same
        // answer the pre-check gives a ZDR token: that subscription cannot give zero
        // retention, so nothing is sent and no other model stands in.
        if req.zdr {
            return Err(crate::zdr_precheck::http(
                &mp_contracts::zdr_refusal::Refusal::CONNECTION_NOT_SUPPORTED,
            ));
        }
        let agentic = req.features.iter().any(|feature| feature == "agentic");
        let other_unsupported = req
            .structured_output_schema
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || !req.attachments.is_empty()
            || req.generate_image
            || req.browse_web
            || req.deep_research
            || !req.tools.is_empty();
        match stream_policy {
            // The unary endpoint never reaches the agent loop, so today's rule
            // holds there whatever the switch says.
            None => {
                if other_unsupported || req.plan_mode || agentic {
                    return Err(refusal(SUBSCRIPTION_STRICT_REFUSAL, None));
                }
            }
            Some(policy) if !policy.enabled() => {
                // The root cause is named first: a client that sends the
                // agentic feature to a server without the switch gets a code it
                // can match on, not the generic text.
                if agentic {
                    return Err(refusal(
                        SUBSCRIPTION_STRICT_REFUSAL,
                        Some(mp_contracts::agentic_subscription::DISABLED_CODE),
                    ));
                }
                if other_unsupported || req.plan_mode {
                    return Err(refusal(SUBSCRIPTION_STRICT_REFUSAL, None));
                }
            }
            Some(policy) => {
                if other_unsupported {
                    return Err(refusal(SUBSCRIPTION_AGENTIC_REFUSAL, None));
                }
                if req.plan_mode && !agentic {
                    return Err(refusal(
                        "plan mode on openai-codex-subscription is available only with an agentic run",
                        None,
                    ));
                }
                if agentic {
                    // Read-only first, matching what the composer sends. The
                    // second switch relaxes this once plan approval is wired
                    // to a subscription run; until then a run that is not in
                    // plan mode could act, and nothing here has approved that.
                    if !req.plan_mode && !policy.effects_permitted() {
                        return Err(refusal(
                            "agentic runs on openai-codex-subscription are read-only for now; send them with plan_mode",
                            None,
                        ));
                    }
                    // execution-core refuses a pseudo model for this route, so
                    // refuse it here, before a durable run is prepared.
                    if mp_contracts::agentic_subscription::is_pseudo_model(&model) {
                        return Err(refusal(
                            "an agentic run on openai-codex-subscription needs an explicit model",
                            None,
                        ));
                    }
                }
            }
        }
    }

    // Validate model against allowlist — but skip when the model is unspecified
    // (empty): inference-core resolves it to a provider default downstream, so
    // there is nothing to validate against the allowlist yet.
    if let Some(refusal) = deployment_allowlist_refusal(load_allowed_models().as_deref(), &model) {
        return Err(refusal);
    }

    if let Some(max_cost) = req.max_cost_usd {
        if max_cost <= 0.0 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "max_cost_usd must be positive" })),
            ));
        }
    }
    if let Some(max_tokens) = req.max_tokens {
        if max_tokens == 0 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "max_tokens must be positive" })),
            ));
        }
    }

    // Fail closed at the edge on an unknown tier numeric: a NEWER client naming
    // a tier this build does not know must be refused here rather than silently
    // treated as no constraint downstream.
    let min_privacy_tier = match req.min_privacy_tier {
        None | Some(0) => None,
        Some(value) => {
            let tier =
                mp_contracts::model_plane::v1::PrivacyTier::try_from(value).map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": format!("unknown privacy tier value: {value}"),
                        })),
                    )
                })?;
            // 0 (UNSPECIFIED) imposes no constraint, mirroring inference-core:
            // GLOBAL=1 is a real floor and IS honored.
            (tier != mp_contracts::model_plane::v1::PrivacyTier::Unspecified).then_some(tier)
        }
    };

    Ok(NormalizedRequest {
        content,
        model,
        provider_hint,
        subscription_connection_id,
        session_key: req.session_key.clone(),
        thread_id: req.thread_id.clone(),
        space_context: req.space_context.clone(),
        space_append_context: req.space_append_context.clone(),
        structured_output_schema: req.structured_output_schema.clone(),
        zdr: req.zdr,
        min_privacy_tier,
        max_cost_usd: req.max_cost_usd,
        max_tokens: req.max_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(models: &[&str]) -> Vec<String> {
        models.iter().map(|model| (*model).to_owned()).collect()
    }

    #[test]
    fn a_model_off_the_deployment_allowlist_is_a_typed_403_naming_the_model_and_the_reason() {
        let allowed = listed(&["a"]);
        let (status, Json(body)) =
            deployment_allowlist_refusal(Some(&allowed), "b").expect("b is not on the list");
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "model_not_entitled");
        assert_eq!(body["error"]["retryable"], false);
        assert_eq!(body["error"]["action"], "choose_another_model");
        assert_eq!(body["error"]["details"]["model"], "b");
        assert_eq!(body["error"]["details"]["reason"], "deployment_allowlist");
        // The list itself is the operator's, not the caller's.
        assert!(!body.to_string().contains("\"a\""));
        assert_eq!(
            mp_contracts::model_entitlement::Refusal::from_body(&body)
                .and_then(|refusal| refusal.reason()),
            Some(mp_contracts::model_entitlement::Reason::DeploymentAllowlist)
        );
    }

    #[test]
    fn the_deployment_allowlist_refuses_nothing_it_did_not_refuse_before() {
        let allowed = listed(&["a", "b"]);
        assert!(
            deployment_allowlist_refusal(None, "z").is_none(),
            "no list set"
        );
        assert!(
            deployment_allowlist_refusal(Some(&[]), "z").is_none(),
            "an empty list"
        );
        assert!(
            deployment_allowlist_refusal(Some(&allowed), "").is_none(),
            "no model named"
        );
        assert!(
            deployment_allowlist_refusal(Some(&allowed), "b").is_none(),
            "a listed model"
        );
    }

    fn make_request(content: &str, model: Option<&str>) -> InvokeRequest {
        InvokeRequest {
            wake_context: None,
            effort: None,
            pinned_message_ids: Vec::new(),
            regenerated: false,
            edited_resubmit: false,
            regenerate_of_message_id: None,
            edit_of_message_id: None,
            content: content.to_owned(),
            model: model.map(str::to_owned),
            provider: None,
            subscription_connection_id: None,
            session_key: None,
            thread_id: None,
            space_context: None,
            space_append_context: None,
            structured_output_schema: None,
            zdr: false,
            min_privacy_tier: None,
            browse_web: false,
            skill_ids: Vec::new(),
            deep_research: false,
            retrieval_scope: None,
            connector_scope: None,
            max_cost_usd: None,
            max_tokens: None,
            profile: None,
            agent_mode: None,
            verbosity: None,
            features: Vec::new(),
            idempotency_key: None,
            attachments: Vec::new(),
            generate_image: false,
            plan_mode: false,
            execute_granted: false,
            approved_plan_id: None,
            tools: Vec::new(),
            org_name: None,
            user_name: None,
            agent_name: None,
            agent_system_prompt: None,
            org_instructions: None,
            space_instructions: None,
            persona_instructions: None,
            persona_tone: None,
            locale: None,
            client_timezone: None,
            client_utc_offset_minutes: None,
            ui_locale: None,
            agent_roles: None,
        }
    }

    #[test]
    fn trims_whitespace() {
        let req = make_request("  hello world  ", None);
        let result = normalize(&req).expect("should succeed");
        assert_eq!(result.content, "hello world");
    }

    #[test]
    fn rejects_empty_content() {
        let req = make_request("   ", None);
        assert!(normalize(&req).is_err());
    }

    #[test]
    fn rejects_oversized_content() {
        let content = "x".repeat(MAX_CONTENT_BYTES + 1);
        let req = make_request(&content, None);
        assert!(normalize(&req).is_err());
    }

    #[test]
    fn unspecified_model_resolves_to_configured_default() {
        let req = make_request("hello", None);
        let result = normalize(&req).expect("should succeed");
        // Unspecified model → the gateway's configured default: `DEFAULT_MODEL`
        // env when set, else empty so inference-core resolves a per-provider
        // default downstream ("Verevon Auto"). Deterministic regardless of env.
        assert_eq!(result.model, load_default_model());
    }

    #[test]
    fn explicit_model_is_preserved() {
        let req = make_request("hello", Some("claude-sonnet-5-5"));
        let result = normalize(&req).expect("should succeed");
        assert_eq!(result.model, "claude-sonnet-5-5");
    }

    #[test]
    fn subscription_provider_requires_a_connection_and_refuses_tools() {
        let mut req = make_request("hello", None);
        req.provider = Some("openai-codex-subscription".to_owned());
        assert!(normalize(&req).is_err(), "connection id is required");

        req.subscription_connection_id = Some(" conn_example ".to_owned());
        let normalized = normalize(&req).expect("text-only subscription request is valid");
        assert_eq!(normalized.provider_hint, "openai-codex-subscription");
        assert_eq!(normalized.subscription_connection_id, "conn_example");

        // `tools` is `Vec<ToolSpec>` (http_routes.rs); this literal had drifted
        // to `Vec<String>` and stopped the whole lib test target compiling.
        req.tools = vec![crate::http_routes::ToolSpec {
            name: "web_search".to_owned(),
            description: String::new(),
            parameters_json: "{}".to_owned(),
        }];
        assert!(
            normalize(&req).is_err(),
            "subscription provider must not run tools"
        );
    }

    #[test]
    fn subscription_provider_allows_the_tools_feature_but_not_agentic() {
        // `tools` (the FEATURE, distinct from the `req.tools` client-declared
        // specs covered above) enables sse.rs's server-side builtins
        // (get_weather, code_interpreter, ...) through the inline tool loop,
        // whose decision rounds are constrained proposals on the subscription
        // route itself. `agentic` is the governed execution-core loop, which
        // the strict (unary) `normalize` never admits; the streaming endpoint
        // admits it only under `AgenticSubscriptionPolicy` (tests below).
        let mut req = make_request("hello", None);
        req.provider = Some("openai-codex-subscription".to_owned());
        req.subscription_connection_id = Some("conn_example".to_owned());

        req.features = vec!["tools".to_owned()];
        assert!(
            normalize(&req).is_ok(),
            "the tools feature must reach a subscription turn for get_weather/code_interpreter parity"
        );

        req.features = vec!["tools".to_owned(), "agentic".to_owned()];
        assert!(
            normalize(&req).is_err(),
            "agentic must still be rejected for a subscription turn"
        );
    }

    #[test]
    fn absent_and_unspecified_tiers_normalize_to_no_constraint() {
        // Absent field and explicit UNSPECIFIED (0) both mean "no constraint";
        // GLOBAL=1 is a real floor and must survive normalization.
        let none = normalize(&make_request("hi", None)).expect("ok");
        let unspecified = {
            let mut req = make_request("hi", None);
            req.min_privacy_tier = Some(0);
            normalize(&req).expect("ok")
        };
        let global = {
            let mut req = make_request("hi", None);
            req.min_privacy_tier = Some(1);
            normalize(&req).expect("ok")
        };
        let sovereign = {
            let mut req = make_request("hi", None);
            req.min_privacy_tier = Some(4);
            normalize(&req).expect("ok")
        };

        assert!(none.min_privacy_tier.is_none());
        assert!(unspecified.min_privacy_tier.is_none());
        assert_eq!(min_privacy_tier_wire(&none), 0);
        assert_eq!(min_privacy_tier_wire(&global), 1);
        assert_eq!(min_privacy_tier_wire(&sovereign), 4);
    }

    #[test]
    fn an_unknown_tier_numeric_fails_closed_at_the_edge() {
        for unknown in [-1, 5, 42, i32::MAX] {
            let mut req = make_request("hi", None);
            req.min_privacy_tier = Some(unknown);
            let error = normalize(&req).expect_err("unknown tier must be refused");
            let message = error.1 .0.to_string();
            assert!(message.contains("unknown privacy tier"), "got: {message}");
        }
    }

    #[test]
    fn retrieval_scope_normalizes_known_values() {
        assert_eq!(retrieval_scope(Some("knowledge_only")), RetrievalScope::KnowledgeOnly);
        assert_eq!(retrieval_scope(Some("web_only")), RetrievalScope::WebOnly);
    }

    /// A subscription request for an agentic run exactly as the composer sends
    /// it: the agentic feature, plan mode, an explicit model, one connection.
    fn agentic_subscription_request() -> InvokeRequest {
        let mut req = make_request("research the Q3 numbers", Some("gpt-5.6-terra"));
        req.provider = Some("openai-codex-subscription".to_owned());
        req.subscription_connection_id = Some("conn_example".to_owned());
        req.features = vec!["agentic".to_owned()];
        req.plan_mode = true;
        req
    }

    fn error_body(
        result: Result<NormalizedRequest, (StatusCode, Json<serde_json::Value>)>,
    ) -> serde_json::Value {
        let (status, Json(body)) = result.expect_err("the request must be refused");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        body
    }

    const ON: AgenticSubscriptionPolicy = AgenticSubscriptionPolicy::new(true, false);

    #[test]
    fn an_agentic_subscription_run_is_admitted_only_with_the_switch_on() {
        let req = agentic_subscription_request();

        let admitted = normalize_stream(&req, ON).expect("switch on admits the read-only run");
        assert_eq!(admitted.provider_hint, "openai-codex-subscription");
        assert_eq!(admitted.subscription_connection_id, "conn_example");
        assert_eq!(admitted.model, "gpt-5.6-terra");

        // Off, whether spelled out or defaulted: refused with the stable code
        // and the text a subscription turn has always got.
        for off in [
            AgenticSubscriptionPolicy::OFF,
            AgenticSubscriptionPolicy::default(),
            AgenticSubscriptionPolicy::new(false, true),
        ] {
            let body = error_body(normalize_stream(&req, off));
            assert_eq!(body["code"], "agentic_subscription_disabled");
            assert_eq!(body["code"], mp_contracts::agentic_subscription::DISABLED_CODE);
            assert_eq!(body["error"], SUBSCRIPTION_STRICT_REFUSAL);
            assert!(
                body["error"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("openai-codex-subscription supports text-only")),
                "the existing 400 text is kept"
            );
        }
    }

    #[test]
    fn the_unary_endpoint_stays_strict_whatever_the_switch_says() {
        // `normalize` takes no policy: the unary invoke has no path to the
        // agent loop. Same answer as before this exception existed, with no
        // `code` field.
        let req = agentic_subscription_request();
        let body = error_body(normalize(&req));
        assert_eq!(body, json!({ "error": SUBSCRIPTION_STRICT_REFUSAL }));

        let mut plan_only = agentic_subscription_request();
        plan_only.features.clear();
        assert_eq!(
            error_body(normalize(&plan_only)),
            json!({ "error": SUBSCRIPTION_STRICT_REFUSAL }),
            "plan_mode without agentic is refused on the unary path too"
        );
        // And a plain subscription turn is unaffected by all of this.
        let mut plain = agentic_subscription_request();
        plain.features.clear();
        plain.plan_mode = false;
        assert!(normalize(&plain).is_ok());
        assert!(normalize_stream(&plain, ON).is_ok());
        assert!(normalize_stream(&plain, AgenticSubscriptionPolicy::OFF).is_ok());
    }

    #[test]
    fn an_agentic_subscription_run_must_be_read_only_until_effects_are_allowed() {
        let mut req = agentic_subscription_request();
        req.plan_mode = false;

        let body = error_body(normalize_stream(&req, ON));
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|text| text.contains("read-only") && text.contains("plan_mode")),
            "{body}"
        );
        assert!(body.get("code").is_none(), "no stable code beyond the disabled one");

        // The second switch relaxes exactly this, and only with the first on.
        assert!(normalize_stream(&req, AgenticSubscriptionPolicy::new(true, true)).is_ok());
        let body = error_body(normalize_stream(&req, AgenticSubscriptionPolicy::new(false, true)));
        assert_eq!(body["code"], "agentic_subscription_disabled");
    }

    #[test]
    fn plan_mode_alone_is_still_refused_on_a_subscription_turn() {
        let mut req = agentic_subscription_request();
        req.features.clear();
        // Off: the original refusal. On: refused for its own reason.
        let off = error_body(normalize_stream(&req, AgenticSubscriptionPolicy::OFF));
        assert_eq!(off["error"], SUBSCRIPTION_STRICT_REFUSAL);
        assert!(off.get("code").is_none());
        let on = error_body(normalize_stream(&req, ON));
        assert!(
            on["error"].as_str().is_some_and(|text| text.contains("only with an agentic run")),
            "{on}"
        );
        // A "tools"-only feature set does not make plan mode admissible.
        req.features = vec!["tools".to_owned()];
        assert!(normalize_stream(&req, ON).is_err());
    }

    #[test]
    fn every_other_subscription_restriction_still_holds_for_an_agentic_run() {
        type Mutation = fn(&mut InvokeRequest);
        let mutations: [(&str, Mutation); 8] = [
            ("zdr", |req| req.zdr = true),
            ("attachments", |req| {
                req.attachments = vec![crate::vision::InvokeAttachment::default()];
            }),
            ("browse_web", |req| req.browse_web = true),
            ("deep_research", |req| req.deep_research = true),
            ("generate_image", |req| req.generate_image = true),
            ("structured schema", |req| {
                req.structured_output_schema = Some(r#"{"type":"object"}"#.to_owned());
            }),
            ("client tools", |req| {
                req.tools = vec![crate::http_routes::ToolSpec {
                    name: "web_search".to_owned(),
                    description: String::new(),
                    parameters_json: "{}".to_owned(),
                }];
            }),
            ("effects without plan mode", |req| req.plan_mode = false),
        ];
        for (label, mutate) in mutations {
            let mut req = agentic_subscription_request();
            mutate(&mut req);
            assert!(
                normalize_stream(&req, ON).is_err(),
                "{label} must stay refused with the switch on"
            );
            // And with the switch off it is refused too, never turned into a
            // turn on another route.
            assert!(
                normalize_stream(&req, AgenticSubscriptionPolicy::OFF).is_err(),
                "{label} must stay refused with the switch off"
            );
        }
        // The connection id is still required for the route to mean anything.
        let mut no_connection = agentic_subscription_request();
        no_connection.subscription_connection_id = None;
        assert!(normalize_stream(&no_connection, ON).is_err());
    }

    #[test]
    fn an_agentic_subscription_run_needs_an_explicit_model() {
        for pseudo in ["verevon-balance", "Verevon-Genius", "auto", "default", "verevon"] {
            let mut req = agentic_subscription_request();
            req.model = Some(pseudo.to_owned());
            let body = error_body(normalize_stream(&req, ON));
            assert!(
                body["error"].as_str().is_some_and(|text| text.contains("explicit model")),
                "{pseudo}: {body}"
            );
        }
        // No model at all resolves to DEFAULT_MODEL, which is empty unless an
        // operator pinned one; empty is refused, a pinned concrete id is not.
        let mut req = agentic_subscription_request();
        req.model = None;
        let outcome = normalize_stream(&req, ON);
        if load_default_model().is_empty() {
            assert!(outcome.is_err(), "an empty model is not explicit");
        }
        // The plain subscription chat turn does not need one.
        let mut chat = agentic_subscription_request();
        chat.features.clear();
        chat.plan_mode = false;
        chat.model = Some("verevon-balance".to_owned());
        assert!(normalize_stream(&chat, ON).is_ok());
    }

    #[test]
    fn the_policy_does_not_touch_a_turn_that_is_not_on_the_subscription() {
        // The same request, with and without the switch, on every other
        // provider: identical outcome, so enabling the route for one org's
        // seat changes nothing for anyone else.
        for provider in [None, Some("azure-openai"), Some("anthropic")] {
            for features in [vec![], vec!["agentic".to_owned()], vec!["tools".to_owned()]] {
                for plan_mode in [false, true] {
                    let mut req = make_request("hello", Some("gpt-4o"));
                    req.provider = provider.map(str::to_owned);
                    req.features = features.clone();
                    req.plan_mode = plan_mode;
                    let strict = normalize(&req).map(|normalized| normalized.provider_hint);
                    for policy in [AgenticSubscriptionPolicy::OFF, ON] {
                        let streamed =
                            normalize_stream(&req, policy).map(|normalized| normalized.provider_hint);
                        assert_eq!(
                            strict.as_ref().ok(),
                            streamed.as_ref().ok(),
                            "{provider:?} {features:?} plan_mode={plan_mode}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn subscription_aliases_are_recognised_and_the_canonical_id_wins() {
        for alias in [
            "openai-codex-subscription",
            "OpenAI-Codex-Subscription",
            " codex-subscription ",
            "chatgpt-codex",
        ] {
            assert!(is_codex_subscription_hint(alias), "{alias:?}");
        }
        for other in ["", "azure-openai", "openai", "codex"] {
            assert!(!is_codex_subscription_hint(other), "{other:?}");
        }
        // An alias is admitted like the canonical id; the route the gateway
        // forwards to execution-core is canonicalised separately (sse.rs).
        let mut req = agentic_subscription_request();
        req.provider = Some("chatgpt-codex".to_owned());
        assert!(normalize_stream(&req, ON).is_ok());
    }

    #[test]
    fn the_policy_defaults_to_off_and_effects_need_the_master_switch() {
        assert!(!AgenticSubscriptionPolicy::default().enabled());
        assert!(!AgenticSubscriptionPolicy::OFF.effects_permitted());
        assert!(!AgenticSubscriptionPolicy::new(false, true).effects_permitted());
        assert!(!ON.effects_permitted());
        assert!(AgenticSubscriptionPolicy::new(true, true).effects_permitted());
    }

    #[test]
    fn retrieval_scope_fails_safe_to_auto() {
        // Absent, unknown, and empty values must all land on Auto — never a
        // restrictive mode a newer client didn't actually ask for.
        assert_eq!(retrieval_scope(None), RetrievalScope::Auto);
        assert_eq!(retrieval_scope(Some("")), RetrievalScope::Auto);
        assert_eq!(retrieval_scope(Some("knowledgeOnly")), RetrievalScope::Auto);
        assert_eq!(retrieval_scope(Some("bogus")), RetrievalScope::Auto);
        assert!(RetrievalScope::Auto.is_auto());
        assert!(!RetrievalScope::KnowledgeOnly.is_auto());
        assert!(!RetrievalScope::WebOnly.is_auto());
    }

    fn version_request(regenerate: Option<&str>, edit: Option<&str>) -> InvokeRequest {
        let mut req = make_request("question", Some("m"));
        req.thread_id = Some("thread-1".to_owned());
        req.regenerate_of_message_id = regenerate.map(str::to_owned);
        req.edit_of_message_id = edit.map(str::to_owned);
        req
    }

    #[test]
    fn a_regenerate_or_an_edit_names_its_turn_and_a_blank_id_is_no_turn() {
        let regenerate = version_turn(&version_request(Some(" answer-1 "), None))
            .expect("a regenerate of an answer is valid");
        assert_eq!(
            regenerate,
            Some(VersionTurn::Regenerate {
                message_id: "answer-1".to_owned()
            })
        );
        let edit = version_turn(&version_request(None, Some("turn-2"))).expect("an edit is valid");
        assert_eq!(
            edit,
            Some(VersionTurn::Edit {
                message_id: "turn-2".to_owned()
            })
        );
        let blank = version_turn(&version_request(Some("  "), None)).expect("a blank id is absent");
        assert_eq!(blank, None);
    }

    #[test]
    fn a_turn_naming_both_a_regenerate_and_an_edit_is_malformed() {
        let body = error_body(normalize(&version_request(Some("a"), Some("b"))));
        assert_eq!(body["code"], "version_turn_invalid");
    }

    #[test]
    fn a_version_turn_without_its_thread_is_malformed() {
        let mut req = version_request(Some("answer-1"), None);
        req.thread_id = None;
        let body = error_body(normalize(&req));
        assert_eq!(body["code"], "version_turn_invalid");
    }

    #[test]
    fn a_do_turn_cannot_regenerate_or_edit() {
        let mut agentic = version_request(Some("answer-1"), None);
        agentic.features = vec!["agentic".to_owned()];
        assert_eq!(error_body(normalize(&agentic))["code"], "feature_not_supported");

        let mut granted = version_request(None, Some("turn-2"));
        granted.execute_granted = true;
        assert_eq!(error_body(normalize(&granted))["code"], "feature_not_supported");
    }

    #[test]
    fn the_streaming_endpoint_refuses_a_version_turn_until_it_moves_branches() {
        let (status, Json(body)) = normalize_stream(
            &version_request(Some("answer-1"), None),
            AgenticSubscriptionPolicy::OFF,
        )
        .expect_err("a stream turn that moves a branch is refused");
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["code"], "history_versions_unavailable");

        assert!(normalize_stream(&make_request("hi", Some("m")), AgenticSubscriptionPolicy::OFF).is_ok());
    }
}
