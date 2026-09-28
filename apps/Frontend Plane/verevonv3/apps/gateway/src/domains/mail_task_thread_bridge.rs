//! Delegated-identity design, Gap B (§2.4(ii)) — the reqwest-based client a
//! real [`super::mail_task::MailTaskPipeline`] implementation uses to reach
//! model-gateway's internal HTTP→gRPC bridge
//! (`POST /v1/internal/mail-task/prepare-thread`), which wraps session-core's
//! `PrepareUserOwnedThread`.
//!
//! **Not yet wired into `AppState.mail_task_pipeline`.** This module is
//! exactly the "reqwest-based minting helper mirroring this crate's existing
//! `audience_tokens.rs` pattern" the design calls for (§2.4(ii)) — real,
//! tested, and ready to be called from a `MailTaskPipeline::run`
//! implementation's thread-preparation step. It deliberately stops there:
//! `MailTaskPipeline::run` must also post the synthesized first chat turn and
//! invoke exactly one execution-core turn on the prepared thread — work that
//! has no unattended (no-live-user) precedent anywhere in this codebase yet
//! and is explicitly out of scope for this design (see `mail_task.rs`'s
//! module doc and `03-design-2-token-exchange.md` §2.11's "Explicitly out of
//! scope" note). Wiring a partial pipeline that only prepares a thread but
//! never posts a turn would make the endpoint silently ack an email as
//! processed when nothing was actually drafted — worse than the current,
//! honest `503 mail_task_pipeline_not_configured` default, which Postmark's
//! webhook retry correctly keeps retrying. `AppState.mail_task_pipeline`
//! therefore stays `None` in every build until that larger piece lands.

#![allow(
    dead_code,
    reason = "not yet called by a real MailTaskPipeline implementation, see this module's doc comment for why"
)]

use serde::Deserialize;

/// Mints this crate's own `aud=model-gateway` service-principal credential
/// via auth-core's existing `POST /api/model-plane/internal-token` — the
/// same generic service-principal endpoint `model-plane-token.controller.ts`
/// already exposes, reusing `authorizePlaneServicePrincipal`. Absent by
/// default: no currently-registered `verevon-gateway` principal exists in
/// auth-core's `PLANE_SERVICE_PRINCIPALS_JSON` until a deployment adds one
/// with `audiences: ["model-gateway"]`.
#[derive(Debug)]
pub(crate) enum ThreadBridgeError {
    #[allow(dead_code, reason = "constructed by from_env's caller-facing configuration check; kept for a future MailTaskPipeline implementation")]
    Configuration(&'static str),
    CredentialTransport(String),
    CredentialRefused(reqwest::StatusCode),
    InvalidCredential,
    BridgeTransport(String),
    BridgeRefused(reqwest::StatusCode),
    InvalidResponse,
}

impl std::fmt::Display for ThreadBridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(name) => write!(f, "mail-task thread bridge is not configured: {name}"),
            Self::CredentialTransport(error) => {
                write!(f, "auth-core mail-task bridge credential request failed: {error}")
            }
            Self::CredentialRefused(status) => {
                write!(f, "auth-core refused the mail-task bridge credential ({status})")
            }
            Self::InvalidCredential => write!(f, "auth-core returned an invalid mail-task bridge credential"),
            Self::BridgeTransport(error) => write!(f, "model-gateway mail-task bridge request failed: {error}"),
            Self::BridgeRefused(status) => {
                write!(f, "model-gateway refused the mail-task thread-prepare request ({status})")
            }
            Self::InvalidResponse => {
                write!(f, "model-gateway returned an invalid mail-task thread-prepare response")
            }
        }
    }
}

impl std::error::Error for ThreadBridgeError {}

#[derive(Deserialize)]
struct CredentialResponse {
    token: String,
}

#[derive(Deserialize)]
struct PrepareThreadResponse {
    thread_id: String,
}

