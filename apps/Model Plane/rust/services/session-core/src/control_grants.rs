//! A small client for Control's owner-asserted grant surface in user-core (W4-SH-4; ADR
//! `docs/decisions/conversation-share-model.md` decisions 3 and 7).
//!
//! Control owns every grant. session-core is the registered owner service for the
//! `conversation_snapshot` resource type, so user-core admits it on
//! `POST` and `DELETE /api/v1/internal/authz/grant` and `GET /api/v1/internal/authz/grants` for that
//! type, and no other. Control cannot see a snapshot, so it trusts this service to have proved the
//! actor owns it before calling; the callers in `thread_shares.rs` do that.
//!
//! The read side (W4-SH-5) is [`ControlGrantsClient::check_view`]: a `GET` of
//! `/api/v1/internal/authz/check` for the reader's own delegation, asking whether that member holds
//! a `view` grant. It proves nothing about the owner and needs none, because it asks only about the
//! reader's own grant.
//!
//! Every call carries two proofs of who is acting:
//!
//! * the actor's own delegated credential (`Authorization: Bearer`, audience `data-plane`, already
//!   verified and bound to the caller by `JwtVerifier::delegated_data_plane_bearer`), which user-core
//!   checks again and compares with the delegation's user and organisation;
//! * a v2 service delegation, an HMAC over the method, request URI, user, organisation, operation,
//!   resource, reason, nonce and body digest, keyed by this service's user-core credential. It names
//!   exactly one operation on exactly one snapshot, so it cannot be replayed on another.
//!
//! Control takes `granted_by` from the delegated user, never from the body, and it refuses a grantee
//! who is not an active member of the actor's organisation and any role but `view`. Nothing here
//! stores a grant: a failure is an error for the caller to report, never a local record.
//!
//! Configuration is two environment values, read once at boot: [`BASE_URL_ENV`] and
//! [`SERVICE_TOKEN_ENV`]. Either one unset leaves the client out, and the share RPCs then answer
//! `unavailable` `snapshot_grants_unconfigured`.

use chrono::{DateTime, SecondsFormat, Utc};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use std::fmt;
use std::time::Duration;

use base64::Engine as _;

/// user-core's root URL, for example `http://user-core:8080`.
pub(crate) const BASE_URL_ENV: &str = "USER_CORE_URL";

/// This service's user-core credential (`USER_CORE_SERVICE_CREDENTIALS`, principal `session-core`,
/// scopes `authz:read` and `authz:write`). It signs the delegation and is sent as the service token.
pub(crate) const SERVICE_TOKEN_ENV: &str = "SESSION_USER_CORE_SERVICE_TOKEN";

/// The principal user-core registers this credential under.
const SERVICE_ID: &str = "session-core";

/// The audience every user-core credential is registered for.
const AUDIENCE: &str = "user-core";

/// The resource type Control registers `session-core` as the owner service of.
pub(crate) const RESOURCE_TYPE: &str = "conversation_snapshot";

/// The only role a snapshot grant carries.
const ROLE_VIEW: &str = "view";

const GRANT_PATH: &str = "/api/v1/internal/authz/grant";
const GRANTS_PATH: &str = "/api/v1/internal/authz/grants";
const CHECK_PATH: &str = "/api/v1/internal/authz/check";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

/// The most of a Control answer that is read; a grant list for one snapshot is far smaller.
const MAX_RESPONSE_BYTES: usize = 1 << 20;

/// Control's stable code for a grantee who is not an active member of the actor's organisation.
pub(crate) const MEMBER_NOT_IN_ORGANIZATION: &str = "member_not_in_organization";

/// Why a Control call did not give an answer to act on.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ControlGrantsError {
    /// The client has no URL or credential, or a value it was given could not be sent.
    #[error("the Control grant client is not configured")]
    NotConfigured,
    /// The request did not reach Control, or Control did not answer in time.
    #[error("Control could not be reached: {0}")]
    Transport(String),
    /// Control answered with something other than 200. `code` is Control's stable code when it
    /// sent one, such as [`MEMBER_NOT_IN_ORGANIZATION`].
    #[error("Control answered {status}")]
    Refused {
        /// The HTTP status Control answered with.
        status: u16,
        /// Control's stable code, or empty.
        code: String,
    },
    /// Control answered 200 with a body that is not the grant shape.
    #[error("Control answered with an unreadable grant")]
    Malformed,
}

