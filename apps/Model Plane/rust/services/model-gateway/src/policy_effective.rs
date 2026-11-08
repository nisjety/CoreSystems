//! `GET /v1/policy/effective?thread_id=`: the values the gateway enforces for one thread, each with the layer that set
//! it and that layer's revision (Wave 3 EP-9 T2).
//!
//! Each owner answers for its own layers and the BFF aggregates them, so this view carries only what the gateway
//! decides itself: the approval posture and whether a pin fixes it, the run budget, and the organization's agent
//! switches as the gateway enforces them. An organization value is reported only where the enforced policy changed it
//! from the platform default. A policy the gateway cannot read is unavailable for each key it would have set, never a
//! guess. Session Core says whether the caller owns the thread, and a thread the caller does not own answers exactly as
//! one that does not exist. The view is read-only and stores nothing.

use std::sync::{Arc, OnceLock};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use mp_contracts::agent_mode::AgentMode;
use mp_contracts::model_plane::v1::{AutonomyRung, ResolveThreadOwnerRequest};
use mp_contracts::org_agent_policy::{
    Status, DEFAULT_MAX_FAN_OUT_ITEMS, RESEARCH_MAX_FAN_OUT_ITEMS, SETTINGS,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::auth::{Claims, VerifiedCapabilityBearer, VerifiedSessionBearer};
use crate::http_routes::authenticated_session_request;
use crate::org_agent_policy::{ensure, policy_mode, shared_cache, OrgPolicy, PolicyMode};
use crate::profile::ApprovalPosture;
use crate::run_budget::{run_budget_from_env, RunBudget};
use crate::state::AppState;

/// The environment switch of this view (Wave 3 EP-9 T2). Off by default: the route answers 404 and nothing is read.
/// Parsed by the shared strict parser, so only the literal `true` enables it.
pub const POLICY_EFFECTIVE_ENV: &str = "MODEL_GATEWAY_POLICY_EFFECTIVE";

/// The one strict parser of the switch.
#[must_use]
pub fn policy_effective_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// Whether the switch is on in this process. Read once, so it cannot change under a request.
#[must_use]
pub fn policy_effective_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        policy_effective_from(std::env::var(POLICY_EFFECTIVE_ENV).ok().as_deref())
    })
}

const ORIGIN_PLATFORM: &str = "platform";
const ORIGIN_ORG: &str = "org";

/// The longest thread id the view accepts. A session thread id is a short opaque string.
const MAX_THREAD_ID_LEN: usize = 200;

const APPROVAL_POSTURE: &str = "approval.posture";
const BUDGET_MAX_ROUNDS: &str = "budget.max_rounds";
const BUDGET_MAX_SUBAGENTS: &str = "budget.max_subagents";
const MODE_CAP: &str = "mode_cap";
const RUNG_CAP: &str = "rung_cap";
const WEB_MODE: &str = "web.mode";
const WEB_PAID_PROVIDERS: &str = "web.paid_providers";

/// The organization keys this view reports from the enforced policy, in the order the view lists them.
const ORG_KEYS: [&str; 5] = [
    MODE_CAP,
    RUNG_CAP,
    WEB_MODE,
    WEB_PAID_PROVIDERS,
    RESEARCH_MAX_FAN_OUT_ITEMS,
];

/// The rung cap a policy that sets none reads as: the loosest rung, which caps nothing.
const RUNG_CAP_DEFAULT: &str = "danger_full_access";

