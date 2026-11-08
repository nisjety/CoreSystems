//! Key provider for profile envelopes (WS17.E1).
//!
//! The trait wraps and unwraps the per-profile DEK. The interim provider,
//! `EnvKeyProvider`, holds master keys from the edge environment
//! (`QUARRY_PROFILE_MASTER_KEYS`, a JSON object of `{"<id>": "<base64 32 bytes>"}`,
//! with `QUARRY_PROFILE_MASTER_KEY_ACTIVE` naming the master used for new wraps).
//! The Control Plane provider, `OrgCoreKeyProvider` (WS17.E2b), has org-core
//! mint and unwrap DEKs under the org's KEK (`key_id` `oc:<kek_version>`), and
//! `KeyRouter` picks the provider by the `env:`/`oc:` prefix of a stored row so
//! `env:` rows reseal lazily under org-core.
//!
//! Wrapped DEK layout: `nonce(12) || AES-256-GCM(master, dek) || tag(16)`, with
//! AAD `"quarry.profile.wrap\0v1\0" || len32(org) || org || len32(profile) ||
//! profile`. The `key_id` (`env:<master_id>`) lives in the envelope header, not
//! in the wrap. Unwrapped DEKs are cached for 300 s (at most 4096 entries,
//! evictable by org) and are never logged.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;

use crate::profile_crypto::push_len_prefixed;

/// Prefix of `key_id` values minted by the env provider.
pub const ENV_KEY_ID_PREFIX: &str = "env:";

/// TTL of an unwrapped DEK in the cache.
pub const KEY_CACHE_TTL_S: u64 = 300;

/// Maximum number of cached unwrapped DEKs.
pub const KEY_CACHE_MAX_ENTRIES: u64 = 4096;

/// Why a key operation failed. Carries a class only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// The provider cannot answer right now (for example the Control Plane is
    /// down). The caller must not treat the profile as absent.
    Unavailable,
    /// The key that wrapped the DEK is no longer configured.
    Destroyed,
    /// The wrap does not verify for this org or profile.
    Denied,
    /// Malformed wrap or key material.
    Invalid,
}

impl KeyError {
    pub fn class(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Destroyed => "destroyed",
            Self::Denied => "denied",
            Self::Invalid => "invalid",
        }
    }
}

/// A new DEK with its wrap and the key that produced the wrap.
pub struct GeneratedDek {
    pub dek: Zeroizing<[u8; 32]>,
    pub wrapped: Vec<u8>,
    pub key_id: String,
}

/// Wraps and unwraps per-profile DEKs.
pub trait ProfileKeyProvider: Send + Sync {
    /// A fresh random DEK, wrapped under the current key.
    fn generate(&self, org: &str, profile: &str) -> Result<GeneratedDek, KeyError> {
        let mut dek = Zeroizing::new([0u8; 32]);
        SystemRandom::new()
            .fill(&mut dek[..])
            .map_err(|_| KeyError::Unavailable)?;
        let (wrapped, key_id) = self.wrap(org, profile, &dek)?;
        Ok(GeneratedDek {
            dek,
            wrapped,
            key_id,
        })
    }

    /// Wrap `dek` under the current key. Returns the wrap and its `key_id`.
    fn wrap(&self, org: &str, profile: &str, dek: &[u8; 32]) -> Result<(Vec<u8>, String), KeyError>;

    /// Unwrap a DEK that was wrapped under `key_id`.
    fn unwrap(
        &self,
        org: &str,
        profile: &str,
        key_id: &str,
        wrapped: &[u8],
    ) -> Result<Zeroizing<[u8; 32]>, KeyError>;

    /// The `key_id` that new wraps use.
    fn current_key_id(&self) -> String;

    /// Whether a row of `org` sealed under `key_id` is already under the current
    /// key (no reseal needed). A provider whose current key is per org (org-core)
    /// overrides this; the default compares with `current_key_id`.
    fn is_current_key(&self, _org: &str, key_id: &str) -> bool {
        key_id == self.current_key_id()
    }

    /// Drop any cached unwrapped DEKs of one org (WS17.E3 erasure). A provider
    /// without a cache does nothing.
    fn evict_org(&self, _org: &str) {}

    /// Unwrap, then wrap under the current key (used after a KEK rotation).
    fn rewrap(
        &self,
        org: &str,
        profile: &str,
        key_id: &str,
        wrapped: &[u8],
    ) -> Result<(Vec<u8>, String), KeyError> {
        let dek = self.unwrap(org, profile, key_id, wrapped)?;
        self.wrap(org, profile, &dek)
    }
}

/// Why the env master-key configuration was refused at boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyConfigError {
    /// The JSON object is missing, empty, or does not parse.
    Malformed,
    /// A master key is not 32 bytes of base64.
    BadKeyLength,
    /// `QUARRY_PROFILE_MASTER_KEY_ACTIVE` names no configured key.
    ActiveNotConfigured,
}

