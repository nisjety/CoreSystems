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

#[cfg(test)]
mod tests {
    //! WS05-S8 at the SCIM edge. The edge is session-less, so these drive the real router with no
    //! cookie against an org-core double. The double sees only what the edge forwards: its own
    //! principal, the identity provider's bearer and the allow-listed query.
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::config::ScimEdgeConfig;

    const EDGE_TOKEN: &str = "scim-edge-token-0123456789abcdef0123456789";
    const IDP_BEARER: &str = "Bearer idp-bearer-token-abcdefghijklmnop";

    fn armed_state(org_core: &MockServer) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.org_core_url = org_core.uri();
        state.space_features.scim_edge = ScimEdgeConfig {
            enabled: true,
            token: EDGE_TOKEN.to_owned(),
        };
        state
    }

    async fn org_core_serving_users() -> MockServer {
        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/internal/scim/v2/Users"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "Resources": [],
                "totalResults": 0
            })))
            .mount(&org_core)
            .await;
        org_core
    }

    async fn send(state: AppState, request: Request<Body>) -> (u16, axum::http::HeaderMap, Vec<u8>) {
        let response = crate::build_router(state)
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes()
            .to_vec();
        (status, headers, body)
    }

    async fn upstream_calls(org_core: &MockServer) -> Vec<wiremock::Request> {
        org_core.received_requests().await.expect("request recording")
    }

    #[test]
    fn the_path_and_method_table_forwards_only_the_scim_surface() {
        let get = Method::GET;
        assert_eq!(
            classify(&get, "ServiceProviderConfig"),
            Route::Forward("ServiceProviderConfig".to_owned())
        );
        assert_eq!(classify(&get, "ResourceTypes"), Route::Forward("ResourceTypes".to_owned()));
        assert_eq!(classify(&get, "Schemas"), Route::Forward("Schemas".to_owned()));
        assert_eq!(
            classify(&Method::POST, "ServiceProviderConfig"),
            Route::MethodNotAllowed
        );
        assert_eq!(classify(&get, "Users"), Route::Forward("Users".to_owned()));
        assert_eq!(classify(&Method::POST, "Users"), Route::Forward("Users".to_owned()));
        assert_eq!(classify(&Method::PUT, "Users"), Route::MethodNotAllowed);
        for method in [Method::GET, Method::PUT, Method::PATCH, Method::DELETE] {
            assert_eq!(
                classify(&method, "Users/abc-123"),
                Route::Forward("Users/abc-123".to_owned())
            );
        }
        assert_eq!(classify(&Method::POST, "Users/abc-123"), Route::MethodNotAllowed);
        assert_eq!(classify(&get, "Groups"), Route::Groups);
        assert_eq!(classify(&Method::POST, "Groups/abc-123"), Route::Groups);
        assert_eq!(classify(&get, "Users/bad%20id"), Route::NotFound);
        assert_eq!(classify(&get, "Users/a/b"), Route::NotFound);
        assert_eq!(classify(&get, "Bulk"), Route::NotFound);
        assert_eq!(classify(&get, ""), Route::NotFound);
    }

    #[test]
    fn only_the_list_parameters_are_forwarded_and_each_value_is_bounded() {
        assert_eq!(
            forwarded_query(Some(
                "filter=userName%20eq%20%22a%22&startIndex=1&count=20&attributes=secret&evil=1"
            )),
            "?filter=userName%20eq%20%22a%22&startIndex=1&count=20"
        );
        assert_eq!(forwarded_query(Some("attributes=secret&evil=1")), "");
        assert_eq!(forwarded_query(None), "");
        let too_long = "x".repeat(MAX_QUERY_VALUE_LEN + 1);
        assert_eq!(forwarded_query(Some(&format!("filter={too_long}"))), "");
        let at_the_limit = "x".repeat(MAX_QUERY_VALUE_LEN);
        assert_eq!(
            forwarded_query(Some(&format!("count={at_the_limit}"))),
            format!("?count={at_the_limit}")
        );
    }

    #[test]
    fn the_identity_providers_bearer_is_taken_verbatim_or_refused() {
        let mut headers = HeaderMap::new();
        assert!(bearer_header(&headers).is_none());
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic dXNlcjpwYXNz"),
        );
        assert!(bearer_header(&headers).is_none());
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert!(bearer_header(&headers).is_none());
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bearer idp-token-1"),
        );
        assert_eq!(
            bearer_header(&headers).expect("a bearer").to_str().unwrap(),
            "bearer idp-token-1"
        );
        let oversized = format!("Bearer {}", "t".repeat(MAX_BEARER_LEN));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&oversized).unwrap(),
        );
        assert!(bearer_header(&headers).is_none());
    }

    #[tokio::test]
    async fn the_edge_is_501_and_calls_nothing_while_off_or_unarmed() {
        let org_core = org_core_serving_users().await;
        for (enabled, token) in [(false, EDGE_TOKEN), (true, "")] {
            let mut state = armed_state(&org_core);
            state.space_features.scim_edge = ScimEdgeConfig {
                enabled,
                token: token.to_owned(),
            };
            let request = Request::builder()
                .method("GET")
                .uri("/api/v1/scim/v2/Users")
                .header("authorization", IDP_BEARER)
                .body(Body::empty())
                .unwrap();
            let (status, _, body) = send(state, request).await;
            assert_eq!(status, 501, "enabled={enabled} token_set={}", !token.is_empty());
            let body: Value = serde_json::from_slice(&body).expect("a SCIM error");
            assert_eq!(body["status"], "501");
        }
        assert!(
            upstream_calls(&org_core).await.is_empty(),
            "nothing is forwarded while the edge is not armed"
        );
    }

    #[tokio::test]
    async fn a_request_without_an_idp_bearer_is_401_and_calls_nothing() {
        let org_core = org_core_serving_users().await;
        let request = Request::builder()
            .method("GET")
            .uri("/api/v1/scim/v2/Users")
            .body(Body::empty())
            .unwrap();
        let (status, headers, body) = send(armed_state(&org_core), request).await;
        assert_eq!(status, 401);
        let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
        assert_eq!(text("www-authenticate").as_deref(), Some("Bearer realm=\"scim\""));
        assert_eq!(text("content-type").as_deref(), Some(SCIM_CONTENT_TYPE));
        assert_eq!(text("cache-control").as_deref(), Some("no-store"));
        let body: Value = serde_json::from_slice(&body).expect("a SCIM error");
        assert_eq!(body["schemas"][0], SCIM_ERROR_SCHEMA);
        assert!(upstream_calls(&org_core).await.is_empty());
    }

    #[tokio::test]
    async fn the_upstream_request_carries_only_the_edge_identity_and_the_idp_bearer() {
        let org_core = org_core_serving_users().await;
        let request = Request::builder()
            .method("GET")
            .uri("/api/v1/scim/v2/Users?filter=userName%20eq%20%22a%22&attributes=secret")
            .header("authorization", IDP_BEARER)
            .header("x-org-id", "org-attacker")
            .header("x-service-id", "verevon-gateway")
            .header("x-service-token", "forged-service-token")
            .header("x-user-id", "user-forged")
            .header("cookie", "better-auth.session_token=session-1")
            .body(Body::empty())
            .unwrap();
        let (status, _, _) = send(armed_state(&org_core), request).await;
        assert_eq!(status, 200);
        let calls = upstream_calls(&org_core).await;
        assert_eq!(calls.len(), 1, "exactly one upstream call");
        let sent = &calls[0];
        let header = |name: &str| {
            sent.headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        assert_eq!(header("x-service-id").as_deref(), Some("verevon-scim-edge"));
        assert_eq!(header("x-service-token").as_deref(), Some(EDGE_TOKEN));
        assert_eq!(header("authorization").as_deref(), Some(IDP_BEARER));
        for forged in ["x-org-id", "x-user-id", "cookie"] {
            assert!(header(forged).is_none(), "{forged} must never reach org-core");
        }
        assert_eq!(sent.url.query(), Some("filter=userName%20eq%20%22a%22"));
    }

    #[tokio::test]
    async fn groups_are_answered_here_and_never_forwarded() {
        let org_core = org_core_serving_users().await;
        let request = Request::builder()
            .method("GET")
            .uri("/api/v1/scim/v2/Groups")
            .header("authorization", IDP_BEARER)
            .body(Body::empty())
            .unwrap();
        let (status, _, body) = send(armed_state(&org_core), request).await;
        assert_eq!(status, 501);
        let body: Value = serde_json::from_slice(&body).expect("a SCIM error");
        assert_eq!(body["detail"], "Groups are not supported.");
        assert!(upstream_calls(&org_core).await.is_empty());
    }

    #[tokio::test]
    async fn a_body_over_64_kib_is_refused_before_any_upstream_call() {
        let org_core = org_core_serving_users().await;
        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/scim/v2/Users")
            .header("authorization", IDP_BEARER)
            .header("content-type", "application/json")
            .body(Body::from(vec![b' '; MAX_BODY_BYTES + 1]))
            .unwrap();
        let (status, _, _) = send(armed_state(&org_core), request).await;
        assert_eq!(status, 413);
        assert!(upstream_calls(&org_core).await.is_empty());
    }
}