/// One value in the view: what it is, the layer that set it, and whether a layer above the person fixes it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EffectiveItem {
    pub key: &'static str,
    /// The value in force. `None` only when `unavailable` is set.
    pub value: Option<Value>,
    /// The layer that set the value: `platform`, `org`, `space`, `user` or `request`. `None` only when unavailable.
    pub origin: Option<&'static str>,
    /// The organization policy revision the value was set at. Set only for an organization origin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    /// Whether a layer above the person fixes the value, so the person cannot choose a looser one.
    pub locked: bool,
    /// The gateway could not answer for this key, so it gives no value rather than a guess.
    #[serde(skip_serializing_if = "is_false")]
    pub unavailable: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl EffectiveItem {
    fn platform(key: &'static str, value: Value, locked: bool) -> Self {
        Self {
            key,
            value: Some(value),
            origin: Some(ORIGIN_PLATFORM),
            revision: None,
            locked,
            unavailable: false,
        }
    }

    fn unavailable(key: &'static str) -> Self {
        Self {
            key,
            value: None,
            origin: None,
            revision: None,
            locked: false,
            unavailable: true,
        }
    }
}

/// What the gateway's layers hold for one call, read before the view is built.
#[derive(Debug)]
pub struct Layers {
    /// The approval posture floor: the platform default, or `Ask` when the verified scopes pin it.
    pub posture: ApprovalPosture,
    /// The run budget the environment sets for a standard route.
    pub budget: RunBudget,
    /// The organization's agent policy as the gateway enforces it for this call.
    pub org: OrgLayer,
}

/// The organization layer as the gateway enforces it. Only an enforced policy binds a value.
#[derive(Debug)]
pub enum OrgLayer {
    /// The switch does not enforce the policy (off, or shadow), so the platform defaults apply.
    NotBinding,
    /// The enforced policy, held through the cache.
    Held(Arc<OrgPolicy>),
    /// The switch enforces the policy but it cannot be read. Each organization key is unavailable, never guessed.
    Unreadable,
}

/// The view for one call, in the order the view lists it: the approval posture, the run budget, the organization's
/// switches as enforced, then the reserved keys at their platform default.
#[must_use]
pub fn effective_items(layers: &Layers) -> Vec<EffectiveItem> {
    let mut items = vec![
        EffectiveItem::platform(
            APPROVAL_POSTURE,
            json!(layers.posture.as_permission_mode()),
            layers.posture == ApprovalPosture::Ask,
        ),
        // No person sets the budget, so the platform's value is fixed for every person.
        EffectiveItem::platform(BUDGET_MAX_ROUNDS, json!(layers.budget.max_rounds), true),
        EffectiveItem::platform(
            BUDGET_MAX_SUBAGENTS,
            json!(layers.budget.max_subagents),
            true,
        ),
    ];
    items.extend(org_items(&layers.org));
    // A reserved key is enforced by no owner yet, so its platform default is what is in force. It is listed so the
    // view names every key the organization may set, not only the ones enforced today.
    items.extend(
        SETTINGS
            .iter()
            .filter(|def| def.status == Status::Reserved)
            .map(|def| EffectiveItem::platform(def.key, json!(def.default), false)),
    );
    items
}

fn org_items(org: &OrgLayer) -> Vec<EffectiveItem> {
    match org {
        // A policy that does not bind is the platform defaults, which is exactly the document with no keys.
        OrgLayer::NotBinding => held_items(&OrgPolicy::from_settings(0, "", &Map::new())),
        OrgLayer::Held(policy) => held_items(policy),
        OrgLayer::Unreadable => ORG_KEYS
            .into_iter()
            .map(EffectiveItem::unavailable)
            .collect(),
    }
}

/// The organization keys as the enforced policy states them. A key the organization did not change reads as the
/// platform default, so the origin names the organization only for a value it set.
fn held_items(policy: &OrgPolicy) -> Vec<EffectiveItem> {
    let revision = policy.revision;
    let caps = policy.web;
    let cap_changed = policy.mode_cap != AgentMode::Auto;
    let rung_changed = policy.rung_cap.is_some();
    let fan_out = policy.max_fan_out_items;
    vec![
        org_key(
            MODE_CAP,
            json!(policy.mode_cap.as_str()),
            cap_changed,
            cap_changed,
            revision,
        ),
        org_key(
            RUNG_CAP,
            json!(policy.rung_cap.map_or(RUNG_CAP_DEFAULT, rung_token)),
            rung_changed,
            rung_changed,
            revision,
        ),
        org_key(
            WEB_MODE,
            json!(if caps.live { "live" } else { "off" }),
            !caps.live,
            !caps.live,
            revision,
        ),
        org_key(
            WEB_PAID_PROVIDERS,
            json!(if caps.paid { "on" } else { "off" }),
            !caps.paid,
            !caps.paid,
            revision,
        ),
        // A fan-out cap the organization raised is not a restriction; only a lower one is locked.
        org_key(
            RESEARCH_MAX_FAN_OUT_ITEMS,
            json!(fan_out),
            fan_out != DEFAULT_MAX_FAN_OUT_ITEMS,
            fan_out < DEFAULT_MAX_FAN_OUT_ITEMS,
            revision,
        ),
    ]
}

