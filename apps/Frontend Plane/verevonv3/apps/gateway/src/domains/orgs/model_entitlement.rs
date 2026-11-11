//! WS10-S5: the org model-entitlement admin routes (ADR-0005).
//!
//! org-core stores, validates, resolves and explains the policy; inference-core
//! enforces it. This module is only the Frontend Plane's door to org-core: an
//! owner or admin reads and replaces the policy set, and any member can ask why
//! they see the models they see. It decides nothing about who may use a model.
//!
//! Two admin gates guard a write, on purpose: this gateway's session role
//! (`require_org_admin`) and org-core's own membership table (`CallerRole`),
//! because the role this gateway forwards is a hint and never authority. When
//! they disagree the stricter one wins and org-core's refusal is passed through.
//!
//! Everything here is behind the admin switch on `AppState`. Off, GET answers
//! `{"enabled": false}` so the SPA hides the section, and every write and every
//! explain is a 404: an operator cannot set a policy that nothing enforces yet.

use std::collections::{BTreeSet, HashMap};

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::{
    config::AppState, domains::chat::shared as model_plane, envelope::error,
    middleware::AuthenticatedUser, upstream::proxy_json,
};

use super::settings::{
    mint_model_plane_credentials, model_attests_zdr_for_chat, read_model_catalogue,
};
use super::shared::{actor_for, require_active_org, require_org_admin, GatewayJsonResponse};

/// Limits a policy set must stay inside. They mirror org-core's
/// `modelentitlement` limits so an oversized body is refused here, before the
/// delegation is signed; org-core stays the owner of every semantic rule
/// (normalisation, known efforts and privacy tiers, defaults against the policy
/// they belong to) and answers its own `invalid_model_policy`.
const MAX_ENTRIES: usize = 500;
const MAX_ENTRY_LENGTH: usize = 128;
const MAX_ROLES: usize = 20;
const MAX_SURFACES: usize = 16;
const MAX_BODY_BYTES: usize = 64 << 10;

/// The only fields a PUT body, a policy and a surface default may carry. An
/// unknown field is refused rather than forwarded: a typo such as `denied` would
/// otherwise be dropped by org-core and read by the operator as "denied".
const PUT_FIELDS: [&str; 3] = ["expected_revision", "org_default", "roles"];
const POLICY_FIELDS: [&str; 6] = [
    "allow_mode",
    "allow",
    "deny",
    "max_effort",
    "min_privacy_tier",
    "defaults",
];
const SURFACE_DEFAULT_FIELDS: [&str; 2] = ["model", "effort"];

fn not_enabled() -> GatewayJsonResponse {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "not_found",
            "Model entitlement administration is not enabled.",
        )),
    )
}

fn org_core_url(state: &AppState, org_id: &str, suffix: &str) -> String {
    format!(
        "{}/api/v1/organizations/{}/model-entitlement{suffix}",
        state.org_core_url,
        urlencoding::encode(org_id)
    )
}

