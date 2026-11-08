//! One credentialless redirect hop for assistant navigation.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

use crate::{auth::Claims, state::AppState, web_policy_edge};

const TIMEOUT: Duration = Duration::from_secs(4);
static CAPACITY: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(16);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RedirectProbeRequest {
    url: String,
}

#[derive(Serialize)]
struct RedirectProbeResult {
    target: Option<String>,
}

pub(crate) async fn probe(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<RedirectProbeRequest>,
) -> Response {
    let request_id = crate::request_id::current();
    let result = async {
        let _permit = CAPACITY.try_acquire().map_err(|_| {
            QuarryError::new(ErrorCode::RateLimited, "redirect probe capacity is busy")
        })?;
        tokio::time::timeout(TIMEOUT, probe_inner(&state, &claims, request))
            .await
            .map_err(|_| QuarryError::new(ErrorCode::Timeout, "redirect probe deadline"))?
    }
    .await;
    match result {
        Ok(target) => {
            Json(Envelope::ok(request_id, RedirectProbeResult { target })).into_response()
        }
        Err(error) => (
            StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(Envelope::<()>::err(request_id, error)),
        )
            .into_response(),
    }
}

async fn probe_inner(
    state: &AppState,
    claims: &Claims,
    request: RedirectProbeRequest,
) -> QuarryResult<Option<String>> {
    let blocked = || QuarryError::new(ErrorCode::SecurityBlocked, "redirect probe URL blocked");
    if request.url.len() > 4096 {
        return Err(blocked());
    }
    let mut url = Url::parse(request.url.trim()).map_err(|_| blocked())?;
    url.set_fragment(None);
    quarry_runtime::fetch::validate_redirect_probe_url(&url)?;
    let policy = web_policy_edge::decide(&state.policy, claims, claims.effective_zdr(false), None)?;
    web_policy_edge::gate_url(&policy, url.as_str())?;
    if state.security.preflight(&url).await.decision != quarry_security::Decision::Allow {
        return Err(blocked());
    }
    let target = state.driver.probe_redirect(&url).await.map_err(|error| {
        // DNS and transport errors can contain internal names or addresses.
        QuarryError::new(
            error.code,
            "redirect probe could not resolve the public URL",
        )
    })?;
    if let Some(target) = target.as_ref() {
        web_policy_edge::gate_url(&policy, target.as_str())?;
    }
    Ok(target.map(|target| target.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quarry_core::output::DriverKind;
    use quarry_runtime::{driver::Driver, fetch::FetchResponse};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct ProbeDriver(AtomicUsize);

    #[async_trait]
    impl Driver for ProbeDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }

        async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
            panic!("redirect probe must not fetch page content")
        }

        async fn probe_redirect(&self, url: &Url) -> QuarryResult<Option<Url>> {
            self.0.fetch_add(1, Ordering::Relaxed);
            assert_eq!(url.as_str(), "https://example.com/");
            Ok(Some(Url::parse("https://example.net/destination").unwrap()))
        }
    }

    #[tokio::test]
    async fn redirect_probe_route_returns_only_the_destination_and_blocks_local_input() {
        let driver = Arc::new(ProbeDriver(AtomicUsize::new(0)));
        let state = crate::test_support::test_state(driver.clone());
        let claims = crate::test_support::claims_for_org("alpha");
        let response = probe(
            State(state.clone()),
            Extension(claims.clone()),
            Json(RedirectProbeRequest {
                url: "https://example.com/#fragment".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = crate::test_support::response_json(response).await;
        assert_eq!(
            body["data"],
            serde_json::json!({ "target": "https://example.net/destination" })
        );

        let response = probe(
            State(state),
            Extension(claims),
            Json(RedirectProbeRequest {
                url: "http://127.0.0.1/".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(driver.0.load(Ordering::Relaxed), 1);
    }
}
