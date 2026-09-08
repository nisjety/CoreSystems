//! Remote-support domain — hands the SPA the connection details for the
//! RustDesk-compatible Support Plane (`apps/Support Plane`: hbbs/hbbr behind
//! TLS termination), so `@verevon/remote-core` never hardcodes them.
//!
//! This is a read of infrastructure configuration, not a session operation.
//! The live remote-desktop session itself runs browser→hbbs/hbbr over WSS and
//! never transits this gateway — that is the whole point of the design (see
//! `packages/remote-core/docs/architecture.md`). `server_public_key` is
//! RustDesk's public access key: public by design, safe to hand to the
//! browser. No secret leaves this handler.
//!
//! Deliberately NOT here yet: a durable "remote session started" record
//! (audit / session metadata). That needs an owning core to persist it and an
//! action-registry contract with a real dispatcher. Until one exists, the SPA
//! says so honestly rather than this domain pretending to record sessions —
//! per the repo's "honest empty state over fabricated demo state" rule.
//!
//! `configured` is strict on purpose: it requires BOTH a rendezvous URL and
//! the server public key, because without the key the browser cannot
//! authenticate the peer and the product UI must never fall back to an
//! unverified handshake (`docs/security.md` in remote-core).

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;

use crate::{config::AppState, envelope::ok, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/remote-support/config", get(get_config))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteSupportConfig {
    /// True only when a browser session can be established AND authenticated.
    configured: bool,
    rendezvous_url: Option<String>,
    relay_url: Option<String>,
    server_public_key: Option<String>,
    /// Human-readable reasons `configured` is false; empty when it is true.
    missing: Vec<&'static str>,
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

pub(crate) fn build_config(state: &AppState) -> RemoteSupportConfig {
    let rendezvous_url = non_empty(&state.remote_support_rendezvous_url);
    let relay_url = non_empty(&state.remote_support_relay_url);
    let server_public_key = non_empty(&state.remote_support_server_public_key);

    let mut missing = Vec::new();
    if rendezvous_url.is_none() {
        missing.push("REMOTE_SUPPORT_RENDEZVOUS_URL");
    }
    if server_public_key.is_none() {
        missing.push("REMOTE_SUPPORT_SERVER_PUBLIC_KEY");
    }

    RemoteSupportConfig {
        configured: missing.is_empty(),
        rendezvous_url,
        relay_url,
        server_public_key,
        missing,
    }
}

async fn get_config(State(state): State<AppState>) -> impl IntoResponse {
    Json(ok(build_config(&state)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    fn state_with(rendezvous: &str, relay: &str, key: &str) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.remote_support_rendezvous_url = rendezvous.to_owned();
        state.remote_support_relay_url = relay.to_owned();
        state.remote_support_server_public_key = key.to_owned();
        state
    }

    #[test]
    fn fully_configured_reports_configured_with_nothing_missing() {
        let config = build_config(&state_with(
            "wss://remote.example.com",
            "wss://relay.example.com",
            "WFWFmTBXVXsKoNuvFVU9jQerGLMsI9Y5HxvCmrW+luc=",
        ));

        assert!(config.configured);
        assert!(config.missing.is_empty());
        assert_eq!(
            config.rendezvous_url.as_deref(),
            Some("wss://remote.example.com")
        );
        assert_eq!(config.relay_url.as_deref(), Some("wss://relay.example.com"));
    }

    #[test]
    fn relay_url_is_optional_for_configured() {
        // The rendezvous server can advertise the relay itself, so the relay
        // override is a convenience, not a requirement.
        let config = build_config(&state_with("wss://remote.example.com", "", "key"));
        assert!(config.configured);
        assert_eq!(config.relay_url, None);
    }

    #[test]
    fn missing_public_key_is_not_configured_even_with_urls() {
        // Without the key the peer cannot be authenticated, and the product UI
        // must never fall back to an unverified handshake.
        let config = build_config(&state_with(
            "wss://remote.example.com",
            "wss://relay.example.com",
            "",
        ));
        assert!(!config.configured);
        assert_eq!(config.missing, vec!["REMOTE_SUPPORT_SERVER_PUBLIC_KEY"]);
    }

    #[test]
    fn nothing_configured_names_every_missing_setting() {
        let config = build_config(&state_with("", "", ""));
        assert!(!config.configured);
        assert_eq!(
            config.missing,
            vec![
                "REMOTE_SUPPORT_RENDEZVOUS_URL",
                "REMOTE_SUPPORT_SERVER_PUBLIC_KEY"
            ]
        );
        assert_eq!(config.rendezvous_url, None);
    }

    #[test]
    fn whitespace_only_values_count_as_unset() {
        let config = build_config(&state_with("   ", "", "  "));
        assert!(!config.configured);
        assert_eq!(config.rendezvous_url, None);
    }

    #[tokio::test]
    async fn unauthenticated_request_is_rejected_before_reaching_the_handler() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let state = state_with("wss://remote.example.com", "", "key");

        let request = Request::builder()
            .method("GET")
            .uri("/api/v1/remote-support/config")
            .body(Body::empty())
            .unwrap();
        let response = crate::build_router(state).oneshot(request).await.unwrap();

        // No session cookie at all: the config must not leak to an anonymous
        // caller, whether the middleware answers 401 or (auth-core unreachable
        // in a unit test) 503. Either way it is never 200.
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        assert_ne!(status, 200, "{body}");
        assert!(
            body.get("data").is_none(),
            "config leaked to anonymous caller: {body}"
        );
    }
}