/// One organization key: the platform default unless the organization changed it, and then the organization at the
/// revision it changed it at.
fn org_key(
    key: &'static str,
    value: Value,
    changed: bool,
    locked: bool,
    revision: i64,
) -> EffectiveItem {
    if !changed {
        return EffectiveItem::platform(key, value, false);
    }
    EffectiveItem {
        key,
        value: Some(value),
        origin: Some(ORIGIN_ORG),
        revision: Some(revision),
        locked,
        unavailable: false,
    }
}

/// The schema token of a rung cap. A cap is only ever read as read-only or workspace-write, so any other rung is the
/// narrowest one, as `rung_from_token` reads it.
fn rung_token(rung: AutonomyRung) -> &'static str {
    match rung {
        AutonomyRung::WorkspaceWrite => "workspace_write",
        _ => "read_only",
    }
}

/// The query of the view: the one thread the caller asks about.
#[derive(Debug, Deserialize)]
pub struct PolicyEffectiveQuery {
    thread_id: Option<String>,
}

/// `GET /v1/policy/effective?thread_id=`: the effective policy of one thread for the caller. The organization comes
/// from the verified token, never from the query, and the thread must be the caller's.
pub async fn get_policy_effective(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    session: VerifiedSessionBearer,
    capability: VerifiedCapabilityBearer,
    Query(query): Query<PolicyEffectiveQuery>,
) -> Response {
    if !policy_effective_enabled() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let thread_id = match query.thread_id.as_deref().map(str::trim) {
        Some(id) if !id.is_empty() && id.len() <= MAX_THREAD_ID_LEN => id.to_owned(),
        _ => {
            return typed(
                StatusCode::BAD_REQUEST,
                "thread_id_required",
                "A thread_id of 1 to 200 characters is required.",
            )
        }
    };
    // Session Core owns the thread. It is asked with the caller's own identity, so a thread that is not theirs
    // answers the same as one that does not exist.
    let request = match authenticated_session_request(
        ResolveThreadOwnerRequest {
            thread_id,
            org_id: claims.org_id.clone(),
            user_id: claims.user_id.clone(),
        },
        &session,
    ) {
        Ok(request) => request,
        Err(_) => return unavailable(),
    };
    let authorized = match state.run_client.clone().resolve_thread_owner(request).await {
        Ok(response) => response.into_inner().authorized,
        Err(status) => {
            tracing::warn!(
                code = ?status.code(),
                "Session Core thread ownership lookup unavailable for the policy view"
            );
            return unavailable();
        }
    };
    if !authorized {
        return typed(
            StatusCode::NOT_FOUND,
            "thread_not_found",
            "No thread with that id is visible to the caller.",
        );
    }
    let layers = Layers {
        posture: crate::sse::posture_floor(&claims),
        budget: run_budget_from_env(false),
        org: org_layer(&state, &claims, &capability).await,
    };
    Json(json!({ "data": { "items": effective_items(&layers) } })).into_response()
}