/// GET /api/v1/orgs/:id/model-entitlement: the stored policy set, for the admin
/// section. Any member of the org learns whether the feature is on; only an
/// owner or admin reads the policy itself.
pub(crate) async fn get_model_entitlement(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_active_org(&user, &id) {
        return response;
    }
    if !state.model_entitlement_admin_enabled {
        return (StatusCode::OK, Json(json!({ "enabled": false })));
    }
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    let (status, Json(body)) = proxy_json(
        &state,
        Method::GET,
        &org_core_url(&state, &id, ""),
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await;
    match body {
        Value::Object(mut policy_set) if status.is_success() => {
            policy_set.insert("enabled".to_owned(), Value::Bool(true));
            (status, Json(Value::Object(policy_set)))
        }
        other => (status, Json(other)),
    }
}

/// PUT /api/v1/orgs/:id/model-entitlement: replace the whole policy set.
///
/// The body carries the revision it was edited from; org-core answers 409
/// `revision_conflict` to a stale editor and this route passes that through.
/// When the org's Zero Data Retention is on, a policy that would leave a role
/// with no attested model is refused here first (D10), because a ZDR request is
/// refused by inference-core when nothing attested is allowed.
pub(crate) async fn put_model_entitlement(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if !state.model_entitlement_admin_enabled {
        return not_enabled();
    }
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    if let Err(message) = validate_put_body(&body) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("validation_error", message)),
        );
    }
    if body.to_string().len() > MAX_BODY_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(error(
                "body_too_large",
                format!(
                    "The model policy is larger than {} KB.",
                    MAX_BODY_BYTES >> 10
                ),
            )),
        );
    }
    if let Some(refusal) = zdr_write_refusal(&state, &user, &headers, &id, &body).await {
        return refusal;
    }
    proxy_json(
        &state,
        Method::PUT,
        &org_core_url(&state, &id, ""),
        Some(body),
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// GET /api/v1/orgs/:id/model-entitlement/explain[?user_id=]: why the caller
/// sees the models they see. Any member may explain themselves; explaining
/// someone else discloses that person's role and policy, so it is an owner or
/// admin act (org-core checks it again against its membership table).
pub(crate) async fn explain_model_entitlement(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> GatewayJsonResponse {
    if !state.model_entitlement_admin_enabled {
        return not_enabled();
    }
    if let Err(response) = require_active_org(&user, &id) {
        return response;
    }
    let target = query.get("user_id");
    if target.is_some_and(|target| target.trim() != user.user_id.trim()) {
        if let Err(response) = require_org_admin(&state, &user, &id).await {
            return response;
        }
    }
    let suffix = match target {
        Some(target) => format!("/explain?user_id={}", urlencoding::encode(target)),
        None => "/explain".to_owned(),
    };
    proxy_json(
        &state,
        Method::GET,
        &org_core_url(&state, &id, &suffix),
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// Structural check of a PUT body: shape, unknown fields and size limits. The
/// first problem is reported; org-core reports every semantic one at once.
fn validate_put_body(body: &Value) -> Result<(), String> {
    let Some(fields) = body.as_object() else {
        return Err("The request body must be a JSON object.".to_owned());
    };
    reject_unknown_fields("the request body", fields, &PUT_FIELDS)?;
    if !fields
        .get("expected_revision")
        .and_then(Value::as_i64)
        .is_some_and(|revision| revision >= 0)
    {
        return Err(
            "expected_revision is required and must be an integer, zero or greater (0 for the first write)."
                .to_owned(),
        );
    }
    match fields.get("org_default") {
        None | Some(Value::Null) => {}
        Some(policy) => validate_policy("org_default", policy)?,
    }
    match fields.get("roles") {
        None | Some(Value::Null) => {}
        Some(Value::Object(roles)) => {
            if roles.len() > MAX_ROLES {
                return Err(format!("At most {MAX_ROLES} role overrides are allowed."));
            }
            for (name, policy) in roles {
                validate_policy(&format!("roles.{name}"), policy)?;
            }
        }
        Some(_) => return Err("roles must be an object keyed by role name.".to_owned()),
    }
    Ok(())
}

fn reject_unknown_fields(
    path: &str,
    fields: &Map<String, Value>,
    allowed: &[&str],
) -> Result<(), String> {
    match fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(unknown) => Err(format!("Unknown field '{unknown}' in {path}.")),
        None => Ok(()),
    }
}

fn validate_policy(path: &str, policy: &Value) -> Result<(), String> {
    let Some(fields) = policy.as_object() else {
        return Err(format!("{path} must be an object."));
    };
    reject_unknown_fields(path, fields, &POLICY_FIELDS)?;
    let allow_mode = fields
        .get("allow_mode")
        .and_then(Value::as_str)
        .map(|mode| mode.trim().to_ascii_lowercase());
    if !matches!(allow_mode.as_deref(), Some("all" | "listed")) {
        return Err(format!("{path}.allow_mode must be \"all\" or \"listed\"."));
    }
    for list in ["allow", "deny"] {
        match fields.get(list) {
            None | Some(Value::Null) => {}
            Some(Value::Array(entries)) => {
                if entries.len() > MAX_ENTRIES {
                    return Err(format!(
                        "{path}.{list} has more than {MAX_ENTRIES} entries."
                    ));
                }
                for entry in entries {
                    let Some(entry) = entry.as_str() else {
                        return Err(format!("{path}.{list} entries must be strings."));
                    };
                    if entry.trim().len() > MAX_ENTRY_LENGTH {
                        return Err(format!(
                            "{path}.{list} has an entry longer than {MAX_ENTRY_LENGTH} characters."
                        ));
                    }
                }
            }
            Some(_) => return Err(format!("{path}.{list} must be an array of strings.")),
        }
    }
    for text in ["max_effort", "min_privacy_tier"] {
        if !matches!(
            fields.get(text),
            None | Some(Value::Null | Value::String(_))
        ) {
            return Err(format!("{path}.{text} must be a string."));
        }
    }
    match fields.get("defaults") {
        None | Some(Value::Null) => {}
        Some(Value::Object(surfaces)) => {
            if surfaces.len() > MAX_SURFACES {
                return Err(format!(
                    "{path}.defaults has more than {MAX_SURFACES} surfaces."
                ));
            }
            for (surface, default) in surfaces {
                let surface_path = format!("{path}.defaults.{surface}");
                let Some(default) = default.as_object() else {
                    return Err(format!("{surface_path} must be an object."));
                };
                reject_unknown_fields(&surface_path, default, &SURFACE_DEFAULT_FIELDS)?;
                if default
                    .values()
                    .any(|value| !(value.is_string() || value.is_null()))
                {
                    return Err(format!("{surface_path} fields must be strings."));
                }
            }
        }
        Some(_) => {
            return Err(format!(
                "{path}.defaults must be an object keyed by surface."
            ))
        }
    }
    Ok(())
}

/// A policy layer that could take a model away: a listed allow mode, a deny
/// entry or a minimum privacy tier. A layer that is `all` with no deny and no
/// tier cannot strip an attested model, so the write guard never asks about it.
fn layer_restricts(policy: &Map<String, Value>) -> bool {
    let non_empty = |field: &str| match policy.get(field) {
        Some(Value::Array(entries)) => !entries.is_empty(),
        Some(Value::String(text)) => !text.trim().is_empty(),
        _ => false,
    };
    let listed = policy
        .get("allow_mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode.trim().eq_ignore_ascii_case("listed"));
    listed || non_empty("deny") || non_empty("min_privacy_tier")
}

/// The policy in the shape model-gateway's `GetModelAccess` takes as `draft`
/// (`ModelPolicyDoc`): the same fields org-core stores, with the entries and
/// names folded the way org-core folds them so the matcher sees what would be
/// stored.
fn draft_doc(policy: &Map<String, Value>) -> Value {
    let folded = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .map(|text| text.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };
    let entries = |field: &str| -> Vec<String> {
        policy
            .get(field)
            .and_then(Value::as_array)
            .map(|list| list.iter().map(|entry| folded(Some(entry))).collect())
            .unwrap_or_default()
    };
    let mut doc = json!({
        "allow_mode": folded(policy.get("allow_mode")),
        "allow": entries("allow"),
        "deny": entries("deny"),
        "max_effort": folded(policy.get("max_effort")),
        "min_privacy_tier": folded(policy.get("min_privacy_tier")),
    });
    if let Some(defaults) = policy.get("defaults").filter(|value| value.is_object()) {
        doc["defaults"] = defaults.clone();
    }
    doc
}

/// The restricting layers of a PUT body, each with the name the refusal uses.
fn restricting_layers(body: &Value) -> Vec<(String, Value)> {
    let mut layers = Vec::new();
    if let Some(policy) = body.get("org_default").and_then(Value::as_object) {
        if layer_restricts(policy) {
            layers.push(("the organization default".to_owned(), draft_doc(policy)));
        }
    }
    if let Some(roles) = body.get("roles").and_then(Value::as_object) {
        for (name, policy) in roles {
            if let Some(policy) = policy.as_object().filter(|policy| layer_restricts(policy)) {
                layers.push((format!("the {name} role"), draft_doc(policy)));
            }
        }
    }
    layers
}

fn guard_unavailable() -> GatewayJsonResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "model_policy_guard_unavailable",
            "The model policy cannot be saved right now: the Zero Data Retention check could not be completed. Try again shortly.",
        )),
    )
}