impl KeyConfigError {
    pub fn class(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::BadKeyLength => "bad_key_length",
            Self::ActiveNotConfigured => "active_not_configured",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct UnwrapCacheKey {
    org: String,
    profile: String,
    key_id: String,
    wrapped_digest: [u8; 32],
}

/// Interim `env` provider: master keys from the edge environment.
pub struct EnvKeyProvider {
    masters: HashMap<String, Zeroizing<[u8; 32]>>,
    active: String,
    cache: moka::sync::Cache<UnwrapCacheKey, Zeroizing<[u8; 32]>>,
}

impl EnvKeyProvider {
    /// Build from the two environment values. Refuses anything weak, so the
    /// boot check can name the problem instead of falling back silently.
    pub fn from_config(master_keys_json: &str, active: &str) -> Result<Self, KeyConfigError> {
        let raw: HashMap<String, String> =
            serde_json::from_str(master_keys_json).map_err(|_| KeyConfigError::Malformed)?;
        if raw.is_empty() {
            return Err(KeyConfigError::Malformed);
        }
        let mut masters = HashMap::with_capacity(raw.len());
        for (id, b64) in raw {
            let bytes = B64
                .decode(b64.as_bytes())
                .map_err(|_| KeyConfigError::BadKeyLength)?;
            let arr: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| KeyConfigError::BadKeyLength)?;
            masters.insert(id, Zeroizing::new(arr));
        }
        if !masters.contains_key(active) {
            return Err(KeyConfigError::ActiveNotConfigured);
        }
        Ok(Self {
            masters,
            active: active.to_string(),
            cache: moka::sync::Cache::builder()
                .max_capacity(KEY_CACHE_MAX_ENTRIES)
                .time_to_live(Duration::from_secs(KEY_CACHE_TTL_S))
                .support_invalidation_closures()
                .build(),
        })
    }

    /// Drop every cached DEK of one org (called on profile delete and org purge).
    pub fn evict_org(&self, org: &str) {
        let org = org.to_string();
        let _ = self
            .cache
            .invalidate_entries_if(move |k, _| k.org == org);
    }

    /// Entries currently cached (after pending maintenance). Tests and metrics.
    pub fn cached_entries(&self) -> u64 {
        self.cache.run_pending_tasks();
        self.cache.entry_count()
    }
}

fn wrap_aad(org: &str, profile: &str) -> Vec<u8> {
    let mut out = b"quarry.profile.wrap\0v1\0".to_vec();
    push_len_prefixed(&mut out, org.as_bytes());
    push_len_prefixed(&mut out, profile.as_bytes());
    out
}

fn wrap_with(
    master: &[u8; 32],
    org: &str,
    profile: &str,
    dek: &[u8; 32],
) -> Result<Vec<u8>, KeyError> {
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| KeyError::Unavailable)?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, master).map_err(|_| KeyError::Invalid)?,
    );
    let mut in_out = dek.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(wrap_aad(org, profile)),
        &mut in_out,
    )
    .map_err(|_| KeyError::Invalid)?;
    let mut out = Vec::with_capacity(NONCE_LEN + in_out.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&in_out);
    Ok(out)
}

fn unwrap_with(
    master: &[u8; 32],
    org: &str,
    profile: &str,
    wrapped: &[u8],
) -> Result<Zeroizing<[u8; 32]>, KeyError> {
    // nonce + 32-byte DEK + 16-byte tag.
    if wrapped.len() != NONCE_LEN + 32 + AES_256_GCM.tag_len() {
        return Err(KeyError::Invalid);
    }
    let nonce: [u8; NONCE_LEN] = wrapped[..NONCE_LEN]
        .try_into()
        .map_err(|_| KeyError::Invalid)?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, master).map_err(|_| KeyError::Invalid)?,
    );
    let mut buf = wrapped[NONCE_LEN..].to_vec();
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(wrap_aad(org, profile)),
            &mut buf,
        )
        .map_err(|_| KeyError::Denied)?;
    let arr: [u8; 32] = plain.try_into().map_err(|_| KeyError::Invalid)?;
    Ok(Zeroizing::new(arr))
}

impl ProfileKeyProvider for EnvKeyProvider {
    fn wrap(&self, org: &str, profile: &str, dek: &[u8; 32]) -> Result<(Vec<u8>, String), KeyError> {
        let master = self.masters.get(&self.active).ok_or(KeyError::Destroyed)?;
        let wrapped = wrap_with(master, org, profile, dek)?;
        Ok((wrapped, self.current_key_id()))
    }

    fn unwrap(
        &self,
        org: &str,
        profile: &str,
        key_id: &str,
        wrapped: &[u8],
    ) -> Result<Zeroizing<[u8; 32]>, KeyError> {
        let cache_key = UnwrapCacheKey {
            org: org.to_string(),
            profile: profile.to_string(),
            key_id: key_id.to_string(),
            wrapped_digest: *blake3::hash(wrapped).as_bytes(),
        };
        if let Some(hit) = self.cache.get(&cache_key) {
            return Ok(hit);
        }
        let master_id = key_id
            .strip_prefix(ENV_KEY_ID_PREFIX)
            .ok_or(KeyError::Invalid)?;
        let master = self.masters.get(master_id).ok_or(KeyError::Destroyed)?;
        let dek = unwrap_with(master, org, profile, wrapped)?;
        self.cache.insert(cache_key, dek.clone());
        Ok(dek)
    }

    fn current_key_id(&self) -> String {
        format!("{ENV_KEY_ID_PREFIX}{}", self.active)
    }

    fn evict_org(&self, org: &str) {
        EnvKeyProvider::evict_org(self, org);
    }
}

// ---------------------------------------------------------------------------
// WS17.E2b: the org-core key provider and the key router.
// ---------------------------------------------------------------------------

/// Prefix of `key_id` values minted by org-core (`oc:<kek_version>`).
pub const OC_KEY_ID_PREFIX: &str = "oc:";

/// Env var with org-core's base URL (also read through `EdgeConfig`).
pub const ORG_CORE_KEYS_URL_ENV: &str = "QUARRY_EDGE__ORG_CORE_KEYS_URL";

/// Env var with the quarry-edge org-core service token. Read from the
/// environment by name only, never through `EdgeConfig`, so it is never part of
/// a config dump.
pub const ORG_CORE_KEYS_TOKEN_ENV: &str = "QUARRY_EDGE__ORG_CORE_KEYS_TOKEN";

/// The service principal the token belongs to (org-core `ORG_CORE_SERVICE_CREDENTIALS`).
pub const ORG_CORE_KEYS_PRINCIPAL: &str = "quarry-edge";

/// The purpose org-core binds into the DEK's associated data.
pub const ORG_CORE_KEYS_PURPOSE: &str = "quarry.profile";

/// Timeout of one org-core call (connect and whole request).
pub const ORG_CORE_KEYS_TIMEOUT: Duration = Duration::from_secs(3);

/// Largest org-core answer the provider reads. A data-key answer is under 1 KiB.
pub const ORG_CORE_KEYS_MAX_RESPONSE_BYTES: usize = 16 * 1024;

/// Largest wrapped DEK the provider sends or accepts (org-core caps bodies at 8 KiB).
pub const ORG_CORE_KEYS_MAX_WRAPPED_BYTES: usize = 1024;

const ORG_ID_MAX_LEN: usize = 128;
const PROFILE_ID_MAX_LEN: usize = 256;