/// The acting member, proved twice over: by the delegated credential and by the signed delegation.
pub(crate) struct Delegation<'a> {
    /// The actor's organisation.
    pub(crate) org_id: &'a str,
    /// The actor's user id.
    pub(crate) user_id: &'a str,
    /// The actor's verified delegated credential, the raw token without `Bearer `.
    pub(crate) bearer: &'a str,
}

/// One user grant on a snapshot as Control lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlGrant {
    /// The member the snapshot is shared with.
    pub(crate) grantee_user_id: String,
    /// When Control recorded the grant; `None` when Control's timestamp did not parse.
    pub(crate) granted_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct WireGrant {
    #[serde(default)]
    subject_type: String,
    #[serde(default)]
    subject_id: String,
    #[serde(default)]
    granted_at: String,
}

#[derive(Deserialize)]
struct WireGrantList {
    #[serde(default)]
    grants: Vec<WireGrant>,
}

/// Control's answer to a grant check: whether the subject holds a grant, and at which role.
#[derive(Deserialize)]
struct WireCheck {
    #[serde(default)]
    allowed: bool,
    #[serde(default)]
    role: String,
}

#[derive(Deserialize, Default)]
struct WireRefusal {
    #[serde(default)]
    code: String,
}

impl WireGrant {
    fn into_grant(self) -> Option<ControlGrant> {
        // Only a user subject is a member a snapshot is shared with. A Space subject cannot be
        // written for this type, so one here is not a grant this service made.
        if self.subject_type != "user" || self.subject_id.is_empty() {
            return None;
        }
        Some(ControlGrant {
            granted_at: DateTime::parse_from_rfc3339(&self.granted_at)
                .ok()
                .map(|at| at.with_timezone(&Utc)),
            grantee_user_id: self.subject_id,
        })
    }
}

/// The grant client. Cheap to clone; holds one HTTP client and the credential.
#[derive(Clone)]
pub(crate) struct ControlGrantsClient {
    base_url: String,
    service_token: String,
    http: reqwest::Client,
}