/// One resolved mail-task alias's binding, exactly what's needed to call
/// `PrepareUserOwnedThread` through the bridge. `control_decision_token` is
/// forwarded opaquely — this client never inspects it.
pub(crate) struct PrepareThreadRequest<'a> {
    pub(crate) org_id: &'a str,
    pub(crate) user_id: &'a str,
    pub(crate) session_key: &'a str,
    pub(crate) control_decision_token: &'a str,
}

pub(crate) struct ThreadBridgeClient {
    auth_core_url: String,
    model_gateway_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
}

impl ThreadBridgeClient {
    /// `None` when `MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY` is unset —
    /// the expected state until an operator provisions the
    /// `verevon-gateway`@`aud=model-gateway` registry row (deploy step,
    /// not shipped by this code change).
    pub(crate) fn from_env(auth_core_url: &str, model_gateway_url: &str, http: reqwest::Client) -> Option<Self> {
        let credential = std::env::var("MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let service_id = std::env::var("MAIL_TASK_GATEWAY_BRIDGE_SERVICE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "verevon-gateway".to_owned());
        Some(Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            model_gateway_url: model_gateway_url.trim_end_matches('/').to_owned(),
            service_id,
            credential,
            http,
        })
    }

    #[cfg(test)]
    fn new_for_test(
        auth_core_url: &str,
        model_gateway_url: &str,
        service_id: &str,
        credential: &str,
    ) -> Self {
        Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            model_gateway_url: model_gateway_url.trim_end_matches('/').to_owned(),
            service_id: service_id.to_owned(),
            credential: credential.to_owned(),
            http: reqwest::Client::new(),
        }
    }

    async fn mint_credential(&self, org_id: &str) -> Result<String, ThreadBridgeError> {
        let response = self
            .http
            .post(format!("{}/api/model-plane/internal-token", self.auth_core_url))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", &self.credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": ["mail-task:prepare-thread"],
                "reason": "verevon-gateway mail-task thread bridge",
            }))
            .send()
            .await
            .map_err(|error| ThreadBridgeError::CredentialTransport(error.to_string()))?;
        if !response.status().is_success() {
            return Err(ThreadBridgeError::CredentialRefused(response.status()));
        }
        let bundle = response
            .json::<CredentialResponse>()
            .await
            .map_err(|_| ThreadBridgeError::InvalidCredential)?;
        if bundle.token.trim().is_empty() {
            return Err(ThreadBridgeError::InvalidCredential);
        }
        Ok(bundle.token)
    }

    /// Prepares (creates or reuses) a user-owned, `origin="mail_task"`
    /// thread and returns its `thread_id`. Mints a fresh credential on every
    /// call — this is not a hot path (one call per inbound email, not per
    /// request), so the added simplicity of no cache is worth it over the
    /// multi-caller cache `session_terminal_auth.rs`'s model-gateway-side
    /// counterpart needs.
    pub(crate) async fn prepare_thread(
        &self,
        request: PrepareThreadRequest<'_>,
    ) -> Result<String, ThreadBridgeError> {
        let credential = self.mint_credential(request.org_id).await?;
        let response = self
            .http
            .post(format!(
                "{}/v1/internal/mail-task/prepare-thread",
                self.model_gateway_url
            ))
            .bearer_auth(credential)
            .json(&serde_json::json!({
                "org_id": request.org_id,
                "user_id": request.user_id,
                "origin": "mail_task",
                "session_key": request.session_key,
                "control_decision_token": request.control_decision_token,
            }))
            .send()
            .await
            .map_err(|error| ThreadBridgeError::BridgeTransport(error.to_string()))?;
        if !response.status().is_success() {
            return Err(ThreadBridgeError::BridgeRefused(response.status()));
        }
        let bundle = response
            .json::<PrepareThreadResponse>()
            .await
            .map_err(|_| ThreadBridgeError::InvalidResponse)?;
        if bundle.thread_id.trim().is_empty() {
            return Err(ThreadBridgeError::InvalidResponse);
        }
        Ok(bundle.thread_id)
    }
}