/// Why the org-core provider was refused at boot. Carries a class only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrgCoreConfigError {
    /// `QUARRY_EDGE__ORG_CORE_KEYS_URL` is missing or blank.
    MissingUrl,
    /// The URL does not parse or is not `http(s)://host`.
    BadUrl,
    /// `QUARRY_EDGE__ORG_CORE_KEYS_TOKEN` is missing, blank, or not a header value.
    MissingToken,
    /// The HTTP client or its runtime could not be built.
    Client,
}

impl OrgCoreConfigError {
    pub fn class(self) -> &'static str {
        match self {
            Self::MissingUrl => "missing_url",
            Self::BadUrl => "bad_url",
            Self::MissingToken => "missing_token",
            Self::Client => "client",
        }
    }
}

/// The `KeyError` an org-core HTTP status maps to. 503 (and every other 5xx,
/// after the one retry) is `Unavailable`, 410 `Destroyed`, 403 `Denied`. A 404
/// (org or key version unknown) and a 422 (wrong org, profile or a tampered
/// wrap: org-core gives one answer for all) are `Denied`. A 401 means the
/// credential drifted between the two stacks: `Unavailable`, never "absent".
pub fn org_core_status_error(status: u16) -> KeyError {
    match status {
        410 => KeyError::Destroyed,
        403 | 404 | 422 => KeyError::Denied,
        401 | 408 | 429 => KeyError::Unavailable,
        s if s >= 500 => KeyError::Unavailable,
        _ => KeyError::Invalid,
    }
}

/// Parse `oc:<kek_version>` (version 1 or more, digits only).
fn parse_oc_version(key_id: &str) -> Option<u32> {
    let digits = key_id.strip_prefix(OC_KEY_ID_PREFIX)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u32>().ok().filter(|v| *v >= 1)
}

fn valid_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

/// An org id the provider puts in an org-core path. On top of `valid_id` it
/// may hold no whitespace at all: no real org id has one, and a malformed id
/// is refused here (`Invalid`) rather than sent to org-core, whose 404 would
/// read as `Denied`.
fn valid_org_id(value: &str) -> bool {
    valid_id(value, ORG_ID_MAX_LEN) && !value.chars().any(char::is_whitespace)
}

/// The final outcome of one call (after the retry). Never carries a body
/// except a successful one, and that one is zeroized on drop.
enum Attempt {
    Body(Zeroizing<Vec<u8>>),
    Status(u16),
    Transport,
    TooLarge,
}

#[derive(serde::Serialize)]
struct OcContext<'a> {
    profile_id: &'a str,
}

#[derive(serde::Serialize)]
struct OcRequest<'a> {
    purpose: &'a str,
    context: OcContext<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wrapped_dek_b64: Option<String>,
}

#[derive(serde::Deserialize)]
struct OcData<T> {
    data: T,
}

#[derive(serde::Deserialize)]
struct OcGenerated {
    key_id: String,
    kek_version: u32,
    dek_b64: String,
    wrapped_dek_b64: String,
}

#[derive(serde::Deserialize)]
struct OcUnwrapped {
    dek_b64: String,
    current_kek_version: u32,
}

#[derive(serde::Deserialize)]
struct OcRewrapped {
    key_id: String,
    kek_version: u32,
    wrapped_dek_b64: String,
}

/// One log line per failed call: the operation, the HTTP status (0 when there
/// was none) and the error class. Never a body, a key, a token or the org id.
fn log_key_failure(op: &'static str, status: u16, e: KeyError) {
    tracing::warn!(
        op = op,
        status = status,
        key_error = e.class(),
        "org-core key call failed"
    );
}

fn decode_dek(b64: &str) -> Result<Zeroizing<[u8; 32]>, KeyError> {
    let bytes = Zeroizing::new(B64.decode(b64.as_bytes()).map_err(|_| KeyError::Invalid)?);
    let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| KeyError::Invalid)?;
    Ok(Zeroizing::new(arr))
}

fn decode_wrapped(b64: &str) -> Result<Vec<u8>, KeyError> {
    let bytes = B64.decode(b64.as_bytes()).map_err(|_| KeyError::Invalid)?;
    if bytes.is_empty() || bytes.len() > ORG_CORE_KEYS_MAX_WRAPPED_BYTES {
        return Err(KeyError::Invalid);
    }
    Ok(bytes)
}

/// Read a success body, refusing anything over the cap.
async fn read_capped(mut resp: reqwest::Response) -> Attempt {
    if resp
        .content_length()
        .is_some_and(|n| n > ORG_CORE_KEYS_MAX_RESPONSE_BYTES as u64)
    {
        return Attempt::TooLarge;
    }
    // Reserved up front so the buffer never reallocates (a realloc would leave
    // an unzeroized copy of the DEK behind).
    let mut buf = Zeroizing::new(Vec::with_capacity(ORG_CORE_KEYS_MAX_RESPONSE_BYTES));
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if buf.len() + chunk.len() > ORG_CORE_KEYS_MAX_RESPONSE_BYTES {
                    return Attempt::TooLarge;
                }
                buf.extend_from_slice(&chunk);
            }
            Ok(None) => return Attempt::Body(buf),
            Err(_) => return Attempt::Transport,
        }
    }
}

/// POST once, and once more on a 5xx or a transport error. Logs nothing: it runs
/// on the provider's own runtime, and the caller logs the outcome.
async fn post_with_retry(
    client: reqwest::Client,
    url: reqwest::Url,
    token: reqwest::header::HeaderValue,
    body: Vec<u8>,
) -> Attempt {
    let mut last = Attempt::Transport;
    for attempt in 0..2 {
        let sent = client
            .post(url.clone())
            .header("x-service-id", ORG_CORE_KEYS_PRINCIPAL)
            .header("x-service-token", token.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT, "application/json")
            .body(body.clone())
            .send()
            .await;
        match sent {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return read_capped(resp).await;
                }
                last = Attempt::Status(status.as_u16());
                if status.is_server_error() && attempt == 0 {
                    continue;
                }
                return last;
            }
            Err(_) => {
                last = Attempt::Transport;
            }
        }
    }
    last
}

/// Wait for the provider runtime's answer without starving the caller's runtime.
fn wait_for<T>(rx: std::sync::mpsc::Receiver<T>) -> Option<T> {
    let on_multi_thread = matches!(
        tokio::runtime::Handle::try_current().map(|h| h.runtime_flavor()),
        Ok(tokio::runtime::RuntimeFlavor::MultiThread)
    );
    if on_multi_thread {
        tokio::task::block_in_place(|| rx.recv().ok())
    } else {
        rx.recv().ok()
    }
}

