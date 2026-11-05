//! HTTP client for Control Plane's signed Space sandbox capability decision
//! endpoint (`POST /api/v1/internal/spaces/sandbox-capability-decision`).
//! See apps/Frontend Plane/verevonv3/docs/S3_2_SANDBOX_LEASE_CLOSEOUT_DESIGN_2026-09-10.md.
//!
//! Absent configuration disables this client entirely (`from_env` returns
//! `None`) rather than defaulting to some fallback behavior: a Space
//! capability profile without an attached decision is a fully valid, honest
//! response (`/capability-profile`'s `decision` field is `Option`) — this
//! process must never fabricate a signature it cannot obtain from Control.

use std::fmt;
use std::time::Duration;

use reqwest::{Client, StatusCode, Url};
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
    claims: SpaceCapabilityClaims,
}

/// Mirrors Control's unsigned claims sidecar byte-for-byte
/// (`user-core/internal/http/spaces.go`'s `sandboxCapabilityClaims`, and
/// independently sandbox-manager's own copy of the same shape,
/// `internal/authz/capability_verifier.go`'s `SpaceCapabilityClaims` — kept as
/// separate structs on both Go sides because the services are independently
/// deployed modules with no shared package for this envelope; this is the
/// THIRD independent copy, for the same reason). Field order does not need to
/// match on the wire — JSON object keys are unordered for `encoding/json`
/// unmarshaling — but the field NAMES and JSON tags must, since
/// sandbox-manager's `AcquireLease` deserializes this exact JSON via
/// `json.Unmarshal` into its own copy of the struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceCapabilityClaims {
    pub backend_id: String,
    pub profile_digest: String,
    pub persistence: String,
    pub processes: String,
    pub backup: bool,
    pub egress: String,
    pub credential_mode: String,
    /// The Space egress allow-list Control signed over (EP-9 E2), present only at the `allowlist` tier;
    /// carried through unchanged so sandbox-manager's digest check sees the list Control bound.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub egress_allowlist: Vec<String>,
}

/// Why a capability decision was not obtained.
///
/// Control answers a typed Space refusal (entitlement off, role floor, kind
/// unsupported) with the 4xx its denial vocabulary assigns (403) and keeps 503
/// for "could not answer". The two must stay apart: a refusal is final, so a
/// caller that retries it only repeats the same question, while an outage or a
/// dead socket is worth retrying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityDecisionError {
    /// Control answered NO with a final 4xx. `code` is the typed denial code of
    /// the body when it carried one (for example `space_entitlement_off`).
    Refused { status: u16, code: Option<String> },
    /// Control could not answer: a transport failure, a 5xx (503
    /// `space_authority_unavailable` included), a transient 4xx (401, 408, 425,
    /// 429) or a malformed response. Retryable.
    Unavailable(String),
}

impl CapabilityDecisionError {
    /// True when retrying the same request cannot change Control's answer.
    #[must_use]
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Refused { .. })
    }
}

