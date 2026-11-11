//! What a zero-data-retention (ZDR) turn does with each field of an invoke
//! request, and the typed refusal a request for something it cannot do gets.
//!
//! A ZDR turn takes a persistence-free path: no thread, run, event, memory,
//! cache, idempotency record or tool loop, and one model call. Before this
//! module, a field that path could not perform was dropped without a word:
//! browsing, a client's tool list, a structured-output schema, plan mode, a
//! spend cap and a pinned context all reached the answer as if they had not
//! been asked for. Every field is now one of three things, written down here:
//!
//! * [`Treatment::Honored`]: the path performs it (instructions, verbosity,
//!   the token and thinking budgets, grounding in org knowledge).
//! * [`Treatment::Refused`]: a request that uses it gets the catalogue refusal
//!   for that code and feature, before any model call. Only a request that asks
//!   for the thing is refused; a default value (a `false` flag, an empty list)
//!   is not a request, so an ordinary turn is untouched.
//! * [`Treatment::Inert`]: accepted, with no effect on a ZDR turn by design, and
//!   the reason says why. Nothing the user asked for is lost, because nothing
//!   was asked for.
//!
//! [`invoke_field_names`] destructures [`InvokeRequest`] without a `..` rest
//! pattern, so a new field stops the build here until it is named, and
//! [`FIELD_TREATMENTS`] must then classify it (the table test checks both).

use mp_contracts::zdr_refusal::{code, Refusal};

use crate::http_routes::InvokeRequest;

/// How a ZDR turn treats one request field or one feature string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Treatment {
    /// The persistence-free path performs it.
    Honored,
    /// A request that uses it is refused before any model call, with this code
    /// and, when the code takes one, this feature.
    Refused {
        code: &'static str,
        feature: Option<&'static str>,
    },
    /// Accepted with no effect on a ZDR turn, for the stated reason.
    Inert(&'static str),
    /// Classified member by member: the field is a list of feature strings, and
    /// each is classified in [`FEATURE_TREATMENTS`].
    PerMember,
}

/// A row that refuses a request asking for `feature`.
const fn refused_feature(feature: &'static str) -> Treatment {
    Treatment::Refused {
        code: code::FEATURE_NOT_SUPPORTED,
        feature: Some(feature),
    }
}