/// `org_core` provider (WS17.E2b): DEKs are minted, unwrapped and rewrapped by
/// org-core (`/internal/orgs/:orgId/keys/data-keys*`, WS17.E2a) under the org's
/// KEK. The `key_id` is `oc:<kek_version>`.
///
/// The trait is synchronous, so the calls run on a small runtime the provider
/// owns and the caller waits (`block_in_place` on a multi-thread runtime). Each
/// call has a 3 s timeout and one retry on a 5xx or a transport error. Bodies,
/// DEKs and the token are never logged. Unwrapped DEKs are cached like the env
/// provider's (300 s, 4096 entries, evictable by org), and the newest KEK
/// version seen per org drives the lazy reseal (`is_current_key`).
pub struct OrgCoreKeyProvider {
    client: reqwest::Client,
    base: reqwest::Url,
    token: reqwest::header::HeaderValue,
    rt: Option<tokio::runtime::Runtime>,
    cache: moka::sync::Cache<UnwrapCacheKey, Zeroizing<[u8; 32]>>,
    current: moka::sync::Cache<String, u32>,
}

impl OrgCoreKeyProvider {
    /// Build from a base URL and the service token. Refuses a blank or
    /// malformed value, so the boot check can name it.
    pub fn new(base_url: &str, token: &str) -> Result<Self, OrgCoreConfigError> {
        let base_url = base_url.trim();
        if base_url.is_empty() {
            return Err(OrgCoreConfigError::MissingUrl);
        }
        let base = reqwest::Url::parse(base_url).map_err(|_| OrgCoreConfigError::BadUrl)?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || base.cannot_be_a_base()
            || base.query().is_some()
        {
            return Err(OrgCoreConfigError::BadUrl);
        }
        let token = token.trim();
        if token.is_empty() {
            return Err(OrgCoreConfigError::MissingToken);
        }
        let mut token = reqwest::header::HeaderValue::from_str(token)
            .map_err(|_| OrgCoreConfigError::MissingToken)?;
        token.set_sensitive(true);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("quarry-org-core-keys")
            .enable_all()
            .build()
            .map_err(|_| OrgCoreConfigError::Client)?;
        let client = {
            let _entered = rt.enter();
            reqwest::Client::builder()
                .timeout(ORG_CORE_KEYS_TIMEOUT)
                .connect_timeout(ORG_CORE_KEYS_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| OrgCoreConfigError::Client)?
        };
        Ok(Self {
            client,
            base,
            token,
            rt: Some(rt),
            cache: moka::sync::Cache::builder()
                .max_capacity(KEY_CACHE_MAX_ENTRIES)
                .time_to_live(Duration::from_secs(KEY_CACHE_TTL_S))
                .support_invalidation_closures()
                .build(),
            current: moka::sync::Cache::builder()
                .max_capacity(KEY_CACHE_MAX_ENTRIES)
                .time_to_live(Duration::from_secs(KEY_CACHE_TTL_S))
                .build(),
        })
    }

    /// Build from the environment: the URL from `url` (the `EdgeConfig` value)
    /// or `QUARRY_EDGE__ORG_CORE_KEYS_URL`, the token from
    /// `QUARRY_EDGE__ORG_CORE_KEYS_TOKEN`.
    pub fn from_env(url: Option<&str>) -> Result<Self, OrgCoreConfigError> {
        let url = match url.filter(|u| !u.trim().is_empty()) {
            Some(u) => u.to_string(),
            None => std::env::var(ORG_CORE_KEYS_URL_ENV).unwrap_or_default(),
        };
        let token = Zeroizing::new(std::env::var(ORG_CORE_KEYS_TOKEN_ENV).unwrap_or_default());
        Self::new(&url, &token)
    }

    /// Drop every cached DEK and the known KEK version of one org.
    pub fn evict_org(&self, org: &str) {
        let owned = org.to_string();
        let _ = self
            .cache
            .invalidate_entries_if(move |k, _| k.org == owned);
        self.current.invalidate(org);
    }

    /// Entries currently cached (after pending maintenance). Tests and metrics.
    pub fn cached_entries(&self) -> u64 {
        self.cache.run_pending_tasks();
        self.cache.entry_count()
    }

    fn url_for(&self, org: &str, tail: &[&str]) -> Result<reqwest::Url, KeyError> {
        if !valid_org_id(org) {
            return Err(KeyError::Invalid);
        }
        let mut url = self.base.clone();
        {
            let mut segments = url.path_segments_mut().map_err(|_| KeyError::Invalid)?;
            segments
                .pop_if_empty()
                .extend(["internal", "orgs", org, "keys"])
                .extend(tail.iter().copied());
        }
        Ok(url)
    }

    fn note_current(&self, org: &str, version: u32) {
        let newest = self.current.get(org).map_or(version, |seen| seen.max(version));
        self.current.insert(org.to_string(), newest);
    }

    fn request_body(
        profile: &str,
        key_id: Option<&str>,
        wrapped: Option<&[u8]>,
    ) -> Result<Vec<u8>, KeyError> {
        if !valid_id(profile, PROFILE_ID_MAX_LEN) {
            return Err(KeyError::Invalid);
        }
        let request = OcRequest {
            purpose: ORG_CORE_KEYS_PURPOSE,
            context: OcContext { profile_id: profile },
            key_id,
            wrapped_dek_b64: wrapped.map(|w| B64.encode(w)),
        };
        serde_json::to_vec(&request).map_err(|_| KeyError::Invalid)
    }