/// Whether org-core says the org has interactive ZDR on. `None` when org-core
/// cannot answer: the guard then fails closed, never reads it as "off".
async fn org_zdr_enabled(state: &AppState, user: &AuthenticatedUser, org_id: &str) -> Option<bool> {
    // The shared reader, read live: the guard decides whether a policy may be saved.
    match crate::zdr::org_posture_live(state, user, org_id).await {
        crate::zdr::OrgPosture::Zdr => Some(true),
        crate::zdr::OrgPosture::NotZdr => Some(false),
        crate::zdr::OrgPosture::Unknown => None,
    }
}

/// The ids of the chat models in a catalogue that carry a zero-retention
/// attestation. `None` when the body is not a catalogue.
fn attested_chat_model_ids(catalogue: &Value) -> Option<BTreeSet<String>> {
    let models = catalogue.get("models")?.as_array()?;
    Some(
        models
            .iter()
            .filter(|model| model_attests_zdr_for_chat(model))
            .filter_map(|model| model.get("id").and_then(Value::as_str))
            .map(str::to_owned)
            .collect(),
    )
}

/// Whether the draft leaves at least one of `attested` entitled, from the
/// per-model verdicts `GetModelAccess` returns for `include_catalogue`. `None`
/// when the answer has no verdict list: nothing is known, which is not "none".
fn draft_keeps_an_attested_model(access: &Value, attested: &BTreeSet<String>) -> Option<bool> {
    let verdicts = access.get("catalogue")?.as_array()?;
    Some(verdicts.iter().any(|entry| {
        entry.get("entitled").and_then(Value::as_bool) == Some(true)
            && entry
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| attested.contains(id))
    }))
}

/// The ids of the chat models a verdict list marks as attested, read from each
/// verdict's `zdr_attested`. `None` unless every verdict carries the field, which
/// an answer from a model-gateway that predates it does not, so the guard falls
/// back to the catalogue. The flag is the deployment's, so it does not depend on
/// which policy the verdict was judged under.
fn attested_from_verdicts(access: &Value) -> Option<BTreeSet<String>> {
    let verdicts = access.get("catalogue")?.as_array()?;
    let mut attested = BTreeSet::new();
    for entry in verdicts {
        let flagged = entry.get("zdr_attested")?.as_bool()?;
        if let Some(id) = entry.get("id").and_then(Value::as_str).filter(|_| flagged) {
            attested.insert(id.to_owned());
        }
    }
    Some(attested)
}