/// Every field of [`InvokeRequest`], in declaration order, with its treatment.
/// The table test requires exactly one row per field.
pub const FIELD_TREATMENTS: &[(&str, Treatment)] = &[
    ("content", Treatment::Honored),
    ("model", Treatment::Honored),
    // A platform provider is honored. The user's own connection is refused by the turn's posture
    // decision (`zdr_posture`), which answers it before this module is consulted.
    (
        "provider",
        Treatment::Refused {
            code: code::CONNECTION_NOT_ACKNOWLEDGED,
            feature: None,
        },
    ),
    (
        "subscription_connection_id",
        Treatment::Refused {
            code: code::CONNECTION_NOT_ACKNOWLEDGED,
            feature: None,
        },
    ),
    // A customer-gateway connection is refused by the turn's posture decision as well (W4-BYO-6), before this
    // module is consulted; the same code as the subscription connection.
    (
        "model_connection_id",
        Treatment::Refused {
            code: code::CONNECTION_NOT_ACKNOWLEDGED,
            feature: None,
        },
    ),
    (
        "session_key",
        Treatment::Inert(
            "a ZDR turn writes no thread, so there is nothing for a session key to address",
        ),
    ),
    (
        "thread_id",
        Treatment::Inert(
            "a ZDR turn writes no thread: the answer is returned, and it is not appended to one",
        ),
    ),
    (
        "space_append_context",
        Treatment::Inert("it appends to a durable thread, which a ZDR turn never writes"),
    ),
    ("space_context", Treatment::Honored),
    // A wake starts a durable run from a stored post. The invoke stream refuses it first with the
    // same code (`space_sse::check_wake`); this row keeps the table total and the answer the same.
    (
        "wake_context",
        Treatment::Refused {
            code: code::DURABLE_MUTATION_FORBIDDEN,
            feature: None,
        },
    ),
    // The schema is sent to the model with the turn, on both ZDR paths, as the
    // unary path always did: the provider answers in the schema's shape.
    ("structured_output_schema", Treatment::Honored),
    ("zdr", Treatment::Honored),
    ("min_privacy_tier", Treatment::Honored),
    ("browse_web", refused_feature("web_search")),
    ("skill_ids", refused_feature("skills")),
    ("deep_research", refused_feature("deep_research")),
    // EP-2 S5: the plan and run turns of a two-turn deep research. Both only shape a deep-research
    // turn, which `deep_research` refuses before either phase, so a plan turn under ZDR gets the
    // same refusal and neither field has anything to do on its own.
    (
        "research_phase",
        Treatment::Inert("read only on a deep-research turn, which a ZDR turn refuses through `deep_research`"),
    ),
    (
        "research_plan",
        Treatment::Inert("read only on a deep-research turn, which a ZDR turn refuses through `deep_research`"),
    ),
    // `web_only` is refused. `auto` and `knowledge_only` are honored: grounding
    // then reads org knowledge only, and no web source was asked for. A
    // knowledge-only deep research (EP-2 S7) is still refused, through
    // `deep_research`, which [`refusal`] decides before the scope.
    ("retrieval_scope", refused_feature("web_search")),
    ("connector_scope", refused_feature("connector_read")),
    (
        "regenerated",
        Treatment::Inert("a learning signal, and learning does not run on a ZDR turn"),
    ),
    ("pinned_message_ids", refused_feature("pinned_messages")),
    (
        "edited_resubmit",
        Treatment::Inert("a learning signal, and learning does not run on a ZDR turn"),
    ),
    // A version move names a durable message of the thread, and a ZDR turn writes none (WS17 S9).
    ("regenerate_of_message_id", refused_feature("message_versions")),
    ("edit_of_message_id", refused_feature("message_versions")),
    ("max_cost_usd", refused_feature("spend_cap")),
    // The answer is capped at the caller's `max_tokens` (the lower of the two).
    ("max_tokens", Treatment::Honored),
    // `chat` is honored. A deployed-agent profile is an unattended agent, which
    // ZDR refuses.
    ("profile", refused_feature("deployed_agent")),
    // The mode a person chose is resolved before a turn is admitted (WS25-S6): it only tightens the
    // posture, and a plan gate it sets is refused through `plan_mode` like any plan flag.
    ("agent_mode", Treatment::Honored),
    ("verbosity", Treatment::Honored),
    ("effort", Treatment::Honored),
    // The speed tier is a subscription route's choice (`InferRequest.speed_tier`), and a ZDR turn never runs
    // on a subscription route (`subscription_connection_id` above), so no ZDR call names a tier.
    (
        "speed",
        Treatment::Inert(
            "the priority tier is served only on a subscription route, which a ZDR turn refuses, so no ZDR call names a speed tier",
        ),
    ),
    ("features", Treatment::PerMember),
    (
        "idempotency_key",
        Treatment::Inert(
            "a ZDR turn keeps no idempotency record, so a replayed request runs again",
        ),
    ),
    ("attachments", refused_feature("attachments")),
    ("generate_image", refused_feature("image_generation")),
    ("plan_mode", refused_feature("plan_mode")),
    // An execute turn runs an executing loop under a plan grant. A ZDR turn keeps no
    // plan and no grant, so it is refused through the `agentic` feature it rides on.
    ("execute_granted", refused_feature("agentic")),
    ("approved_plan_id", refused_feature("agentic")),
    // Coordinator mode is never sent for a ZDR turn: the gateway sends the flag as false, so the turn is answered with
    // the tools an ordinary run is offered, and nothing is refused for asking.
    (
        "coordinator",
        Treatment::Inert("a ZDR turn is never coordinated: the gateway sends the flag as false on it"),
    ),
    // A client's own tool definitions. Those the composer derives from its
    // actions are declared here too, so an action is refused the same way.
    ("tools", refused_feature("client_tools")),
    ("org_name", Treatment::Honored),
    ("user_name", Treatment::Honored),
    ("agent_name", Treatment::Honored),
    ("agent_system_prompt", Treatment::Honored),
    ("org_instructions", Treatment::Honored),
    ("space_instructions", Treatment::Honored),
    ("persona_instructions", Treatment::Honored),
    ("persona_tone", Treatment::Honored),
    ("locale", Treatment::Honored),
    ("client_timezone", Treatment::Honored),
    ("client_utc_offset_minutes", Treatment::Honored),
    ("ui_locale", Treatment::Honored),
    (
        "agent_roles",
        Treatment::Inert(
            "no tool loop runs on a ZDR turn, so no subagent takes a role; the catalogue is never built",
        ),
    ),
    // EP-6 S9: the pinned agent version is read before the ZDR branch, so its allowlist and
    // autonomy cap narrow a ZDR turn exactly as any other; only ids and the digest are logged.
    ("agent_ref", Treatment::Honored),
    ("agent_version", Treatment::Honored),
    ("agent_digest", Treatment::Honored),
    ("agent_tool_capability_ids", Treatment::Honored),
    ("agent_autonomy_cap", Treatment::Honored),
    // EP-2 S8: a wide-research run is a Do run, refused under ZDR through the `agentic` feature it
    // rides on; the estimate route refuses a ZDR caller the same way.
    ("wide_research", refused_feature("agentic")),
    // EP-3 S5: a reference is resolved against durable state (a document's ACL, a thread, a
    // revision), so a ZDR turn carrying any is refused, and none is silently dropped.
    ("context_refs", refused_feature("context_references")),
    // EP-5 S7: a deck template is a skill bundle; a ZDR turn never mounts one, so a pick is refused like a skill.
    ("deck_template", refused_feature("skills")),
    // EP-7 S1: a goal is user content in a durable store, so a ZDR turn naming one is refused.
    ("goal_id", refused_feature("thread_goal")),
    // EP-7 S4: a pump-sent goal turn continues a goal, so a ZDR turn carrying the mark is refused like the goal.
    ("goal_auto", refused_feature("thread_goal")),
];

