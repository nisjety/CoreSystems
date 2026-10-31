//! Webhook subscriptions for the Innhenting page, under `/api/ingestions/webhooks`.
//!
//! Proxies quarry-edge's `/v1/webhook-subscriptions` (INNH-5) with the caller's bearer
//! token, so the organization comes from the session. The edge answers 501 while its
//! switch is off, and 403 `webhook_off_process` for a Zero Data Retention organization
//! on create, patch and retry; both pass through unchanged. Quarry returns a signing
//! secret from create and rotate only, and those two answers carry
//! `Cache-Control: no-store`. The SPA shows the secret once and keeps it in memory.

use std::collections::HashMap;

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, envelope::ok, middleware::AuthenticatedUser,
    public_url::normalize_public_http_url,
};

use super::shared::{
    array_at, cookie_header, first_str, forward, no_store, okay, quarry_call, quarry_token, str_at,
    str_or_null, validation,
};

/// The one event a subscription can name. Quarry's list is closed; the SPA offers only this.
const WEBHOOK_EVENT: &str = "change_detected";
/// An organization holds at most ten subscriptions, so one page is all of them.
const SUBSCRIPTION_PAGE: u32 = 50;
const DELIVERY_PAGE: u32 = 25;
const MAX_DELIVERY_PAGE: u32 = 100;
const MAX_CURSOR_BYTES: usize = 256;

/// `GET /api/ingestions/webhooks` — the organization's webhook subscriptions.
pub(super) async fn list_webhooks(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &format!("/v1/webhook-subscriptions?limit={SUBSCRIPTION_PAGE}"),
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }
    okay(page_view(
        &crate::envelope::unwrap_data(&body),
        subscription_view,
    ))
}

/// `POST /api/ingestions/webhooks` with `{ url, description? }`. The event list is fixed to
/// `change_detected` here, so the SPA cannot ask for another. The URL passes the same guard
/// as the monitors before Quarry sees it; Quarry checks it again at create and at each connect.
pub(super) async fn create_webhook(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let Some(raw_url) = first_str(&body, &["url"]) else {
        return validation("A URL is required.");
    };
    let url = match normalize_public_http_url(&raw_url) {
        Ok(url) => url,
        Err(message) => return validation(&message),
    };
    let description = first_str(&body, &["description"]);
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, reply) = quarry_call(
        &state,
        Method::POST,
        "/v1/webhook-subscriptions",
        Some(create_body(&url, description.as_deref())),
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, reply);
    }
    let data = crate::envelope::unwrap_data(&reply);
    let mut view = subscription_view(&data);
    view["secret"] = str_or_null(&data, "secret");
    no_store((StatusCode::CREATED, Json(ok(view))))
}

/// `PATCH /api/ingestions/webhooks/{id}` with `{ active?, description? }`. The URL and the
/// events cannot change, so they are dropped here before Quarry sees the body.
pub(super) async fn update_webhook(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let Some(webhook_id) = valid_webhook_id(&id) else {
        return validation("A webhook id is required.");
    };
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, reply) = quarry_call(
        &state,
        Method::PATCH,
        &format!("/v1/webhook-subscriptions/{webhook_id}"),
        Some(patch_body(&body)),
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, reply);
    }
    okay(subscription_view(&crate::envelope::unwrap_data(&reply)))
}

/// `DELETE /api/ingestions/webhooks/{id}` — removes the subscription and its deliveries.
pub(super) async fn delete_webhook(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let Some(webhook_id) = valid_webhook_id(&id) else {
        return validation("A webhook id is required.");
    };
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, reply) = quarry_call(
        &state,
        Method::DELETE,
        &format!("/v1/webhook-subscriptions/{webhook_id}"),
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, reply);
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `POST /api/ingestions/webhooks/{id}/rotate-secret` — a new secret, shown once. The old one
/// stops verifying at once, so the SPA asks for confirmation before it calls this.
pub(super) async fn rotate_webhook_secret(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let Some(webhook_id) = valid_webhook_id(&id) else {
        return validation("A webhook id is required.");
    };
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, reply) = quarry_call(
        &state,
        Method::POST,
        &format!("/v1/webhook-subscriptions/{webhook_id}/rotate-secret"),
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, reply);
    }
    no_store((
        StatusCode::OK,
        Json(ok(secret_view(&crate::envelope::unwrap_data(&reply)))),
    ))
}

/// `GET /api/ingestions/webhooks/{id}/deliveries?status=&limit=&cursor=` — the newest
/// deliveries of one subscription. A delivery carries no payload, only its outcome.
pub(super) async fn list_webhook_deliveries(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let Some(webhook_id) = valid_webhook_id(&id) else {
        return validation("A webhook id is required.");
    };
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, reply) = quarry_call(
        &state,
        Method::GET,
        &deliveries_path(&webhook_id, &params),
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, reply);
    }
    okay(page_view(
        &crate::envelope::unwrap_data(&reply),
        delivery_view,
    ))
}

