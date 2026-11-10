//! Static-bearer MCP servers (W4-CC-5; ADR-NEXT-sandbox-credential-injection sections 2, 6 and 7).
//!
//! An organisation admin stores a server's bearer token once, as an integration-corev2 credential binding
//! (W4-CC-3). A static-bearer server registers with that binding's id and never with the token, and the gateway
//! keeps no copy of it. For each call the gateway asks integration-corev2 for a sealed reference
//! (`POST /internal/credential-refs/seal`), redeems the reference for the plaintext
//! (`POST /internal/credential-refs/redeem`), and hands the plaintext to the one request builder that sends it.
//! The plaintext is dropped when that call ends. It is never cached, never logged and never returned in a tool
//! result: a response that echoes it is redacted first ([`redact`]).
//!
//! The path is dark until `MCP_SECRET_REF_AUTH_ENABLED` is the literal `true`, read on every use. A zero-retention
//! or waived turn redeems nothing: it is refused with `feature_not_supported` (feature [`ZDR_FEATURE`]) before any
//! call leaves the gateway, and a listing offers no static-bearer server to such a turn.
//!
//! This service calls integration-corev2 with its own principal key ([`PRINCIPAL_KEY_VAR`]). integration-corev2
//! admits that key on the seal and redeem routes only, never on the shared internal key.

use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use mp_contracts::zdr_refusal::Refusal;
use mp_ids::new_ulid;
use serde_json::{json, Value};

use crate::zdr_posture::TurnPosture;

/// The switch. Only the literal `true` lets a registration name a static-bearer server.
pub const ENABLED_VAR: &str = "MCP_SECRET_REF_AUTH_ENABLED";
/// The `auth_kind` a registration names to bind a stored credential.
pub const AUTH_KIND_STATIC_BEARER: &str = "static_bearer";
/// The `auth_kind` such a server is catalogued under. capability-core accepts `none`, `bearer`, `api_key` and
/// `oauth` only, so a stored bearer is catalogued as a `bearer` whose `secret_ref` names the binding.
pub const CATALOG_AUTH_KIND: &str = "bearer";
/// The catalogue feature a zero-retention or waived turn is refused with.
pub const ZDR_FEATURE: &str = "mcp_secret_ref";
/// This service's own principal key for the credential-reference routes. It must equal integration-corev2's
/// `CREDENTIAL_REF_SEAL_KEYS` entry for `model-gateway`.
pub const PRINCIPAL_KEY_VAR: &str = "IC_CREDENTIAL_REF_KEY";

/// The header integration-corev2 reads its internal keys from (its default `INTERNAL_API_KEY_HEADER`).
const PRINCIPAL_KEY_HEADER: &str = "x-internal-api-key";
const SEAL_PATH: &str = "/internal/credential-refs/seal";
const REDEEM_PATH: &str = "/internal/credential-refs/redeem";
/// The slot a call's reference is sealed for. A slot names a sandbox route, which an MCP call never uses.
const SLOT: &str = "mcp";
/// The one header a static binding places its token in. integration-corev2 binds `mcp_bearer` to it.
const BEARER_HEADER: &str = "Authorization";
/// The call deadline a reference is sealed for. integration-corev2 adds its own grace and caps the lifetime.
const CALL_DEADLINE_SECS: i64 = 30;
/// The longest one round trip to integration-corev2 may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// The longest identifier integration-corev2 accepts for a binding, an organisation or a call.
const MAX_ID_BYTES: usize = 128;
/// The longest error or reason code copied out of an integration-corev2 answer.
const MAX_CODE_BYTES: usize = 64;

/// Whether the switch is on in this process, read on every use so that a restart is not needed to turn it off.
#[must_use]
pub fn enabled() -> bool {
    parse_enabled(std::env::var(ENABLED_VAR).ok().as_deref())
}