/// The organization layer for one call. A policy the switch does not enforce binds nothing. An enforced one is read
/// through the cache, and one that cannot be read is reported as unreadable.
async fn org_layer(
    state: &AppState,
    claims: &Claims,
    capability: &VerifiedCapabilityBearer,
) -> OrgLayer {
    match policy_mode() {
        PolicyMode::Off | PolicyMode::Shadow => OrgLayer::NotBinding,
        PolicyMode::Enforce => match ensure(
            shared_cache(),
            &state.http_client,
            &state.capability_core_base_url,
            &claims.org_id,
            capability.as_str(),
        )
        .await
        {
            Ok(policy) => OrgLayer::Held(policy),
            Err(error) => {
                tracing::warn!(%error, "org agent policy cannot be read for the policy view");
                OrgLayer::Unreadable
            }
        },
    }
}

/// The typed error envelope every refusal of this view uses: `{ error: { code, message, details } }`.
fn typed(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
        .into_response()
}

fn unavailable() -> Response {
    typed(
        StatusCode::SERVICE_UNAVAILABLE,
        "policy_unavailable",
        "The effective policy cannot be read right now; try again.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    fn held(revision: i64, body: Value) -> OrgLayer {
        OrgLayer::Held(Arc::new(OrgPolicy::from_settings(
            revision,
            "sha256:test",
            &settings(body),
        )))
    }

    fn layers(org: OrgLayer) -> Layers {
        Layers {
            posture: ApprovalPosture::Auto,
            budget: RunBudget::default(),
            org,
        }
    }

    fn item<'a>(items: &'a [EffectiveItem], key: &str) -> &'a EffectiveItem {
        items
            .iter()
            .find(|item| item.key == key)
            .unwrap_or_else(|| panic!("{key} is listed"))
    }

    #[test]
    fn the_switch_is_strict_and_off_by_default() {
        assert!(policy_effective_from(Some("true")));
        assert!(policy_effective_from(Some(" TRUE ")));
        for off in [
            None,
            Some(""),
            Some("false"),
            Some("1"),
            Some("on"),
            Some("yes"),
            Some("truee"),
        ] {
            assert!(!policy_effective_from(off), "{off:?} must stay off");
        }
    }

    #[test]
    fn the_posture_is_platform_and_locked_only_when_its_floor_is_ask() {
        let auto = effective_items(&layers(OrgLayer::NotBinding));
        assert_eq!(item(&auto, APPROVAL_POSTURE).value, Some(json!("auto")));
        assert_eq!(item(&auto, APPROVAL_POSTURE).origin, Some("platform"));
        assert!(!item(&auto, APPROVAL_POSTURE).locked);

        let ask = effective_items(&Layers {
            posture: ApprovalPosture::Ask,
            ..layers(OrgLayer::NotBinding)
        });
        assert_eq!(item(&ask, APPROVAL_POSTURE).value, Some(json!("ask")));
        assert!(item(&ask, APPROVAL_POSTURE).locked);
    }

    #[test]
    fn the_budget_is_platform_and_locked_because_no_person_sets_it() {
        let items = effective_items(&Layers {
            budget: RunBudget {
                max_rounds: 6,
                max_subagents: 2,
            },
            ..layers(OrgLayer::NotBinding)
        });
        assert_eq!(item(&items, BUDGET_MAX_ROUNDS).value, Some(json!(6)));
        assert_eq!(item(&items, BUDGET_MAX_ROUNDS).origin, Some("platform"));
        assert!(item(&items, BUDGET_MAX_ROUNDS).locked);
        assert_eq!(item(&items, BUDGET_MAX_SUBAGENTS).value, Some(json!(2)));
    }

    #[test]
    fn a_policy_that_does_not_bind_reads_the_platform_defaults() {
        let items = effective_items(&layers(OrgLayer::NotBinding));
        for (key, value) in [
            (MODE_CAP, json!("auto")),
            (RUNG_CAP, json!("danger_full_access")),
            (WEB_MODE, json!("live")),
            (WEB_PAID_PROVIDERS, json!("on")),
            (RESEARCH_MAX_FAN_OUT_ITEMS, json!(8)),
        ] {
            let found = item(&items, key);
            assert_eq!(found.value, Some(value), "{key}");
            assert_eq!(found.origin, Some("platform"), "{key}");
            assert_eq!(found.revision, None, "{key}");
            assert!(!found.locked, "{key}");
        }
    }

    #[test]
    fn an_enforced_policy_names_the_organization_and_its_revision_for_a_value_it_changed() {
        let items = effective_items(&layers(held(4, json!({"web.mode": "off"}))));
        let web = item(&items, WEB_MODE);
        assert_eq!(web.value, Some(json!("off")));
        assert_eq!(web.origin, Some("org"));
        assert_eq!(web.revision, Some(4));
        assert!(web.locked, "an organization switch that is off fixes web access");
        // A key the organization did not change keeps the platform's name and carries no revision.
        assert_eq!(item(&items, WEB_PAID_PROVIDERS).origin, Some("platform"));
        assert_eq!(item(&items, WEB_PAID_PROVIDERS).revision, None);
        assert_eq!(item(&items, MODE_CAP).origin, Some("platform"));
    }

    #[test]
    fn a_value_the_organization_sets_to_its_default_still_reads_as_platform() {
        let items = effective_items(&layers(held(2, json!({"web.mode": "live"}))));
        assert_eq!(item(&items, WEB_MODE).origin, Some("platform"));
        assert!(!item(&items, WEB_MODE).locked);
    }

    #[test]
    fn a_mode_cap_and_a_rung_cap_are_locked_and_read_as_their_tokens() {
        let items = effective_items(&layers(held(
            5,
            json!({"mode_cap": "read_only", "rung_cap": "workspace_write"}),
        )));
        assert_eq!(item(&items, MODE_CAP).value, Some(json!("read_only")));
        assert_eq!(item(&items, MODE_CAP).origin, Some("org"));
        assert!(item(&items, MODE_CAP).locked);
        assert_eq!(item(&items, RUNG_CAP).value, Some(json!("workspace_write")));
        assert!(item(&items, RUNG_CAP).locked);
    }

    #[test]
    fn a_fan_out_cap_is_locked_only_when_the_organization_lowers_it() {
        let lower = effective_items(&layers(held(3, json!({"research.max_fan_out_items": 3}))));
        let cap = item(&lower, RESEARCH_MAX_FAN_OUT_ITEMS);
        assert_eq!(cap.value, Some(json!(3)));
        assert_eq!(cap.origin, Some("org"));
        assert!(cap.locked);
    }

    #[test]
    fn an_unreadable_organization_is_unavailable_for_each_key_and_never_guessed() {
        let items = effective_items(&layers(OrgLayer::Unreadable));
        for key in ORG_KEYS {
            let found = item(&items, key);
            assert!(found.unavailable, "{key}");
            assert_eq!(found.value, None, "{key}");
            assert_eq!(found.origin, None, "{key}");
        }
        // The platform's own values are still answered when the organization cannot be read.
        assert!(!item(&items, APPROVAL_POSTURE).unavailable);
    }

    #[test]
    fn a_reserved_key_is_listed_at_its_platform_default_whatever_the_document_says() {
        let items = effective_items(&layers(held(1, json!({"ext.skills": "none"}))));
        let skills = item(&items, "ext.skills");
        assert_eq!(skills.value, Some(json!("any")));
        assert_eq!(skills.origin, Some("platform"));
        assert!(!skills.locked);
    }

    #[test]
    fn an_unavailable_item_serializes_with_a_null_value_and_no_revision() {
        let value = serde_json::to_value(EffectiveItem::unavailable(MODE_CAP))
            .expect("the item serializes");
        assert_eq!(
            value,
            json!({"key": "mode_cap", "value": null, "origin": null, "locked": false, "unavailable": true})
        );
    }

    #[test]
    fn a_platform_item_carries_no_revision_key() {
        let value = serde_json::to_value(EffectiveItem::platform(
            APPROVAL_POSTURE,
            json!("auto"),
            false,
        ))
        .expect("the item serializes");
        assert_eq!(
            value,
            json!({"key": "approval.posture", "value": "auto", "origin": "platform", "locked": false})
        );
    }
}