impl fmt::Debug for ControlGrantsClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlGrantsClient")
            .field("base_url", &self.base_url)
            .field("service_token", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl ControlGrantsClient {
    /// Build the client from [`BASE_URL_ENV`] and [`SERVICE_TOKEN_ENV`]; `None` when either is unset
    /// or empty, or the HTTP client cannot be built.
    pub(crate) fn from_env() -> Option<Self> {
        let base_url = std::env::var(BASE_URL_ENV).ok()?;
        let service_token = std::env::var(SERVICE_TOKEN_ENV).ok()?;
        Self::new(&base_url, &service_token).ok()
    }

    /// A client for `base_url` signing with `service_token`.
    pub(crate) fn new(base_url: &str, service_token: &str) -> Result<Self, ControlGrantsError> {
        let base_url = base_url.trim().trim_end_matches('/').to_owned();
        let service_token = service_token.trim().to_owned();
        if base_url.is_empty() || service_token.is_empty() {
            return Err(ControlGrantsError::NotConfigured);
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| ControlGrantsError::NotConfigured)?;
        Ok(Self {
            base_url,
            service_token,
            http,
        })
    }

    /// Write, idempotently, a `view` grant for `grantee_user_id` on `snapshot_id`. The caller has
    /// proved `who` owns the live snapshot.
    pub(crate) async fn grant_view(
        &self,
        who: &Delegation<'_>,
        snapshot_id: &str,
        grantee_user_id: &str,
    ) -> Result<ControlGrant, ControlGrantsError> {
        let body = serde_json::to_vec(&serde_json::json!({
            "org_id": who.org_id,
            "resource_type": RESOURCE_TYPE,
            "resource_id": snapshot_id,
            "subject_id": grantee_user_id,
            "subject_type": "user",
            "role": ROLE_VIEW,
        }))
        .map_err(|_| ControlGrantsError::Malformed)?;
        let payload = self
            .call(
                reqwest::Method::POST,
                GRANT_PATH.to_owned(),
                Some(body),
                who,
                "authz:grant",
                snapshot_id,
                "share conversation snapshot with a member",
            )
            .await?;
        let wire: WireGrant =
            serde_json::from_slice(&payload).map_err(|_| ControlGrantsError::Malformed)?;
        // The write answers with the row it stored, whose subject type is the one asked for.
        let granted_at = DateTime::parse_from_rfc3339(&wire.granted_at)
            .ok()
            .map(|at| at.with_timezone(&Utc));
        if wire.subject_id != grantee_user_id {
            return Err(ControlGrantsError::Malformed);
        }
        Ok(ControlGrant {
            grantee_user_id: wire.subject_id,
            granted_at,
        })
    }

    /// Remove the user grant for `grantee_user_id` on `snapshot_id`. Control answers 200 for an
    /// absent grant, so this is idempotent.
    pub(crate) async fn revoke(
        &self,
        who: &Delegation<'_>,
        snapshot_id: &str,
        grantee_user_id: &str,
    ) -> Result<(), ControlGrantsError> {
        let query = query_string(&[
            ("org_id", who.org_id),
            ("resource_id", snapshot_id),
            ("resource_type", RESOURCE_TYPE),
            ("subject_id", grantee_user_id),
            ("subject_type", "user"),
        ]);
        self.call(
            reqwest::Method::DELETE,
            format!("{GRANT_PATH}?{query}"),
            None,
            who,
            "authz:revoke",
            snapshot_id,
            "remove a conversation snapshot share",
        )
        .await
        .map(|_| ())
    }

    /// Every user grant Control holds on `snapshot_id`.
    pub(crate) async fn list(
        &self,
        who: &Delegation<'_>,
        snapshot_id: &str,
    ) -> Result<Vec<ControlGrant>, ControlGrantsError> {
        let query = query_string(&[
            ("org_id", who.org_id),
            ("resource_id", snapshot_id),
            ("resource_type", RESOURCE_TYPE),
        ]);
        let payload = self
            .call(
                reqwest::Method::GET,
                format!("{GRANTS_PATH}?{query}"),
                None,
                who,
                "authz:grants",
                snapshot_id,
                "list conversation snapshot shares",
            )
            .await?;
        let list: WireGrantList =
            serde_json::from_slice(&payload).map_err(|_| ControlGrantsError::Malformed)?;
        Ok(list
            .grants
            .into_iter()
            .filter_map(WireGrant::into_grant)
            .collect())
    }

    /// Whether the member `who` acts for holds a `view` grant on `snapshot_id`. Control answers for
    /// that delegated user only, so the answer is the reader's own grant. Only `allowed` with the
    /// `view` role is a yes; any other answer, or one that is not the check's shape, is not.
    pub(crate) async fn check_view(
        &self,
        who: &Delegation<'_>,
        snapshot_id: &str,
    ) -> Result<bool, ControlGrantsError> {
        let query = query_string(&[
            ("org_id", who.org_id),
            ("resource_id", snapshot_id),
            ("resource_type", RESOURCE_TYPE),
            ("role", ROLE_VIEW),
            ("subject_id", who.user_id),
            ("subject_type", "user"),
        ]);
        let payload = self
            .call(
                reqwest::Method::GET,
                format!("{CHECK_PATH}?{query}"),
                None,
                who,
                "authz:check",
                snapshot_id,
                "read a conversation snapshot shared with a member",
            )
            .await?;
        let answer: WireCheck =
            serde_json::from_slice(&payload).map_err(|_| ControlGrantsError::Malformed)?;
        Ok(answer.allowed && answer.role == ROLE_VIEW)
    }

    /// Send one signed, delegated request and return the 200 payload. `path_and_query` is signed
    /// and sent exactly as given, so Control verifies the URI it receives.
    #[allow(clippy::too_many_arguments)]
    async fn call(
        &self,
        method: reqwest::Method,
        path_and_query: String,
        body: Option<Vec<u8>>,
        who: &Delegation<'_>,
        operation: &str,
        snapshot_id: &str,
        reason: &str,
    ) -> Result<Vec<u8>, ControlGrantsError> {
        if who.org_id.trim().is_empty()
            || who.user_id.trim().is_empty()
            || who.bearer.trim().is_empty()
            || who.bearer.contains(['\r', '\n'])
            || snapshot_id.trim().is_empty()
        {
            return Err(ControlGrantsError::NotConfigured);
        }
        let body_bytes: &[u8] = body.as_deref().unwrap_or(&[]);
        let headers = delegation_headers(
            &self.service_token,
            method.as_str(),
            &path_and_query,
            who,
            operation,
            snapshot_id,
            reason,
            body_bytes,
            Utc::now(),
            &fresh_nonce()?,
        );
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url, path_and_query))
            .header("Authorization", format!("Bearer {}", who.bearer))
            .header("X-Service-Token", &self.service_token)
            .header("X-Service-Id", SERVICE_ID);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        if let Some(body) = body {
            request = request
                .header("Content-Type", "application/json")
                .body(body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| ControlGrantsError::Transport(error.without_url().to_string()))?;
        let status = response.status();
        let payload = response
            .bytes()
            .await
            .map_err(|error| ControlGrantsError::Transport(error.without_url().to_string()))?;
        if payload.len() > MAX_RESPONSE_BYTES {
            return Err(ControlGrantsError::Malformed);
        }
        if status != reqwest::StatusCode::OK {
            let refusal: WireRefusal = serde_json::from_slice(&payload).unwrap_or_default();
            return Err(ControlGrantsError::Refused {
                status: status.as_u16(),
                code: refusal.code.trim().to_owned(),
            });
        }
        Ok(payload.to_vec())
    }
}

