//! Webhook subscriptions on the edge (INNH-5): the tenant-facing passthrough to control's
//! org-scoped `/v1/webhook-subscriptions` routes.
//!
//! ```text
//! POST   /v1/webhook-subscriptions                                 create (secret shown once)
//! GET    /v1/webhook-subscriptions                                 list
//! GET    /v1/webhook-subscriptions/:id                             read
//! PATCH  /v1/webhook-subscriptions/:id                             active / events / description / notify_on_change
//! DELETE /v1/webhook-subscriptions/:id                             delete (204)
//! POST   /v1/webhook-subscriptions/:id/rotate-secret               new secret, shown once
//! GET    /v1/webhook-subscriptions/:id/deliveries                  delivery log, no payload body
//! POST   /v1/webhook-subscriptions/:id/deliveries/:did/retry       re-queue a failed delivery
//! ```
//!
//! The org is the verified token's, never a query parameter or a body field: a caller-supplied
//! `org_id` is ignored, and an unknown body field is a 400 (`deny_unknown_fields`), so nothing
//! the caller adds can reach control. Control answers a foreign id with 404 and this file
//! passes that through. Every answer is rebuilt from a typed view, so a stray `secret` in a
//! control read can never leave the edge: only the create and rotate views carry one.
//!
//! Dark by default: `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS` must be on, otherwise every route
//! answers 501 before control is called. Control has its own switch
//! (`QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS`); a control that has it off answers a plain 404,
//! which this file recognises (a real not-found carries control's error envelope) and reports
//! as 501 with the name of the missing switch.
//!
//! Zero Data Retention: a subscription pushes page URLs and fingerprints to a third party,
//! which ZDR forbids. A caller whose token floor is ZDR cannot create a subscription, patch
//! one or retry a delivery (403, `details.reason == "webhook_off_process"`); it can still
//! read, delete and rotate the secret of one it already holds (wind-down). A service
//! principal is refused 403 by `auth::require_service_route_scope`, which does not name these
//! paths, and again here.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::resources::{
    WebhookDeliveryList, WebhookDeliveryView, WebhookSecretRotated, WebhookSubscription,
    WebhookSubscriptionCreated, WebhookSubscriptionList,
};

use crate::auth::Claims;
use crate::control_client::{self, ControlReply};
use crate::job_lifecycle_routes::fail;
use crate::state::AppState;

/// Turns the subscription routes on. Off (the default), every route answers 501.
pub const ENV_WEBHOOK_SUBSCRIPTIONS: &str = "QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS";

/// The switch control needs on as well; named in the 501 when control reports the route missing.
pub const ENV_CONTROL_WEBHOOK_SUBSCRIPTIONS: &str = "QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS";

/// Largest page of a list (control's default of 50 applies when `limit` is absent).
pub const MAX_LIMIT: u32 = 200;

/// The reason a ZDR caller is refused: the feature pushes data off-process.
pub const ZDR_REASON: &str = "webhook_off_process";

/// Whether the subscription routes are enabled. Read per request, so a flip needs no restart.
pub fn webhook_subscriptions_enabled() -> bool {
    crate::job_results_routes::flag_is_on(std::env::var(ENV_WEBHOOK_SUBSCRIPTIONS).ok().as_deref())
}

/// What a route does to the subscription, for the ZDR rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Starts or continues pushing data off-process: create, patch, retry. Refused under ZDR.
    Push,
    /// Reads, deletes, rotates a secret. Open under ZDR: wind-down and hygiene.
    Other,
}

/// The checks every route makes before control is asked: the flag first (a dark feature
/// answers 501 whoever asks), then who may call, then the ZDR floor.
pub fn gate(enabled: bool, claims: &Claims, access: Access) -> Result<(), QuarryError> {
    if !enabled {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            format!(
                "webhook subscriptions are not enabled on this edge: set {ENV_WEBHOOK_SUBSCRIPTIONS}=1 \
                 (see docs/RUNBOOKS.md, \"Webhook subscriptions\")"
            ),
        ));
    }
    if claims.is_service() {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "service principals cannot manage webhook subscriptions",
        ));
    }
    if access == Access::Push && claims.effective_zdr(false) {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "webhook subscriptions are not available under zero data retention",
        )
        .with_details(json!({ "reason": ZDR_REASON })));
    }
    Ok(())
}