    /// One org-core call. Logs the failure class (never the body) and returns
    /// the success body.
    fn call(
        &self,
        op: &'static str,
        org: &str,
        tail: &[&str],
        body: Vec<u8>,
    ) -> Result<Zeroizing<Vec<u8>>, KeyError> {
        let url = self.url_for(org, tail)?;
        let Some(rt) = self.rt.as_ref() else {
            log_key_failure(op, 0, KeyError::Unavailable);
            return Err(KeyError::Unavailable);
        };
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let fut = post_with_retry(self.client.clone(), url, self.token.clone(), body);
        rt.spawn(async move {
            let _ = tx.send(fut.await);
        });
        match wait_for(rx) {
            Some(Attempt::Body(raw)) => Ok(raw),
            Some(Attempt::Status(status)) => {
                let e = org_core_status_error(status);
                log_key_failure(op, status, e);
                Err(e)
            }
            Some(Attempt::TooLarge) => {
                log_key_failure(op, 200, KeyError::Invalid);
                Err(KeyError::Invalid)
            }
            Some(Attempt::Transport) | None => {
                log_key_failure(op, 0, KeyError::Unavailable);
                Err(KeyError::Unavailable)
            }
        }
    }

    fn parse<T: serde::de::DeserializeOwned>(
        op: &'static str,
        raw: &[u8],
    ) -> Result<T, KeyError> {
        match serde_json::from_slice::<OcData<T>>(raw) {
            Ok(parsed) => Ok(parsed.data),
            Err(_) => {
                log_key_failure(op, 200, KeyError::Invalid);
                Err(KeyError::Invalid)
            }
        }
    }

    fn cache_key(org: &str, profile: &str, key_id: &str, wrapped: &[u8]) -> UnwrapCacheKey {
        UnwrapCacheKey {
            org: org.to_string(),
            profile: profile.to_string(),
            key_id: key_id.to_string(),
            wrapped_digest: *blake3::hash(wrapped).as_bytes(),
        }
    }
}

