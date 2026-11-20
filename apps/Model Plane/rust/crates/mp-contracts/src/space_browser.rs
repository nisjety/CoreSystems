//! The room browser run decision the model-gateway verifies before a Space run starts (build plan
//! WS20-S2).
//!
//! Control (`user-core` `space_browser_run_decision.go`, WS20-S1) signs a `model.space.browser_run`
//! decision for one starter, one Space and one goal commitment. It binds the room audience and its
//! revision, the canonical host list, the view scope, the limits and a two-minute window. This module
//! is the single Rust copy of the check the gateway runs on it:
//!
//! * the envelope `v2.<key-id>.<payload>.<signature>`, Ed25519 over the first three segments, the form
//!   session-core verifies for every Space decision;
//! * the payload digest, byte for byte as Control's `browserRunPayloadDigest`, pinned by
//!   `user-core/internal/spaces/testdata/space-browser-run-decision.golden.json`;
//! * the claims, read strictly, and the policy every room run must meet.
//!
//! Space authority never comes from the request body. The caller states the org, subject, Space,
//! thread, goal digest and idempotency key it verified or derived itself, and the decision must equal
//! each of them. A verified run yields a grant request with the narrowed hosts and the short lifetime,
//! and no Space reference (C1). Frames go to the starter alone in v1 (SD-23), whatever the view scope.
//!
//! The rollout switch [`SPACE_BROWSER_RUN_API_ENV`] is a control, never an authorization input.

use base64::{
    alphabet,
    engine::{
        general_purpose::{GeneralPurpose, NO_PAD, PAD, URL_SAFE_NO_PAD},
        Engine as _,
    },
};
use chrono::{DateTime, TimeDelta, Utc};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::agentic_subscription::switch_value_is_on;

/// The rollout switch for the room browser run start in model-gateway. Off unless set.
pub const SPACE_BROWSER_RUN_API_ENV: &str = "SPACE_BROWSER_RUN_API";
/// The one Control decision key id this process trusts; the envelope must name it.
pub const CONTROL_KEY_ID_ENV: &str = "CONTROL_SPACE_DECISION_KEY_ID";
/// The Control decision public key (unpadded base64url, as Control emits it; padded and standard
/// base64 are accepted as session-core accepts them).
pub const CONTROL_PUBLIC_KEY_ENV: &str = "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64";

/// The action the decision authorizes.
pub const BROWSER_RUN_ACTION_ID: &str = "model.space.browser_run";
/// The audience the decision is minted for: model-gateway verifies it (WS20-S1, D13).
pub const BROWSER_RUN_AUDIENCE: &str = "model-plane-model-gateway";
/// The schema revision the decision and the digest both carry.
pub const BROWSER_RUN_SCHEMA: &str = "sha256:space-browser-run-v1";
/// Opens the payload digest. Part of the wire contract with Control.
const DIGEST_HEADER: &[u8] = b"model.space.browser_run\0v1\0";

/// Every room run carries this permission.
pub const PERMISSION_RUN: &str = "space:browser:run";
/// Carried exactly when the view scope is `room`.
pub const PERMISSION_VIEW_ROOM: &str = "space:browser:view_room";
/// The only browser profile a room run may use. Control forces it.
pub const PROFILE_EPHEMERAL: &str = "ephemeral";
/// The thread mode of a run that names its thread: the decision binds that thread id.
pub const THREAD_MODE_EXISTING: &str = "existing";
/// The thread mode of a run that starts a new thread (D4): the decision binds no thread id, and
/// the gateway creates the thread before the start.
pub const THREAD_MODE_NEW: &str = "new";
/// The view of the run's starter alone.
pub const VIEW_OWNER: &str = "owner";
/// The view the room may also see. v1 gives the room receipts only (SD-23).
pub const VIEW_ROOM: &str = "room";

/// SD-23 limits, the bounds Control enforces: 5 to 40 steps, 30 to 600 seconds, 1 to 8 hosts.
pub const MIN_STEPS: u32 = 5;
pub const MAX_STEPS: u32 = 40;
pub const MIN_RUNTIME_S: u32 = 30;
pub const MAX_RUNTIME_S: u32 = 600;
pub const MAX_HOSTS: usize = 8;
const MAX_HOST_BYTES: usize = 253;
const MAX_LABEL_BYTES: usize = 63;
const MAX_THREAD_ID_BYTES: usize = 256;

/// A room run may have at most this many browser runs in flight in one Space (SD-23).
pub const ROOM_RUNS_PER_SPACE: u32 = 2;
/// The derived run grant lives the run's own limit plus this slack ...
const GRANT_TTL_SLACK_S: u32 = 60;
/// ... and never longer than this (SD-23: at most ten minutes).
const GRANT_TTL_MAX_S: u32 = 600;

/// Largest decision token accepted, and largest claims document (bounds the parse).
const MAX_TOKEN_BYTES: usize = 8 * 1024;
const MAX_CLAIMS_BYTES: usize = 4 * 1024;
/// The envelope version Control signs (its `decisionVersion`).
const ENVELOPE_VERSION: &str = "v2";
/// A decision issued more than this far in the future is refused (Control's clock slack).
const FUTURE_SLACK_S: i64 = 60;

// Go's decoder ignores unused trailing bits and skips CR and LF. These engines mirror the four
// predefined engines session-core accepts for a Control public key, with the same padding rules.
const LENIENT_URL_SAFE_NO_PAD: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    NO_PAD.with_decode_allow_trailing_bits(true),
);
const LENIENT_URL_SAFE: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    PAD.with_decode_allow_trailing_bits(true),
);
const LENIENT_STANDARD: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    PAD.with_decode_allow_trailing_bits(true),
);
const LENIENT_STANDARD_NO_PAD: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    NO_PAD.with_decode_allow_trailing_bits(true),
);