fn bad(field: &str, reason: &str, message: &str) -> QuarryError {
    QuarryError::new(ErrorCode::BadRequest, message)
        .with_details(json!({ "field": field, "reason": reason }))
}

/// A path id of the given prefix (`whk_`, `whkd_`): letters, digits, `_` and `-` after the
/// prefix, 80 characters at most. Anything else is a 404 before control sees it, so an id
/// never carries a path segment or a query into the control URL.
pub fn valid_id(raw: &str, prefix: &str) -> bool {
    raw.len() > prefix.len()
        && raw.len() <= 80
        && raw.starts_with(prefix)
        && raw[prefix.len()..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn not_found() -> QuarryError {
    QuarryError::new(ErrorCode::NotFound, "not found")
}

/// The paging query of the two list routes, checked. `status` is only read by deliveries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListParams {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub status: Option<String>,
}

const STATUSES: &[&str] = &["pending", "in_flight", "success", "failed", "dlq"];

/// Parses `limit` (1..=200), `cursor` (opaque, URL-safe base64 alphabet, 256 characters at
/// most) and, when `with_status`, `status`. A bad value is a 400 that names the field.
pub fn parse_list_params(
    query: &HashMap<String, String>,
    with_status: bool,
) -> Result<ListParams, QuarryError> {
    let limit = match query.get("limit") {
        None => None,
        Some(raw) => match raw.parse::<u32>() {
            Ok(n) if (1..=MAX_LIMIT).contains(&n) => Some(n),
            _ => return Err(bad("limit", "limit_invalid", "limit must be an integer from 1 to 200")),
        },
    };
    let cursor = match query.get("cursor") {
        None => None,
        Some(raw)
            if !raw.is_empty()
                && raw.len() <= 256
                && raw.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '=')) =>
        {
            Some(raw.clone())
        }
        Some(_) => return Err(bad("cursor", "cursor_invalid", "cursor is not one this API issued")),
    };
    let status = match (with_status, query.get("status")) {
        (true, Some(raw)) if STATUSES.contains(&raw.as_str()) => Some(raw.clone()),
        (true, Some(_)) => {
            return Err(bad(
                "status",
                "status_unsupported",
                "status must be one of pending, in_flight, success, failed, dlq",
            ))
        }
        _ => None,
    };
    Ok(ListParams { limit, cursor, status })
}

impl ListParams {
    fn forward(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if let Some(limit) = self.limit {
            out.push(("limit", limit.to_string()));
        }
        if let Some(cursor) = &self.cursor {
            out.push(("cursor", cursor.clone()));
        }
        if let Some(status) = &self.status {
            out.push(("status", status.clone()));
        }
        out
    }
}

/// The body of `POST /v1/webhook-subscriptions`. An unknown field (an `org_id`, a `secret`)
/// is a 400, never silently dropped.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBody {
    url: String,
    events: Vec<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    notify_on_change: Option<NotifyFilterBody>,
}

/// The body of `PATCH /v1/webhook-subscriptions/:id`. The URL is not editable: delete and
/// create again, so every target goes through the create-time check.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchBody {
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    events: Option<Vec<String>>,
    #[serde(default)]
    description: Option<String>,
    /// Absent keeps the filter, `null` removes it, and an object replaces it.
    #[serde(default, deserialize_with = "nullable")]
    notify_on_change: Option<Option<NotifyFilterBody>>,
}

/// The `notify_on_change` filter of a subscription body (E3.5-7). Its keys are closed here, so an
/// unknown key is a 400 like any other body field. Its values are checked by control, which names
/// the reason of a refusal.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NotifyFilterBody {
    #[serde(default)]
    types: Vec<String>,
    #[serde(default)]
    min_paragraph_delta: i64,
}

impl NotifyFilterBody {
    /// The filter as control's body takes it.
    fn to_json(&self) -> Value {
        json!({ "types": self.types, "min_paragraph_delta": self.min_paragraph_delta })
    }
}

/// Reads a field that may be JSON `null`. With `#[serde(default)]` an absent field is `None`, a
/// `null` is `Some(None)` and a value is `Some(Some(value))`, so PATCH can tell "keep" from "remove".
fn nullable<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

fn parse_body<T: DeserializeOwned>(raw: &Bytes) -> Result<T, QuarryError> {
    serde_json::from_slice(raw).map_err(|e| {
        bad(
            "body",
            "body_invalid",
            &format!("request body is not valid for this route: {}", short(&e.to_string())),
        )
    })
}

