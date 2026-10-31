//! Signed calls from the edge to control's job resources (WS15.L1).
//!
//! The lifecycle reads (`/v1/jobs/{id}/summary`, `/errors`, `/ongoing`) are
//! org-scoped: the verified org travels as the `org_id` query parameter, and the
//! request is signed with the edge's internal HMAC over the exact path and query
//! it sends (`crate::internal_auth`). Control's error envelope is mapped onto the
//! edge's typed error by status; control's body is never echoed to the caller,
//! because it can carry internal detail.

use std::time::Duration;

use serde_json::Value;

use quarry_core::error::{ErrorCode, QuarryError};

use crate::state::AppState;

/// Maps a control status onto the edge's typed error. The mapping follows the
/// mutation path of `resource_routes::forward_mutation`: 404 is NotFound, 400 is
/// BadRequest, 401 and 403 are Forbidden, 409 is Conflict, and anything else is a
/// driver failure. An unreadable or unexpected body maps the same way.
pub fn map_status(status: reqwest::StatusCode) -> QuarryError {
    match status.as_u16() {
        400 => QuarryError::new(ErrorCode::BadRequest, "control rejected the request"),
        401 | 403 => QuarryError::new(ErrorCode::Forbidden, "control refused the request"),
        404 => QuarryError::new(ErrorCode::NotFound, "not found"),
        409 => QuarryError::new(ErrorCode::Conflict, "the job changed; read it again"),
        _ => QuarryError::new(ErrorCode::DriverFailed, "control request failed"),
    }
}

fn client() -> Result<reqwest::Client, QuarryError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {}", e.without_url())))
}

/// The control URL for `path` with the verified org and any extra query pairs.
fn control_url(state: &AppState, path: &str, org_id: &str, query: &[(&str, String)]) -> Result<reqwest::Url, QuarryError> {
    if state.control_base_url.is_empty() {
        return Err(QuarryError::new(ErrorCode::Internal, "control base url empty"));
    }
    let mut url = reqwest::Url::parse(&format!("{}{}", state.control_base_url.trim_end_matches('/'), path))
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("control url: {e}")))?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("org_id", org_id);
        for (key, value) in query {
            pairs.append_pair(key, value);
        }
    }
    Ok(url)
}

/// The signature headers for one request, when the edge has a signer.
fn signed_headers(
    state: &AppState,
    method: &str,
    url: &reqwest::Url,
    body: &[u8],
) -> Vec<(&'static str, String)> {
    let Some(signer) = state.internal_signer.as_deref() else {
        return Vec::new();
    };
    let path_q = match url.query() {
        Some(query) => format!("{}?{}", url.path(), query),
        None => url.path().to_string(),
    };
    let signed = crate::internal_auth::apply_to_request(signer, method, &path_q, body);
    vec![
        (crate::internal_auth::HEADER_SIG, signed.signature),
        (crate::internal_auth::HEADER_TS, signed.timestamp),
        (crate::internal_auth::HEADER_NONCE, signed.nonce),
    ]
}

/// Reads one control resource as JSON. Control wraps its answers in `{data}`;
/// that wrapper is removed here, so the caller gets the resource itself.
pub async fn get_json(
    state: &AppState,
    path: &str,
    org_id: &str,
    query: &[(&str, String)],
) -> Result<Value, QuarryError> {
    let url = control_url(state, path, org_id, query)?;
    let mut request = client()?.get(url.clone());
    for (name, value) in signed_headers(state, "GET", &url, b"") {
        request = request.header(name, value);
    }
    let resp = request.send().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::UpstreamBlocked,
            format!("control unreachable: {}", e.without_url()),
        )
    })?;
    let status = resp.status();
    if !status.is_success() {
        return Err(map_status(status));
    }
    let raw: Value = resp
        .json()
        .await
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "control response unreadable"))?;
    Ok(raw.get("data").cloned().unwrap_or(raw))
}

/// Sends a JSON mutation to control (POST, PUT) and returns the resource it
/// answers with. Used by the lifecycle writes of WS15.L2; the signature covers
/// the exact body and query sent.
#[allow(dead_code)]
pub async fn send_json(
    state: &AppState,
    method: reqwest::Method,
    path: &str,
    org_id: &str,
    body: &Value,
    idempotency_key: Option<&str>,
) -> Result<Value, QuarryError> {
    let url = control_url(state, path, org_id, &[])?;
    let body_bytes = serde_json::to_vec(body)
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("encode control body: {e}")))?;
    let method_name = method.as_str().to_string();
    let mut request = client()?
        .request(method, url.clone())
        .header("content-type", "application/json")
        .body(body_bytes.clone());
    if let Some(key) = idempotency_key {
        request = request.header("idempotency-key", key);
    }
    for (name, value) in signed_headers(state, &method_name, &url, &body_bytes) {
        request = request.header(name, value);
    }
    let resp = request.send().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::UpstreamBlocked,
            format!("control unreachable: {}", e.without_url()),
        )
    })?;
    let status = resp.status();
    if !status.is_success() {
        return Err(map_status(status));
    }
    let raw: Value = resp
        .json()
        .await
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "control response unreadable"))?;
    Ok(raw.get("data").cloned().unwrap_or(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_statuses_map_to_the_typed_errors() {
        assert_eq!(map_status(reqwest::StatusCode::NOT_FOUND).code, ErrorCode::NotFound);
        assert_eq!(map_status(reqwest::StatusCode::BAD_REQUEST).code, ErrorCode::BadRequest);
        assert_eq!(map_status(reqwest::StatusCode::FORBIDDEN).code, ErrorCode::Forbidden);
        assert_eq!(map_status(reqwest::StatusCode::UNAUTHORIZED).code, ErrorCode::Forbidden);
        assert_eq!(map_status(reqwest::StatusCode::CONFLICT).code, ErrorCode::Conflict);
        assert_eq!(map_status(reqwest::StatusCode::BAD_GATEWAY).code, ErrorCode::DriverFailed);
    }

    #[test]
    fn the_mapped_error_never_carries_the_control_body() {
        let rendered = serde_json::to_string(&map_status(reqwest::StatusCode::NOT_FOUND)).unwrap();
        assert!(!rendered.contains("internal_dsn"));
    }
}