/// The feature strings the gateway reads, each with its treatment. A feature
/// string not listed here has no effect on either path, so it is inert.
pub const FEATURE_TREATMENTS: &[(&str, Treatment)] = &[
    (
        "agentic",
        Treatment::Refused {
            code: code::FEATURE_NOT_SUPPORTED,
            feature: Some("agentic"),
        },
    ),
    ("conversation_only", Treatment::Honored),
    ("knowledge", Treatment::Honored),
    ("rag", Treatment::Honored),
    ("citations", Treatment::Honored),
    (
        "tools",
        Treatment::Inert("no tool loop runs on a ZDR turn; knowledge grounding runs through retrieval instead"),
    ),
    (
        "artifacts",
        Treatment::Inert("no artifact is produced on a ZDR turn; the requests that could ask for one (tools, Do mode, deep research, image generation) are refused"),
    ),
    (
        "memory",
        Treatment::Inert("memory is neither read nor written on a ZDR turn"),
    ),
    // One usage frame when the request asks for it: the counts, the serving
    // provider, its residency and tier, and the retention class. The cost and the
    // cache counts are stated only behind `MODEL_GATEWAY_ZDR_STREAM_COST` (WS27-S5),
    // computed for that frame and stored nowhere (DEF-Z-06b). No content is carried.
    ("usage", Treatment::Honored),
    (
        "reasoning",
        Treatment::Inert("a ZDR turn streams no reasoning text, although the effort setting is honored"),
    ),
    (
        "steps",
        Treatment::Inert("a ZDR turn runs no steps: it has no tool loop and no plan"),
    ),
];

/// The treatment of a feature string: its row when it has one, otherwise inert.
#[must_use]
pub fn feature_treatment(name: &str) -> Treatment {
    FEATURE_TREATMENTS
        .iter()
        .find(|(feature, _)| *feature == name)
        .map_or(
            Treatment::Inert(
                "a feature name the gateway does not read has no effect on either path",
            ),
            |(_, treatment)| *treatment,
        )
}

