//! The member's personal Spaces sidebar layout: the BFF half of WS16-S8.
//!
//! `GET` and `PUT /api/v1/spaces/sidebar` read and save one small document of references (starred
//! rooms, hidden rooms, named sections of room refs and closed group ids; see Application's
//! `spaceSidebarRules.ts` for the shape and caps). Application (Convex `spaceSidebar`) stores it,
//! compare-and-set on a revision. This module holds no Space state, mints no Control decision and
//! makes no Model Plane call: which rooms a person can open is still Control's answer, and nothing
//! in an authority path reads this document.
//!
//! The organization comes from the verified session membership and the user from the session; the
//! body can name neither. Keys other than `base_revision` and `doc` are refused. The document is
//! validated in Application (one rule set, one copy); the gateway checks only the envelope and
//! the 64 KiB body cap, so an oversized body never reaches Convex.
//!
//! Zero Data Retention split (SD-12 option A): labels in the document are user-authored text, so a
//! ZDR organization does not get a synced sidebar. `GET` answers `mode: "zdr_local"` with no
//! document and the SPA keeps a refs-and-labels-only copy in the browser; `PUT` is refused with
//! the typed 412 before any Application call. An org-core outage is a 503 on both verbs, never a
//! silent flip to `zdr_local` and never a guess toward "not ZDR".
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_sidebar_doc` (`details.reason`), 403
//! `active_membership_required`, 404 `space_sidebar_disabled`, 409 `space_sidebar_conflict`
//! (`details:{revision, doc}`), 412 `zdr_durable_mutation_forbidden`, 413 `sidebar_too_large`, 503
//! `space_sidebar_unavailable` / `zdr_decision_unavailable`.

use axum::{
    body::Bytes,
    extract::{rejection::BytesRejection, DefaultBodyLimit, Extension, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use serde_json::{json, Map, Value};

use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
    zdr::{durable_write_refusal, org_zdr_lookup, request_zdr, OrgZdrPosture},
};

type Reply = (StatusCode, Json<Value>);

/// The body cap of the `PUT` route. Application's own cap on the document is 32 KiB; this is the
/// transport ceiling above it (the JSON envelope and whitespace), so nothing larger is buffered.
const BODY_LIMIT_BYTES: usize = 64 * 1024;

/// The caps the SPA shows, in the plan's names. Mirrors Application's `SIDEBAR_LIMITS`
/// (`test/fixtures/space-sidebar-limits.json`); the golden parity test pins both copies.
fn limits() -> Value {
    json!({
        "sections": 30,
        "refs_per_section": 200,
        "starred": 100,
        "hidden": 500,
        "name": 40,
    })
}

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/sidebar",
        get(get_sidebar)
            .put(put_sidebar)
            .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES)),
    )
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn feature_off() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "space_sidebar_disabled",
        "The Spaces sidebar layout is not enabled.",
    )
}

fn unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "space_sidebar_unavailable",
        "The sidebar layout is unavailable. Nothing was changed.",
    )
}

fn invalid(reason: &str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "invalid_sidebar_doc",
            "The sidebar layout is not valid.",
            json!({ "reason": reason }),
        )),
    )
}

/// Flag, then the verified organization. Everything both verbs share before the ZDR split.
async fn gate(state: &AppState, user: &AuthenticatedUser) -> Result<String, Reply> {
    if !state.space_features.sidebar {
        return Err(feature_off());
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to use the sidebar layout.",
        ));
    }
    Ok(org_id)
}

/// One Application call. `Ok` is the function's value; `Err(Some(message))` is a thrown error
/// (Convex answers a non-success status or `status: "error"` with an `errorMessage`);
/// `Err(None)` is every other failure (not configured, unreachable, unreadable).
async fn convex_call(
    state: &AppState,
    operation: &str,
    path: &str,
    mut args: Value,
) -> Result<Value, Option<String>> {
    let convex_url = state.application_convex_url.clone();
    let service_key = state.application_convex_service_key.clone();
    if convex_url.trim().is_empty() || service_key.trim().is_empty() {
        return Err(None);
    }
    args.as_object_mut()
        .ok_or(None)?
        .insert("serviceKey".to_owned(), Value::String(service_key));
    let response = crate::upstream::send_with_retry(
        state
            .client
            .post(format!(
                "{}/api/{operation}",
                convex_url.trim_end_matches('/')
            ))
            .json(&json!({ "path": path, "args": args })),
    )
    .await
    .map_err(|_| None)?;
    let http_ok = response.status().is_success();
    let body = response.json::<Value>().await.map_err(|_| None)?;
    if http_ok && body.get("status").and_then(Value::as_str) != Some("error") {
        return body.get("value").cloned().ok_or(None);
    }
    Err(body
        .get("errorMessage")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// The validation token out of an `invalid_sidebar_doc:<reason>` Application error, lowercase
/// letters, digits and underscores only (nothing else from the message is ever returned). An
/// argument-validator failure (a key or type the table does not accept) is `doc_shape`.
fn invalid_reason(message: &str) -> Option<String> {
    const PREFIX: &str = "invalid_sidebar_doc:";
    if let Some(start) = message.find(PREFIX) {
        let token: String = message[start + PREFIX.len()..]
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
            .take(64)
            .collect();
        return Some(if token.is_empty() {
            "doc_shape".to_owned()
        } else {
            token
        });
    }
    if message.contains("ArgumentValidationError") || message.contains("Validator error") {
        return Some("doc_shape".to_owned());
    }
    None
}

/// `GET /api/v1/spaces/sidebar`.
async fn get_sidebar(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Reply {
    let org_id = match gate(&state, &user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    // A request-level ZDR assertion counts as ZDR; the org posture can only raise it further.
    let zdr_local = if request_zdr(&headers, None) {
        true
    } else {
        match org_zdr_lookup(&state, &user).await {
            OrgZdrPosture::Enabled => true,
            OrgZdrPosture::Disabled => false,
            OrgZdrPosture::Unknown => return unavailable(),
        }
    };
    if zdr_local {
        return (
            StatusCode::OK,
            Json(json!({"data": {
                "mode": "zdr_local",
                "revision": 0,
                "doc": Value::Null,
                "limits": limits(),
            }})),
        );
    }
    match convex_call(
        &state,
        "query",
        "spaceSidebar:getForGateway",
        json!({ "externalAuthId": user.user_id, "externalOrgId": org_id }),
    )
    .await
    {
        Ok(Value::Null) => (
            StatusCode::OK,
            Json(json!({"data": {
                "mode": "server",
                "revision": 0,
                "doc": Value::Null,
                "limits": limits(),
            }})),
        ),
        Ok(row) => match (row.get("revision").and_then(Value::as_u64), row.get("doc")) {
            (Some(revision), Some(doc)) if doc.is_object() => (
                StatusCode::OK,
                Json(json!({"data": {
                    "mode": "server",
                    "revision": revision,
                    "doc": doc,
                    "limits": limits(),
                }})),
            ),
            _ => unavailable(),
        },
        Err(_) => unavailable(),
    }
}

/// `PUT /api/v1/spaces/sidebar {"base_revision": n, "doc": {...}}`.
async fn put_sidebar(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Reply {
    // The body is read lazily so the flag, organization and ZDR refusals come first.
    match gate(&state, &user).await {
        Ok(_) => {}
        Err(reply) => return reply,
    }
    if let Some(refusal) = durable_write_refusal(&state, &user, Some(&headers), None).await {
        return refusal;
    }
    let bytes = match body {
        Ok(bytes) => bytes,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return refuse(
                StatusCode::PAYLOAD_TOO_LARGE,
                "sidebar_too_large",
                "The sidebar layout is too large to save.",
            )
        }
        Err(_) => return invalid("body_unreadable"),
    };
    let Ok(body) = serde_json::from_slice::<Value>(&bytes) else {
        return invalid("body_not_json");
    };
    save_sidebar(&state, &user, &headers, &body).await
}

/// The whole save, shared with the `spaces.update_sidebar` action: flag, organization, ZDR, the
/// envelope, then the compare-and-set in Application. `body` is the untrusted
/// `{base_revision, doc}` object.
pub(crate) async fn save_sidebar(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    body: &Value,
) -> Reply {
    let org_id = match gate(state, user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    if let Some(refusal) = durable_write_refusal(state, user, Some(headers), Some(body)).await {
        return refusal;
    }
    let Some(object) = body.as_object() else {
        return invalid("body_not_object");
    };
    if object.keys().any(|key| key != "base_revision" && key != "doc") {
        return invalid("unknown_key");
    }
    let Some(base_revision) = object.get("base_revision").and_then(Value::as_u64) else {
        return invalid("bad_base_revision");
    };
    let Some(doc) = object.get("doc").filter(|doc| doc.is_object()) else {
        return invalid("doc_not_object");
    };
    // A second cheap bound for the action path, which has no body extractor in front of it.
    if doc.to_string().len() > BODY_LIMIT_BYTES {
        return refuse(
            StatusCode::PAYLOAD_TOO_LARGE,
            "sidebar_too_large",
            "The sidebar layout is too large to save.",
        );
    }
    match convex_call(
        state,
        "mutation",
        "spaceSidebar:putForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "baseRevision": base_revision,
            "doc": doc,
        }),
    )
    .await
    {
        Ok(result) => match result.get("status").and_then(Value::as_str) {
            Some("ok") => match result.get("revision").and_then(Value::as_u64) {
                Some(revision) => (
                    StatusCode::OK,
                    Json(json!({"data": { "revision": revision, "doc": doc }})),
                ),
                None => unavailable(),
            },
            Some("conflict") => {
                let mut details = Map::new();
                details.insert(
                    "revision".to_owned(),
                    result.get("revision").cloned().unwrap_or(Value::Null),
                );
                details.insert(
                    "doc".to_owned(),
                    result.get("doc").cloned().unwrap_or(Value::Null),
                );
                (
                    StatusCode::CONFLICT,
                    Json(error_with_details(
                        "space_sidebar_conflict",
                        "The sidebar layout changed somewhere else. Reload it and apply your change again.",
                        Value::Object(details),
                    )),
                )
            }
            _ => unavailable(),
        },
        Err(Some(message)) => match invalid_reason(&message) {
            Some(reason) => invalid(&reason),
            None => unavailable(),
        },
        Err(None) => unavailable(),
    }
}