/// D10: the refusal to answer with when this PUT would leave an org whose ZDR
/// is on with no attested model, or `None` when the write may go on.
///
/// Each restricting layer is judged on its own (a role override replaces the
/// org default, it never merges), by asking model-gateway what the draft would
/// leave the acting admin. Attestation and entitlement both come from the
/// verdicts, which model-gateway answers from the whole catalogue, so a model the
/// admin cannot see still counts. Only an answer that predates `zdr_attested`
/// falls back to the admin's own catalogue, read once. Fails closed: a verdict,
/// the catalogue fallback or the org that cannot be read is a 503, never a pass.
/// A body that restricts nothing needs no answer and reads nothing.
async fn zdr_write_refusal(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
    body: &Value,
) -> Option<GatewayJsonResponse> {
    let layers = restricting_layers(body);
    if layers.is_empty() {
        return None;
    }
    match org_zdr_enabled(state, user, org_id).await {
        Some(true) => {}
        Some(false) => return None,
        None => return Some(guard_unavailable()),
    }
    let Some((model_token, inference_token)) =
        mint_model_plane_credentials(state, user, headers).await
    else {
        return Some(guard_unavailable());
    };
    let url = format!("{}/v1/models/access", state.model_gateway_url);
    let mut from_catalogue: Option<BTreeSet<String>> = None;
    for (name, draft) in layers {
        let (status, Json(access)) = model_plane::proxy_model_json_with_inference(
            state,
            Method::POST,
            &url,
            Some(json!({ "draft": draft, "include_catalogue": true })),
            Some(&model_token),
            Some(&inference_token),
            user,
        )
        .await;
        let attested = if status.is_success() {
            match attested_from_verdicts(&access) {
                Some(attested) => Some(attested),
                // An answer that predates `zdr_attested`: the catalogue is read only
                // then, and only once for the whole write.
                None => {
                    if from_catalogue.is_none() {
                        from_catalogue =
                            read_model_catalogue(state, user, &model_token, &inference_token)
                                .await
                                .as_ref()
                                .and_then(attested_chat_model_ids);
                    }
                    from_catalogue.clone()
                }
            }
        } else {
            None
        };
        let verdict =
            attested.and_then(|attested| draft_keeps_an_attested_model(&access, &attested));
        match verdict {
            Some(true) => {}
            Some(false) => {
                tracing::info!(
                    org_id,
                    "model policy refused: no attested model would remain for a ZDR organization"
                );
                return Some((
                    StatusCode::CONFLICT,
                    Json(error(
                        "model_policy_leaves_no_zdr_model",
                        format!(
                            "This model policy would leave {name} with no model that has a zero-retention attestation. Zero Data Retention is on for this organization, so every interactive request from them would be refused. Allow at least one attested model."
                        ),
                    )),
                ));
            }
            None => return Some(guard_unavailable()),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    //! The routes are driven through the REAL router: the real session
    //! middleware decides the caller's org role, org-core answers only a request
    //! that carries the gateway's delegated-service identity for this org and the
    //! exact body, and model-gateway answers only a request that carries both
    //! delegated bearers. An upstream that must receive nothing is asserted to.
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{
        body_json as wm_body_json, body_partial_json as wm_partial, header as wm_header,
        method as wm_method, path as wm_path, query_param as wm_query,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::config::AppState;

    use super::{
        attested_chat_model_ids, attested_from_verdicts, draft_keeps_an_attested_model,
        restricting_layers, validate_put_body,
    };

    const COOKIE: &str = "better-auth.session_token=session-1";
    const MODEL_TOKEN: &str = "model-gateway-token-for-user-1";
    const INFERENCE_TOKEN: &str = "inference-core-token-for-user-1";
    const ROUTE: &str = "/api/v1/orgs/org-1/model-entitlement";
    const ORG_CORE_PATH: &str = "/api/v1/organizations/org-1/model-entitlement";

    struct Upstreams {
        auth: MockServer,
        user_core: MockServer,
        model_gateway: MockServer,
        org_core: MockServer,
    }

    impl Upstreams {
        /// A signed-in user of org-1 whose session role is `role`.
        async fn start(role: &str) -> Self {
            let auth = MockServer::start().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/auth/get-session"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                    "session": {"activeOrganizationId": "org-1"}
                })))
                .mount(&auth)
                .await;
            for (path, token) in [
                ("/api/model-plane/token", MODEL_TOKEN),
                ("/api/inference-core/token", INFERENCE_TOKEN),
            ] {
                Mock::given(wm_method("GET"))
                    .and(wm_path(path))
                    .and(wm_header("cookie", COOKIE))
                    .and(wm_header("x-internal-api-key", "test-key"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                        "token": token, "expiresInSeconds": 300
                    })))
                    .mount(&auth)
                    .await;
            }
            let user_core = MockServer::start().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/v1/me/session-context"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "userId": "user-1", "orgId": "org-1", "role": role,
                    "onboardingStatus": "COMPLETED"
                })))
                .mount(&user_core)
                .await;
            Self {
                auth,
                user_core,
                model_gateway: MockServer::start().await,
                org_core: MockServer::start().await,
            }
        }

        fn state(&self, enabled: bool) -> AppState {
            let mut state = crate::tests::test_state(false);
            state.auth_core_url = self.auth.uri();
            state.user_core_url = self.user_core.uri();
            state.org_core_url = self.org_core.uri();
            state.model_gateway_url = self.model_gateway.uri();
            state.model_entitlement_admin_enabled = enabled;
            state
        }

        /// org-core's policy route for `method`, answering only a request that carries the
        /// gateway's delegated identity for THIS org and user (and the exact body, when given).
        async fn org_core_policy(
            &self,
            method: &str,
            body: Option<Value>,
            response: ResponseTemplate,
        ) {
            let mut mock = Mock::given(wm_method(method))
                .and(wm_path(ORG_CORE_PATH))
                .and(wm_header("x-service-id", "verevon-gateway"))
                .and(wm_header("x-delegation-version", "v3"))
                .and(wm_header("x-org-id", "org-1"))
                .and(wm_header("x-user-id", "user-1"));
            if let Some(body) = body {
                mock = mock.and(wm_body_json(body));
            }
            mock.respond_with(response).mount(&self.org_core).await;
        }

        async fn org_core_organization(&self, response: ResponseTemplate) {
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/v1/organizations/org-1"))
                .respond_with(response)
                .mount(&self.org_core)
                .await;
        }

        async fn catalogue(&self, response: ResponseTemplate) {
            Mock::given(wm_method("GET"))
                .and(wm_path("/v1/models"))
                .and(wm_header(
                    "authorization",
                    format!("Bearer {MODEL_TOKEN}").as_str(),
                ))
                .and(wm_header(
                    "x-inference-authorization",
                    format!("Bearer {INFERENCE_TOKEN}").as_str(),
                ))
                .respond_with(response)
                .mount(&self.model_gateway)
                .await;
        }

        /// model-gateway's `POST /v1/models/access` for a draft whose allow list is `allow`.
        async fn access_for_allow(&self, allow: Value, response: ResponseTemplate) {
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/models/access"))
                .and(wm_header(
                    "authorization",
                    format!("Bearer {MODEL_TOKEN}").as_str(),
                ))
                .and(wm_partial(
                    json!({"draft": {"allow": allow}, "include_catalogue": true}),
                ))
                .respond_with(response)
                .mount(&self.model_gateway)
                .await;
        }

        async fn org_core_requests(&self) -> usize {
            self.org_core
                .received_requests()
                .await
                .expect("recording")
                .len()
        }

        async fn model_gateway_requests(&self) -> usize {
            self.model_gateway
                .received_requests()
                .await
                .expect("recording")
                .len()
        }
    }

    async fn send(state: AppState, method: &str, uri: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", COOKIE);
        let body = match body {
            Some(body) => {
                request = request.header("content-type", "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = crate::build_router(state)
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let parsed: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, parsed)
    }

    fn stored_set(revision: i64) -> Value {
        json!({
            "organization_id": "org-1",
            "revision": revision,
            "org_default": null,
            "roles": {"member": {"allow_mode": "listed", "allow": ["gpt-open"], "deny": []}},
            "updated_at": null,
            "updated_by": "user-1"
        })
    }

    fn ok_set(revision: i64) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(stored_set(revision))
    }

    fn member_only_put() -> Value {
        json!({
            "expected_revision": 3,
            "roles": {"member": {"allow_mode": "listed", "allow": ["gpt-open"]}}
        })
    }

    fn zdr_organization(on: bool) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(
            json!({"id": "org-1", "metadata": {"interactiveRetention": {"zdr": on}}}),
        )
    }

    fn two_model_catalogue() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"models": [
            {"id": "gpt-attested", "modality": "chat", "features": ["zdr"]},
            {"id": "gpt-open", "modality": "chat", "features": []},
            {"id": "embed-attested", "modality": "embedding", "features": ["zdr"]}
        ]}))
    }

    fn verdicts(entries: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"state": "role", "catalogue": entries}))
    }

    #[tokio::test]
    async fn with_the_switch_off_get_says_so_and_every_other_route_is_a_404_and_nothing_is_called()
    {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core_policy("GET", None, ok_set(3)).await;

        let (status, body) = send(up.state(false), "GET", ROUTE, None).await;
        assert_eq!((status, body), (200, json!({"enabled": false})));

        let (status, body) = send(up.state(false), "PUT", ROUTE, Some(member_only_put())).await;
        assert_eq!(status, 404, "{body}");
        let (status, _) = send(up.state(false), "GET", &format!("{ROUTE}/explain"), None).await;
        assert_eq!(status, 404);

        assert_eq!(up.org_core_requests().await, 0);
        assert_eq!(up.model_gateway_requests().await, 0);
    }

    #[tokio::test]
    async fn a_member_of_another_org_is_refused_even_with_the_switch_off() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        let (status, _) = send(
            up.state(false),
            "GET",
            "/api/v1/orgs/org-2/model-entitlement",
            None,
        )
        .await;
        assert_eq!(status, 403);
    }

    #[tokio::test]
    async fn a_member_may_not_read_or_write_the_policy_and_org_core_is_never_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("member").await;
        up.org_core_policy("GET", None, ok_set(3)).await;
        up.org_core_policy("PUT", None, ok_set(4)).await;

        let (status, body) = send(up.state(true), "PUT", ROUTE, Some(member_only_put())).await;
        assert_eq!(status, 403, "{body}");
        let (status, _) = send(up.state(true), "GET", ROUTE, None).await;
        assert_eq!(status, 403);

        assert_eq!(up.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn an_owner_put_round_trips_through_org_core_and_shows_in_get() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(false)).await;
        up.org_core_policy("PUT", Some(member_only_put()), ok_set(4))
            .await;
        up.org_core_policy("GET", None, ok_set(4)).await;

        let (status, saved) = send(up.state(true), "PUT", ROUTE, Some(member_only_put())).await;
        assert_eq!(status, 200, "{saved}");
        assert_eq!(saved["revision"], 4);

        let (status, read) = send(up.state(true), "GET", ROUTE, None).await;
        assert_eq!(status, 200, "{read}");
        assert_eq!(read["enabled"], true);
        assert_eq!(read["revision"], 4);
        assert_eq!(read["roles"]["member"]["allow"], json!(["gpt-open"]));
        assert_eq!(up.model_gateway_requests().await, 0);
    }

    #[tokio::test]
    async fn the_two_admin_gates_disagree_and_org_cores_refusal_wins() {
        // The session says admin; org-core's membership table says the caller is not one (a role
        // projection lag). The gateway's role is a hint, so org-core's 403 reaches the caller.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("admin").await;
        up.org_core_organization(zdr_organization(false)).await;
        up.org_core_policy(
            "PUT",
            None,
            ResponseTemplate::new(403).set_body_json(json!({
                "error": "organization owner or admin role required"
            })),
        )
        .await;

        let (status, body) = send(up.state(true), "PUT", ROUTE, Some(member_only_put())).await;
        assert_eq!(status, 403, "{body}");
        assert_eq!(
            up.org_core_requests().await,
            2,
            "the organization read, then the PUT the gateway let through and org-core refused"
        );
    }

    #[tokio::test]
    async fn a_stale_revision_is_passed_through_as_the_409_org_core_answered() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(false)).await;
        up.org_core_policy(
            "PUT",
            None,
            ResponseTemplate::new(409).set_body_json(json!({
                "error": "the model policy changed since it was read",
                "code": "revision_conflict",
                "current_revision": 7
            })),
        )
        .await;

        let (status, body) = send(up.state(true), "PUT", ROUTE, Some(member_only_put())).await;
        assert_eq!(status, 409);
        assert_eq!(body["code"], "revision_conflict");
        assert_eq!(body["current_revision"], 7);
    }

    #[tokio::test]
    async fn an_invalid_body_is_refused_before_org_core_sees_it() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core_policy("PUT", None, ok_set(4)).await;

        let long_entry = "x".repeat(129);
        let too_many_roles: serde_json::Map<String, Value> = (0..21)
            .map(|index| (format!("role{index}"), json!({"allow_mode": "all"})))
            .collect();
        let cases = [
            json!({"roles": {}}),
            json!({"expected_revision": -1}),
            json!({"expected_revision": "3"}),
            json!({"expected_revision": 0, "denied": []}),
            json!({"expected_revision": 0, "org_default": {"allow_mode": "all", "denied": ["x"]}}),
            json!({"expected_revision": 0, "org_default": {"allow_mode": "some"}}),
            json!({"expected_revision": 0, "org_default": {"allow": ["a"]}}),
            json!({"expected_revision": 0, "org_default": {"allow_mode": "listed", "allow": [1]}}),
            json!({"expected_revision": 0, "org_default": {"allow_mode": "listed", "allow": [long_entry]}}),
            json!({"expected_revision": 0, "roles": too_many_roles}),
            json!({"expected_revision": 0, "roles": ["member"]}),
            json!({"expected_revision": 0, "org_default": {"allow_mode": "all",
                "defaults": {"chat": {"model": "m", "temperature": "hot"}}}}),
            json!({"expected_revision": 0, "org_default": {"allow_mode": "all", "max_effort": 5}}),
        ];
        for case in cases {
            let (status, body) = send(up.state(true), "PUT", ROUTE, Some(case.clone())).await;
            assert_eq!(status, 400, "{case} -> {body}");
            assert_eq!(body["error"]["code"], "validation_error", "{case}");
        }
        assert_eq!(up.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn an_oversized_body_is_a_413_and_the_per_field_limits_are_inclusive() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core_policy("PUT", None, ok_set(4)).await;

        // 500 entries of 128 characters is over 64 KB though each limit is met exactly.
        let entries: Vec<String> = (0..500).map(|index| format!("{index:0>128}")).collect();
        let big = json!({"expected_revision": 0, "org_default": {"allow_mode": "listed", "allow": entries}});
        assert!(validate_put_body(&big).is_ok());
        let (status, body) = send(up.state(true), "PUT", ROUTE, Some(big)).await;
        assert_eq!(status, 413, "{body}");
        assert_eq!(up.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn explain_is_open_to_a_member_for_themselves_and_to_an_admin_for_others() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let explain = json!({"organization_id": "org-1", "user_id": "user-1", "state": "none"});
        let uri = format!("{ROUTE}/explain");

        let member = Upstreams::start("member").await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("{ORG_CORE_PATH}/explain")))
            .and(wm_header("x-user-id", "user-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(explain.clone()))
            .mount(&member.org_core)
            .await;
        let (status, body) = send(member.state(true), "GET", &uri, None).await;
        assert_eq!((status, &body["state"]), (200, &json!("none")), "{body}");
        let (status, _) = send(
            member.state(true),
            "GET",
            &format!("{uri}?user_id=user-1"),
            None,
        )
        .await;
        assert_eq!(status, 200, "naming oneself is still a self explain");
        let before = member.org_core_requests().await;
        let (status, _) = send(
            member.state(true),
            "GET",
            &format!("{uri}?user_id=user-2"),
            None,
        )
        .await;
        assert_eq!(status, 403, "a member cannot explain someone else");
        assert_eq!(
            member.org_core_requests().await,
            before,
            "refused before org-core"
        );

        let admin = Upstreams::start("admin").await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("{ORG_CORE_PATH}/explain")))
            .and(wm_query("user_id", "user-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(explain))
            .mount(&admin.org_core)
            .await;
        let (status, body) = send(
            admin.state(true),
            "GET",
            &format!("{uri}?user_id=user-2"),
            None,
        )
        .await;
        assert_eq!(status, 200, "{body}");
    }

    #[tokio::test]
    async fn a_zdr_org_cannot_save_a_policy_that_leaves_no_attested_model() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(true)).await;
        up.catalogue(two_model_catalogue()).await;
        // The draft allows only the unattested chat model: the attested embedding model does
        // not count, and neither does an attested chat model the draft refuses.
        up.access_for_allow(
            json!(["gpt-open"]),
            verdicts(json!([
                {"id": "gpt-attested", "provider": "azure", "entitled": false, "reason": "not_in_allow_list"},
                {"id": "gpt-open", "provider": "openai", "entitled": true}
            ])),
        )
        .await;
        up.org_core_policy("PUT", None, ok_set(4)).await;

        let (status, body) = send(up.state(true), "PUT", ROUTE, Some(member_only_put())).await;

        assert_eq!(status, 409, "{body}");
        assert_eq!(body["error"]["code"], "model_policy_leaves_no_zdr_model");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("the member role"),
            "{body}"
        );
        assert_eq!(
            up.org_core_requests().await,
            1,
            "only the organization read, never the PUT"
        );
    }

    #[tokio::test]
    async fn a_zdr_org_may_save_a_policy_that_keeps_an_attested_model() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(true)).await;
        up.catalogue(two_model_catalogue()).await;
        up.access_for_allow(
            json!(["gpt-open"]),
            verdicts(json!([
                {"id": "gpt-attested", "provider": "azure", "entitled": true},
                {"id": "gpt-open", "provider": "openai", "entitled": true}
            ])),
        )
        .await;
        up.org_core_policy("PUT", Some(member_only_put()), ok_set(4))
            .await;

        let (status, body) = send(up.state(true), "PUT", ROUTE, Some(member_only_put())).await;
        assert_eq!(status, 200, "{body}");
    }

    #[tokio::test]
    async fn the_attestation_comes_from_the_verdicts_when_they_carry_it() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(true)).await;
        // The admin's own catalogue, filtered by the stored policy, shows no attested model.
        up.catalogue(ResponseTemplate::new(200).set_body_json(json!({"models": [
            {"id": "gpt-open", "modality": "chat", "features": []}
        ]})))
        .await;
        // The verdicts answer from the whole catalogue: the attested model is entitled.
        up.access_for_allow(
            json!(["gpt-attested"]),
            verdicts(json!([
                {"id": "gpt-attested", "provider": "azure", "entitled": true, "zdr_attested": true},
                {"id": "gpt-open", "provider": "openai", "entitled": false,
                 "reason": "not_in_allow_list", "zdr_attested": false}
            ])),
        )
        .await;
        up.org_core_policy("PUT", None, ok_set(4)).await;

        let policy = json!({
            "expected_revision": 3,
            "roles": {"member": {"allow_mode": "listed", "allow": ["gpt-attested"]}}
        });
        let (status, body) = send(up.state(true), "PUT", ROUTE, Some(policy)).await;
        assert_eq!(status, 200, "{body}");
        // Only the question was asked: the admin's own catalogue was never read.
        assert_eq!(up.model_gateway_requests().await, 1);
    }

    #[tokio::test]
    async fn a_verdict_that_does_not_attest_refuses_whatever_the_admins_catalogue_shows() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(true)).await;
        // The admin's catalogue lists the model as attested, but the verdict, which
        // is the deployment's own answer, does not.
        up.catalogue(two_model_catalogue()).await;
        up.access_for_allow(
            json!(["gpt-attested"]),
            verdicts(json!([
                {"id": "gpt-attested", "provider": "azure", "entitled": true, "zdr_attested": false}
            ])),
        )
        .await;
        up.org_core_policy("PUT", None, ok_set(4)).await;

        let policy = json!({
            "expected_revision": 3,
            "roles": {"member": {"allow_mode": "listed", "allow": ["gpt-attested"]}}
        });
        let (status, body) = send(up.state(true), "PUT", ROUTE, Some(policy)).await;
        assert_eq!(status, 409, "{body}");
        assert_eq!(body["error"]["code"], "model_policy_leaves_no_zdr_model");
    }

    #[tokio::test]
    async fn every_restricting_layer_is_judged_and_one_bad_role_refuses_the_write() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(true)).await;
        up.catalogue(two_model_catalogue()).await;
        up.access_for_allow(
            json!(["gpt-attested"]),
            verdicts(json!([{"id": "gpt-attested", "provider": "azure", "entitled": true}])),
        )
        .await;
        up.access_for_allow(
            json!(["gpt-open"]),
            verdicts(json!([{"id": "gpt-open", "provider": "openai", "entitled": true}])),
        )
        .await;
        up.org_core_policy("PUT", None, ok_set(4)).await;

        let body = json!({
            "expected_revision": 3,
            "org_default": {"allow_mode": "listed", "allow": ["GPT-Attested "]},
            "roles": {"intern": {"allow_mode": "listed", "allow": ["gpt-open"]}}
        });
        let (status, refusal) = send(up.state(true), "PUT", ROUTE, Some(body)).await;

        assert_eq!(status, 409, "{refusal}");
        assert!(
            refusal["error"]["message"]
                .as_str()
                .unwrap()
                .contains("the intern role"),
            "{refusal}"
        );
    }

    #[tokio::test]
    async fn the_zdr_guard_fails_closed_when_it_cannot_decide() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;

        // The access verdict cannot be read.
        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(true)).await;
        up.catalogue(two_model_catalogue()).await;
        up.access_for_allow(json!(["gpt-open"]), ResponseTemplate::new(500))
            .await;
        up.org_core_policy("PUT", None, ok_set(4)).await;
        let (status, body) = send(up.state(true), "PUT", ROUTE, Some(member_only_put())).await;
        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"]["code"], "model_policy_guard_unavailable");
        assert_eq!(up.org_core_requests().await, 1, "never the PUT");

        // The verdict answered, but not as a verdict list: unknown is not "none left" and is
        // not a pass.
        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(true)).await;
        up.catalogue(two_model_catalogue()).await;
        up.access_for_allow(
            json!(["gpt-open"]),
            ResponseTemplate::new(200).set_body_json(json!({"state": "role"})),
        )
        .await;
        let (status, _) = send(up.state(true), "PUT", ROUTE, Some(member_only_put())).await;
        assert_eq!(status, 503);

        // The catalogue cannot be read. Only a verdict list that predates `zdr_attested`
        // needs the catalogue, so this answer carries no such field.
        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(true)).await;
        up.catalogue(ResponseTemplate::new(502)).await;
        up.access_for_allow(
            json!(["gpt-open"]),
            verdicts(json!([{"id": "gpt-open", "provider": "openai", "entitled": true}])),
        )
        .await;
        let (status, _) = send(up.state(true), "PUT", ROUTE, Some(member_only_put())).await;
        assert_eq!(status, 503);

        // The organization cannot be read, so its ZDR posture is unknown.
        let up = Upstreams::start("owner").await;
        up.org_core_organization(ResponseTemplate::new(500)).await;
        let (status, _) = send(up.state(true), "PUT", ROUTE, Some(member_only_put())).await;
        assert_eq!(status, 503);
        assert_eq!(up.model_gateway_requests().await, 0);
    }

    #[tokio::test]
    async fn without_zdr_or_without_a_restriction_the_guard_reads_nothing_from_the_model_plane() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;

        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(false)).await;
        up.org_core_policy("PUT", Some(member_only_put()), ok_set(4))
            .await;
        let (status, body) = send(up.state(true), "PUT", ROUTE, Some(member_only_put())).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(up.model_gateway_requests().await, 0);

        // `all`, no deny, no tier: nothing can take a model away, so not even the org is read.
        let permissive = json!({"expected_revision": 3, "org_default": {"allow_mode": "all"}});
        let up = Upstreams::start("owner").await;
        up.org_core_policy("PUT", Some(permissive.clone()), ok_set(4))
            .await;
        let (status, body) = send(up.state(true), "PUT", ROUTE, Some(permissive)).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(up.org_core_requests().await, 1, "just the PUT");
        assert_eq!(up.model_gateway_requests().await, 0);
    }

    #[tokio::test]
    async fn the_set_model_entitlement_action_goes_through_the_same_guard_as_the_route() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core_organization(zdr_organization(true)).await;
        up.catalogue(two_model_catalogue()).await;
        up.access_for_allow(
            json!(["gpt-open"]),
            verdicts(json!([{"id": "gpt-open", "provider": "openai", "entitled": true}])),
        )
        .await;
        up.org_core_policy("PUT", None, ok_set(4)).await;

        let mut input = member_only_put();
        input["orgId"] = json!("org-1");
        let (status, body) = send(
            up.state(true),
            "POST",
            "/api/v1/actions/execute",
            Some(json!({"actionId": "org.set_model_entitlement", "input": input})),
        )
        .await;
        assert_eq!(status, 409, "{body}");
        assert_eq!(body["error"]["code"], "model_policy_leaves_no_zdr_model");

        // Off, the action is as unreachable as the route.
        let (status, _) = send(
            up.state(false),
            "POST",
            "/api/v1/actions/execute",
            Some(json!({"actionId": "org.set_model_entitlement", "input": {
                "orgId": "org-1", "expected_revision": 3
            }})),
        )
        .await;
        assert_eq!(status, 404);
    }

    #[test]
    fn only_a_layer_that_can_take_a_model_away_is_judged() {
        let body = json!({
            "expected_revision": 1,
            "org_default": {"allow_mode": "all"},
            "roles": {
                "a": {"allow_mode": "all", "deny": ["x"]},
                "b": {"allow_mode": "all", "min_privacy_tier": "zdr_contractual"},
                "c": {"allow_mode": "LISTED "},
                "d": {"allow_mode": "all", "deny": [], "min_privacy_tier": " ", "max_effort": "low"}
            }
        });
        let names: Vec<String> = restricting_layers(&body)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["the a role", "the b role", "the c role"]);
    }

    #[test]
    fn the_draft_is_folded_the_way_org_core_folds_it() {
        let body = json!({"expected_revision": 1, "org_default": {
            "allow_mode": " Listed", "allow": [" GPT-5 "], "deny": ["Provider:OpenAI"]
        }});
        let layers = restricting_layers(&body);
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].1["allow_mode"], "listed");
        assert_eq!(layers[0].1["allow"], json!(["gpt-5"]));
        assert_eq!(layers[0].1["deny"], json!(["provider:openai"]));
    }

    #[test]
    fn attestation_counts_chat_models_only_and_a_verdict_must_name_an_entitled_one() {
        let catalogue = json!({"models": [
            {"id": "a", "modality": "chat", "features": ["zdr"]},
            {"id": "b", "modality": "chat", "privacy_tier": "sovereign"},
            {"id": "c", "modality": "chat", "features": []},
            {"id": "d", "modality": "embedding", "features": ["zdr"]}
        ]});
        let attested = attested_chat_model_ids(&catalogue).expect("a catalogue");
        assert_eq!(
            attested.iter().map(String::as_str).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(attested_chat_model_ids(&json!({"models": "x"})), None);

        let keeps = |entries: Value| {
            draft_keeps_an_attested_model(&json!({"catalogue": entries}), &attested)
        };
        assert_eq!(keeps(json!([{"id": "a", "entitled": true}])), Some(true));
        assert_eq!(
            keeps(json!([{"id": "a", "entitled": false}, {"id": "c", "entitled": true}])),
            Some(false)
        );
        assert_eq!(keeps(json!([])), Some(false));
        assert_eq!(draft_keeps_an_attested_model(&json!({}), &attested), None);
    }

    #[test]
    fn a_verdict_list_carries_its_own_attestation_or_the_guard_falls_back() {
        use std::collections::BTreeSet;
        let flagged = json!({"catalogue": [
            {"id": "a", "entitled": false, "zdr_attested": true},
            {"id": "b", "entitled": true, "zdr_attested": false},
        ]});
        assert_eq!(
            attested_from_verdicts(&flagged),
            Some(BTreeSet::from(["a".to_owned()]))
        );
        // One verdict without the field is an answer that predates it.
        assert_eq!(
            attested_from_verdicts(&json!({"catalogue": [{"id": "a", "entitled": true}]})),
            None
        );
        // No verdict list at all is no answer, which is not "nothing is attested".
        assert_eq!(attested_from_verdicts(&json!({"state": "role"})), None);
    }
}