/// Only the literal `true` turns the switch on.
#[must_use]
pub fn parse_enabled(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether `value` is an identifier integration-corev2 accepts in a reference: 1 to 128 letters, digits and
/// `_ - . :`.
fn token_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

/// Whether `binding_id` names an integration-corev2 credential binding: the `cb_` prefix and an identifier.
#[must_use]
pub fn binding_id_valid(binding_id: &str) -> bool {
    binding_id.starts_with("cb_") && token_valid(binding_id)
}

/// The `secret_ref` capability-core records for a binding.
#[must_use]
pub fn secret_ref_for(binding_id: &str) -> String {
    format!("secret://integration-corev2/credential-bindings/{binding_id}")
}

/// The call id a reference is sealed for: the request's own id when integration-corev2 accepts it as an
/// identifier, and a fresh one otherwise.
#[must_use]
pub fn call_id_for(request_id: &str) -> String {
    let trimmed = request_id.trim();
    if token_valid(trimmed) {
        trimmed.to_owned()
    } else {
        new_ulid()
    }
}

/// The refusal a static-bearer server gets on a turn with this posture, or `None` when the turn may redeem its
/// binding (a persistent turn).
#[must_use]
pub fn turn_refusal(posture: &TurnPosture) -> Option<Refusal> {
    (posture.zdr() || posture.is_waived()).then(|| Refusal::feature_not_supported(ZDR_FEATURE))
}

/// Replace every copy of a redeemed token in `text` with `[redacted]`, so that a server which echoes its own
/// credential cannot hand it to a model or to a client.
pub fn redact(text: &mut String, secret: &str) {
    if !secret.is_empty() && text.contains(secret) {
        *text = text.replace(secret, "[redacted]");
    }
}

/// The plaintext token one call runs with. It has no `Debug`, `Display` or `Clone`, it is built only by
/// [`bearer_for`], and it lives for the call that redeemed it.
pub struct SecretBearer(String);

impl SecretBearer {
    /// The token, for the request builder that sends it and for nothing else.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Why a static-bearer credential could not be released for a call. Every reason names what happened and carries
/// no token, reference or header value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialRefError {
    /// `MCP_SECRET_REF_AUTH_ENABLED` is not the literal `true`.
    Disabled,
    /// This service has no principal key for the credential-reference routes.
    NotConfigured,
    /// The turn is zero-retention or waived. Nothing was sent.
    ZdrOrWaived,
    /// The server URL is not https, so the token would travel in clear.
    HttpsRequired,
    /// The server URL cannot name a binding host: it has no host, or it names a port other than 443.
    HostNotBindable,
    /// An identifier or a binding id is malformed.
    Malformed,
    /// integration-corev2 refused the step, with its error code and, when it gives one, its reason code.
    Refused { code: String, reason: String },
    /// integration-corev2 could not be reached, or it answered something that is not its envelope.
    Unavailable,
    /// The binding releases a header other than `Authorization`.
    HeaderMismatch,
}

impl CredentialRefError {
    /// What a tool result or a log line says about the failure.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Disabled => {
                "static-bearer MCP servers are not enabled on this deployment".to_owned()
            }
            Self::NotConfigured => {
                "the stored credential is not configured on this deployment".to_owned()
            }
            Self::ZdrOrWaived => {
                "a stored credential is not used on a zero-retention or waived turn".to_owned()
            }
            Self::HttpsRequired => {
                "the server URL must be https to use a stored credential".to_owned()
            }
            Self::HostNotBindable => "the server URL cannot take a stored credential".to_owned(),
            Self::Malformed => "the stored credential reference is malformed".to_owned(),
            Self::Refused { code, reason } if reason.is_empty() => {
                format!("the stored credential was refused ({code})")
            }
            Self::Refused { code, reason } => {
                format!("the stored credential was refused ({code}, {reason})")
            }
            Self::Unavailable => "the stored credential could not be read right now".to_owned(),
            Self::HeaderMismatch => "the stored credential is not a bearer token".to_owned(),
        }
    }
}

impl fmt::Display for CredentialRefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

/// Whether `server_url` can take a stored credential: an https URL on the default port, whose host a binding can
/// name.
///
/// # Errors
/// [`CredentialRefError::HttpsRequired`] or [`CredentialRefError::HostNotBindable`].
pub fn validate_server_url(server_url: &str) -> Result<(), CredentialRefError> {
    bound_host(server_url).map(|_| ())
}

/// The host a binding names for `server_url`, lowercased. The grammar itself (no IP literal, no trailing dot) is
/// integration-corev2's, and it checks the host again on every seal.
fn bound_host(server_url: &str) -> Result<String, CredentialRefError> {
    let endpoint =
        reqwest::Url::parse(server_url.trim()).map_err(|_| CredentialRefError::HostNotBindable)?;
    if endpoint.scheme() != "https" {
        return Err(CredentialRefError::HttpsRequired);
    }
    if endpoint.port().is_some() {
        return Err(CredentialRefError::HostNotBindable);
    }
    endpoint
        .host_str()
        .filter(|host| !host.is_empty())
        .map(str::to_ascii_lowercase)
        .ok_or(CredentialRefError::HostNotBindable)
}