/// Why a browser run decision was refused. The caller maps every variant to a fixed response; the
/// detail is for logs only, through [`BrowserRunRefusal::label`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserRunRefusal {
    /// The envelope, a field or the claims are not in the golden shape.
    Malformed,
    /// The key id is not the configured Control key, or the signature does not verify.
    Untrusted,
    /// The decision binds another org, subject, Space, thread, goal, key, audience or digest.
    Mismatch,
    /// Not a room run this gateway may start: permission, profile, view, limits, hosts or ZDR.
    Policy,
    /// Outside its validity window (expired, or issued too far in the future).
    Window,
    /// The browser decision and the append decision name different recipient audience revisions.
    Stale,
}

impl BrowserRunRefusal {
    /// A closed telemetry label. Never a fragment of the input.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::Untrusted => "untrusted",
            Self::Mismatch => "mismatch",
            Self::Policy => "policy",
            Self::Window => "window",
            Self::Stale => "stale",
        }
    }
}

/// The Control decision key this process trusts, parsed once at start-up.
#[derive(Clone)]
pub struct ControlDecisionKey {
    id: String,
    key: VerifyingKey,
}

impl ControlDecisionKey {
    /// Builds the trusted key from the configured key id and public key. The refusal names the
    /// variable that is missing or invalid, never its value.
    pub fn from_config(id: Option<&str>, public_key: Option<&str>) -> Result<Self, &'static str> {
        let id = id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(CONTROL_KEY_ID_ENV)?;
        let encoded = public_key
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(CONTROL_PUBLIC_KEY_ENV)?;
        let bytes: [u8; 32] = decode_public_key(encoded)
            .and_then(|raw| <[u8; 32]>::try_from(raw).ok())
            .ok_or(CONTROL_PUBLIC_KEY_ENV)?;
        let key = VerifyingKey::from_bytes(&bytes).map_err(|_| CONTROL_PUBLIC_KEY_ENV)?;
        Ok(Self {
            id: id.to_owned(),
            key,
        })
    }

    /// The key id the envelope must name.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

fn decode_public_key(encoded: &str) -> Option<Vec<u8>> {
    let cleaned: String = encoded
        .chars()
        .filter(|c| !matches!(c, '\r' | '\n'))
        .collect();
    LENIENT_URL_SAFE_NO_PAD
        .decode(cleaned.as_str())
        .or_else(|_| LENIENT_URL_SAFE.decode(cleaned.as_str()))
        .or_else(|_| LENIENT_STANDARD.decode(cleaned.as_str()))
        .or_else(|_| LENIENT_STANDARD_NO_PAD.decode(cleaned.as_str()))
        .ok()
}

/// The rollout switch as a pure value: `value` is the raw variable, `None` when unset.
#[must_use]
pub fn space_browser_run_api_enabled(value: Option<&str>) -> bool {
    switch_value_is_on(value)
}

/// The signed decision payload: Control's `Decision` JSON, read for the fields this verifier binds.
/// Unknown fields are ignored, as Go's `json.Unmarshal` ignores them. Every field named here is
/// always present in Control's output, so a missing one refuses the decision, including a missing
/// `zero_data_retention`, which must never default to false.
#[derive(Debug, Clone, Deserialize)]
pub struct SignedDecision {
    pub decision_ref: String,
    pub org_id: String,
    pub space_ref: String,
    pub subject_id: String,
    pub service_audience: String,
    pub action_id: String,
    pub action_schema_hash: String,
    pub payload_digest: String,
    pub idempotency_key: String,
    pub recipient_audience_ref: String,
    pub recipient_audience_hash: String,
    pub privacy_policy_ref: String,
    pub resource_authorization_ref: String,
    pub authority_revision: i64,
    pub membership_revision: i64,
    pub privacy_revision: i64,
    pub recipient_audience_revision: i64,
    pub entitlement_revision: i64,
    pub permissions: Vec<String>,
    pub purpose: String,
    pub lawful_basis: String,
    pub privacy_class: String,
    pub retention_class: String,
    pub residency: String,
    pub deletion_scope: String,
    pub zero_data_retention: bool,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub nonce: String,
}

/// The non-secret claims the digest binds, as Control returns them beside the token. Read strictly:
/// an unknown or missing field refuses the decision.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserRunClaims {
    pub thread_id: String,
    pub thread_mode: String,
    pub goal_digest: String,
    pub allowed_domains: Vec<String>,
    pub profile_scope: String,
    pub view_scope: String,
    pub max_steps: u32,
    pub max_runtime_s: u32,
}

/// What the caller verified or derived itself. Each field must equal the decision's binding.
#[derive(Debug, Clone, Copy)]
pub struct BrowserRunExpectation<'a> {
    /// The verified organization of the starter.
    pub org_id: &'a str,
    /// The verified subject of the starter (the run owner).
    pub subject_id: &'a str,
    /// The Space named in the route path, checked against membership and lifecycle by the caller.
    pub space_ref: &'a str,
    /// The thread the decision binds: empty when the decision is the new-thread mode (D4). The caller then
    /// checks the run's own thread (the one its gateway created for the decision) itself; otherwise it is
    /// the run's thread, which must equal the decision's.
    pub thread_id: &'a str,
    /// `sha256:` plus the digest of the goal text the gateway will start the run with.
    pub goal_digest: &'a str,
    /// The idempotency key the gateway derived for this start.
    pub idempotency_key: &'a str,
    /// The current time, for the validity window.
    pub now: DateTime<Utc>,
}

/// A decision that passed every check, with what the run start and the grant need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedBrowserRun {
    pub decision_ref: String,
    pub recipient_audience_revision: i64,
    pub expires_at: DateTime<Utc>,
    pub claims: BrowserRunClaims,
}

/// Verifies a room browser run decision and its claims against the configured Control key and the
/// caller's expectation. Every check fails closed. The order is: the envelope and key id, the
/// signature, the payload shape, the claims and their policy, the binding and digest, the window.
pub fn verify_browser_run_decision(
    key: &ControlDecisionKey,
    token: &str,
    claims_json: &str,
    expect: &BrowserRunExpectation<'_>,
) -> Result<VerifiedBrowserRun, BrowserRunRefusal> {
    let decision = open_decision(key, token)?;
    let claims = parse_claims(claims_json)?;
    check_claims(&claims)?;
    bind(&decision, &claims, expect)?;
    check_window(&decision, expect.now)?;
    Ok(VerifiedBrowserRun {
        decision_ref: decision.decision_ref,
        recipient_audience_revision: decision.recipient_audience_revision,
        expires_at: decision.expires_at,
        claims,
    })
}

