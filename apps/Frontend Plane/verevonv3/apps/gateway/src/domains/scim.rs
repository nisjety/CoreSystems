//! The public SCIM edge (WS05-S8): `/api/v1/scim/v2/*`, called by an organization's
//! identity provider (Entra ID, Okta) with a SCIM bearer token.
//!
//! This module is stateless and holds no authority. It has no session and trusts no
//! client header: the bearer token is forwarded opaquely in `Authorization` and
//! org-core resolves it to an organization. The upstream request is rebuilt from an
//! allow-list (path, query, body, bearer) and presented under the separate
//! org-core principal `verevon-scim-edge` (scope `org:scim:any`), never as the
//! gateway's `verevon-gateway` principal. Supported surface: `ServiceProviderConfig`,
//! `ResourceTypes`, `Schemas` and `Users`; `Groups` is 501.
//!
//! While `SCIM_EDGE_ENABLED` is off, or the edge credential is not a secure secret,
//! every request gets the same 501 SCIM error the platform answered before SCIM
//! existed, and nothing is forwarded. The flag is a rollout control, never an
//! authorization input.

use std::time::Duration;

use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, RawQuery, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use serde_json::json;

use crate::config::AppState;

/// Largest request body the edge accepts; SCIM user documents are far smaller.
const MAX_BODY_BYTES: usize = 64 << 10;
const MAX_BEARER_LEN: usize = 256;
const MAX_QUERY_VALUE_LEN: usize = 256;
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(25);
const SCIM_CONTENT_TYPE: &str = "application/scim+json";
const SCIM_ERROR_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:Error";

pub(crate) fn router(_state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/scim/v2/{*rest}", any(scim_edge))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
}

fn scim_error(status: StatusCode, detail: &str) -> Response {
    let body = json!({
        "schemas": [SCIM_ERROR_SCHEMA],
        "status": status.as_u16().to_string(),
        "detail": detail,
    });
    let mut response = Response::new(Body::from(body.to_string()));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(SCIM_CONTENT_TYPE),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The answer while the edge is off: identical to the pre-SCIM 501.
fn not_available() -> Response {
    scim_error(
        StatusCode::NOT_IMPLEMENTED,
        "SCIM provisioning is not available. No change was applied.",
    )
}

/// An id segment is a UUID shape only; org-core checks it for real.
fn valid_id_segment(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// What the edge does with a path and method.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    /// Forward to org-core at this `/internal/scim/v2/...` tail.
    Forward(String),
    /// Groups: answered here, never forwarded.
    Groups,
    NotFound,
    MethodNotAllowed,
}

fn classify(method: &Method, rest: &str) -> Route {
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        ["ServiceProviderConfig"] | ["ResourceTypes"] | ["Schemas"] => {
            if method == Method::GET {
                Route::Forward(rest.to_owned())
            } else {
                Route::MethodNotAllowed
            }
        }
        ["Users"] => {
            if method == Method::GET || method == Method::POST {
                Route::Forward("Users".to_owned())
            } else {
                Route::MethodNotAllowed
            }
        }
        ["Users", id] if valid_id_segment(id) => {
            if method == Method::GET
                || method == Method::PUT
                || method == Method::PATCH
                || method == Method::DELETE
            {
                Route::Forward(format!("Users/{id}"))
            } else {
                Route::MethodNotAllowed
            }
        }
        ["Groups"] => Route::Groups,
        ["Groups", id] if valid_id_segment(id) => Route::Groups,
        _ => Route::NotFound,
    }
}

/// Only the SCIM list parameters are forwarded, each bounded.
fn forwarded_query(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return String::new();
    };
    let mut pairs: Vec<String> = Vec::new();
    for (key, value) in url_pairs(raw) {
        if !matches!(key.as_str(), "filter" | "startIndex" | "count") {
            continue;
        }
        if value.len() > MAX_QUERY_VALUE_LEN {
            continue;
        }
        pairs.push(format!(
            "{}={}",
            urlencoding::encode(&key),
            urlencoding::encode(&value)
        ));
    }
    if pairs.is_empty() {
        String::new()
    } else {
        format!("?{}", pairs.join("&"))
    }
}

fn url_pairs(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            let decode = |text: &str| {
                urlencoding::decode(&text.replace('+', " "))
                    .map(|decoded| decoded.into_owned())
                    .unwrap_or_default()
            };
            (decode(key), decode(value))
        })
        .collect()
}

/// The IdP's bearer token, verbatim, or `None` when the header is absent, not a
/// bearer credential, or oversized.
fn bearer_header(headers: &HeaderMap) -> Option<HeaderValue> {
    let value = headers.get(header::AUTHORIZATION)?;
    let text = value.to_str().ok()?.trim();
    if text.len() <= 7 || text.len() > MAX_BEARER_LEN || !text[..7].eq_ignore_ascii_case("bearer ") {
        return None;
    }
    HeaderValue::from_str(text).ok()
}

async fn scim_edge(
    State(state): State<AppState>,
    method: Method,
    Path(rest): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let edge = &state.space_features.scim_edge;
    if !edge.enabled || edge.token.is_empty() {
        return not_available();
    }
    let tail = match classify(&method, &rest) {
        Route::Forward(tail) => tail,
        Route::Groups => {
            return scim_error(StatusCode::NOT_IMPLEMENTED, "Groups are not supported.")
        }
        Route::NotFound => return scim_error(StatusCode::NOT_FOUND, "Resource not found."),
        Route::MethodNotAllowed => {
            return scim_error(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed.")
        }
    };
    let Some(bearer) = bearer_header(&headers) else {
        let mut response = scim_error(
            StatusCode::UNAUTHORIZED,
            "A valid SCIM bearer token is required.",
        );
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer realm=\"scim\""),
        );
        return response;
    };

    let url = format!(
        "{}/internal/scim/v2/{}{}",
        state.org_core_url,
        tail,
        forwarded_query(query.as_deref())
    );
    let mut request = state
        .client
        .request(method.clone(), &url)
        .timeout(UPSTREAM_TIMEOUT)
        .header("x-service-id", "verevon-scim-edge")
        .header("x-service-token", edge.token.as_str())
        .header(header::AUTHORIZATION, bearer)
        .header(header::ACCEPT, SCIM_CONTENT_TYPE);
    if method == Method::POST || method == Method::PUT || method == Method::PATCH {
        request = request
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_vec());
    }
    let upstream = match request.send().await {
        Ok(response) => response,
        Err(_) => {
            let mut response =
                scim_error(StatusCode::SERVICE_UNAVAILABLE, "SCIM is unavailable.");
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("30"));
            return response;
        }
    };

    let status = StatusCode::from_u16(upstream.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let mut passthrough: Vec<(HeaderName, HeaderValue)> = Vec::new();
    for name in [
        header::LOCATION,
        header::RETRY_AFTER,
        header::WWW_AUTHENTICATE,
    ] {
        if let Some(value) = upstream.headers().get(name.as_str()) {
            if let Ok(value) = HeaderValue::from_bytes(value.as_bytes()) {
                passthrough.push((name, value));
            }
        }
    }
    let payload = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return scim_error(StatusCode::BAD_GATEWAY, "SCIM is unavailable."),
    };
    let mut response = Response::new(Body::from(payload));
    *response.status_mut() = status;
    let out = response.headers_mut();
    out.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(SCIM_CONTENT_TYPE),
    );
    out.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    for (name, value) in passthrough {
        out.insert(name, value);
    }
    response
}