impl Drop for OrgCoreKeyProvider {
    fn drop(&mut self) {
        // A runtime may not be dropped in an async context; this never blocks.
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

impl ProfileKeyProvider for OrgCoreKeyProvider {
    /// org-core mints the DEK (`POST .../data-keys`); it never leaves the
    /// answer's zeroized buffer except as the returned DEK.
    fn generate(&self, org: &str, profile: &str) -> Result<GeneratedDek, KeyError> {
        let body = Self::request_body(profile, None, None)?;
        let raw = self.call("generate", org, &["data-keys"], body)?;
        let answer: OcGenerated = Self::parse("generate", &raw)?;
        let dek_b64 = Zeroizing::new(answer.dek_b64);
        if parse_oc_version(&answer.key_id) != Some(answer.kek_version) {
            log_key_failure("generate", 200, KeyError::Invalid);
            return Err(KeyError::Invalid);
        }
        let dek = decode_dek(&dek_b64)?;
        let wrapped = decode_wrapped(&answer.wrapped_dek_b64)?;
        self.note_current(org, answer.kek_version);
        self.cache.insert(
            Self::cache_key(org, profile, &answer.key_id, &wrapped),
            dek.clone(),
        );
        Ok(GeneratedDek {
            dek,
            wrapped,
            key_id: answer.key_id,
        })
    }

    /// Not supported: org-core has no route that wraps a caller's DEK (the DEK
    /// is minted there). New wraps go through `generate`, moves through `rewrap`.
    fn wrap(&self, _org: &str, _profile: &str, _dek: &[u8; 32]) -> Result<(Vec<u8>, String), KeyError> {
        Err(KeyError::Invalid)
    }

    fn unwrap(
        &self,
        org: &str,
        profile: &str,
        key_id: &str,
        wrapped: &[u8],
    ) -> Result<Zeroizing<[u8; 32]>, KeyError> {
        if parse_oc_version(key_id).is_none()
            || wrapped.is_empty()
            || wrapped.len() > ORG_CORE_KEYS_MAX_WRAPPED_BYTES
        {
            return Err(KeyError::Invalid);
        }
        let cache_key = Self::cache_key(org, profile, key_id, wrapped);
        if let Some(hit) = self.cache.get(&cache_key) {
            return Ok(hit);
        }
        let body = Self::request_body(profile, Some(key_id), Some(wrapped))?;
        let raw = self.call("unwrap", org, &["data-keys", "unwrap"], body)?;
        let answer: OcUnwrapped = Self::parse("unwrap", &raw)?;
        let dek_b64 = Zeroizing::new(answer.dek_b64);
        let dek = decode_dek(&dek_b64)?;
        if answer.current_kek_version >= 1 {
            self.note_current(org, answer.current_kek_version);
        }
        self.cache.insert(cache_key, dek.clone());
        Ok(dek)
    }

    /// The KEK version is per org, so there is no single current id: this is the
    /// bare prefix, which no stored row equals. Use `is_current_key`.
    fn current_key_id(&self) -> String {
        OC_KEY_ID_PREFIX.to_string()
    }

    /// An `oc:` row is current unless org-core has told us of a newer KEK
    /// version for this org (an unknown org counts as current: no churn).
    fn is_current_key(&self, org: &str, key_id: &str) -> bool {
        match parse_oc_version(key_id) {
            Some(version) => self.current.get(org).is_none_or(|newest| version >= newest),
            None => false,
        }
    }

    fn evict_org(&self, org: &str) {
        OrgCoreKeyProvider::evict_org(self, org);
    }

    /// Rewrap under the org's active KEK inside org-core (the DEK never leaves it).
    fn rewrap(
        &self,
        org: &str,
        profile: &str,
        key_id: &str,
        wrapped: &[u8],
    ) -> Result<(Vec<u8>, String), KeyError> {
        if parse_oc_version(key_id).is_none()
            || wrapped.is_empty()
            || wrapped.len() > ORG_CORE_KEYS_MAX_WRAPPED_BYTES
        {
            return Err(KeyError::Invalid);
        }
        let body = Self::request_body(profile, Some(key_id), Some(wrapped))?;
        let raw = self.call("rewrap", org, &["data-keys", "rewrap"], body)?;
        let answer: OcRewrapped = Self::parse("rewrap", &raw)?;
        if parse_oc_version(&answer.key_id) != Some(answer.kek_version) {
            log_key_failure("rewrap", 200, KeyError::Invalid);
            return Err(KeyError::Invalid);
        }
        let rewrapped = decode_wrapped(&answer.wrapped_dek_b64)?;
        self.note_current(org, answer.kek_version);
        Ok((rewrapped, answer.key_id))
    }
}

/// Picks the provider by the `key_id` prefix of a stored envelope (WS17.E2b).
///
/// New wraps always go to org-core. `env:` rows are read through the env master
/// (kept only for that, during the transition) and are never current, so the
/// envelope store reseals them lazily under a fresh org-core DEK. Without an env
/// provider an `env:` row is `Destroyed`. Any other prefix is `Invalid`.
pub struct KeyRouter {
    env: Option<Arc<dyn ProfileKeyProvider>>,
    oc: Arc<dyn ProfileKeyProvider>,
}

impl KeyRouter {
    pub fn new(env: Option<Arc<dyn ProfileKeyProvider>>, oc: Arc<dyn ProfileKeyProvider>) -> Self {
        Self { env, oc }
    }

    /// Whether `env:` rows can still be read.
    pub fn has_env(&self) -> bool {
        self.env.is_some()
    }

    fn route(&self, key_id: &str) -> Result<&dyn ProfileKeyProvider, KeyError> {
        if key_id.starts_with(ENV_KEY_ID_PREFIX) {
            self.env.as_deref().ok_or(KeyError::Destroyed)
        } else if key_id.starts_with(OC_KEY_ID_PREFIX) {
            Ok(self.oc.as_ref())
        } else {
            Err(KeyError::Invalid)
        }
    }
}

impl ProfileKeyProvider for KeyRouter {
    fn generate(&self, org: &str, profile: &str) -> Result<GeneratedDek, KeyError> {
        self.oc.generate(org, profile)
    }

    fn wrap(&self, org: &str, profile: &str, dek: &[u8; 32]) -> Result<(Vec<u8>, String), KeyError> {
        self.oc.wrap(org, profile, dek)
    }

    fn unwrap(
        &self,
        org: &str,
        profile: &str,
        key_id: &str,
        wrapped: &[u8],
    ) -> Result<Zeroizing<[u8; 32]>, KeyError> {
        self.route(key_id)?.unwrap(org, profile, key_id, wrapped)
    }

    fn current_key_id(&self) -> String {
        self.oc.current_key_id()
    }

    fn is_current_key(&self, org: &str, key_id: &str) -> bool {
        key_id.starts_with(OC_KEY_ID_PREFIX) && self.oc.is_current_key(org, key_id)
    }

    fn evict_org(&self, org: &str) {
        if let Some(env) = &self.env {
            env.evict_org(org);
        }
        self.oc.evict_org(org);
    }

    /// `oc:` rows rewrap inside org-core. An `env:` row cannot keep its DEK across
    /// the move (org-core does not wrap a caller's DEK): it reseals with a fresh
    /// DEK through `generate` instead, so this refuses it.
    fn rewrap(
        &self,
        org: &str,
        profile: &str,
        key_id: &str,
        wrapped: &[u8],
    ) -> Result<(Vec<u8>, String), KeyError> {
        if key_id.starts_with(OC_KEY_ID_PREFIX) {
            self.oc.rewrap(org, profile, key_id, wrapped)
        } else {
            Err(KeyError::Invalid)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64_key(byte: u8) -> String {
        B64.encode([byte; 32])
    }

    fn provider() -> EnvKeyProvider {
        let json = format!(r#"{{"v1":"{}","v2":"{}"}}"#, b64_key(1), b64_key(2));
        EnvKeyProvider::from_config(&json, "v1").expect("config")
    }

    #[test]
    fn env_provider_wrap_unwrap() {
        let p = provider();
        let g = p.generate("org_a", "prof_1").expect("generate");
        assert_eq!(g.key_id, "env:v1");
        let back = p
            .unwrap("org_a", "prof_1", &g.key_id, &g.wrapped)
            .expect("unwrap");
        assert_eq!(*back, *g.dek);
    }

    #[test]
    fn unwrap_with_wrong_org_fails() {
        let p = provider();
        let g = p.generate("org_a", "prof_1").expect("generate");
        assert_eq!(
            p.unwrap("org_b", "prof_1", &g.key_id, &g.wrapped).err(),
            Some(KeyError::Denied)
        );
        assert_eq!(
            p.unwrap("org_a", "prof_2", &g.key_id, &g.wrapped).err(),
            Some(KeyError::Denied)
        );
    }

    #[test]
    fn unknown_master_is_destroyed_and_rewrap_moves_to_current() {
        let p = provider();
        let other = EnvKeyProvider::from_config(
            &format!(r#"{{"v2":"{}"}}"#, b64_key(2)),
            "v2",
        )
        .unwrap();
        let g = p.generate("org_a", "prof_1").unwrap();
        assert_eq!(
            other.unwrap("org_a", "prof_1", "env:v1", &g.wrapped).err(),
            Some(KeyError::Destroyed)
        );
        let (rewrapped, key_id) = p.rewrap("org_a", "prof_1", "env:v1", &g.wrapped).unwrap();
        assert_eq!(key_id, "env:v1");
        assert!(p.unwrap("org_a", "prof_1", &key_id, &rewrapped).is_ok());
    }

    #[test]
    fn key_cache_ttl_and_org_eviction() {
        let p = provider();
        let g = p.generate("org_a", "prof_1").unwrap();
        let h = p.generate("org_b", "prof_1").unwrap();
        p.unwrap("org_a", "prof_1", &g.key_id, &g.wrapped).unwrap();
        p.unwrap("org_b", "prof_1", &h.key_id, &h.wrapped).unwrap();
        assert_eq!(p.cached_entries(), 2);
        p.evict_org("org_a");
        assert_eq!(p.cached_entries(), 1);
    }

    #[test]
    fn config_refuses_weak_or_missing_material() {
        assert_eq!(
            EnvKeyProvider::from_config("{}", "v1").err(),
            Some(KeyConfigError::Malformed)
        );
        assert_eq!(
            EnvKeyProvider::from_config("not json", "v1").err(),
            Some(KeyConfigError::Malformed)
        );
        let short = format!(r#"{{"v1":"{}"}}"#, B64.encode([1u8; 16]));
        assert_eq!(
            EnvKeyProvider::from_config(&short, "v1").err(),
            Some(KeyConfigError::BadKeyLength)
        );
        let ok = format!(r#"{{"v1":"{}"}}"#, b64_key(1));
        assert_eq!(
            EnvKeyProvider::from_config(&ok, "v9").err(),
            Some(KeyConfigError::ActiveNotConfigured)
        );
    }
}

/// WS17.E2b: the org-core provider against a wiremock org-core, and the router.
/// The provider runs its HTTP on its own runtime and the caller blocks, so these
/// are current-thread tests (the wiremock server runs on its own thread).
#[cfg(test)]
mod org_core_tests {
    use super::*;
    use std::sync::Mutex;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TOKEN: &str = "oc-test-token-not-a-secret";
    const DEK: [u8; 32] = [7u8; 32];
    const MARKER: &str = "BODY-MARKER-MUST-NOT-BE-LOGGED";

    fn wrapped() -> Vec<u8> {
        vec![9u8; 60]
    }

    fn provider(server: &MockServer) -> OrgCoreKeyProvider {
        OrgCoreKeyProvider::new(&server.uri(), TOKEN).expect("provider")
    }

    fn generated_body(version: u32) -> serde_json::Value {
        serde_json::json!({"data": {
            "key_id": format!("oc:{version}"),
            "kek_version": version,
            "alg": "A256GCM",
            "dek_b64": B64.encode(DEK),
            "wrapped_dek_b64": B64.encode(wrapped()),
        }})
    }

    fn unwrapped_body(key_id: &str, current: u32) -> serde_json::Value {
        serde_json::json!({"data": {
            "key_id": key_id,
            "dek_b64": B64.encode(DEK),
            "current_kek_version": current,
        }})
    }

    fn unwrap_path(org: &str) -> String {
        format!("/internal/orgs/{org}/keys/data-keys/unwrap")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn org_core_provider_maps_statuses() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/orgs/org_a/keys/data-keys"))
            .and(header("x-service-id", ORG_CORE_KEYS_PRINCIPAL))
            .and(header("x-service-token", TOKEN))
            .respond_with(ResponseTemplate::new(201).set_body_json(generated_body(3)))
            .expect(1)
            .mount(&server)
            .await;
        let p = provider(&server);

        // 201: a fresh DEK under oc:3, cached so its unwrap makes no call.
        let g = p.generate("org_a", "prof_1").expect("generate");
        assert_eq!(g.key_id, "oc:3");
        assert_eq!(*g.dek, DEK);
        assert_eq!(g.wrapped, wrapped());
        let back = p.unwrap("org_a", "prof_1", &g.key_id, &g.wrapped).expect("cached unwrap");
        assert_eq!(*back, DEK);
        assert!(p.is_current_key("org_a", "oc:3"));

        // Error statuses. 503 is retried once (two calls), the rest are not.
        for (org, status, want, calls) in [
            ("org_503", 503u16, KeyError::Unavailable, 2u64),
            ("org_500", 500, KeyError::Unavailable, 2),
            ("org_410", 410, KeyError::Destroyed, 1),
            ("org_403", 403, KeyError::Denied, 1),
            ("org_404", 404, KeyError::Denied, 1),
            ("org_422", 422, KeyError::Denied, 1),
            ("org_401", 401, KeyError::Unavailable, 1),
        ] {
            Mock::given(method("POST"))
                .and(path(unwrap_path(org)))
                .respond_with(ResponseTemplate::new(status))
                .expect(calls)
                .mount(&server)
                .await;
            assert_eq!(
                p.unwrap(org, "prof_1", "oc:1", &wrapped()).err(),
                Some(want),
                "status {status}"
            );
        }

        // One 503, then 200: the retry answers, and the newer KEK version makes oc:3 stale.
        Mock::given(method("POST"))
            .and(path(unwrap_path("org_retry")))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(unwrap_path("org_retry")))
            .respond_with(ResponseTemplate::new(200).set_body_json(unwrapped_body("oc:3", 4)))
            .with_priority(2)
            .expect(1)
            .mount(&server)
            .await;
        let dek = p.unwrap("org_retry", "prof_1", "oc:3", &wrapped()).expect("retry answers");
        assert_eq!(*dek, DEK);
        assert!(!p.is_current_key("org_retry", "oc:3"), "org-core reported kek 4");
        assert!(p.is_current_key("org_retry", "oc:4"));
        assert!(p.is_current_key("org_unknown", "oc:1"), "no churn for an org not seen yet");

        // An oversized answer is refused, never parsed.
        Mock::given(method("POST"))
            .and(path(unwrap_path("org_big")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("x".repeat(ORG_CORE_KEYS_MAX_RESPONSE_BYTES + 1)),
            )
            .mount(&server)
            .await;
        assert_eq!(
            p.unwrap("org_big", "prof_1", "oc:1", &wrapped()).err(),
            Some(KeyError::Invalid)
        );

        // Malformed key ids and ids never reach org-core.
        assert_eq!(p.unwrap("org_a", "prof_1", "env:v1", &wrapped()).err(), Some(KeyError::Invalid));
        assert_eq!(p.unwrap("org_a", "prof_1", "oc:x", &wrapped()).err(), Some(KeyError::Invalid));
        assert_eq!(p.unwrap("org a", "prof_1", "oc:1", &wrapped()).err(), Some(KeyError::Invalid));
        assert_eq!(p.wrap("org_a", "prof_1", &DEK).err(), Some(KeyError::Invalid));

        // Nothing listening: Unavailable, never "absent".
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let down = OrgCoreKeyProvider::new(&format!("http://{closed}"), TOKEN).unwrap();
        assert_eq!(
            down.unwrap("org_a", "prof_1", "oc:1", &wrapped()).err(),
            Some(KeyError::Unavailable)
        );

        // Org eviction drops the cached DEK.
        assert!(p.cached_entries() >= 1);
        p.evict_org("org_a");
        p.evict_org("org_retry");
        assert_eq!(p.cached_entries(), 0);
    }

    #[test]
    fn org_core_config_is_refused_by_class() {
        assert_eq!(
            OrgCoreKeyProvider::new("", TOKEN).err(),
            Some(OrgCoreConfigError::MissingUrl)
        );
        assert_eq!(
            OrgCoreKeyProvider::new("org-core:8080", TOKEN).err(),
            Some(OrgCoreConfigError::BadUrl)
        );
        assert_eq!(
            OrgCoreKeyProvider::new("ftp://org-core", TOKEN).err(),
            Some(OrgCoreConfigError::BadUrl)
        );
        assert_eq!(
            OrgCoreKeyProvider::new("http://org-core:8080", " ").err(),
            Some(OrgCoreConfigError::MissingToken)
        );
        assert!(OrgCoreKeyProvider::new("http://org-core:8080", TOKEN).is_ok());
    }

    /// Captures every event on the installing thread.
    #[derive(Clone, Default)]
    struct EventLog(Arc<Mutex<Vec<String>>>);

    impl EventLog {
        fn lines(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    impl tracing::Subscriber for EventLog {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            struct Fields(String);
            impl tracing::field::Visit for Fields {
                fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                    self.0.push_str(&format!(" {}={:?}", field.name(), value));
                }
            }
            let mut fields = Fields(String::new());
            event.record(&mut fields);
            self.0
                .lock()
                .unwrap()
                .push(format!("{}{}", event.metadata().level(), fields.0));
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    /// See `pipeline.rs` `keep_per_thread_subscribers_reliable`: two process-wide
    /// dispatchers keep a callsite from being cached as "never" by another thread.
    fn keep_per_thread_subscribers_reliable() {
        static KEEP: std::sync::OnceLock<[tracing::Dispatch; 2]> = std::sync::OnceLock::new();
        KEEP.get_or_init(|| {
            [
                tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()),
                tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()),
            ]
        });
    }

    #[tokio::test(flavor = "current_thread")]
    async fn provider_never_logs_bodies() {
        keep_per_thread_subscribers_reliable();
        let server = MockServer::start().await;
        let dek_b64 = B64.encode(DEK);
        // A 5xx whose body carries the marker and a DEK.
        Mock::given(method("POST"))
            .and(path("/internal/orgs/org_a/keys/data-keys"))
            .respond_with(
                ResponseTemplate::new(500).set_body_string(format!("{MARKER} {dek_b64}")),
            )
            .mount(&server)
            .await;
        // A 200 that does not parse, carrying the marker and the DEK.
        Mock::given(method("POST"))
            .and(path(unwrap_path("org_a")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(format!("{{\"data\":{{\"dek_b64\":\"{dek_b64}\",\"note\":\"{MARKER}\"")),
            )
            .mount(&server)
            .await;
        // A 403 with an error envelope carrying the marker.
        Mock::given(method("POST"))
            .and(path(unwrap_path("org_b")))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "error": {"code": "forbidden", "message": MARKER, "details": {}}
            })))
            .mount(&server)
            .await;

        let p = provider(&server);
        let events = EventLog::default();
        {
            let _log = tracing::subscriber::set_default(events.clone());
            assert_eq!(p.generate("org_a", "prof_1").err(), Some(KeyError::Unavailable));
            assert_eq!(
                p.unwrap("org_a", "prof_1", "oc:1", &wrapped()).err(),
                Some(KeyError::Invalid)
            );
            assert_eq!(
                p.unwrap("org_b", "prof_1", "oc:1", &wrapped()).err(),
                Some(KeyError::Denied)
            );
        }
        let lines = events.lines();
        assert!(
            lines.iter().filter(|l| l.contains("org-core key call failed")).count() >= 3,
            "each failure is logged by class: {lines:?}"
        );
        let wrapped_b64 = B64.encode(wrapped());
        for line in &lines {
            assert!(!line.contains(MARKER), "a body reached the log: {line}");
            assert!(!line.contains(&dek_b64), "a DEK reached the log: {line}");
            assert!(!line.contains(&wrapped_b64), "a wrapped DEK reached the log: {line}");
            assert!(!line.contains(TOKEN), "the token reached the log: {line}");
            assert!(!line.contains("org_a") && !line.contains("org_b"), "an org id reached the log: {line}");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn router_unwraps_env_and_oc_rows() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(unwrap_path("org_a")))
            .respond_with(ResponseTemplate::new(200).set_body_json(unwrapped_body("oc:2", 2)))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/internal/orgs/org_a/keys/data-keys"))
            .respond_with(ResponseTemplate::new(201).set_body_json(generated_body(2)))
            .expect(1)
            .mount(&server)
            .await;

        let env_json = format!(r#"{{"v1":"{}"}}"#, B64.encode([1u8; 32]));
        let env: Arc<dyn ProfileKeyProvider> =
            Arc::new(EnvKeyProvider::from_config(&env_json, "v1").expect("env"));
        let env_row = env.generate("org_a", "prof_1").expect("env generate");
        assert_eq!(env_row.key_id, "env:v1");

        let oc: Arc<dyn ProfileKeyProvider> = Arc::new(provider(&server));
        let router = KeyRouter::new(Some(env.clone()), oc.clone());
        assert!(router.has_env());

        // An env: row opens through the env master, an oc: row through org-core.
        let from_env = router
            .unwrap("org_a", "prof_1", &env_row.key_id, &env_row.wrapped)
            .expect("env row");
        assert_eq!(*from_env, *env_row.dek);
        let from_oc = router
            .unwrap("org_a", "prof_1", "oc:2", &wrapped())
            .expect("oc row");
        assert_eq!(*from_oc, DEK);

        // Lazy reseal: env: rows are never current, oc: rows at the newest version are.
        assert!(!router.is_current_key("org_a", &env_row.key_id));
        assert!(router.is_current_key("org_a", "oc:2"));
        assert!(router.current_key_id().starts_with(OC_KEY_ID_PREFIX));

        // New wraps (and the reseal of an env: row) go to org-core.
        let resealed = router.generate("org_a", "prof_1").expect("generate");
        assert_eq!(resealed.key_id, "oc:2");
        assert_eq!(
            router.rewrap("org_a", "prof_1", &env_row.key_id, &env_row.wrapped).err(),
            Some(KeyError::Invalid),
            "an env: row reseals with a fresh DEK, never rewraps"
        );

        // Unknown prefixes are refused; without the env master an env: row is destroyed.
        assert_eq!(
            router.unwrap("org_a", "prof_1", "kms:1", &wrapped()).err(),
            Some(KeyError::Invalid)
        );
        let oc_only = KeyRouter::new(None, oc);
        assert!(!oc_only.has_env());
        assert_eq!(
            oc_only
                .unwrap("org_a", "prof_1", &env_row.key_id, &env_row.wrapped)
                .err(),
            Some(KeyError::Destroyed)
        );
    }
}