impl fmt::Display for CapabilityDecisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused {
                status,
                code: Some(code),
            } => write!(
                f,
                "sandbox capability decision request returned {status} ({code})"
            ),
            Self::Refused { status, code: None } => {
                write!(f, "sandbox capability decision request returned {status}")
            }
            Self::Unavailable(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CapabilityDecisionError {}

/// A non-success Control status is a final refusal when it is a 4xx that
/// retrying cannot change. 401 is the service credential (it can be rotated
/// into place) and 408, 425 and 429 are transient by definition, so those
/// retry like a 5xx.
fn is_final_refusal(status: StatusCode) -> bool {
    status.is_client_error()
        && !matches!(
            status,
            StatusCode::UNAUTHORIZED
                | StatusCode::REQUEST_TIMEOUT
                | StatusCode::TOO_EARLY
                | StatusCode::TOO_MANY_REQUESTS
        )
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
    /// its backend and Space. Returns the opaque decision token AND the
    /// unsigned claims sidecar Control returned alongside it — a caller that
    /// only reports the profile (like `http_health::capability_profile`) can
    /// discard the claims, but a caller that presents this decision onward to
    /// sandbox-manager's `AcquireLease` needs both: the decision's
    /// `PayloadDigest` binds to exactly these claims, and sandbox-manager
    /// recomputes that binding independently rather than trusting a
    /// caller-reconstructed copy.
    ///
    /// # Errors
    /// Returns an error for any transport failure, non-2xx response, or
    /// malformed response body. A final 4xx is [`CapabilityDecisionError::Refused`]
    /// and everything else is [`CapabilityDecisionError::Unavailable`]. Callers
    /// treat both the same as "no decision available" for health/profile
    /// reporting — see `http_health::capability_profile`'s doc comment — never
    /// as a reason to fail it; a lease acquisition retries only the second.
    pub async fn request_decision(
        &self,
        request: &CapabilityDecisionRequest<'_>,
    ) -> Result<(String, SpaceCapabilityClaims), CapabilityDecisionError> {
        let endpoint = service_endpoint(&self.base_url, CONTROL_SANDBOX_CAPABILITY_PATH)
            .map_err(CapabilityDecisionError::Unavailable)?;
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
            .map_err(|error| {
                CapabilityDecisionError::Unavailable(format!(
                    "sandbox capability decision request failed: {error}"
                ))
            })?;
        let status = response.status();
        if !status.is_success() {
            if is_final_refusal(status) {
                // Only the typed `code` is read from a refusal body; nothing else
                // from it is kept.
                let code = response
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .and_then(|body| body.get("code")?.as_str().map(str::to_owned));
                return Err(CapabilityDecisionError::Refused {
                    status: status.as_u16(),
                    code,
                });
            }
            return Err(CapabilityDecisionError::Unavailable(format!(
                "sandbox capability decision request returned {status}"
            )));
        }
        let envelope: DecisionResponseEnvelope = response.json().await.map_err(|error| {
            CapabilityDecisionError::Unavailable(format!(
                "sandbox capability decision response malformed: {error}"
            ))
        })?;
        Ok((envelope.data.decision, envelope.data.claims))
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

    #[tokio::test]
    async fn request_decision_returns_both_the_token_and_the_claims_sidecar() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(CONTROL_SANDBOX_CAPABILITY_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "decision": "v2.<key>.<payload>.<sig>",
                    "claims": {
                        "backend_id": "backend-1",
                        "profile_digest": "sha256:abc",
                        "persistence": "ephemeral",
                        "processes": "bounded_oneshot",
                        "backup": false,
                        "egress": "disabled_by_default",
                        "credential_mode": "none",
                    },
                },
            })))
            .mount(&server)
            .await;
        let client =
            CapabilityClient::new_with_transport(&server.uri(), &"a".repeat(32), true).unwrap();
        let profile = crate::sandbox::capability_profile();

        let (decision, claims) = client
            .request_decision(&CapabilityDecisionRequest {
                org_id: "org-a",
                space_ref: "space-1",
                subject_id: "user-1",
                backend_id: "backend-1",
                profile: &profile,
                idempotency_key: "run-1",
            })
            .await
            .expect("decision request succeeds");

        assert_eq!(decision, "v2.<key>.<payload>.<sig>");
        assert_eq!(
            claims,
            SpaceCapabilityClaims {
                backend_id: "backend-1".to_owned(),
                profile_digest: "sha256:abc".to_owned(),
                persistence: "ephemeral".to_owned(),
                processes: "bounded_oneshot".to_owned(),
                backup: false,
                egress: "disabled_by_default".to_owned(),
                credential_mode: "none".to_owned(),
                egress_allowlist: Vec::new(),
            }
        );
    }

    /// Control answers a typed refusal with 403 and keeps 503 for "could not
    /// answer": the first is final, everything else stays retryable.
    #[tokio::test]
    async fn request_decision_separates_a_final_refusal_from_an_outage() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let typed_refusal = serde_json::json!({
            "error": "personal Space sandbox authority unavailable",
            "code": "space_entitlement_off",
            "resolvable_by": "org_admin",
            "details": {"entitlement": "sandbox_capability"},
        });
        let unavailable = serde_json::json!({
            "error": "personal Space sandbox authority unavailable",
            "code": "space_authority_unavailable",
            "resolvable_by": "retry",
            "details": {},
        });
        let cases: Vec<(u16, serde_json::Value, bool, Option<&str>)> = vec![
            (403, typed_refusal, true, Some("space_entitlement_off")),
            (403, serde_json::json!({"error": "current personal Space authority required"}), true, None),
            (400, serde_json::json!({"error": "valid space capability intent is required"}), true, None),
            (503, unavailable, false, None),
            (500, serde_json::json!({"error": "boom"}), false, None),
            (401, serde_json::json!({"error": "unauthorized"}), false, None),
            (429, serde_json::json!({"error": "slow down"}), false, None),
        ];
        for (status, body, want_final, want_code) in cases {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path(CONTROL_SANDBOX_CAPABILITY_PATH))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .mount(&server)
                .await;
            let client =
                CapabilityClient::new_with_transport(&server.uri(), &"a".repeat(32), true)
                    .unwrap();
            let profile = crate::sandbox::capability_profile();
            let error = client
                .request_decision(&CapabilityDecisionRequest {
                    org_id: "org-a",
                    space_ref: "space-1",
                    subject_id: "user-1",
                    backend_id: "backend-1",
                    profile: &profile,
                    idempotency_key: "run-1",
                })
                .await
                .expect_err("a non-2xx answer is an error");
            assert_eq!(error.is_final(), want_final, "status {status}: {error}");
            if let CapabilityDecisionError::Refused { status: got, code } = &error {
                assert_eq!(*got, status);
                assert_eq!(code.as_deref(), want_code);
            }
        }
    }

    #[tokio::test]
    async fn request_decision_treats_a_dead_socket_as_retryable() {
        let client =
            CapabilityClient::new_with_transport("http://127.0.0.1:1", &"a".repeat(32), true)
                .unwrap();
        let profile = crate::sandbox::capability_profile();
        let error = client
            .request_decision(&CapabilityDecisionRequest {
                org_id: "org-a",
                space_ref: "space-1",
                subject_id: "user-1",
                backend_id: "backend-1",
                profile: &profile,
                idempotency_key: "run-1",
            })
            .await
            .expect_err("nothing listens on port 1");
        assert!(!error.is_final(), "{error}");
    }
}