/// `POST /api/ingestions/webhooks/{id}/deliveries/{delivery_id}/retry` — queues a failed or
/// dead-lettered delivery again. Quarry answers 409 for one that is pending, in flight or done.
pub(super) async fn retry_webhook_delivery(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((id, delivery_id)): Path<(String, String)>,
) -> Response {
    let (Some(webhook_id), Some(row_id)) = (valid_webhook_id(&id), valid_webhook_id(&delivery_id))
    else {
        return validation("A webhook id is required.");
    };
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, reply) = quarry_call(
        &state,
        Method::POST,
        &format!("/v1/webhook-subscriptions/{webhook_id}/deliveries/{row_id}/retry"),
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, reply);
    }
    (
        StatusCode::ACCEPTED,
        Json(ok(delivery_view(&crate::envelope::unwrap_data(&reply)))),
    )
        .into_response()
}

/// The body Quarry gets from a create: the URL and the one event, plus a description when one was sent.
fn create_body(url: &str, description: Option<&str>) -> Value {
    let mut body = json!({ "url": url, "events": [WEBHOOK_EVENT] });
    if let Some(text) = description {
        body["description"] = json!(text);
    }
    body
}

/// The fields a PATCH may change. Anything else, the URL and the events included, is dropped.
fn patch_body(body: &Value) -> Value {
    let mut patch = json!({});
    if let Some(active) = body.get("active").and_then(Value::as_bool) {
        patch["active"] = json!(active);
    }
    if let Some(text) = body.get("description").and_then(Value::as_str) {
        patch["description"] = json!(text.trim());
    }
    patch
}

/// The Quarry path of a delivery read. `limit` is clamped to 1..=100 (default 25); `status`
/// and `cursor` are forwarded only when they are well formed.
fn deliveries_path(webhook_id: &str, params: &HashMap<String, String>) -> String {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(1, MAX_DELIVERY_PAGE))
        .unwrap_or(DELIVERY_PAGE);
    let mut query = vec![format!("limit={limit}")];
    if let Some(status) = params
        .get("status")
        .map(|v| v.trim())
        .filter(|v| is_status_token(v))
    {
        query.push(format!("status={status}"));
    }
    if let Some(cursor) = params
        .get("cursor")
        .map(|v| v.trim())
        .filter(|v| !v.is_empty() && v.len() <= MAX_CURSOR_BYTES)
    {
        query.push(format!("cursor={}", urlencoding::encode(cursor)));
    }
    format!(
        "/v1/webhook-subscriptions/{webhook_id}/deliveries?{}",
        query.join("&")
    )
}

/// A delivery status is a short lowercase token (`pending`, `dlq`); anything else never reaches the edge.
fn is_status_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// A subscription as the SPA reads it. The secret is not here: Quarry returns it from create and rotate only.
fn subscription_view(sub: &Value) -> Value {
    let events: Vec<Value> = array_at(sub, "events")
        .into_iter()
        .filter(Value::is_string)
        .collect();
    json!({
        "id": str_at(sub, "id"),
        "url": str_at(sub, "url"),
        "events": events,
        "active": sub.get("active").and_then(Value::as_bool).unwrap_or(false),
        "description": str_or_null(sub, "description"),
        "secretHint": str_at(sub, "secret_hint"),
        "createdAt": str_at(sub, "created_at"),
        "lastDelivery": last_delivery_view(sub.get("last_delivery")),
    })
}

fn last_delivery_view(last: Option<&Value>) -> Value {
    match last.filter(|v| v.is_object()) {
        Some(last) => json!({
            "status": str_at(last, "status"),
            "at": str_at(last, "at"),
            "eventId": str_at(last, "event_id"),
        }),
        None => Value::Null,
    }
}

fn delivery_view(row: &Value) -> Value {
    json!({
        "id": str_at(row, "id"),
        "eventId": str_at(row, "event_id"),
        "eventType": str_at(row, "event_type"),
        "status": str_at(row, "status"),
        "attempt": row.get("attempt").cloned().unwrap_or(json!(0)),
        "nextAttemptAt": str_or_null(row, "next_attempt_at"),
        "lastError": str_or_null(row, "last_error"),
        "createdAt": str_at(row, "created_at"),
    })
}

fn secret_view(data: &Value) -> Value {
    json!({
        "id": str_at(data, "id"),
        "secret": str_at(data, "secret"),
        "secretHint": str_at(data, "secret_hint"),
    })
}