fn short(text: &str) -> String {
    text.chars().take(160).collect()
}

/// A control reason or field: lowercase letters, digits and `_`, 64 characters at most. It
/// is the only free text taken from control's error details.
fn token(value: &Value) -> Option<String> {
    let s = value.as_str()?;
    (!s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
        .then(|| s.to_string())
}

/// Turns control's non-2xx answer into the edge's typed error. Control's validation reasons
/// (`field`, `reason`, and for the cap `limit`) survive; nothing else of its body does.
pub fn map_control_error(reply: &ControlReply) -> QuarryError {
    let envelope = reply.body.get("error").filter(|e| e.is_object());
    let details = envelope.and_then(|e| e.get("details"));
    let pick = |key: &str| details.and_then(|d| d.get(key)).and_then(token);
    let message = envelope
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .map(short);
    let mut kept = serde_json::Map::new();
    for key in ["field", "reason", "status"] {
        if let Some(value) = pick(key) {
            kept.insert(key.to_string(), Value::String(value));
        }
    }
    if let Some(limit) = details.and_then(|d| d.get("limit")).and_then(Value::as_u64) {
        kept.insert("limit".into(), json!(limit));
    }
    let with = |err: QuarryError| {
        if kept.is_empty() {
            err
        } else {
            err.with_details(Value::Object(kept.clone()))
        }
    };
    match reply.status {
        400 => with(QuarryError::new(
            ErrorCode::BadRequest,
            message.unwrap_or_else(|| "control rejected the request".into()),
        )),
        401 | 403 => QuarryError::new(ErrorCode::Forbidden, "control refused the request"),
        404 if envelope.is_some() => not_found(),
        // A plain 404 with no control envelope is a router miss: control has the switch off.
        404 => QuarryError::new(
            ErrorCode::Unsupported,
            format!(
                "webhook subscriptions are not enabled on control: set {ENV_CONTROL_WEBHOOK_SUBSCRIPTIONS}=1 \
                 (see docs/RUNBOOKS.md, \"Webhook subscriptions\")"
            ),
        ),
        409 => with(QuarryError::new(
            ErrorCode::Conflict,
            message.unwrap_or_else(|| "the request conflicts with the current state".into()),
        )),
        429 => with(QuarryError::new(
            ErrorCode::RateLimited,
            message.unwrap_or_else(|| "the organization is at its webhook subscription limit".into()),
        )),
        _ => QuarryError::new(ErrorCode::DriverFailed, "control request failed"),
    }
}

/// Runs one call: the gate, then control, then the typed error mapping.
async fn call(
    state: &AppState,
    claims: &Claims,
    access: Access,
    method: reqwest::Method,
    path: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
) -> Result<ControlReply, QuarryError> {
    gate(webhook_subscriptions_enabled(), claims, access)?;
    let reply = control_client::send_raw(state, method, path, &claims.org_id, query, body).await?;
    if (200..300).contains(&reply.status) {
        Ok(reply)
    } else {
        Err(map_control_error(&reply))
    }
}

/// Reads control's `data` as the typed view `T`; anything that does not fit is a 500, not a
/// pass-through of control's raw body.
fn typed<T: DeserializeOwned>(reply: ControlReply) -> Result<T, QuarryError> {
    let data = reply.body.get("data").cloned().unwrap_or(Value::Null);
    serde_json::from_value(data)
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "control answer unreadable"))
}

fn render<T: Serialize>(request_id: String, status: StatusCode, result: Result<T, QuarryError>) -> Response {
    match result {
        Ok(value) => (status, Json(Envelope::ok(request_id, value))).into_response(),
        Err(err) => fail(&request_id, err).into_response(),
    }
}

fn require_id(raw: &str, prefix: &str) -> Result<(), QuarryError> {
    if valid_id(raw, prefix) {
        Ok(())
    } else {
        Err(not_found())
    }
}