/// The name of every field of [`InvokeRequest`], in declaration order.
///
/// The destructuring names each field and has no `..`, so a field added to the
/// struct makes this function fail to build until it is named here. The list it
/// returns is then compared with [`FIELD_TREATMENTS`] by the table test.
#[must_use]
pub fn invoke_field_names(request: &InvokeRequest) -> Vec<&'static str> {
    let InvokeRequest {
        content: _,
        model: _,
        provider: _,
        subscription_connection_id: _,
        model_connection_id: _,
        session_key: _,
        thread_id: _,
        space_append_context: _,
        space_context: _,
        wake_context: _,
        structured_output_schema: _,
        zdr: _,
        min_privacy_tier: _,
        browse_web: _,
        skill_ids: _,
        deep_research: _,
        research_phase: _,
        research_plan: _,
        retrieval_scope: _,
        connector_scope: _,
        regenerated: _,
        pinned_message_ids: _,
        edited_resubmit: _,
        regenerate_of_message_id: _,
        edit_of_message_id: _,
        max_cost_usd: _,
        max_tokens: _,
        profile: _,
        agent_mode: _,
        verbosity: _,
        effort: _,
        speed: _,
        features: _,
        idempotency_key: _,
        attachments: _,
        generate_image: _,
        plan_mode: _,
        execute_granted: _,
        approved_plan_id: _,
        coordinator: _,
        tools: _,
        org_name: _,
        user_name: _,
        agent_name: _,
        agent_system_prompt: _,
        org_instructions: _,
        space_instructions: _,
        persona_instructions: _,
        persona_tone: _,
        locale: _,
        client_timezone: _,
        client_utc_offset_minutes: _,
        ui_locale: _,
        agent_roles: _,
        agent_ref: _,
        agent_version: _,
        agent_digest: _,
        agent_tool_capability_ids: _,
        agent_autonomy_cap: _,
        wide_research: _,
        context_refs: _,
        deck_template: _,
        goal_id: _,
        goal_auto: _,
    } = request;
    vec![
        "content",
        "model",
        "provider",
        "subscription_connection_id",
        "model_connection_id",
        "session_key",
        "thread_id",
        "space_append_context",
        "space_context",
        "wake_context",
        "structured_output_schema",
        "zdr",
        "min_privacy_tier",
        "browse_web",
        "skill_ids",
        "deep_research",
        "research_phase",
        "research_plan",
        "retrieval_scope",
        "connector_scope",
        "regenerated",
        "pinned_message_ids",
        "edited_resubmit",
        "regenerate_of_message_id",
        "edit_of_message_id",
        "max_cost_usd",
        "max_tokens",
        "profile",
        "agent_mode",
        "verbosity",
        "effort",
        "speed",
        "features",
        "idempotency_key",
        "attachments",
        "generate_image",
        "plan_mode",
        "execute_granted",
        "approved_plan_id",
        "coordinator",
        "tools",
        "org_name",
        "user_name",
        "agent_name",
        "agent_system_prompt",
        "org_instructions",
        "space_instructions",
        "persona_instructions",
        "persona_tone",
        "locale",
        "client_timezone",
        "client_utc_offset_minutes",
        "ui_locale",
        "agent_roles",
        "agent_ref",
        "agent_version",
        "agent_digest",
        "agent_tool_capability_ids",
        "agent_autonomy_cap",
        "wide_research",
        "context_refs",
        "deck_template",
        "goal_id",
        "goal_auto",
    ]
}

/// The refusal a ZDR turn gets for what its request asks for that the
/// persistence-free path cannot do, or `None` when everything it asks for is
/// honored or inert.
///
/// The order is fixed, so a request that asks for several things is refused for
/// the first of them, and the same request always gets the same answer. The
/// connection route is not decided here: the turn's posture decision answers it first.
#[must_use]
pub fn refusal(request: &InvokeRequest) -> Option<Refusal> {
    refusal_with(request, crate::deck_templates::enabled())
}