/// Redeem the stored bearer of one static-bearer server for one call. integration-corev2 seals a reference for the
/// binding, the server's host and `call_id`, and then redeems that reference for the plaintext. Nothing is cached.
///
/// `turn_zdr_or_waived` is the turn's posture as [`turn_refusal`] reads it. A ZDR or waived turn is refused before
/// any call leaves the gateway.
///
/// # Errors
/// [`CredentialRefError`]: the switch is off, the turn is zero-retention or waived, the server URL cannot name a
/// binding host, an identifier is malformed, the principal key is missing, integration-corev2 refuses or cannot be
/// reached, or the binding releases a header other than `Authorization`.
pub async fn bearer_for(
    server_url: &str,
    binding_id: &str,
    org_id: &str,
    call_id: &str,
    turn_zdr_or_waived: bool,
) -> Result<SecretBearer, CredentialRefError> {
    if !enabled() {
        return Err(CredentialRefError::Disabled);
    }
    if turn_zdr_or_waived {
        return Err(CredentialRefError::ZdrOrWaived);
    }
    if !binding_id_valid(binding_id) || !token_valid(org_id) || !token_valid(call_id) {
        return Err(CredentialRefError::Malformed);
    }
    let host = bound_host(server_url)?;
    let key = principal_key()?;
    let client = client()?;
    let base = crate::tool_loop::connector_reads::integration_base_url();
    let deadline = Utc::now() + chrono::Duration::seconds(CALL_DEADLINE_SECS);

    let mut sealed = post(
        client,
        &base,
        SEAL_PATH,
        &key,
        &json!({
            "binding_id": binding_id,
            "org_id": org_id,
            "call_id": call_id,
            "slot": SLOT,
            "host": host,
            "deadline": deadline.to_rfc3339_opts(SecondsFormat::Secs, true),
            "zdr": false,
            "waived": false,
        }),
    )
    .await?;
    let reference = take_data(&mut sealed, "ref")?;

    let mut redeemed = post(
        client,
        &base,
        REDEEM_PATH,
        &key,
        &json!({
            "ref": reference,
            "host": host,
            "call_id": call_id,
        }),
    )
    .await?;
    let header = take_data(&mut redeemed, "header_name")?;
    if !header.eq_ignore_ascii_case(BEARER_HEADER) {
        return Err(CredentialRefError::HeaderMismatch);
    }
    let secret = take_data(&mut redeemed, "secret")?;
    Ok(SecretBearer(secret))
}

/// One internal POST to integration-corev2 with this service's principal key. A 2xx answer comes back as its JSON
/// body. Any other answer is reduced to its error code and reason, never its text.
async fn post(
    client: &reqwest::Client,
    base: &str,
    path: &str,
    key: &str,
    body: &Value,
) -> Result<Value, CredentialRefError> {
    let response = client
        .post(format!("{base}{path}"))
        .header(PRINCIPAL_KEY_HEADER, key)
        .timeout(REQUEST_TIMEOUT)
        .json(body)
        .send()
        .await
        .map_err(|_| CredentialRefError::Unavailable)?;
    let status = response.status();
    let text = crate::runtime_registries::bounded_mcp_response(response)
        .await
        .map_err(|_| CredentialRefError::Unavailable)?;
    let value: Value = serde_json::from_str(&text).map_err(|_| CredentialRefError::Unavailable)?;
    if status.is_success() {
        Ok(value)
    } else {
        Err(refusal_of(&value))
    }
}

/// The code and reason of an integration-corev2 refusal. Neither is free text: both are cut to letters, digits and
/// underscores.
fn refusal_of(value: &Value) -> CredentialRefError {
    let Some(code) = value
        .pointer("/error/code")
        .and_then(Value::as_str)
        .map(safe_code)
    else {
        return CredentialRefError::Unavailable;
    };
    let reason = value
        .pointer("/error/details/reason_code")
        .and_then(Value::as_str)
        .map(safe_code)
        .unwrap_or_default();
    CredentialRefError::Refused { code, reason }
}

fn safe_code(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .take(MAX_CODE_BYTES)
        .collect()
}

/// Move one non-empty string field out of an answer's `data` object. A missing or empty field is a malformed answer.
fn take_data(value: &mut Value, field: &str) -> Result<String, CredentialRefError> {
    value
        .get_mut("data")
        .and_then(|data| data.get_mut(field))
        .map(Value::take)
        .and_then(|taken| match taken {
            Value::String(text) if !text.is_empty() => Some(text),
            _ => None,
        })
        .ok_or(CredentialRefError::Malformed)
}

/// This service's principal key, read from the environment for each call.
fn principal_key() -> Result<String, CredentialRefError> {
    std::env::var(PRINCIPAL_KEY_VAR)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or(CredentialRefError::NotConfigured)
}

/// The client for integration-corev2. It never follows a redirect, so the principal key cannot be sent on to
/// another host.
fn client() -> Result<&'static reqwest::Client, CredentialRefError> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()
                .ok()
        })
        .as_ref()
        .ok_or(CredentialRefError::Unavailable)
}
