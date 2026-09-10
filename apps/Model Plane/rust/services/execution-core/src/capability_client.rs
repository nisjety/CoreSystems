//! HTTP client for Control Plane's signed Space sandbox capability decision
//! endpoint (`POST /api/v1/internal/spaces/sandbox-capability-decision`).
//! See apps/Frontend Plane/verevonv3/docs/S3_2_SANDBOX_LEASE_CLOSEOUT_DESIGN_2026-09-10.md.
//!
//! Absent configuration disables this client entirely (`from_env` returns
//! `None`) rather than defaulting to some fallback behavior: a Space
//! capability profile without an attached decision is a fully valid, honest
//! response (`/capability-profile`'s `decision` field is `Option`) — this
//! process must never fabricate a signature it cannot obtain from Control.

use std::time::Duration;

use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};

use crate::control_http_client::{
    bounded_secret, env_flag, env_value, service_base_url, service_endpoint,
};
use crate::sandbox::{capability_profile_digest, SandboxCapabilityProfile};

const CONTROL_SANDBOX_CAPABILITY_PATH: &str =
    "/api/v1/internal/spaces/sandbox-capability-decision";
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize)]
struct DecisionRequest<'a> {
    intent: DecisionIntent<'a>,
}

#[derive(Debug, Serialize)]
struct DecisionIntent<'a> {
    org_id: &'a str,
    space_ref: &'a str,
    subject_id: &'a str,
    backend_id: &'a str,
    profile_digest: &'a str,
    persistence: &'a str,
    processes: &'a str,
    backup: bool,
    egress: &'a str,
    credential_mode: &'a str,
    idempotency_key: &'a str,
}

#[derive(Debug, Deserialize)]
struct DecisionResponseEnvelope {
    data: DecisionResponseData,
}

#[derive(Debug, Deserialize)]
struct DecisionResponseData {
    decision: String,
}

/// Everything a caller needs to request one signed capability decision for
/// one Space, backend, and measured profile.
pub struct CapabilityDecisionRequest<'a> {
    pub org_id: &'a str,
    pub space_ref: &'a str,
    pub subject_id: &'a str,
    pub backend_id: &'a str,
    pub profile: &'a SandboxCapabilityProfile,
    pub idempotency_key: &'a str,
}

pub struct CapabilityClient {
    http: Client,
    base_url: Url,
    service_token: String,
}

impl CapabilityClient {
    /// Builds from `CONTROL_PLANE_USER_CORE_URL` (shared with the ticket
    /// action adapter in `ticket_tools.rs` — same Control Plane, a
    /// different endpoint) and
    /// `EXECUTION_CORE_CONTROL_SANDBOX_CAPABILITY_TOKEN`. Returns `None`
    /// when either is absent: a Space capability profile without an
    /// attached decision is a valid response, not an error.
    ///
    /// # Errors
    /// Returns an error only for a *present but malformed* configuration
    /// (invalid URL, wrong-length token, unbuildable HTTP client) — never
    /// for absence, which callers must not confuse with misconfiguration.
    pub fn from_env() -> Result<Option<Self>, String> {
        let Some(base_url) = env_value("CONTROL_PLANE_USER_CORE_URL") else {
            return Ok(None);
        };
        let Some(service_token) = env_value("EXECUTION_CORE_CONTROL_SANDBOX_CAPABILITY_TOKEN")
        else {
            return Ok(None);
        };
        let allow_insecure_loopback =
            env_flag("EXECUTION_CORE_ALLOW_INSECURE_SANDBOX_CAPABILITY_LOOPBACK");
        Self::new_with_transport(&base_url, &service_token, allow_insecure_loopback).map(Some)
    }

    fn new_with_transport(
        base_url: &str,
        service_token: &str,
        allow_insecure_loopback: bool,
    ) -> Result<Self, String> {
        let base_url = service_base_url(
            base_url,
            "Control Plane sandbox capability",
            allow_insecure_loopback,
        )?;
        let service_token =
            bounded_secret(service_token, "Control sandbox capability service token")?;
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|error| format!("sandbox capability HTTP client: {error}"))?;
        Ok(Self {
            http,
            base_url,
            service_token,
        })
    }

    /// Requests a signed decision binding `request`'s measured profile to
    /// its backend and Space. Returns the opaque decision token; the caller
    /// already holds every field the recipient needs to reconstruct the
    /// claims sidecar (profile + backend_id), so this does not also parse
    /// and return a claims struct.
    ///
    /// # Errors
    /// Returns an error for any transport failure, non-2xx response, or
    /// malformed response body. Callers treat this the same as "no
    /// decision available" — see `http_health::capability_profile`'s doc
    /// comment — never as a reason to fail health/profile reporting.
    pub async fn request_decision(
        &self,
        request: &CapabilityDecisionRequest<'_>,
    ) -> Result<String, String> {
        let endpoint = service_endpoint(&self.base_url, CONTROL_SANDBOX_CAPABILITY_PATH)?;
        let profile_digest = capability_profile_digest(request.profile);
        let body = DecisionRequest {
            intent: DecisionIntent {
                org_id: request.org_id,
                space_ref: request.space_ref,
                subject_id: request.subject_id,
                backend_id: request.backend_id,
                profile_digest: &profile_digest,
                persistence: request.profile.persistence,
                processes: request.profile.processes,
                backup: request.profile.backup,
                egress: request.profile.egress,
                credential_mode: request.profile.credential_mode,
                idempotency_key: request.idempotency_key,
            },
        };
        let response = self
            .http
            .post(endpoint)
            .header("X-Service-Id", "execution-core")
            .header("X-Service-Token", &self.service_token)
            .json(&body)
            .send()
            .await
            .map_err(|error| format!("sandbox capability decision request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "sandbox capability decision request returned {}",
                response.status()
            ));
        }
        let envelope: DecisionResponseEnvelope = response
            .json()
            .await
            .map_err(|error| format!("sandbox capability decision response malformed: {error}"))?;
        Ok(envelope.data.decision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_is_none_without_configuration() {
        for name in [
            "CONTROL_PLANE_USER_CORE_URL",
            "EXECUTION_CORE_CONTROL_SANDBOX_CAPABILITY_TOKEN",
        ] {
            std::env::remove_var(name);
        }
        assert!(CapabilityClient::from_env().expect("no error for absence").is_none());
    }

    #[test]
    fn new_with_transport_rejects_plaintext_and_short_token() {
        assert!(CapabilityClient::new_with_transport(
            "http://control.example",
            &"a".repeat(32),
            false
        )
        .is_err());
        assert!(CapabilityClient::new_with_transport(
            "https://control.example",
            "too-short",
            false
        )
        .is_err());
        assert!(CapabilityClient::new_with_transport(
            "https://control.example",
            &"a".repeat(32),
            false
        )
        .is_ok());
    }
}