/// Opens a Control Space decision envelope: the size bound, the version, the key id, the Ed25519
/// signature over the first three segments, then the payload read and its shape checked. Shared by
/// every Space decision this crate verifies (the browser run start and the starter stop, D21h), so
/// the envelope rules exist once.
pub(crate) fn open_decision(
    key: &ControlDecisionKey,
    token: &str,
) -> Result<SignedDecision, BrowserRunRefusal> {
    if token.len() > MAX_TOKEN_BYTES {
        return Err(BrowserRunRefusal::Malformed);
    }
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 4 || parts[0] != ENVELOPE_VERSION {
        return Err(BrowserRunRefusal::Malformed);
    }
    let key_id = URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()
        .and_then(|raw| String::from_utf8(raw).ok())
        .ok_or(BrowserRunRefusal::Untrusted)?;
    if key_id != key.id {
        return Err(BrowserRunRefusal::Untrusted);
    }
    let signature_bytes = URL_SAFE_NO_PAD
        .decode(parts[3])
        .map_err(|_| BrowserRunRefusal::Untrusted)?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| BrowserRunRefusal::Untrusted)?;
    let signed = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
    key.key
        .verify(signed.as_bytes(), &signature)
        .map_err(|_| BrowserRunRefusal::Untrusted)?;

    let payload = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| BrowserRunRefusal::Malformed)?;
    let decision: SignedDecision =
        serde_json::from_slice(&payload).map_err(|_| BrowserRunRefusal::Malformed)?;
    check_shape(&decision)?;
    Ok(decision)
}

/// Reads the claims document strictly and within its size bound.
pub fn parse_claims(claims_json: &str) -> Result<BrowserRunClaims, BrowserRunRefusal> {
    if claims_json.len() > MAX_CLAIMS_BYTES {
        return Err(BrowserRunRefusal::Malformed);
    }
    serde_json::from_str(claims_json).map_err(|_| BrowserRunRefusal::Malformed)
}

/// The fields Control always fills, the revisions (positive, as Control's `Validate` requires) and
/// the validity order.
fn check_shape(decision: &SignedDecision) -> Result<(), BrowserRunRefusal> {
    let required = [
        &decision.decision_ref,
        &decision.org_id,
        &decision.space_ref,
        &decision.subject_id,
        &decision.service_audience,
        &decision.action_id,
        &decision.action_schema_hash,
        &decision.payload_digest,
        &decision.idempotency_key,
        &decision.recipient_audience_ref,
        &decision.recipient_audience_hash,
        &decision.privacy_policy_ref,
        &decision.resource_authorization_ref,
        &decision.purpose,
        &decision.lawful_basis,
        &decision.privacy_class,
        &decision.retention_class,
        &decision.residency,
        &decision.deletion_scope,
        &decision.nonce,
    ];
    if required.iter().any(|value| value.trim().is_empty()) || decision.permissions.is_empty() {
        return Err(BrowserRunRefusal::Malformed);
    }
    let revisions = [
        decision.authority_revision,
        decision.membership_revision,
        decision.privacy_revision,
        decision.recipient_audience_revision,
        decision.entitlement_revision,
    ];
    if revisions.iter().any(|revision| *revision <= 0) || decision.expires_at <= decision.issued_at {
        return Err(BrowserRunRefusal::Malformed);
    }
    Ok(())
}

/// The room policy every claim must meet: the ephemeral profile, a known view scope, the step and
/// runtime bounds, 1 to 8 canonical hosts (sorted and distinct, as Control writes them) and a
/// well-formed goal commitment and thread id. The thread mode must agree with the thread id: a
/// new-thread decision names no thread, and an existing-thread decision names one.
fn check_claims(claims: &BrowserRunClaims) -> Result<(), BrowserRunRefusal> {
    let thread_mode_ok = match claims.thread_mode.as_str() {
        THREAD_MODE_NEW => claims.thread_id.is_empty(),
        THREAD_MODE_EXISTING => !claims.thread_id.is_empty(),
        _ => false,
    };
    if !valid_goal_digest(&claims.goal_digest)
        || claims.thread_id.len() > MAX_THREAD_ID_BYTES
        || !thread_mode_ok
    {
        return Err(BrowserRunRefusal::Malformed);
    }
    let hosts = &claims.allowed_domains;
    let canonical = hosts.windows(2).all(|pair| pair[0] < pair[1]);
    let hosts_ok = !hosts.is_empty()
        && hosts.len() <= MAX_HOSTS
        && canonical
        && hosts.iter().all(|host| valid_host(host));
    let limits_ok = (MIN_STEPS..=MAX_STEPS).contains(&claims.max_steps)
        && (MIN_RUNTIME_S..=MAX_RUNTIME_S).contains(&claims.max_runtime_s);
    let view_ok = claims.view_scope == VIEW_OWNER || claims.view_scope == VIEW_ROOM;
    if claims.profile_scope != PROFILE_EPHEMERAL || !view_ok || !limits_ok || !hosts_ok {
        return Err(BrowserRunRefusal::Policy);
    }
    Ok(())
}

/// The decision must name exactly the caller's binding and the digest of its own fields and claims.
/// The room-view permission is present exactly when the view scope is `room`, the run permission is
/// present, and the decision is not zero-retention.
fn bind(
    decision: &SignedDecision,
    claims: &BrowserRunClaims,
    expect: &BrowserRunExpectation<'_>,
) -> Result<(), BrowserRunRefusal> {
    let expected_mode = if expect.thread_id.is_empty() {
        THREAD_MODE_NEW
    } else {
        THREAD_MODE_EXISTING
    };
    let same_binding = decision.org_id == expect.org_id
        && decision.subject_id == expect.subject_id
        && decision.space_ref == expect.space_ref
        && decision.service_audience == BROWSER_RUN_AUDIENCE
        && decision.action_id == BROWSER_RUN_ACTION_ID
        && decision.action_schema_hash == BROWSER_RUN_SCHEMA
        && decision.idempotency_key == expect.idempotency_key
        && claims.thread_id == expect.thread_id
        && claims.thread_mode == expected_mode
        && claims.goal_digest == expect.goal_digest;
    if !same_binding || decision.payload_digest != payload_digest(decision, claims) {
        return Err(BrowserRunRefusal::Mismatch);
    }
    let run = decision
        .permissions
        .iter()
        .any(|permission| permission.as_str() == PERMISSION_RUN);
    let view_room = decision
        .permissions
        .iter()
        .any(|permission| permission.as_str() == PERMISSION_VIEW_ROOM);
    if !run || view_room != (claims.view_scope == VIEW_ROOM) || decision.zero_data_retention {
        return Err(BrowserRunRefusal::Policy);
    }
    Ok(())
}