/// A Quarry list page reshaped: each row through `item`, and the cursor for the next page.
fn page_view(page: &Value, item: fn(&Value) -> Value) -> Value {
    json!({
        "items": array_at(page, "items").iter().map(item).collect::<Vec<Value>>(),
        "nextCursor": str_or_null(page, "next_cursor"),
    })
}

/// Accepts the ids Quarry mints; anything with a slash, a dot or a space never reaches the edge.
fn valid_webhook_id(raw: &str) -> Option<String> {
    let id = raw.trim();
    let well_formed = !id.is_empty()
        && id.len() <= 80
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    well_formed.then(|| id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_create_always_names_the_one_event_and_sends_a_description_only_when_given() {
        let plain = create_body("https://hooks.example.com/q", None);
        assert_eq!(plain["events"], json!(["change_detected"]));
        assert!(plain.get("description").is_none());
        let named = create_body("https://hooks.example.com/q", Some("prod"));
        assert_eq!(named["description"], "prod");
    }

    #[test]
    fn a_patch_keeps_only_active_and_description() {
        let patch = patch_body(&json!({
            "active": false,
            "url": "https://evil.example/",
            "events": ["*"],
            "description": " prod "
        }));
        assert_eq!(patch, json!({ "active": false, "description": "prod" }));
        assert_eq!(
            patch_body(&json!({ "url": "https://evil.example/" })),
            json!({})
        );
    }

    #[test]
    fn ids_must_be_plain_tokens() {
        assert_eq!(
            valid_webhook_id(" whk_01HZX9 ").as_deref(),
            Some("whk_01HZX9")
        );
        assert!(valid_webhook_id("").is_none());
        assert!(valid_webhook_id("whk_../../x").is_none());
        assert!(valid_webhook_id("whk_a/b").is_none());
    }

    #[test]
    fn a_subscription_view_has_no_secret_and_reads_the_last_delivery() {
        let raw = json!({
            "id": "whk_1",
            "url": "https://hooks.example.com/q",
            "events": ["change_detected"],
            "active": true,
            "secret_hint": "whsec_...ab12",
            "created_at": "2026-10-31T10:00:00Z",
            "last_delivery": { "status": "success", "at": "2026-10-31T10:05:00Z", "event_id": "evt_1" }
        });
        let view = subscription_view(&raw);
        assert!(view.get("secret").is_none());
        assert_eq!(view["secretHint"], "whsec_...ab12");
        assert_eq!(view["lastDelivery"]["eventId"], "evt_1");
        assert!(view["description"].is_null());

        let fresh = subscription_view(&json!({
            "id": "whk_2", "url": "https://hooks.example.com/q", "events": [],
            "active": false, "secret_hint": "whsec_...cd34", "created_at": "t"
        }));
        assert!(fresh["lastDelivery"].is_null());
        assert_eq!(fresh["active"], false);
    }

    #[test]
    fn deliveries_are_paged_and_filtered_only_by_well_formed_values() {
        let none = HashMap::new();
        assert_eq!(
            deliveries_path("whk_1", &none),
            "/v1/webhook-subscriptions/whk_1/deliveries?limit=25"
        );

        let mut params = HashMap::new();
        params.insert("limit".to_owned(), "900".to_owned());
        params.insert("status".to_owned(), "dlq".to_owned());
        params.insert("cursor".to_owned(), "c 1".to_owned());
        assert_eq!(
            deliveries_path("whk_1", &params),
            "/v1/webhook-subscriptions/whk_1/deliveries?limit=100&status=dlq&cursor=c%201"
        );

        let mut junk = HashMap::new();
        junk.insert("status".to_owned(), "dlq&limit=1".to_owned());
        assert_eq!(
            deliveries_path("whk_1", &junk),
            "/v1/webhook-subscriptions/whk_1/deliveries?limit=25"
        );
    }

    #[test]
    fn a_delivery_view_keeps_the_error_token_and_the_next_attempt() {
        let row = json!({
            "id": "dlv_1", "event_id": "evt_1", "event_type": "change_detected",
            "status": "failed", "attempt": 2, "next_attempt_at": "t2",
            "last_error": "http_503", "created_at": "t1"
        });
        let view = delivery_view(&row);
        assert_eq!(view["lastError"], "http_503");
        assert_eq!(view["attempt"], 2);
        assert_eq!(view["nextAttemptAt"], "t2");
        assert!(delivery_view(&json!({ "id": "dlv_2" }))["lastError"].is_null());
    }

    #[test]
    fn a_page_keeps_the_cursor_quarry_gave() {
        let page = page_view(
            &json!({ "items": [{ "id": "whk_1" }], "next_cursor": "abc" }),
            subscription_view,
        );
        assert_eq!(page["items"][0]["id"], "whk_1");
        assert_eq!(page["nextCursor"], "abc");
        assert!(page_view(&json!({ "items": [] }), delivery_view)["nextCursor"].is_null());
    }
}