/// `POST /v1/webhook-subscriptions`.
pub async fn create(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    body: Bytes,
) -> Response {
    let request_id = crate::request_id::current();
    let result = async {
        // The gate runs before the body is read, so a refused caller learns nothing about it.
        gate(webhook_subscriptions_enabled(), &claims, Access::Push)?;
        let parsed: CreateBody = parse_body(&body)?;
        let mut forward = json!({
            "url": parsed.url,
            "events": parsed.events,
            "created_by": claims.actor_id(),
        });
        if let Some(description) = parsed.description {
            forward["description"] = Value::String(description);
        }
        if let Some(filter) = &parsed.notify_on_change {
            forward["notify_on_change"] = filter.to_json();
        }
        let reply = call(
            &state,
            &claims,
            Access::Push,
            reqwest::Method::POST,
            "/v1/webhook-subscriptions",
            &[],
            Some(&forward),
        )
        .await?;
        typed::<WebhookSubscriptionCreated>(reply)
    }
    .await;
    render(request_id, StatusCode::CREATED, result)
}

/// `GET /v1/webhook-subscriptions`.
pub async fn list(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let request_id = crate::request_id::current();
    let result = async {
        gate(webhook_subscriptions_enabled(), &claims, Access::Other)?;
        let params = parse_list_params(&query, false)?;
        let reply = call(
            &state,
            &claims,
            Access::Other,
            reqwest::Method::GET,
            "/v1/webhook-subscriptions",
            &params.forward(),
            None,
        )
        .await?;
        typed::<WebhookSubscriptionList>(reply)
    }
    .await;
    render(request_id, StatusCode::OK, result)
}