/// [`refusal`] with the deck-template switch (`SKILL_DECK_TEMPLATES_ENABLED`, EP-5 S7) passed in, so
/// a test decides a `deck_template` row under either setting without writing the process environment.
#[must_use]
pub fn refusal_with(request: &InvokeRequest, deck_templates_enabled: bool) -> Option<Refusal> {
    if request.wake_context.is_some() {
        return Some(Refusal::DURABLE_MUTATION_FORBIDDEN);
    }
    let conversation_only = request
        .features
        .iter()
        .any(|feature| feature == "conversation_only");
    let feature = |key: &'static str| Some(Refusal::feature_not_supported(key));
    if !request.attachments.is_empty() {
        return feature("attachments");
    }
    if request.generate_image {
        return feature("image_generation");
    }
    if request.deep_research {
        return feature("deep_research");
    }
    if request.features.iter().any(|value| value == "agentic") {
        return feature("agentic");
    }
    if request.execute_granted {
        return feature("agentic");
    }
    if request.approved_plan_id.as_deref().is_some_and(|id| !id.trim().is_empty()) {
        return feature("agentic");
    }
    if request.wide_research.as_ref().is_some_and(|wide| !wide.is_null()) {
        return feature("agentic");
    }
    if request.regenerate_of_message_id.as_deref().is_some_and(|id| !id.trim().is_empty())
        || request.edit_of_message_id.as_deref().is_some_and(|id| !id.trim().is_empty())
    {
        return feature("message_versions");
    }
    if request.plan_mode {
        return feature("plan_mode");
    }
    if request.browse_web {
        return feature("web_search");
    }
    if !conversation_only
        && crate::normalize::retrieval_scope(request.retrieval_scope.as_deref())
            == crate::normalize::RetrievalScope::WebOnly
    {
        return feature("web_search");
    }
    if request.connector_scope.as_ref().is_some_and(|ids| !ids.is_empty()) {
        return feature("connector_read");
    }
    if !request.tools.is_empty() {
        return feature("client_tools");
    }
    if !request.skill_ids.is_empty() {
        return feature("skills");
    }
    if !request.pinned_message_ids.is_empty() {
        return feature("pinned_messages");
    }
    if crate::context_refs::requested(request.context_refs.as_ref()) {
        return feature(crate::context_refs::ZDR_FEATURE);
    }
    if crate::deck_templates::zdr_refused(deck_templates_enabled, request.deck_template.as_ref()) {
        return feature(crate::deck_templates::ZDR_FEATURE);
    }
    if request.goal_id.as_deref().is_some_and(|id| !id.trim().is_empty()) {
        return feature(crate::thread_goal_routes::ZDR_FEATURE);
    }
    if request.goal_auto {
        return feature(crate::thread_goal_routes::ZDR_FEATURE);
    }
    if request.max_cost_usd.is_some() {
        return feature("spend_cap");
    }
    if matches!(
        crate::profile::AgentProfile::from_wire(request.profile.as_deref()),
        crate::profile::AgentProfile::DeployedAgent
    ) {
        return feature("deployed_agent");
    }
    None
}

/// The framing that tells the model whose org and user it acts for, for a ZDR
/// turn. The same sentence as the grounded identity message, without the
/// instruction to call a knowledge tool: a ZDR turn offers no tools, so that
/// instruction would send the model to a tool that is not there.
#[must_use]
pub fn identity_framing(request: &InvokeRequest) -> Option<String> {
    let org = request
        .org_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let user = request
        .user_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Some(match user {
        Some(user) => format!(
            "You are Verevon, the AI assistant currently helping {org}. The signed-in user is {user}. When they say \"we\", \"us\", \"our\", or \"the company\", they mean {org}, not Verevon itself. When they say \"I\", \"me\", or \"my\", they mean themselves, {user}."
        ),
        None => format!(
            "You are Verevon, the AI assistant currently helping {org}. When the user says \"we\", \"us\", \"our\", or \"the company\", they mean {org}, not Verevon itself."
        ),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn request(body: serde_json::Value) -> InvokeRequest {
        serde_json::from_value(body).expect("a wire body parses")
    }

    /// EP-2 S7: knowledge and connector sources in deep research change nothing on a ZDR turn. Deep
    /// research is refused before the scope or the selection is read, so neither switch can let a
    /// knowledge-only run or a connected-account read slip past it.
    #[test]
    fn deep_research_with_internal_sources_is_refused_under_zdr() {
        let deep_research = Some(Refusal::feature_not_supported("deep_research"));
        // e_zdr_token_knowledge_only_deep_research_refused
        assert_eq!(
            refusal_with(
                &request(json!({
                    "content": "hello",
                    "deep_research": true,
                    "retrieval_scope": "knowledge_only",
                })),
                false,
            ),
            deep_research
        );
        // e_zdr_token_deep_research_connector_sources_refused
        assert_eq!(
            refusal_with(
                &request(json!({
                    "content": "hello",
                    "deep_research": true,
                    "connector_scope": ["conn_1"],
                })),
                false,
            ),
            deep_research
        );
        // Without deep research the knowledge-only scope is honoured and a selection is refused
        // for itself, as before.
        assert_eq!(
            refusal_with(
                &request(json!({"content": "hello", "retrieval_scope": "knowledge_only"})),
                false,
            ),
            None
        );
        assert_eq!(
            refusal_with(
                &request(json!({"content": "hello", "connector_scope": ["conn_1"]})),
                false,
            ),
            Some(Refusal::feature_not_supported("connector_read"))
        );
    }
}