/// A fresh 24-byte nonce, base64url without padding (Control wants 16 to 128 characters).
fn fresh_nonce() -> Result<String, ControlGrantsError> {
    let mut bytes = [0_u8; 24];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| ControlGrantsError::NotConfigured)?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// The v2 delegation headers user-core verifies (`service_delegation.go`) for one operation on one
/// resource. `body` is the exact request body, empty for none: its digest is part of the signed text.
/// Pure, so the signed form is checkable without a network.
#[allow(clippy::too_many_arguments)]
fn delegation_headers(
    service_token: &str,
    method: &str,
    request_uri: &str,
    who: &Delegation<'_>,
    operation: &str,
    resource_id: &str,
    reason: &str,
    body: &[u8],
    now: DateTime<Utc>,
    nonce: &str,
) -> Vec<(&'static str, String)> {
    let timestamp = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let digest = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(body));
    let canonical = [
        "v2",
        SERVICE_ID,
        AUDIENCE,
        &timestamp,
        method,
        request_uri,
        who.user_id,
        who.org_id,
        operation,
        RESOURCE_TYPE,
        resource_id,
        reason,
        "true",
        nonce,
        &digest,
    ]
    .join("\n");
    let key = hmac::Key::new(hmac::HMAC_SHA256, service_token.as_bytes());
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(hmac::sign(&key, canonical.as_bytes()).as_ref());
    vec![
        ("X-User-Id", who.user_id.to_owned()),
        ("X-Org-Id", who.org_id.to_owned()),
        ("X-Delegation-Version", "v2".to_owned()),
        ("X-Delegation-Nonce", nonce.to_owned()),
        ("X-Delegation-Timestamp", timestamp),
        ("X-Delegation-Operation", operation.to_owned()),
        ("X-Delegation-Resource-Type", RESOURCE_TYPE.to_owned()),
        ("X-Delegation-Resource-Id", resource_id.to_owned()),
        ("X-Delegation-Reason", reason.to_owned()),
        ("X-Delegation-ZDR", "true".to_owned()),
        ("X-Delegation-Body-SHA256", digest),
        ("X-Delegation-Signature", signature),
    ]
}

/// A query string with every value percent-encoded, in the order given.
fn query_string(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(name, value)| format!("{name}={}", percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encode everything but the RFC 3986 unreserved characters.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