/// `GET /v1/webhook-subscriptions/:id`.
pub async fn get(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Response {
    let request_id = crate::request_id::current();
    let result = async {
        gate(webhook_subscriptions_enabled(), &claims, Access::Other)?;
        require_id(&id, "whk_")?;
        let reply = call(
            &state,
            &claims,
            Access::Other,
            reqwest::Method::GET,
            &format!("/v1/webhook-subscriptions/{id}"),
            &[],
            None,
        )
        .await?;
        typed::<WebhookSubscription>(reply)
    }
    .await;
    render(request_id, StatusCode::OK, result)
}

/// `PATCH /v1/webhook-subscriptions/:id`.
pub async fn patch(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let request_id = crate::request_id::current();
    let result = async {
        gate(webhook_subscriptions_enabled(), &claims, Access::Push)?;
        require_id(&id, "whk_")?;
        let parsed: PatchBody = parse_body(&body)?;
        let mut forward = serde_json::Map::new();
        if let Some(active) = parsed.active {
            forward.insert("active".into(), Value::Bool(active));
        }
        if let Some(events) = parsed.events {
            forward.insert("events".into(), json!(events));
        }
        if let Some(description) = parsed.description {
            forward.insert("description".into(), Value::String(description));
        }
        if let Some(filter) = parsed.notify_on_change {
            forward.insert(
                "notify_on_change".into(),
                filter.map_or(Value::Null, |f| f.to_json()),
            );
        }
        if forward.is_empty() {
            return Err(bad(
                "body",
                "empty_patch",
                "name at least one of active, events, description, notify_on_change",
            ));
        }
        let reply = call(
            &state,
            &claims,
            Access::Push,
            reqwest::Method::PATCH,
            &format!("/v1/webhook-subscriptions/{id}"),
            &[],
            Some(&Value::Object(forward)),
        )
        .await?;
        typed::<WebhookSubscription>(reply)
    }
    .await;
    render(request_id, StatusCode::OK, result)
}

/// `DELETE /v1/webhook-subscriptions/:id`: 204 with no body. A foreign or unknown id is 404.
pub async fn delete(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Response {
    let request_id = crate::request_id::current();
    let result = async {
        gate(webhook_subscriptions_enabled(), &claims, Access::Other)?;
        require_id(&id, "whk_")?;
        call(
            &state,
            &claims,
            Access::Other,
            reqwest::Method::DELETE,
            &format!("/v1/webhook-subscriptions/{id}"),
            &[],
            None,
        )
        .await
    }
    .await;
    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => fail(&request_id, err).into_response(),
    }
}

/// `POST /v1/webhook-subscriptions/:id/rotate-secret`.
pub async fn rotate_secret(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Response {
    let request_id = crate::request_id::current();
    let result = async {
        gate(webhook_subscriptions_enabled(), &claims, Access::Other)?;
        require_id(&id, "whk_")?;
        let reply = call(
            &state,
            &claims,
            Access::Other,
            reqwest::Method::POST,
            &format!("/v1/webhook-subscriptions/{id}/rotate-secret"),
            &[],
            None,
        )
        .await?;
        typed::<WebhookSecretRotated>(reply)
    }
    .await;
    render(request_id, StatusCode::OK, result)
}

/// `GET /v1/webhook-subscriptions/:id/deliveries`.
pub async fn deliveries(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let request_id = crate::request_id::current();
    let result = async {
        gate(webhook_subscriptions_enabled(), &claims, Access::Other)?;
        require_id(&id, "whk_")?;
        let params = parse_list_params(&query, true)?;
        let reply = call(
            &state,
            &claims,
            Access::Other,
            reqwest::Method::GET,
            &format!("/v1/webhook-subscriptions/{id}/deliveries"),
            &params.forward(),
            None,
        )
        .await?;
        typed::<WebhookDeliveryList>(reply)
    }
    .await;
    render(request_id, StatusCode::OK, result)
}

/// `POST /v1/webhook-subscriptions/:id/deliveries/:did/retry`: 202 with the re-queued delivery.
pub async fn retry_delivery(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((id, did)): Path<(String, String)>,
) -> Response {
    let request_id = crate::request_id::current();
    let result = async {
        gate(webhook_subscriptions_enabled(), &claims, Access::Push)?;
        require_id(&id, "whk_")?;
        require_id(&did, "whkd_")?;
        let reply = call(
            &state,
            &claims,
            Access::Push,
            reqwest::Method::POST,
            &format!("/v1/webhook-subscriptions/{id}/deliveries/{did}/retry"),
            &[],
            None,
        )
        .await?;
        typed::<WebhookDeliveryView>(reply)
    }
    .await;
    render(request_id, StatusCode::ACCEPTED, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(zdr: Option<bool>) -> Claims {
        Claims {
            web_policy: None,
            sub: "user_1".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "org_a".into(),
            user_id: "user_1".into(),
            principal_type: Some("user".into()),
            service_id: None,
            nbf: None,
            aud: None,
            scopes: Vec::new(),
            zdr,
            key_id: None,
            acting_user_id: None,
        }
    }

    fn service() -> Claims {
        Claims {
            principal_type: Some("service".into()),
            service_id: Some("service:model-gateway".into()),
            sub: "service:model-gateway".into(),
            user_id: "service:model-gateway".into(),
            ..user(None)
        }
    }

    #[test]
    fn the_gate_names_the_variable_when_off_and_off_wins_over_everything() {
        let err = gate(false, &user(Some(true)), Access::Push).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unsupported);
        assert!(err.message.contains(ENV_WEBHOOK_SUBSCRIPTIONS), "{}", err.message);
        assert_eq!(gate(false, &service(), Access::Other).unwrap_err().code, ErrorCode::Unsupported);
    }

    #[test]
    fn zdr_refuses_what_pushes_and_leaves_wind_down_open() {
        let zdr = user(Some(true));
        let err = gate(true, &zdr, Access::Push).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert_eq!(err.details.as_ref().unwrap()["reason"], ZDR_REASON);
        assert!(gate(true, &zdr, Access::Other).is_ok());
        assert!(gate(true, &user(Some(false)), Access::Push).is_ok());
        assert!(gate(true, &user(None), Access::Push).is_ok());
    }

    #[test]
    fn a_service_principal_is_refused_even_if_it_got_past_the_route_scope() {
        for access in [Access::Push, Access::Other] {
            assert_eq!(gate(true, &service(), access).unwrap_err().code, ErrorCode::Forbidden);
        }
    }

    #[test]
    fn ids_must_carry_their_prefix_and_no_path_characters() {
        assert!(valid_id("whk_01J9ABC", "whk_"));
        assert!(valid_id("whkd_01J9-abc_X", "whkd_"));
        for bad in ["", "whk_", "wh_01", "whkd_01", "whk_../x", "whk_a/b", "whk_a?x=1", "whk_a b", "whk_%2e", &format!("whk_{}", "a".repeat(80))] {
            assert!(!valid_id(bad, "whk_"), "{bad}");
        }
    }

    #[test]
    fn list_params_are_checked() {
        let q = |pairs: &[(&str, &str)]| {
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<_, _>>()
        };
        let ok = parse_list_params(&q(&[("limit", "25"), ("cursor", "MTIzfHdoa18x"), ("status", "dlq")]), true).unwrap();
        assert_eq!(ok.limit, Some(25));
        assert_eq!(ok.cursor.as_deref(), Some("MTIzfHdoa18x"));
        assert_eq!(ok.status.as_deref(), Some("dlq"));
        // Status is ignored on the subscription list.
        assert_eq!(parse_list_params(&q(&[("status", "weird")]), false).unwrap().status, None);
        for (pairs, field) in [
            (vec![("limit", "0")], "limit"),
            (vec![("limit", "201")], "limit"),
            (vec![("limit", "x")], "limit"),
            (vec![("cursor", "a b")], "cursor"),
            (vec![("cursor", "../x")], "cursor"),
            (vec![("status", "weird")], "status"),
        ] {
            let err = parse_list_params(&q(&pairs), true).unwrap_err();
            assert_eq!(err.code, ErrorCode::BadRequest, "{pairs:?}");
            assert_eq!(err.details.unwrap()["field"], field);
        }
    }

    fn reply(status: u16, body: Value) -> ControlReply {
        ControlReply { status, body }
    }

    #[test]
    fn control_validation_reasons_survive_and_nothing_else_does() {
        let err = map_control_error(&reply(
            400,
            json!({ "error": { "code": "BAD_REQUEST", "message": "url must use https",
                    "details": { "field": "url", "reason": "url_not_https", "dsn": "postgres://secret", "reason2": "x" } } }),
        ));
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert_eq!(err.message, "url must use https");
        let details = err.details.unwrap();
        assert_eq!(details["field"], "url");
        assert_eq!(details["reason"], "url_not_https");
        assert!(details.get("dsn").is_none() && details.get("reason2").is_none(), "{details}");
    }

    #[test]
    fn a_reason_that_is_not_a_token_is_dropped() {
        let err = map_control_error(&reply(
            409,
            json!({ "error": { "message": "exists", "details": { "reason": "Bad Reason!" } } }),
        ));
        assert_eq!(err.code, ErrorCode::Conflict);
        assert!(err.details.is_none());
    }

    #[test]
    fn statuses_map_to_the_typed_errors() {
        let with_env = |status| reply(status, json!({ "error": { "code": "X", "message": "m" } }));
        assert_eq!(map_control_error(&with_env(404)).code, ErrorCode::NotFound);
        assert_eq!(map_control_error(&with_env(409)).code, ErrorCode::Conflict);
        assert_eq!(map_control_error(&with_env(403)).code, ErrorCode::Forbidden);
        assert_eq!(map_control_error(&with_env(500)).code, ErrorCode::DriverFailed);
        let limited = map_control_error(&reply(
            429,
            json!({ "error": { "message": "full", "details": { "reason": "subscription_limit", "limit": 10 } } }),
        ));
        assert_eq!(limited.code, ErrorCode::RateLimited);
        assert_eq!(limited.details.unwrap()["limit"], 10);
    }

    #[test]
    fn a_bare_404_is_control_with_the_switch_off() {
        let err = map_control_error(&reply(404, Value::Null));
        assert_eq!(err.code, ErrorCode::Unsupported);
        assert!(err.message.contains(ENV_CONTROL_WEBHOOK_SUBSCRIPTIONS), "{}", err.message);
    }

    #[test]
    fn the_typed_views_drop_a_stray_secret() {
        let wire = json!({ "data": {
            "id": "whk_1", "url": "https://h.example/q", "events": ["change_detected"], "active": true,
            "secret_hint": "whsec_...ab12", "created_at": "2026-10-31T10:00:00Z", "secret": "whsec_leak" } });
        let sub: WebhookSubscription = typed(reply(200, wire)).unwrap();
        let out = serde_json::to_string(&sub).unwrap();
        assert!(!out.contains("whsec_leak") && !out.contains("\"secret\""), "{out}");
    }

    #[test]
    fn the_created_view_carries_the_secret_beside_the_subscription() {
        let wire = json!({ "data": {
            "id": "whk_1", "url": "https://h.example/q", "events": ["change_detected"], "active": true,
            "secret_hint": "whsec_...ab12", "created_at": "2026-10-31T10:00:00Z", "secret": "whsec_abc" } });
        let created: WebhookSubscriptionCreated = typed(reply(201, wire)).unwrap();
        let out = serde_json::to_value(&created).unwrap();
        assert_eq!(out["secret"], "whsec_abc");
        assert_eq!(out["id"], "whk_1");
        assert!(out.get("last_delivery").is_none() && out.get("description").is_none(), "{out}");
    }
}