pub(crate) fn check_window(
    decision: &SignedDecision,
    now: DateTime<Utc>,
) -> Result<(), BrowserRunRefusal> {
    if now >= decision.expires_at {
        return Err(BrowserRunRefusal::Window);
    }
    if decision.issued_at - now > TimeDelta::seconds(FUTURE_SLACK_S) {
        return Err(BrowserRunRefusal::Window);
    }
    Ok(())
}

/// Writes one length-prefixed text field: name, NUL, 8-byte big-endian length, value.
pub(crate) fn write_field(digest: &mut Sha256, name: &str, value: &str) {
    digest.update(name.as_bytes());
    digest.update([0u8]);
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value.as_bytes());
}

/// Writes one revision: name, NUL, 8-byte big-endian value (Control's `writeDigestRevisions`).
pub(crate) fn write_revision(digest: &mut Sha256, name: &str, value: i64) {
    digest.update(name.as_bytes());
    digest.update([0u8]);
    digest.update((value as u64).to_be_bytes());
}

/// The payload digest of a room browser run, the bytes Control's `browserRunPayloadDigest` hashes,
/// recomputed from the decision and the claims. The domains are joined with LF in the claims' order,
/// which [`check_claims`] has already required to be canonical.
#[must_use]
pub fn payload_digest(decision: &SignedDecision, claims: &BrowserRunClaims) -> String {
    let domains = claims.allowed_domains.join("\n");
    let max_steps = claims.max_steps.to_string();
    let max_runtime_s = claims.max_runtime_s.to_string();
    let mut digest = Sha256::new();
    digest.update(DIGEST_HEADER);
    for (name, value) in [
        ("org_id", decision.org_id.as_str()),
        ("user_id", decision.subject_id.as_str()),
        ("space_id", decision.space_ref.as_str()),
        ("thread_id", claims.thread_id.as_str()),
        ("thread_mode", claims.thread_mode.as_str()),
        ("goal_digest", claims.goal_digest.as_str()),
        ("allowed_domains", domains.as_str()),
        ("profile_scope", claims.profile_scope.as_str()),
        ("view_scope", claims.view_scope.as_str()),
        ("max_steps", max_steps.as_str()),
        ("max_runtime_s", max_runtime_s.as_str()),
        (
            "recipient_audience_ref",
            decision.recipient_audience_ref.as_str(),
        ),
        (
            "recipient_audience_hash",
            decision.recipient_audience_hash.as_str(),
        ),
        ("privacy_policy_ref", decision.privacy_policy_ref.as_str()),
        (
            "resource_authorization_ref",
            decision.resource_authorization_ref.as_str(),
        ),
        ("action_schema_hash", BROWSER_RUN_SCHEMA),
        ("idempotency_key", decision.idempotency_key.as_str()),
    ] {
        write_field(&mut digest, name, value);
    }
    for (name, value) in [
        ("authority_revision", decision.authority_revision),
        ("membership_revision", decision.membership_revision),
        ("privacy_revision", decision.privacy_revision),
        (
            "recipient_audience_revision",
            decision.recipient_audience_revision,
        ),
        ("entitlement_revision", decision.entitlement_revision),
    ] {
        write_revision(&mut digest, name, value);
    }
    format!("sha256:{:x}", digest.finalize())
}

/// The goal commitment Control binds: `sha256:` and the lowercase hex digest of the goal bytes. The
/// goal text itself never reaches Control.
#[must_use]
pub fn goal_digest(goal: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(goal.as_bytes()))
}