#[cfg(test)]
mod tests {
    use super::{PrepareThreadRequest, ThreadBridgeClient, ThreadBridgeError};
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn prepares_a_thread_using_a_freshly_minted_model_gateway_credential() {
        let auth_core = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/model-plane/internal-token"))
            .and(header("x-service-id", "verevon-gateway"))
            .and(header("x-service-api-key", "bridge-secret"))
            .and(body_json(serde_json::json!({
                "orgId": "org-1",
                "scopes": ["mail-task:prepare-thread"],
                "reason": "verevon-gateway mail-task thread bridge",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "model-gateway-bridge-token",
            })))
            .expect(1)
            .mount(&auth_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/internal/mail-task/prepare-thread"))
            .and(header("authorization", "Bearer model-gateway-bridge-token"))
            .and(body_json(serde_json::json!({
                "org_id": "org-1",
                "user_id": "user-1",
                "origin": "mail_task",
                "session_key": "mail_task/abc",
                "control_decision_token": "mail-task-v1.k.p.s",
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"thread_id": "thread-1"})),
            )
            .expect(1)
            .mount(&model_gateway)
            .await;

        let client = ThreadBridgeClient::new_for_test(
            &auth_core.uri(),
            &model_gateway.uri(),
            "verevon-gateway",
            "bridge-secret",
        );
        let thread_id = client
            .prepare_thread(PrepareThreadRequest {
                org_id: "org-1",
                user_id: "user-1",
                session_key: "mail_task/abc",
                control_decision_token: "mail-task-v1.k.p.s",
            })
            .await
            .expect("prepare_thread succeeds");
        assert_eq!(thread_id, "thread-1");
    }

    #[tokio::test]
    async fn fails_closed_when_auth_core_refuses_the_bridge_credential() {
        let auth_core = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/model-plane/internal-token"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&auth_core)
            .await;

        let client = ThreadBridgeClient::new_for_test(
            &auth_core.uri(),
            &model_gateway.uri(),
            "verevon-gateway",
            "bridge-secret",
        );
        let error = client
            .prepare_thread(PrepareThreadRequest {
                org_id: "org-1",
                user_id: "user-1",
                session_key: "mail_task/abc",
                control_decision_token: "mail-task-v1.k.p.s",
            })
            .await
            .expect_err("a refused credential must fail closed");
        assert!(matches!(error, ThreadBridgeError::CredentialRefused(_)));
    }

    #[tokio::test]
    async fn fails_closed_when_model_gateway_refuses_the_bridge_request() {
        let auth_core = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/model-plane/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "model-gateway-bridge-token",
            })))
            .mount(&auth_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/internal/mail-task/prepare-thread"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&model_gateway)
            .await;

        let client = ThreadBridgeClient::new_for_test(
            &auth_core.uri(),
            &model_gateway.uri(),
            "verevon-gateway",
            "bridge-secret",
        );
        let error = client
            .prepare_thread(PrepareThreadRequest {
                org_id: "org-1",
                user_id: "user-1",
                session_key: "mail_task/abc",
                control_decision_token: "mail-task-v1.k.p.s",
            })
            .await
            .expect_err("a refused bridge request must fail closed");
        assert!(matches!(error, ThreadBridgeError::BridgeRefused(_)));
    }

    #[test]
    fn is_absent_without_the_bridge_service_credential_env_var() {
        let previous = std::env::var("MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY").ok();
        std::env::remove_var("MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY");
        let client = ThreadBridgeClient::from_env(
            "http://auth-core.invalid",
            "http://model-gateway.invalid",
            reqwest::Client::new(),
        );
        assert!(client.is_none());
        if let Some(value) = previous {
            std::env::set_var("MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY", value);
        }
    }
}