/// `sha256:` followed by 64 lowercase hex digits.
#[must_use]
pub fn valid_goal_digest(value: &str) -> bool {
    const PREFIX: &str = "sha256:";
    value.len() == PREFIX.len() + 64
        && value.starts_with(PREFIX)
        && value[PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A canonical host: lowercase, at least two dot-separated labels of letters, digits and inner
/// hyphens, with a letter in the final label. IP literals and `localhost` are refused by name. This
/// is Control's `validBrowserHost`; Quarry's SSRF guards remain the enforcement.
#[must_use]
pub fn valid_host(host: &str) -> bool {
    if host.is_empty() || host.len() > MAX_HOST_BYTES || host != host.to_ascii_lowercase() {
        return false;
    }
    if host.parse::<std::net::IpAddr>().is_ok()
        || host == "localhost"
        || host.ends_with(".localhost")
    {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    for label in &labels {
        if label.is_empty()
            || label.len() > MAX_LABEL_BYTES
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return false;
        }
    }
    labels
        .last()
        .is_some_and(|last| last.bytes().any(|byte| byte.is_ascii_lowercase()))
}

/// The start's append decision and the browser decision must name one recipient audience revision.
pub fn check_audience_revisions_agree(browser: i64, append: i64) -> Result<(), BrowserRunRefusal> {
    if browser == append {
        Ok(())
    } else {
        Err(BrowserRunRefusal::Stale)
    }
}

/// Whether another run may start while `active` browser runs are in flight in the Space.
#[must_use]
pub const fn capacity_admits(active: u32) -> bool {
    active < ROOM_RUNS_PER_SPACE
}

/// The lifetime of the derived run grant: the run's own limit plus a minute, never more than ten
/// minutes. A removed starter's run therefore ends within that bound (SD-23).
#[must_use]
pub fn room_grant_ttl_seconds(max_runtime_s: u32) -> u32 {
    max_runtime_s
        .saturating_add(GRANT_TTL_SLACK_S)
        .min(GRANT_TTL_MAX_S)
}

/// A room run takes no caller-chosen profile, no approval hold and no zero-retention flag.
#[must_use]
pub const fn profile_request_admitted(
    profile_id_set: bool,
    require_approval_set: bool,
    zdr: bool,
) -> bool {
    !profile_id_set && !require_approval_set && !zdr
}

/// Frames of a room run go to its starter alone in v1, whatever the view scope. Control may sign
/// view scope `room` while the room-view flag is off; this is the gateway's rule that withholds them
/// from every other viewer.
#[must_use]
pub fn frames_visible_to(viewer_subject_id: &str, starter_subject_id: &str) -> bool {
    !starter_subject_id.is_empty() && viewer_subject_id == starter_subject_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::{json, Value};

    /// Control's golden vectors, read at compile time. Control's copy is `browserRunPayloadDigest`.
    const GOLDEN: &str = include_str!(
        "../../../../../Control Plane/user-core/internal/spaces/testdata/space-browser-run-decision.golden.json"
    );
    /// The goal text the golden commits to, and its digest.
    const GOAL: &str = "read the pricing page";
    const GOAL_DIGEST: &str =
        "sha256:e3eb81231b5a76dd873dfa95da2a0a0f0c576567a5fed957006fc51b68c405dc";
    /// The key id the test Control key signs under.
    const KEY_ID: &str = "control-space-decision-test";

    fn at(instant: &str) -> DateTime<Utc> {
        instant.parse().expect("a fixed instant")
    }

    /// Inside the decision's window (issued 10:00:00, expires 10:02:00).
    fn now() -> DateTime<Utc> {
        at("2026-11-17T10:00:30Z")
    }

    fn control_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn trusted(signing: &SigningKey) -> ControlDecisionKey {
        let encoded = URL_SAFE_NO_PAD.encode(signing.verifying_key().to_bytes());
        ControlDecisionKey::from_config(Some(KEY_ID), Some(&encoded))
            .expect("the test public key parses")
    }

    fn expectation(now: DateTime<Utc>) -> BrowserRunExpectation<'static> {
        BrowserRunExpectation {
            org_id: "org-1",
            subject_id: "user-2",
            space_ref: "space-room-1",
            thread_id: "thread-room-1",
            goal_digest: GOAL_DIGEST,
            idempotency_key: "space-browser-1",
            now,
        }
    }

    /// The claims of a room run that names its thread: owner view, 20 steps, 300 seconds.
    fn existing_claims() -> BrowserRunClaims {
        BrowserRunClaims {
            thread_id: "thread-room-1".to_owned(),
            thread_mode: THREAD_MODE_EXISTING.to_owned(),
            goal_digest: GOAL_DIGEST.to_owned(),
            allowed_domains: vec!["docs.example.org".to_owned(), "fixture.test".to_owned()],
            profile_scope: PROFILE_EPHEMERAL.to_owned(),
            view_scope: VIEW_OWNER.to_owned(),
            max_steps: 20,
            max_runtime_s: 300,
        }
    }

    /// The room policy applied to the existing-thread claims after one edit.
    fn checked(edit: impl FnOnce(&mut BrowserRunClaims)) -> Result<(), BrowserRunRefusal> {
        let mut claims = existing_claims();
        edit(&mut claims);
        check_claims(&claims)
    }

    /// The signed decision fields Control writes for the golden input. The refs are trimmed, as
    /// `newEvidenceDecision` writes them. [`with_digest`] sets the digest.
    fn decision_fields() -> Value {
        json!({
            "decision_ref": "decision-1",
            "org_id": "org-1",
            "space_ref": "space-room-1",
            "subject_id": "user-2",
            "service_audience": BROWSER_RUN_AUDIENCE,
            "action_id": BROWSER_RUN_ACTION_ID,
            "action_schema_hash": BROWSER_RUN_SCHEMA,
            "payload_digest": "",
            "idempotency_key": "space-browser-1",
            "recipient_audience_ref": "audience:space-room-1:4",
            "recipient_audience_hash": "sha256:audience-hash",
            "privacy_policy_ref": "privacy:org-1:5",
            "resource_authorization_ref": "control:space-room-1:browser-run:7",
            "authority_revision": 7,
            "membership_revision": 3,
            "privacy_revision": 5,
            "recipient_audience_revision": 4,
            "entitlement_revision": 6,
            "permissions": [PERMISSION_RUN],
            "purpose": "ai_run",
            "lawful_basis": "contract",
            "privacy_class": "shared_space",
            "retention_class": "ephemeral",
            "residency": "eu",
            "deletion_scope": "run",
            "zero_data_retention": false,
            "issued_at": "2026-11-17T10:00:00Z",
            "expires_at": "2026-11-17T10:02:00Z",
            "nonce": "nonce-1"
        })
    }

    fn decision_of(fields: &Value) -> SignedDecision {
        serde_json::from_value(fields.clone()).expect("the fields are a complete decision")
    }

    /// The fields with the payload digest Control would write over these claims.
    fn with_digest(mut fields: Value, claims: &BrowserRunClaims) -> Value {
        let digest = payload_digest(&decision_of(&fields), claims);
        fields["payload_digest"] = json!(digest);
        fields
    }

    /// The claims document Control returns beside the token.
    fn claims_value(claims: &BrowserRunClaims) -> Value {
        json!({
            "thread_id": claims.thread_id,
            "thread_mode": claims.thread_mode,
            "goal_digest": claims.goal_digest,
            "allowed_domains": claims.allowed_domains,
            "profile_scope": claims.profile_scope,
            "view_scope": claims.view_scope,
            "max_steps": claims.max_steps,
            "max_runtime_s": claims.max_runtime_s,
        })
    }

    /// The envelope `v2.<key-id>.<payload>.<signature>`, Ed25519 over the first three segments.
    fn envelope(signing: &SigningKey, key_id: &str, payload: &Value) -> String {
        let signed = format!(
            "v2.{}.{}",
            URL_SAFE_NO_PAD.encode(key_id),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).expect("the payload serializes"))
        );
        let signature = signing.sign(signed.as_bytes());
        format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }

    /// A decision the test Control key signs over the digest of these claims.
    fn signed(fields: Value, claims: &BrowserRunClaims) -> String {
        envelope(&control_key(), KEY_ID, &with_digest(fields, claims))
    }

    fn verify_with(
        signing: &SigningKey,
        token: &str,
        claims: &Value,
        now: DateTime<Utc>,
    ) -> Result<VerifiedBrowserRun, BrowserRunRefusal> {
        verify_browser_run_decision(
            &trusted(signing),
            token,
            &claims.to_string(),
            &expectation(now),
        )
    }

    fn string_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("a list")
            .iter()
            .map(|item| item.as_str().expect("a string").to_owned())
            .collect()
    }

    #[test]
    fn payload_digest_reproduces_every_control_golden_vector() {
        let golden: Value = serde_json::from_str(GOLDEN).expect("the golden is JSON");
        let input = &golden["input"];
        let vectors = golden["vectors"].as_array().expect("the golden has vectors");
        assert_eq!(
            vectors.len(),
            4,
            "room_owner_view, room_shared_view, domains_listed_reversed and new_thread"
        );
        for vector in vectors {
            let name = vector["name"].as_str().expect("a vector name");
            let mut fields = decision_fields();
            for (field, golden_field) in [
                ("org_id", "org_id"),
                ("subject_id", "user_id"),
                ("space_ref", "space_id"),
                ("recipient_audience_ref", "recipient_audience_ref"),
                ("recipient_audience_hash", "recipient_audience_hash"),
                ("privacy_policy_ref", "privacy_policy_ref"),
                ("resource_authorization_ref", "resource_authorization_ref"),
                ("authority_revision", "authority_revision"),
                ("membership_revision", "membership_revision"),
                ("privacy_revision", "privacy_revision"),
                ("recipient_audience_revision", "recipient_audience_revision"),
                ("entitlement_revision", "entitlement_revision"),
            ] {
                fields[field] = input[golden_field].clone();
            }
            fields["idempotency_key"] = vector["idempotency_key"].clone();
            let claims = BrowserRunClaims {
                thread_id: vector["thread_id"].as_str().expect("thread_id").to_owned(),
                thread_mode: vector["thread_mode"].as_str().expect("thread_mode").to_owned(),
                goal_digest: vector["goal_digest"].as_str().expect("goal_digest").to_owned(),
                allowed_domains: string_list(&vector["canonical_allowed_domains"]),
                profile_scope: PROFILE_EPHEMERAL.to_owned(),
                view_scope: vector["view_scope"].as_str().expect("view_scope").to_owned(),
                max_steps: u32::try_from(vector["max_steps"].as_u64().expect("max_steps"))
                    .expect("max_steps fits a u32"),
                max_runtime_s: u32::try_from(
                    vector["max_runtime_s"].as_u64().expect("max_runtime_s"),
                )
                .expect("max_runtime_s fits a u32"),
            };
            assert_eq!(
                goal_digest(vector["goal"].as_str().expect("goal")),
                claims.goal_digest,
                "{name}: the goal commitment"
            );
            assert_eq!(
                check_claims(&claims),
                Ok(()),
                "{name}: the vector's claims meet the room policy"
            );
            assert_eq!(
                payload_digest(&decision_of(&fields), &claims),
                vector["payload_digest"].as_str().expect("payload_digest"),
                "{name}"
            );
        }
        assert_eq!(GOAL, golden["vectors"][0]["goal"].as_str().expect("goal"));
    }

    #[test]
    fn a_control_signed_decision_verifies_and_yields_its_run() {
        let signing = control_key();
        let claims = existing_claims();
        let token = signed(decision_fields(), &claims);
        let verified = verify_with(&signing, &token, &claims_value(&claims), now())
            .expect("a Control-signed decision verifies");
        assert_eq!(verified.decision_ref, "decision-1");
        assert_eq!(verified.recipient_audience_revision, 4);
        assert_eq!(verified.expires_at, at("2026-11-17T10:02:00Z"));
        assert_eq!(verified.claims, claims);
    }

    #[test]
    fn forged_claims_are_refused_as_a_mismatch() {
        let signing = control_key();
        let claims = existing_claims();
        let token = signed(decision_fields(), &claims);
        let mut forged = claims_value(&claims);
        forged["max_steps"] = json!(21);
        assert_eq!(
            verify_with(&signing, &token, &forged, now()),
            Err(BrowserRunRefusal::Mismatch)
        );
    }

    #[test]
    fn a_decision_under_another_key_id_is_untrusted() {
        let claims = existing_claims();
        let token = envelope(
            &control_key(),
            "some-other-control-key",
            &with_digest(decision_fields(), &claims),
        );
        assert_eq!(
            verify_with(&control_key(), &token, &claims_value(&claims), now()),
            Err(BrowserRunRefusal::Untrusted)
        );
    }

    #[test]
    fn a_decision_signed_by_another_key_is_untrusted() {
        let claims = existing_claims();
        let forger = SigningKey::from_bytes(&[9; 32]);
        let token = envelope(&forger, KEY_ID, &with_digest(decision_fields(), &claims));
        assert_eq!(
            verify_with(&control_key(), &token, &claims_value(&claims), now()),
            Err(BrowserRunRefusal::Untrusted)
        );
    }

    #[test]
    fn a_tampered_payload_is_untrusted() {
        let claims = existing_claims();
        let token = signed(decision_fields(), &claims);
        let mut forged = decision_fields();
        forged["org_id"] = json!("org-9");
        let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&forged).expect("serializes"));
        let segments: Vec<&str> = token.split('.').collect();
        let tampered = [segments[0], segments[1], body.as_str(), segments[3]].join(".");
        assert_eq!(
            verify_with(&control_key(), &tampered, &claims_value(&claims), now()),
            Err(BrowserRunRefusal::Untrusted)
        );
    }

    #[test]
    fn an_expired_or_far_future_decision_is_a_window_refusal() {
        let signing = control_key();
        let claims = existing_claims();
        let token = signed(decision_fields(), &claims);
        let doc = claims_value(&claims);
        assert_eq!(
            verify_with(&signing, &token, &doc, at("2026-11-17T10:02:00Z")),
            Err(BrowserRunRefusal::Window),
            "expired at its expiry instant"
        );
        assert_eq!(
            verify_with(&signing, &token, &doc, at("2026-11-17T09:58:00Z")),
            Err(BrowserRunRefusal::Window),
            "issued more than a minute ahead"
        );
        assert!(
            verify_with(&signing, &token, &doc, at("2026-11-17T09:59:00Z")).is_ok(),
            "a minute ahead is the clock slack"
        );
    }

    #[test]
    fn a_zero_retention_decision_is_a_policy_refusal() {
        let mut fields = decision_fields();
        fields["zero_data_retention"] = json!(true);
        let claims = existing_claims();
        let token = signed(fields, &claims);
        assert_eq!(
            verify_with(&control_key(), &token, &claims_value(&claims), now()),
            Err(BrowserRunRefusal::Policy)
        );
    }

    #[test]
    fn the_room_view_permission_must_agree_with_the_view_scope() {
        let signing = control_key();
        let owner = existing_claims();
        let mut owner_with_room = decision_fields();
        owner_with_room["permissions"] = json!([PERMISSION_RUN, PERMISSION_VIEW_ROOM]);
        let token = signed(owner_with_room, &owner);
        assert_eq!(
            verify_with(&signing, &token, &claims_value(&owner), now()),
            Err(BrowserRunRefusal::Policy),
            "a room permission on an owner view"
        );

        let mut room = existing_claims();
        room.view_scope = VIEW_ROOM.to_owned();
        let token = signed(decision_fields(), &room);
        assert_eq!(
            verify_with(&signing, &token, &claims_value(&room), now()),
            Err(BrowserRunRefusal::Policy),
            "a room view without the permission"
        );

        let mut room_with_room = decision_fields();
        room_with_room["permissions"] = json!([PERMISSION_RUN, PERMISSION_VIEW_ROOM]);
        let token = signed(room_with_room, &room);
        assert!(
            verify_with(&signing, &token, &claims_value(&room), now()).is_ok(),
            "a room view with its permission verifies"
        );
    }

    #[test]
    fn a_decision_without_its_zero_retention_flag_is_malformed() {
        let claims = existing_claims();
        let mut fields = with_digest(decision_fields(), &claims);
        fields
            .as_object_mut()
            .expect("the fields are an object")
            .remove("zero_data_retention");
        let token = envelope(&control_key(), KEY_ID, &fields);
        assert_eq!(
            verify_with(&control_key(), &token, &claims_value(&claims), now()),
            Err(BrowserRunRefusal::Malformed)
        );
    }

    #[test]
    fn claims_with_an_unknown_field_are_malformed() {
        let claims = existing_claims();
        let token = signed(decision_fields(), &claims);
        let mut doc = claims_value(&claims);
        doc["surprise"] = json!(true);
        assert_eq!(
            parse_claims(&doc.to_string()),
            Err(BrowserRunRefusal::Malformed)
        );
        assert_eq!(
            verify_with(&control_key(), &token, &doc, now()),
            Err(BrowserRunRefusal::Malformed)
        );
    }

    #[test]
    fn a_control_decision_with_trimmed_refs_verifies_and_a_padded_ref_is_a_mismatch() {
        let signing = control_key();
        let claims = existing_claims();
        // Control trims the evidence refs before it writes the decision and digests them, so the
        // signed fields carry the trimmed values and the digest is over those.
        let trimmed = with_digest(decision_fields(), &claims);
        let token = envelope(&signing, KEY_ID, &trimmed);
        assert!(
            verify_with(&signing, &token, &claims_value(&claims), now()).is_ok(),
            "the trimmed decision verifies end to end"
        );
        for field in [
            "recipient_audience_ref",
            "recipient_audience_hash",
            "privacy_policy_ref",
            "resource_authorization_ref",
        ] {
            let mut padded = trimmed.clone();
            padded[field] = json!(format!(
                "  {}  ",
                trimmed[field].as_str().expect("a text ref")
            ));
            let token = envelope(&signing, KEY_ID, &padded);
            assert_eq!(
                verify_with(&signing, &token, &claims_value(&claims), now()),
                Err(BrowserRunRefusal::Mismatch),
                "a padded {field} is refused"
            );
        }
    }

    #[test]
    fn the_thread_mode_must_agree_with_the_thread_id() {
        assert_eq!(
            checked(|claims| claims.thread_mode = THREAD_MODE_NEW.to_owned()),
            Err(BrowserRunRefusal::Malformed),
            "a new-mode run names no thread"
        );
        assert_eq!(
            checked(|claims| claims.thread_id = String::new()),
            Err(BrowserRunRefusal::Malformed),
            "an existing-mode run names its thread"
        );
        assert_eq!(
            checked(|claims| claims.thread_mode = "room".to_owned()),
            Err(BrowserRunRefusal::Malformed),
            "any other mode is refused"
        );
        assert_eq!(
            checked(|claims| {
                claims.thread_id = String::new();
                claims.thread_mode = THREAD_MODE_NEW.to_owned();
            }),
            Ok(()),
            "a new-thread run with no thread is valid"
        );
    }

    #[test]
    fn bind_refuses_a_thread_mode_that_disagrees_with_the_expected_thread() {
        let mut new_run = existing_claims();
        new_run.thread_id = String::new();
        new_run.thread_mode = THREAD_MODE_NEW.to_owned();
        let new_decision = decision_of(&with_digest(decision_fields(), &new_run));
        let named = expectation(now());
        let unnamed = BrowserRunExpectation {
            thread_id: "",
            ..expectation(now())
        };
        assert_eq!(
            bind(&new_decision, &new_run, &named),
            Err(BrowserRunRefusal::Mismatch),
            "a new-mode decision when a thread is named"
        );
        assert_eq!(
            bind(&new_decision, &new_run, &unnamed),
            Ok(()),
            "a new-mode decision when no thread is named"
        );

        let existing = existing_claims();
        let existing_decision = decision_of(&with_digest(decision_fields(), &existing));
        assert_eq!(
            bind(&existing_decision, &existing, &unnamed),
            Err(BrowserRunRefusal::Mismatch),
            "an existing-mode decision when no thread is named"
        );
        assert_eq!(
            bind(&existing_decision, &existing, &named),
            Ok(()),
            "an existing-mode decision when its thread is named"
        );
    }

    #[test]
    fn the_room_policy_bounds_steps_runtime_hosts_profile_and_view() {
        assert_eq!(checked(|c| c.max_steps = 4), Err(BrowserRunRefusal::Policy));
        assert_eq!(checked(|c| c.max_steps = 41), Err(BrowserRunRefusal::Policy));
        assert_eq!(checked(|c| c.max_steps = 5), Ok(()));
        assert_eq!(checked(|c| c.max_steps = 40), Ok(()));
        assert_eq!(checked(|c| c.max_runtime_s = 29), Err(BrowserRunRefusal::Policy));
        assert_eq!(checked(|c| c.max_runtime_s = 601), Err(BrowserRunRefusal::Policy));
        assert_eq!(checked(|c| c.max_runtime_s = 30), Ok(()));
        assert_eq!(checked(|c| c.max_runtime_s = 600), Ok(()));

        let nine: Vec<String> = ('a'..='i')
            .map(|letter| format!("{letter}.example.com"))
            .collect();
        assert_eq!(
            checked(|c| c.allowed_domains = nine.clone()),
            Err(BrowserRunRefusal::Policy),
            "nine hosts"
        );
        assert_eq!(checked(|c| c.allowed_domains = nine[..8].to_vec()), Ok(()));
        assert_eq!(
            checked(|c| c.allowed_domains = vec!["a.example.com".to_owned(), "a.example.com".to_owned()]),
            Err(BrowserRunRefusal::Policy),
            "a duplicate host"
        );
        assert_eq!(
            checked(|c| c.allowed_domains = vec!["b.example.com".to_owned(), "a.example.com".to_owned()]),
            Err(BrowserRunRefusal::Policy),
            "an unsorted host list"
        );
        assert_eq!(
            checked(|c| c.profile_scope = "persistent".to_owned()),
            Err(BrowserRunRefusal::Policy)
        );
        assert_eq!(
            checked(|c| c.view_scope = "guest".to_owned()),
            Err(BrowserRunRefusal::Policy)
        );
    }

    #[test]
    fn the_canonical_host_rules_accept_only_canonical_hosts() {
        for host in ["docs.example.org", "fixture.test", "a-b.example.co"] {
            assert!(valid_host(host), "{host} is canonical");
        }
        for host in [
            "10.0.0.1",
            "::1",
            "[::1]",
            "localhost",
            "api.localhost",
            "*.example.com",
            "intranet",
            "example.123",
            "Example.com",
            "https://example.com",
            "example.com/path",
            "example.com:443",
            "",
            "-bad.example.com",
            "bad-.example.com",
            "double..dot.com",
        ] {
            assert!(!valid_host(host), "{host:?} must be refused");
        }
    }

    #[test]
    fn frames_go_to_the_starter_alone() {
        assert!(frames_visible_to("user-2", "user-2"));
        assert!(!frames_visible_to("user-3", "user-2"));
        assert!(!frames_visible_to("", ""), "no starter, no frames");
    }

    #[test]
    fn a_grant_lives_the_run_limit_plus_a_minute_and_at_most_ten_minutes() {
        assert_eq!(room_grant_ttl_seconds(30), 90);
        assert_eq!(room_grant_ttl_seconds(300), 360);
        assert_eq!(room_grant_ttl_seconds(600), 600);
        assert_eq!(room_grant_ttl_seconds(u32::MAX), 600);
    }

    #[test]
    fn a_space_admits_a_room_run_only_below_its_limit() {
        assert!(capacity_admits(0));
        assert!(capacity_admits(1));
        assert!(!capacity_admits(2));
    }

    #[test]
    fn the_rollout_switch_is_on_only_for_an_on_word() {
        for on in ["1", "true", "on", "yes", "enabled", " TRUE "] {
            assert!(space_browser_run_api_enabled(Some(on)), "{on:?} switches on");
        }
        for off in [None, Some(""), Some("0"), Some("off"), Some("no"), Some("tru")] {
            assert!(!space_browser_run_api_enabled(off), "{off:?} stays off");
        }
        assert_eq!(SPACE_BROWSER_RUN_API_ENV, "SPACE_BROWSER_RUN_API");
    }

    #[test]
    fn the_control_key_names_the_variable_it_cannot_read() {
        let raw = control_key().verifying_key().to_bytes();
        let encoded = URL_SAFE_NO_PAD.encode(raw);
        assert_eq!(
            ControlDecisionKey::from_config(None, Some(&encoded)).err(),
            Some(CONTROL_KEY_ID_ENV)
        );
        assert_eq!(
            ControlDecisionKey::from_config(Some("  "), Some(&encoded)).err(),
            Some(CONTROL_KEY_ID_ENV)
        );
        assert_eq!(
            ControlDecisionKey::from_config(Some(KEY_ID), None).err(),
            Some(CONTROL_PUBLIC_KEY_ENV)
        );
        assert_eq!(
            ControlDecisionKey::from_config(Some(KEY_ID), Some("not a key!")).err(),
            Some(CONTROL_PUBLIC_KEY_ENV)
        );
        let short = URL_SAFE_NO_PAD.encode(&raw[..16]);
        assert_eq!(
            ControlDecisionKey::from_config(Some(KEY_ID), Some(&short)).err(),
            Some(CONTROL_PUBLIC_KEY_ENV)
        );
    }

    #[test]
    fn the_control_key_accepts_unpadded_padded_and_standard_base64() {
        let raw = control_key().verifying_key().to_bytes();
        for encoded in [
            URL_SAFE_NO_PAD.encode(raw),
            URL_SAFE.encode(raw),
            STANDARD.encode(raw),
        ] {
            let key = ControlDecisionKey::from_config(Some(KEY_ID), Some(&encoded))
                .expect("a Control key in any accepted encoding");
            assert_eq!(key.id(), KEY_ID);
        }
    }
}
