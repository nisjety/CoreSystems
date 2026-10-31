//! Auth middleware — validates JWTs via JWKS fetched from `auth-core`.
//!
//! P0 / cluster #auth+tenancy. Adapted from the canonical Model Plane
//! gateway implementation at
//! `apps/Model Plane/rust/services/model-gateway/src/auth.rs`. The same
//! Control Plane `auth-core` issues every JWT in the system, so Quarry
//! uses the identical JWKS verification path.
//!
//! Behaviour:
//! - Extracts `Authorization: Bearer <token>` header.
//! - Fetches and TTL-caches the JWKS document from `AUTH_CORE_JWKS_URL`.
//! - Pins JWS algorithm to RS256 (blocks `alg=none` and HS256-against-public-key).
//! - Verifies signature + `exp` + `nbf` + optional `iss` + optional `aud`.
//! - On unknown `kid`, forces a JWKS refresh (key rotation support).
//! - Injects the decoded [`Claims`] into request extensions for handlers.
//! - Accepts canonical Auth Core user and service principals. Service actors
//!   are derived only from signed `service_id`/`sub`, never request headers.
//!
//! Environment:
//! - `AUTH_CORE_JWKS_URL`             — required (unless dev bypass).
//! - `AUTH_CORE_ISSUER`               — optional; enforces `iss` when set.
//! - `AUTH_CORE_AUDIENCE`             — required by default; enforces `aud`.
//!   Set `AUTH_CORE_AUDIENCE_OPTIONAL=1` to opt out (warn-only mode).
//! - `AUTH_CORE_JWKS_TTL_SECS`        — optional; default 300s.
//! - `AUTH_CORE_JWT_LEEWAY_SECS`      — optional; default 30s. Clock-skew
//!   tolerance applied to `exp` and `nbf` validation.
//! - `QUARRY_EDGE_AUTH_DEV_BYPASS`    — `1`/`true` to accept a bearer that
//!   FAILS real verification and inject a stub `Claims` for local dev, so a
//!   legacy static/non-JWT caller can still reach protected routes. NEVER in
//!   production.
//!
//! Dev-bypass ordering (P0 fix — Aquatiq crawl-to-KB pipeline, 2026-07-20):
//! every bearer is run through the SAME real JWKS verification the strict
//! path uses, REGARDLESS of whether the bypass flag is set. Only when that
//! verification fails does the bypass flag get consulted, and only then does
//! the request fall back to the stub `Claims{org_id:"org_placeholder", ...}`.
//! Previously the bypass short-circuited BEFORE verification, so it silently
//! discarded every real, correctly-signed, real-org-scoped bearer the gateway
//! mints per user (`quarry_token()` → `aud=quarry`) and replaced it with the
//! placeholder identity — corrupting `org_id` end-to-end for every crawl/batch
//! handoff and any downstream Data Plane ingest keyed off it. Verifying first
//! means: a real, valid bearer always wins on its own merits; the bypass only
//! ever covers a bearer that could never have passed verification anyway
//! (e.g. a shared static dev token), so it can no longer clobber real tenant
//! identity while remaining available for that legitimate fallback case.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, warn};

use quarry_core::web_policy::PolicyDenial;
use quarry_core::zdr::ZdrMode;

/// Claims extracted from a verified JWT. Every token requires `org_id` plus
/// one unambiguous principal: user tokens carry `user_id`; service tokens carry
/// canonical `principal_type=service` + `service_id` equal to `sub`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub exp: i64,
    pub org_id: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    /// Retention posture asserted by the verified issuer. Auth Core stamps it
    /// on every plane token: for an interactive user it is the organisation's
    /// standing ZDR posture, for a service principal it is the registry row's
    /// `retentionByAudience` (model-gateway and execution-core hold `zdr` for
    /// the quarry audience, so their token says `true` for EVERY org on EVERY
    /// call). `None` means the claim is absent (a token minted before it
    /// existed): not an auth failure, and no floor.
    ///
    /// Never read this field directly for a decision. Its meaning depends on
    /// the principal; go through [`Claims::zdr_floor`] / [`Claims::effective_zdr`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zdr: Option<bool>,
    /// WS7.J: the org web-policy claim, as the issuer wrote it. Untyped on purpose: a malformed
    /// claim must not fail authentication. [`Claims::web_policy_claim`] reads it where a route
    /// decides, and an unreadable claim fails closed there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_policy: Option<serde_json::Value>,
    /// INNH-10: the id of the API key an `api_key` principal token was minted for. Absent on
    /// every user and service token. Never the key itself. Read it through [`Claims::key_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    /// INNH-10: the user who owns the API key (the grant owner an `api_key` principal acts
    /// for). Read it through [`Claims::key_owner_user_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acting_user_id: Option<String>,
}

/// INNH-10: the only scope an `api_key` principal token carries. The route fence
/// (`crate::api_key::require_api_key_route_fence`) refuses a key token without it. It is not
/// `browser:execute`, which is the Model Plane service scope.
pub const SCOPE_BROWSER_AGENT: &str = "browser:agent";

/// INNH-10: the longest `key_id` an `api_key` token may carry, in bytes.
pub const API_KEY_ID_MAX_LEN: usize = 128;

/// INNH-10: the longest `acting_user_id` an `api_key` token may carry, in bytes.
pub const API_KEY_OWNER_MAX_LEN: usize = 256;

/// INNH-10: the `sub` prefix of an `api_key` principal: `apikey:<key_id>`.
pub const API_KEY_SUB_PREFIX: &str = "apikey:";

impl Claims {
    /// Does the token carry the given scope?
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// True only for a canonical Auth Core service principal.
    #[must_use]
    pub fn is_service(&self) -> bool {
        self.principal_type.as_deref() == Some("service")
    }

    /// INNH-10: true only for a canonical `api_key` principal (a token auth-core minted for an
    /// API key). It is neither a person nor a service: code that reads "not a service" as
    /// "a person" must also ask this, or a key inherits a person's powers.
    #[must_use]
    pub fn is_api_key(&self) -> bool {
        self.principal_type.as_deref() == Some("api_key")
    }

    /// INNH-10: the key id of an `api_key` principal, `None` for every other principal even
    /// when the claim is present (it is refused at verification, this is the second guard).
    #[must_use]
    pub fn key_id(&self) -> Option<&str> {
        if self.is_api_key() {
            self.key_id.as_deref()
        } else {
            None
        }
    }

    /// INNH-10: the user who owns the key behind an `api_key` principal. It is used for the
    /// grant owner check and for attribution only, never as the run's actor (the actor is
    /// `apikey:<key_id>`). `None` for every other principal.
    #[must_use]
    pub fn key_owner_user_id(&self) -> Option<&str> {
        if self.is_api_key() {
            self.acting_user_id.as_deref()
        } else {
            None
        }
    }

    /// INNH-10: a person's session: neither a service nor an API key.
    #[must_use]
    pub fn is_person(&self) -> bool {
        !self.is_service() && !self.is_api_key()
    }

    /// The ZDR posture the TOKEN itself imposes, as a floor under whatever the
    /// request body asks for.
    ///
    /// Only an interactive user's claim counts: it carries the organisation's
    /// standing posture, so a caller of a ZDR org cannot lower it by sending
    /// `zdr: false`. A service principal's claim is a property of its registry
    /// row and audience, not of the run it is serving (see [`Claims::zdr`]),
    /// so it is deliberately ignored: for a service caller the run's posture is
    /// the request body flag, which the gateway or execution-core sets from the
    /// run's resolved posture (false on a waived run). Reading it would force
    /// ZDR onto every chat web search of every org.
    ///
    /// A token without a `principal_type` is the legacy user shape
    /// (`validate_and_normalize_principal` accepts it only with a `user_id`
    /// and no `service_id`), so everything that is not a service counts.
    ///
    /// INNH-10: an `api_key` principal counts like a user. Its `zdr` claim is the
    /// organization's standing posture, raised by the key's own ZDR flag (auth-core never
    /// lowers it), so the claim is a property of the caller's tenant, not of a registry row.
    /// `!is_service()` already says so; `api_key_token_zdr_floor_counts` pins it.
    #[must_use]
    pub fn zdr_floor(&self) -> bool {
        !self.is_service() && self.zdr == Some(true)
    }

    /// The posture a request actually runs under: what the body asked for,
    /// raised (never lowered) by [`Claims::zdr_floor`]. Every route that makes
    /// a ZDR decision takes it from here, so there is one rule.
    #[must_use]
    pub fn effective_zdr(&self, requested: bool) -> bool {
        let zdr = requested || self.zdr_floor();
        // The one place every route learns its posture: mark the request span so
        // the log filter (see `crate::zdr_log`) drops URL-bearing events from
        // here on. A no-op off ZDR and outside a request span.
        ZdrMode::from(zdr).mark_current_span();
        zdr
    }

    /// [`Claims::effective_zdr`] for the optional `zdr` field most request
    /// bodies carry, as the runtime's [`ZdrMode`].
    #[must_use]
    pub fn effective_zdr_mode(&self, requested: Option<bool>) -> ZdrMode {
        ZdrMode::from(self.effective_zdr(requested.unwrap_or(false)))
    }

    /// Canonical actor used by ownership/audit paths. `require_auth` normalizes
    /// service principals so this value is the signed `service_id`; user tokens
    /// retain their signed `user_id` unchanged.
    #[must_use]
    pub fn actor_id(&self) -> &str {
        &self.user_id
    }

    fn validate_and_normalize_principal(&mut self) -> Result<(), ()> {
        if self.org_id.trim().is_empty() {
            return Err(());
        }
        match self.principal_type.as_deref() {
            Some("service") => {
                let service_id = self.service_id.as_deref().unwrap_or("").trim();
                if service_id.is_empty()
                    || self.sub != service_id
                    || !self.user_id.trim().is_empty()
                    || !service_id.starts_with("service:")
                    || self.key_id.is_some()
                {
                    return Err(());
                }
                self.user_id = service_id.to_owned();
                Ok(())
            }
            Some("api_key") => {
                // INNH-10: a canonical key principal is `sub = apikey:<key_id>`, a bounded
                // printable key id, the owning user, and nothing that names a person or a
                // service. The actor becomes the `sub`, as a service's becomes its service id,
                // so a run a key starts is scoped to the key and never to its owner.
                let key_id = self.key_id.as_deref().unwrap_or("");
                let owner = self.acting_user_id.as_deref().unwrap_or("");
                let key_id_ok = !key_id.is_empty()
                    && key_id.len() <= API_KEY_ID_MAX_LEN
                    && key_id.bytes().all(|b| (0x21..=0x7e).contains(&b));
                let owner_ok = !owner.is_empty()
                    && owner.len() <= API_KEY_OWNER_MAX_LEN
                    && owner == owner.trim()
                    && !owner.chars().any(char::is_control);
                if !key_id_ok
                    || !owner_ok
                    || self.sub != format!("{API_KEY_SUB_PREFIX}{key_id}")
                    || !self.user_id.trim().is_empty()
                    || self.service_id.is_some()
                {
                    return Err(());
                }
                self.user_id = self.sub.clone();
                Ok(())
            }
            Some("user") | None => {
                // A key id on a token that is not a key principal is ambiguous: refused.
                if self.user_id.trim().is_empty()
                    || self.service_id.is_some()
                    || self.key_id.is_some()
                {
                    Err(())
                } else {
                    Ok(())
                }
            }
            Some(_) => Err(()),
        }
    }

    /// Enforces tenant isolation. Returns `Err(FORBIDDEN)` when the token's
    /// `org_id` does not match the expected value.
    ///
    /// # Errors
    /// [`StatusCode::FORBIDDEN`] on mismatch.
    #[allow(dead_code)] // scaffolding: wired in follow-up
    pub fn require_org(&self, expected: &str) -> Result<(), StatusCode> {
        if self.org_id == expected {
            Ok(())
        } else {
            warn!(
                claim_org = %self.org_id,
                expected_org = %expected,
                "tenant mismatch"
            );
            Err(StatusCode::FORBIDDEN)
        }
    }
}

/// WS7.J: the org web-policy claim as the edge reads it.
///
/// `rev` is the issuer's revision as a number (the claim carries up to 12 hex digits, or
/// `"0"` for no policy). `unknown_keys` counts the capability keys and top-level fields this
/// build does not know: they are ignored, never applied, and counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebPolicyClaim {
    pub layer: quarry_core::web_policy::PolicyLayer,
    pub rev: u64,
    pub unknown_keys: usize,
}

/// The only claim version this build reads. Any other version fails closed.
/// WS13.5: the scope a service principal needs to send scrape-time `actions[]`.
/// The check lives in the scrape handler (`scrape_actions::check_principal`), like
/// `scrape:write` for ingest; a user token does not need it.
pub const SCOPE_SCRAPE_INTERACT: &str = "scrape:interact";

/// INNH-1: the scope a service principal needs to send AI formats (`json`,
/// `summary`, `question`, `highlights`) on `POST /v1/scrape`. They spend model
/// money, so a service gets them only by name. The check lives in the scrape
/// handler (`scrape_contract::ai_formats::check_principal`); a user token needs
/// no scope.
pub const SCOPE_EXTRACT_AI: &str = "extract:ai";

pub const WEB_POLICY_CLAIM_VERSION: u64 = 1;

/// The top-level fields of a version 1 claim. Anything else is an unknown key.
const KNOWN_CLAIM_FIELDS: [&str; 6] = ["v", "rev", "caps", "robots_max", "proxy_max", "domains"];

impl Claims {
    /// The org web-policy layer this token carries (WS7.J).
    ///
    /// `Ok(None)` when the claim is absent (a token from an issuer that does not mint it yet).
    /// What absence means is the caller's decision (see `PolicyBaseline::org_input`).
    /// `Err(PolicyUnreadable)` when the claim is present but cannot be read: malformed JSON
    /// shape, a version other than 1, or a rule a layer may not state. Such a claim fails
    /// closed under `enforce`. The `web_policy` field is untyped so that none of this can fail
    /// authentication itself.
    pub fn web_policy_claim(&self) -> Result<Option<WebPolicyClaim>, PolicyDenial> {
        let Some(raw) = &self.web_policy else {
            return Ok(None);
        };
        let claim = parse_web_policy_claim(raw).map_err(|()| {
            metrics::counter!("web_policy_claim_unreadable_total").increment(1);
            PolicyDenial::PolicyUnreadable
        })?;
        if claim.unknown_keys > 0 {
            metrics::counter!("web_policy_claim_unknown_key_total")
                .increment(claim.unknown_keys as u64);
        }
        Ok(Some(claim))
    }
}

/// Parse a version 1 claim. `Err(())` for anything this build will not apply. The error
/// carries no detail on purpose: the caller counts it and refuses, and the claim is
/// never logged.
fn parse_web_policy_claim(raw: &serde_json::Value) -> Result<WebPolicyClaim, ()> {
    use quarry_core::web_policy::{CapKey, Mode, PolicyLayer, ProxyMax, RobotsMax};
    use serde_json::Value;

    let obj = raw.as_object().ok_or(())?;
    if obj.get("v").and_then(Value::as_u64) != Some(WEB_POLICY_CLAIM_VERSION) {
        return Err(());
    }
    let rev = parse_claim_rev(obj.get("rev").and_then(Value::as_str).ok_or(())?)?;
    let mut unknown_keys = obj
        .keys()
        .filter(|key| !KNOWN_CLAIM_FIELDS.contains(&key.as_str()))
        .count();

    let mut robots: Option<RobotsMax> = match obj.get("robots_max") {
        None | Some(Value::Null) => None,
        Some(value) => Some(parse_ceiling(value)?),
    };
    let mut proxy: Option<ProxyMax> = match obj.get("proxy_max") {
        None | Some(Value::Null) => None,
        Some(value) => Some(parse_ceiling(value)?),
    };
    let mut modes = std::collections::BTreeMap::new();
    if let Some(caps) = obj.get("caps") {
        let caps = caps.as_object().ok_or(())?;
        for (name, value) in caps {
            match CapKey::parse(name) {
                // A key this build does not know: ignored and counted (forward compatibility).
                None => unknown_keys += 1,
                Some(CapKey::RobotsMaxMode) => robots = tighter(robots, Some(parse_ceiling(value)?)),
                Some(CapKey::ProxyMaxTier) => proxy = tighter(proxy, Some(parse_ceiling(value)?)),
                // Domain rules travel in `domains`, never in `caps`.
                Some(CapKey::DomainsAllow | CapKey::DomainsDeny) => return Err(()),
                Some(key) => {
                    let mode: Mode = serde_json::from_value(value.clone()).map_err(|_| ())?;
                    modes.insert(key, mode);
                }
            }
        }
    }

    let (allow, deny) = match obj.get("domains") {
        None | Some(Value::Null) => (None, Vec::new()),
        Some(domains) => {
            let domains = domains.as_object().ok_or(())?;
            unknown_keys += domains
                .keys()
                .filter(|key| !matches!(key.as_str(), "allow" | "deny"))
                .count();
            let allow = match domains.get("allow") {
                None | Some(Value::Null) => None,
                // The claim lists only non-baseline entries, so an empty allow list
                // means no allow rule, not "allow nothing".
                Some(list) => Some(string_list(list)?).filter(|hosts| !hosts.is_empty()),
            };
            let deny = match domains.get("deny") {
                None | Some(Value::Null) => Vec::new(),
                Some(list) => string_list(list)?,
            };
            (allow, deny)
        }
    };

    let layer = PolicyLayer::new(modes, robots, proxy, allow, deny).map_err(|_| ())?;
    Ok(WebPolicyClaim {
        layer,
        rev,
        unknown_keys,
    })
}

/// The claim `rev`: `"0"` or 1 to 12 hex digits.
fn parse_claim_rev(raw: &str) -> Result<u64, ()> {
    if raw.is_empty() || raw.len() > 12 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(());
    }
    u64::from_str_radix(raw, 16).map_err(|_| ())
}

/// A robots or proxy ceiling, read from its wire name (`respect`, `none`, ...).
fn parse_ceiling<T: serde::de::DeserializeOwned>(value: &serde_json::Value) -> Result<T, ()> {
    serde_json::from_value(value.clone()).map_err(|_| ())
}

/// The more restrictive of two optional ceilings (the lower, since ceilings order least
/// permissive first).
fn tighter<T: Ord + Copy>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

fn string_list(list: &serde_json::Value) -> Result<Vec<String>, ()> {
    list.as_array()
        .ok_or(())?
        .iter()
        .map(|item| item.as_str().map(str::to_owned).ok_or(()))
        .collect()
}

struct CachedJwks {
    fetched_at: Instant,
    jwks: JwkSet,
}

/// Module-level JWKS cache with TTL refresh. `None` = never populated.
static JWKS_CACHE: LazyLock<RwLock<Option<CachedJwks>>> = LazyLock::new(|| RwLock::new(None));

const DEFAULT_JWKS_TTL_SECS: u64 = 300;
const DEFAULT_JWT_LEEWAY_SECS: u64 = 30;

fn jwks_ttl() -> Duration {
    std::env::var("AUTH_CORE_JWKS_TTL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or_else(
            || Duration::from_secs(DEFAULT_JWKS_TTL_SECS),
            Duration::from_secs,
        )
}

fn jwt_leeway_secs() -> u64 {
    std::env::var("AUTH_CORE_JWT_LEEWAY_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_JWT_LEEWAY_SECS)
}

fn audience_optional() -> bool {
    std::env::var("AUTH_CORE_AUDIENCE_OPTIONAL")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// `QUARRY_EDGE_AUTH_DEV_BYPASS` is read **once** at process startup and
/// cached for the lifetime of the binary. Two reasons:
///
/// 1. The startup kill switch in `main.rs` reads the same env var to
///    decide whether to refuse-to-start. If we re-read here per request
///    an operator could `docker exec` into a running container, flip
///    the env var, and silently enable the bypass without triggering
///    the startup guard. Latching ties both checks to the same value.
///
/// 2. Per-request `std::env::var` is a syscall; caching avoids it on
///    the auth hot path.
///
/// To toggle the bypass in dev: change the env var and restart the
/// service. Test builds bypass the latch so each test can flip the env
/// var independently.
fn dev_bypass_enabled() -> bool {
    #[cfg(test)]
    {
        read_dev_bypass_from_env()
    }
    #[cfg(not(test))]
    {
        use std::sync::OnceLock;
        static CACHED: OnceLock<bool> = OnceLock::new();
        *CACHED.get_or_init(read_dev_bypass_from_env)
    }
}

fn read_dev_bypass_from_env() -> bool {
    std::env::var("QUARRY_EDGE_AUTH_DEV_BYPASS")
        .map(|v| dev_bypass_flag_value(&v))
        .unwrap_or(false)
}

/// Whether a `QUARRY_EDGE_AUTH_DEV_BYPASS` value turns the bypass on.
///
/// The ONE parse behind both the runtime reader and the production start-up
/// refusal ([`production_with_dev_bypass`]). They used to disagree: the refusal
/// matched `"1" | "true" | "TRUE" | "yes"` exactly while the reader accepted
/// any case of `true`, so `QUARRY_EDGE_AUTH_DEV_BYPASS=True` passed the
/// refusal and still enabled a bypass that accepts any bearer.
pub fn dev_bypass_flag_value(raw: &str) -> bool {
    let value = raw.trim();
    value == "1"
        || ["true", "yes", "on"]
            .iter()
            .any(|word| value.eq_ignore_ascii_case(word))
}

/// True when the process is in a production environment (`prod` or
/// `production`, any case) AND the dev bypass is on: the combination the edge
/// must refuse to start with.
pub fn production_with_dev_bypass(env_name: &str, raw_bypass: Option<&str>) -> bool {
    let env_name = env_name.trim();
    let production =
        env_name.eq_ignore_ascii_case("prod") || env_name.eq_ignore_ascii_case("production");
    production && raw_bypass.is_some_and(dev_bypass_flag_value)
}

async fn fetch_jwks() -> Result<JwkSet, StatusCode> {
    let url = std::env::var("AUTH_CORE_JWKS_URL").map_err(|_| {
        error!("AUTH_CORE_JWKS_URL not set");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let resp = reqwest::get(&url).await.map_err(|e| {
        // zdr-log-ok: runs inside require_auth, before the claims exist and before mark_floor can mark the span, so the filter cannot see a ZDR posture here; the text is a JWT or JWKS error, never caller data, and the url is the operator-set JWKS endpoint
        error!(error = %e, url = %url, "failed to fetch JWKS");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let jwks: JwkSet = resp.json().await.map_err(|e| {
        // zdr-log-ok: runs inside require_auth, before the claims exist and before mark_floor can mark the span, so the filter cannot see a ZDR posture here; the text is a JWT or JWKS error, never caller data
        error!(error = %e, "failed to parse JWKS JSON");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(jwks)
}

async fn get_jwks(force_refresh: bool) -> Result<JwkSet, StatusCode> {
    if !force_refresh {
        let guard = JWKS_CACHE.read().await;
        if let Some(cached) = guard.as_ref() {
            if cached.fetched_at.elapsed() < jwks_ttl() {
                return Ok(cached.jwks.clone());
            }
        }
    }
    let jwks = fetch_jwks().await?;
    let mut guard = JWKS_CACHE.write().await;
    *guard = Some(CachedJwks {
        fetched_at: Instant::now(),
        jwks: jwks.clone(),
    });
    Ok(jwks)
}

/// Verifies a bearer against the real Auth Core JWKS path: header decode,
/// algorithm pin, `kid` lookup (with one forced refresh on miss), signature +
/// `exp`/`nbf`/`iss`/`aud` validation, then principal normalization. This is
/// the strict path — used unconditionally, regardless of `QUARRY_EDGE_AUTH_DEV_BYPASS`,
/// so a real bearer is always verified for real before any bypass is consulted.
///
/// # Errors
/// - [`StatusCode::UNAUTHORIZED`] on header decode / algorithm / kid / signature
///   / claims-validation / principal-normalization failure.
/// - [`StatusCode::INTERNAL_SERVER_ERROR`] when JWKS cannot be fetched, the JWK
///   cannot be materialised into a decoding key, or `AUTH_CORE_AUDIENCE` is
///   unset without `AUTH_CORE_AUDIENCE_OPTIONAL=1`.
async fn verify_jwt_bearer(token: &str) -> Result<Claims, StatusCode> {
    let jwt_header = decode_header(token).map_err(|e| {
        // zdr-log-ok: runs inside require_auth, before the claims exist and before mark_floor can mark the span, so the filter cannot see a ZDR posture here; the text is a JWT or JWKS error, never caller data
        warn!(error = %e, "failed to decode JWT header");
        StatusCode::UNAUTHORIZED
    })?;

    // Pin algorithm — blocks alg-confusion (`none`, HS256 against public key).
    if jwt_header.alg != Algorithm::RS256 {
        warn!(alg = ?jwt_header.alg, "unsupported JWT algorithm");
        return Err(StatusCode::UNAUTHORIZED);
    }

    let kid = jwt_header.kid.ok_or_else(|| {
        warn!("JWT header missing kid");
        StatusCode::UNAUTHORIZED
    })?;

    // Two-pass JWK lookup: on kid miss, force a JWKS refresh to pick up
    // rotated keys, then try again.
    let jwks = get_jwks(false).await?;
    let jwk = if let Some(k) = jwks.find(&kid) {
        k.clone()
    } else {
        let refreshed = get_jwks(true).await?;
        refreshed.find(&kid).cloned().ok_or_else(|| {
            warn!(kid = %kid, "no JWK matches token kid after refresh");
            StatusCode::UNAUTHORIZED
        })?
    };

    let decoding_key = DecodingKey::from_jwk(&jwk).map_err(|e| {
        // zdr-log-ok: runs inside require_auth, before the claims exist and before mark_floor can mark the span, so the filter cannot see a ZDR posture here; the text is a JWT or JWKS error, never caller data
        error!(error = %e, "failed to build decoding key from JWK");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.leeway = jwt_leeway_secs();
    if let Ok(expected_iss) = std::env::var("AUTH_CORE_ISSUER") {
        // An empty AUTH_CORE_ISSUER must NOT enforce iss="" (mirrors the
        // model-gateway fix 672ed214) — skip issuer validation instead.
        if !expected_iss.is_empty() {
            validation.set_issuer(&[expected_iss]);
        }
    }
    match std::env::var("AUTH_CORE_AUDIENCE") {
        Ok(expected_aud) => {
            validation.set_audience(&[expected_aud]);
        }
        Err(_) if audience_optional() => {
            warn!(
                "AUTH_CORE_AUDIENCE unset and AUTH_CORE_AUDIENCE_OPTIONAL=1; \
                 skipping audience validation"
            );
            validation.validate_aud = false;
        }
        Err(_) => {
            error!(
                "AUTH_CORE_AUDIENCE not set; refusing requests. Set the var or \
                 opt out via AUTH_CORE_AUDIENCE_OPTIONAL=1"
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    let mut token_data = decode::<Claims>(token, &decoding_key, &validation).map_err(|e| {
        // zdr-log-ok: runs inside require_auth, before the claims exist and before mark_floor can mark the span, so the filter cannot see a ZDR posture here; the text is a JWT or JWKS error, never caller data
        warn!(error = %e, "JWT validation failed");
        StatusCode::UNAUTHORIZED
    })?;

    if token_data
        .claims
        .validate_and_normalize_principal()
        .is_err()
    {
        warn!("JWT claims contain an invalid or ambiguous principal");
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(token_data.claims)
}

/// The stub identity injected only when `QUARRY_EDGE_AUTH_DEV_BYPASS` is
/// enabled AND the presented bearer already failed real JWKS verification.
/// NEVER used for a bearer that verifies successfully — see [`require_auth`].
fn dev_bypass_stub_claims() -> Claims {
    Claims {
        web_policy: None,
        sub: "user_placeholder".to_owned(),
        iss: "dev".to_owned(),
        exp: i64::MAX,
        org_id: "org_placeholder".to_owned(),
        user_id: "user_placeholder".to_owned(),
        principal_type: Some("user".to_owned()),
        service_id: None,
        nbf: None,
        aud: None,
        // WS17.P0: the dev stub carries the profile import scope so a local run and the edge
        // tests can save a profile. The bypass is refused in production, so no real token does.
        scopes: vec![crate::profile_guard::IMPORT_SCOPE.to_owned()],
        // The dev stub asserts no posture: it can only ever run on the body flag.
        zdr: None,
        key_id: None,
        acting_user_id: None,
    }
}

/// Axum middleware that verifies a Bearer JWT and injects [`Claims`] into
/// the request's extension map. Handlers downstream of this middleware
/// can recover the claims with `Extension<Claims>`.
///
/// Every bearer is verified against the real Auth Core JWKS path FIRST,
/// regardless of `QUARRY_EDGE_AUTH_DEV_BYPASS`. Only when that verification
/// fails is the bypass flag consulted, and only then does the request fall
/// back to a stub `Claims{org_id:"org_placeholder", ...}` for local dev —
/// so the bypass can never silently discard a real, correctly-signed,
/// real-org-scoped bearer (see module docs for the incident this fixes).
///
/// # Errors
/// - [`StatusCode::UNAUTHORIZED`] when no usable token is present, or it
///   fails verification and the bypass is not enabled.
/// - [`StatusCode::INTERNAL_SERVER_ERROR`] when JWKS cannot be fetched or
///   the JWK cannot be materialised into a decoding key (and the bypass is
///   not enabled).
pub async fn require_auth(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let Some(token) = token else {
        warn!("missing or invalid Authorization bearer");
        return Err(StatusCode::UNAUTHORIZED);
    };

    match verify_jwt_bearer(&token).await {
        Ok(claims) => {
            req.extensions_mut().insert(claims);
            Ok(next.run(req).await)
        }
        Err(status) if dev_bypass_enabled() => {
            warn!(
                %status,
                "QUARRY_EDGE_AUTH_DEV_BYPASS enabled — bearer failed real JWKS \
                 verification, falling back to stub claims (org_id=org_placeholder)"
            );
            req.extensions_mut().insert(dev_bypass_stub_claims());
            Ok(next.run(req).await)
        }
        Err(status) => Err(status),
    }
}

/// Returns an Axum middleware that enforces a required scope on the
/// already-validated [`Claims`] in request extensions.
///
/// Must be layered *after* [`require_auth`]. Returns 401 if no `Claims`
/// extension is present (auth wasn't run) and 403 if the scope is missing.
#[allow(dead_code)]
pub fn require_scope(
    scope: &'static str,
) -> impl Fn(
    Request,
    Next,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Response, StatusCode>> + Send>,
> + Clone
       + Send
       + Sync
       + 'static {
    move |req: Request, next: Next| {
        Box::pin(async move {
            let Some(claims) = req.extensions().get::<Claims>() else {
                warn!(scope = %scope, "require_scope invoked without Claims extension");
                return Err(StatusCode::UNAUTHORIZED);
            };
            if claims.has_scope(scope) {
                Ok(next.run(req).await)
            } else {
                warn!(
                    scope = %scope,
                    org = %claims.org_id,
                    user = %claims.user_id,
                    "missing required scope"
                );
                Err(StatusCode::FORBIDDEN)
            }
        })
    }
}

/// Enforces least privilege for service principals while preserving existing
/// interactive-user behavior. Unknown service routes fail closed; each Model
/// Plane integration route declares the only scope(s) it accepts.
pub async fn require_service_route_scope(req: Request, next: Next) -> Result<Response, StatusCode> {
    let Some(claims) = req.extensions().get::<Claims>() else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if !claims.is_service() {
        return Ok(next.run(req).await);
    }

    let path = req.uri().path();
    let permitted = match path {
        "/v1/scrape" => claims.has_scope("scrape:read") || claims.has_scope("scrape:write"),
        // Reading back one scrape record by id. Not the stream route, and not a
        // nested path.
        _ if path
            .strip_prefix("/v1/scrape/")
            .is_some_and(|rest| rest != "stream" && !rest.contains('/')) =>
        {
            claims.has_scope("scrape:read") || claims.has_scope("scrape:write")
        }
        "/v1/audit/page" => claims.has_scope("scrape:read"),
        "/v1/search" => claims.has_scope("search:read"),
        "/v1/map" => claims.has_scope("map:read"),
        "/v1/extract" => claims.has_scope("extract:read"),
        // Reading one artifact by id is part of the scrape/agent contract, not a
        // separate capability: `/v1/scrape` answers with
        // `FormatRef{artifact_id,bytes}` and never inline page text, and agent
        // observations hand back screenshot/trace ids. A principal allowed to
        // produce those refs but not to resolve them can only ever see empty
        // content. The bytes stay tenant-bound in `get_artifact`, which matches
        // the artifact's stored org against this claim.
        //
        // The `/v1/artifacts` *list* route is deliberately not included — no
        // service integration enumerates artifacts.
        _ if path.starts_with("/v1/artifacts/") => {
            claims.has_scope("scrape:read")
                || claims.has_scope("scrape:write")
                || claims.has_scope("browser:execute")
        }
        _ if path.starts_with("/v1/agent/") => claims.has_scope("browser:execute"),
        _ => false,
    };
    if permitted {
        Ok(next.run(req).await)
    } else {
        warn!(
            path,
            service = %claims.actor_id(),
            "service principal lacks the route scope"
        );
        Err(StatusCode::FORBIDDEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body, http::Request as HttpRequest, middleware, routing::get, Extension, Router,
    };
    use serial_test::serial;

    fn clear_env() {
        std::env::remove_var("AUTH_CORE_JWKS_URL");
        std::env::remove_var("AUTH_CORE_ISSUER");
        std::env::remove_var("AUTH_CORE_AUDIENCE");
        std::env::remove_var("AUTH_CORE_AUDIENCE_OPTIONAL");
        std::env::remove_var("AUTH_CORE_JWKS_TTL_SECS");
        std::env::remove_var("AUTH_CORE_JWT_LEEWAY_SECS");
        std::env::remove_var("QUARRY_EDGE_AUTH_DEV_BYPASS");
    }

    fn dev_router() -> Router {
        async fn ok_handler() -> &'static str {
            "ok"
        }
        Router::new()
            .route("/", get(ok_handler))
            .layer(middleware::from_fn(require_auth))
    }

    #[test]
    fn dev_bypass_flag_is_case_insensitive_and_trimmed() {
        for on in [
            "1", "true", "True", "TRUE", "tRuE", "yes", "YES", "on", "On", " 1 ", "true ",
        ] {
            assert!(dev_bypass_flag_value(on), "{on:?} must read as on");
        }
        for off in [
            "", "0", "false", "False", "no", "off", "2", "enabled", "truee",
        ] {
            assert!(!dev_bypass_flag_value(off), "{off:?} must read as off");
        }
    }

    #[test]
    fn prod_refuses_dev_bypass_in_any_case() {
        for env_name in ["prod", "PROD", "production", "Production", " production "] {
            for raw in ["1", "true", "True", "TRUE", "yes", "Yes", "on"] {
                assert!(
                    production_with_dev_bypass(env_name, Some(raw)),
                    "ENVIRONMENT={env_name:?} with bypass={raw:?} must be refused"
                );
            }
            // Off or unset is not a refusal.
            assert!(!production_with_dev_bypass(env_name, None));
            assert!(!production_with_dev_bypass(env_name, Some("0")));
            assert!(!production_with_dev_bypass(env_name, Some("false")));
        }
    }

    #[test]
    fn non_prod_dev_bypass_still_boots() {
        for env_name in ["dev", "development", "test", "staging", ""] {
            assert!(
                !production_with_dev_bypass(env_name, Some("1")),
                "{env_name:?}"
            );
            assert!(
                !production_with_dev_bypass(env_name, Some("True")),
                "{env_name:?}"
            );
        }
    }

    #[test]
    fn claims_serde_roundtrip_default_fields_omitted() {
        let c = Claims {
            web_policy: None,
            sub: "u1".into(),
            iss: "auth-core".into(),
            exp: 1_700_000_000,
            org_id: "o1".into(),
            user_id: "u1".into(),
            principal_type: None,
            service_id: None,
            nbf: None,
            aud: None,
            scopes: Vec::new(),
            zdr: None,
            key_id: None,
            acting_user_id: None,
        };
        let s = serde_json::to_string(&c).unwrap();
        assert!(!s.contains("\"aud\""));
        assert!(!s.contains("\"scopes\""));
        assert!(!s.contains("\"nbf\""));
        let back: Claims = serde_json::from_str(&s).unwrap();
        assert_eq!(back.org_id, "o1");
        assert_eq!(back.user_id, "u1");
        assert!(back.scopes.is_empty());
    }

    #[test]
    fn claims_has_scope_and_require_org() {
        let c = Claims {
            web_policy: None,
            sub: "u1".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: "u1".into(),
            principal_type: None,
            service_id: None,
            nbf: None,
            aud: Some("quarry-edge".into()),
            scopes: vec!["search:read".into(), "crawl:write".into()],
            zdr: None,
            key_id: None,
            acting_user_id: None,
        };
        assert!(c.has_scope("search:read"));
        assert!(!c.has_scope("admin"));
        assert!(c.require_org("alpha").is_ok());
        assert!(c.require_org("beta").is_err());
    }

    #[test]
    fn canonical_service_principal_derives_actor_from_signed_service_id() {
        let mut claims = Claims {
            web_policy: None,
            sub: "service:model-gateway".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: String::new(),
            principal_type: Some("service".into()),
            service_id: Some("service:model-gateway".into()),
            nbf: None,
            aud: Some("quarry".into()),
            scopes: vec!["search:read".into()],
            zdr: None,
            key_id: None,
            acting_user_id: None,
        };

        claims.validate_and_normalize_principal().unwrap();
        assert!(claims.is_service());
        assert_eq!(claims.actor_id(), "service:model-gateway");
    }

    #[test]
    fn canonical_service_payload_without_user_id_deserializes_and_normalizes() {
        let mut claims: Claims = serde_json::from_value(serde_json::json!({
            "sub": "service:execution-core",
            "iss": "auth-core",
            "exp": 1_900_000_000_i64,
            "org_id": "alpha",
            "principal_type": "service",
            "service_id": "service:execution-core",
            "aud": "quarry",
            "scopes": ["browser:execute"]
        }))
        .expect("Auth Core canonical service payload must deserialize");

        assert!(claims.user_id.is_empty());
        claims.validate_and_normalize_principal().unwrap();
        assert_eq!(claims.actor_id(), "service:execution-core");
    }

    #[test]
    fn ambiguous_service_principal_is_rejected() {
        let mut claims = Claims {
            web_policy: None,
            sub: "service:model-gateway".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: "caller-selected-user".into(),
            principal_type: Some("service".into()),
            service_id: Some("service:model-gateway".into()),
            nbf: None,
            aud: Some("quarry".into()),
            scopes: vec!["search:read".into()],
            zdr: None,
            key_id: None,
            acting_user_id: None,
        };
        assert!(claims.validate_and_normalize_principal().is_err());
    }

    fn api_key_claims() -> Claims {
        Claims {
            web_policy: None,
            sub: "apikey:key-1".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: String::new(),
            principal_type: Some("api_key".into()),
            service_id: None,
            nbf: None,
            aud: Some("quarry".into()),
            scopes: vec![SCOPE_BROWSER_AGENT.into()],
            zdr: None,
            key_id: Some("key-1".into()),
            acting_user_id: Some("user-owner".into()),
        }
    }

    #[test]
    fn api_key_principal_derives_its_actor_from_the_signed_key_id() {
        let mut claims = api_key_claims();
        claims.validate_and_normalize_principal().unwrap();
        assert!(claims.is_api_key());
        assert!(!claims.is_service());
        assert!(!claims.is_person());
        assert_eq!(claims.actor_id(), "apikey:key-1");
        assert_eq!(claims.key_id(), Some("key-1"));
        assert_eq!(claims.key_owner_user_id(), Some("user-owner"));
        assert!(claims.has_scope(SCOPE_BROWSER_AGENT));
    }

    #[test]
    fn canonical_api_key_payload_deserializes_and_normalizes() {
        let mut claims: Claims = serde_json::from_value(serde_json::json!({
            "sub": "apikey:key-9",
            "iss": "auth-core",
            "exp": 1_900_000_000_i64,
            "org_id": "alpha",
            "principal_type": "api_key",
            "key_id": "key-9",
            "acting_user_id": "user-owner",
            "aud": "quarry",
            "scopes": ["browser:agent"],
            "zdr": true
        }))
        .expect("the key token payload must deserialize");
        assert!(claims.user_id.is_empty());
        claims.validate_and_normalize_principal().unwrap();
        assert_eq!(claims.actor_id(), "apikey:key-9");
        assert!(claims.zdr_floor());
    }

    #[test]
    fn ambiguous_or_incomplete_api_key_principals_are_rejected() {
        let long_key = "k".repeat(API_KEY_ID_MAX_LEN + 1);
        let at_limit = "k".repeat(API_KEY_ID_MAX_LEN);
        let cases: Vec<(&str, Box<dyn Fn(&mut Claims)>)> = vec![
            ("missing key_id", Box::new(|c| c.key_id = None)),
            ("empty key_id", Box::new(|c| c.key_id = Some(String::new()))),
            ("sub does not match the key", Box::new(|c| c.sub = "apikey:key-2".into())),
            ("sub without the prefix", Box::new(|c| c.sub = "key-1".into())),
            ("sub of a service", Box::new(|c| c.sub = "service:model-gateway".into())),
            ("user_id present", Box::new(|c| c.user_id = "user-owner".into())),
            ("service_id present", Box::new(|c| c.service_id = Some("service:x".into()))),
            ("missing acting_user_id", Box::new(|c| c.acting_user_id = None)),
            ("blank acting_user_id", Box::new(|c| c.acting_user_id = Some("  ".into()))),
            ("padded acting_user_id", Box::new(|c| c.acting_user_id = Some(" user-owner".into()))),
            ("key_id with a space", Box::new(|c| {
                c.key_id = Some("key 1".into());
                c.sub = "apikey:key 1".into();
            })),
            ("key_id with a control character", Box::new(|c| {
                c.key_id = Some("key\n1".into());
                c.sub = "apikey:key\n1".into();
            })),
            ("key_id over the bound", Box::new(move |c| {
                c.key_id = Some(long_key.clone());
                c.sub = format!("apikey:{long_key}");
            })),
            ("empty org", Box::new(|c| c.org_id = String::new())),
        ];
        for (label, mutate) in &cases {
            let mut claims = api_key_claims();
            mutate(&mut claims);
            assert!(
                claims.validate_and_normalize_principal().is_err(),
                "{label} must be refused"
            );
        }
        // The bound is inclusive.
        let mut claims = api_key_claims();
        claims.key_id = Some(at_limit.clone());
        claims.sub = format!("apikey:{at_limit}");
        assert!(claims.validate_and_normalize_principal().is_ok());
    }

    #[test]
    fn a_key_id_on_a_user_or_service_token_is_ambiguous_and_refused() {
        let mut user = Claims {
            principal_type: Some("user".into()),
            sub: "u1".into(),
            user_id: "u1".into(),
            key_id: Some("key-1".into()),
            acting_user_id: None,
            ..api_key_claims()
        };
        assert!(user.validate_and_normalize_principal().is_err());
        let mut service = service_claims(&["browser:execute"]);
        service.user_id = String::new();
        service.key_id = Some("key-1".into());
        assert!(service.validate_and_normalize_principal().is_err());
    }

    #[test]
    fn legacy_user_and_service_tokens_are_unchanged_by_the_key_principal() {
        let mut user = Claims {
            principal_type: None,
            sub: "u1".into(),
            user_id: "u1".into(),
            key_id: None,
            acting_user_id: None,
            scopes: Vec::new(),
            ..api_key_claims()
        };
        user.validate_and_normalize_principal().unwrap();
        assert!(user.is_person());
        assert!(!user.is_api_key());
        assert_eq!(user.actor_id(), "u1");
        assert_eq!(user.key_id(), None);
        assert_eq!(user.key_owner_user_id(), None);

        // A service token may carry `acting_user_id` (token exchange mints it): that is not a key.
        let mut service = service_claims(&["browser:execute"]);
        service.user_id = String::new();
        service.acting_user_id = Some("user-owner".into());
        service.validate_and_normalize_principal().unwrap();
        assert!(service.is_service());
        assert!(!service.is_api_key());
        assert!(!service.is_person());
        assert_eq!(service.key_id(), None);
        assert_eq!(service.key_owner_user_id(), None, "only a key principal has a key owner");
    }

    #[test]
    fn api_key_token_zdr_floor_counts() {
        let mut zdr_key = api_key_claims();
        zdr_key.zdr = Some(true);
        assert!(zdr_key.zdr_floor(), "a key of a ZDR org cannot lower the posture");
        assert!(zdr_key.effective_zdr(false));
        assert!(!api_key_claims().zdr_floor());
        assert!(api_key_claims().effective_zdr(true), "the body can still raise it");
    }

    fn scoped_router(claims: Claims) -> Router {
        async fn ok_handler() -> &'static str {
            "ok"
        }
        Router::new()
            .route("/v1/search", get(ok_handler))
            .route("/v1/map", get(ok_handler))
            .route("/v1/audit/page", get(ok_handler))
            .route("/v1/extract", get(ok_handler))
            .route("/v1/other", get(ok_handler))
            .route("/v1/artifacts", get(ok_handler))
            .route("/v1/artifacts/:id", get(ok_handler))
            .route("/v1/scrape/stream", get(ok_handler))
            .route("/v1/scrape/:id", get(ok_handler))
            .route("/v1/scrape/:id/extra", get(ok_handler))
            .layer(middleware::from_fn(require_service_route_scope))
            .layer(Extension(claims))
    }

    fn service_claims(scopes: &[&str]) -> Claims {
        Claims {
            web_policy: None,
            sub: "service:model-gateway".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: "service:model-gateway".into(),
            principal_type: Some("service".into()),
            service_id: Some("service:model-gateway".into()),
            nbf: None,
            aud: Some("quarry".into()),
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
            zdr: None,
            key_id: None,
            acting_user_id: None,
        }
    }

    #[tokio::test]
    async fn bounded_audit_capture_requires_scrape_read_not_map_or_write() {
        assert_eq!(
            status_for(service_claims(&["scrape:read"]), "/v1/audit/page").await,
            StatusCode::OK
        );
        for scopes in [
            &[][..],
            &["map:read"][..],
            &["scrape:write"][..],
            &["browser:execute"][..],
        ] {
            assert_eq!(
                status_for(service_claims(scopes), "/v1/audit/page").await,
                StatusCode::FORBIDDEN
            );
        }
    }

    async fn status_for(claims: Claims, path: &str) -> StatusCode {
        let request = HttpRequest::builder()
            .uri(path)
            .body(Body::empty())
            .unwrap();
        tower::ServiceExt::oneshot(scoped_router(claims), request)
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn scrape_scoped_service_can_resolve_the_artifact_its_scrape_returned() {
        // `/v1/scrape` answers with `FormatRef{artifact_id,bytes}` and no inline
        // text, so denying the by-id read made every service fetch resolve to
        // empty content.
        for scope in ["scrape:read", "scrape:write", "browser:execute"] {
            assert_eq!(
                status_for(service_claims(&[scope]), "/v1/artifacts/art_01ABC").await,
                StatusCode::OK,
                "{scope} must be able to read an artifact by id"
            );
        }
    }

    #[tokio::test]
    async fn a_scrape_scoped_service_can_read_a_scrape_record_and_nothing_wider() {
        for scope in ["scrape:read", "scrape:write"] {
            assert_eq!(
                status_for(service_claims(&[scope]), "/v1/scrape/art_01ABC").await,
                StatusCode::OK,
                "{scope}"
            );
        }
        // Not a producing scope, the stream route, or a nested path.
        assert_eq!(
            status_for(service_claims(&["search:read"]), "/v1/scrape/art_01ABC").await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status_for(service_claims(&["scrape:read"]), "/v1/scrape/stream").await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status_for(service_claims(&["scrape:read"]), "/v1/scrape/art_01ABC/extra").await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn artifact_read_stays_closed_without_a_producing_scope() {
        assert_eq!(
            status_for(service_claims(&["search:read"]), "/v1/artifacts/art_01ABC").await,
            StatusCode::FORBIDDEN
        );
        // Enumerating artifacts is not part of any service integration, so the
        // collection route stays closed even for a scrape principal.
        assert_eq!(
            status_for(service_claims(&["scrape:read"]), "/v1/artifacts").await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn map_service_scope_is_explicit_and_does_not_grant_other_routes() {
        assert_eq!(
            status_for(service_claims(&["map:read"]), "/v1/map").await,
            StatusCode::OK
        );
        for scope in ["search:read", "scrape:read", "extract:read", "crawl:write"] {
            assert_eq!(
                status_for(service_claims(&[scope]), "/v1/map").await,
                StatusCode::FORBIDDEN
            );
        }
        for path in [
            "/v1/search",
            "/v1/extract",
            "/v1/artifacts/art_01ABC",
            "/v1/other",
        ] {
            assert_eq!(
                status_for(service_claims(&["map:read"]), path).await,
                StatusCode::FORBIDDEN
            );
        }
    }

    #[tokio::test]
    async fn service_route_scope_is_least_privilege_and_unknown_routes_fail_closed() {
        let claims = Claims {
            web_policy: None,
            sub: "service:model-gateway".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: "service:model-gateway".into(),
            principal_type: Some("service".into()),
            service_id: Some("service:model-gateway".into()),
            nbf: None,
            aud: Some("quarry".into()),
            scopes: vec!["search:read".into()],
            zdr: None,
            key_id: None,
            acting_user_id: None,
        };

        let search = HttpRequest::builder()
            .uri("/v1/search")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            tower::ServiceExt::oneshot(scoped_router(claims.clone()), search)
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        for path in ["/v1/extract", "/v1/other"] {
            let request = HttpRequest::builder()
                .uri(path)
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                tower::ServiceExt::oneshot(scoped_router(claims.clone()), request)
                    .await
                    .unwrap()
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
    }

    #[tokio::test]
    async fn service_scope_middleware_preserves_interactive_user_routes() {
        let claims = Claims {
            web_policy: None,
            sub: "user-1".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: "user-1".into(),
            principal_type: Some("user".into()),
            service_id: None,
            nbf: None,
            aud: Some("quarry".into()),
            scopes: Vec::new(),
            zdr: None,
            key_id: None,
            acting_user_id: None,
        };
        let request = HttpRequest::builder()
            .uri("/v1/other")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            tower::ServiceExt::oneshot(scoped_router(claims), request)
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }

    /// Without an Authorization header, the middleware must return 401
    /// regardless of any other state. Tests are serialised because
    /// dev_bypass / JWKS env vars are process-global.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn missing_bearer_returns_401() {
        clear_env();
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");

        let req = HttpRequest::builder().uri("/").body(Body::empty()).unwrap();
        let res = tower::ServiceExt::oneshot(dev_router(), req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        clear_env();
    }

    /// Dev bypass injects a stub Claims so handlers see a deterministic
    /// org_id/user_id even when no real JWT is present.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn dev_bypass_accepts_any_bearer() {
        clear_env();
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");

        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", "Bearer anything")
            .body(Body::empty())
            .unwrap();
        let res = tower::ServiceExt::oneshot(dev_router(), req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        clear_env();
    }

    /// Malformed token returns 401 even with audience optional + no JWKS
    /// configured — the JWT header decode runs before any JWKS fetch.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn malformed_token_returns_401() {
        clear_env();
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");
        // No AUTH_CORE_JWKS_URL — we should never reach the JWKS fetch
        // because the header-decode step fails first.

        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", "Bearer not-a-jwt")
            .body(Body::empty())
            .unwrap();
        let res = tower::ServiceExt::oneshot(dev_router(), req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        clear_env();
    }

    // -----------------------------------------------------------------
    // P0 regression — Aquatiq crawl-to-KB pipeline (2026-07-20): dev-bypass
    // must verify a real bearer first and only ever stub when verification
    // genuinely fails, so it can never again clobber a real per-org JWT the
    // gateway mints (as it did in production, stamping every crawl job
    // "org_placeholder" instead of the real tenant).
    // -----------------------------------------------------------------

    async fn reset_jwks_cache_for_test() {
        *JWKS_CACHE.write().await = None;
    }

    fn echo_org_router() -> Router {
        async fn echo_org(Extension(claims): Extension<Claims>) -> String {
            claims.org_id.clone()
        }
        Router::new()
            .route("/", get(echo_org))
            .layer(middleware::from_fn(require_auth))
    }

    fn rsa_keypair_pem() -> &'static (String, String) {
        use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
        static KP: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
        KP.get_or_init(|| {
            let mut rng = rand::thread_rng();
            let priv_key = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("rsa keygen");
            let pub_key = rsa::RsaPublicKey::from(&priv_key);
            let priv_pem = priv_key
                .to_pkcs8_pem(LineEnding::LF)
                .expect("encode priv pem")
                .to_string();
            let pub_pem = pub_key
                .to_public_key_pem(LineEnding::LF)
                .expect("encode pub pem");
            (priv_pem, pub_pem)
        })
    }

    fn jwk_n_e() -> (String, String) {
        use base64::Engine;
        use rsa::pkcs8::DecodePublicKey;
        use rsa::traits::PublicKeyParts;
        let (_, pub_pem) = rsa_keypair_pem();
        let pk = rsa::RsaPublicKey::from_public_key_pem(pub_pem).expect("decode pub pem");
        let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(pk.n().to_bytes_be());
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(pk.e().to_bytes_be());
        (n, e)
    }

    fn sign_jwt(claims: &Claims, kid: &str) -> String {
        use jsonwebtoken::{encode, EncodingKey, Header};
        let (priv_pem, _) = rsa_keypair_pem();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_string());
        let key = EncodingKey::from_rsa_pem(priv_pem.as_bytes()).expect("load priv key");
        encode(&header, claims, &key).expect("encode jwt")
    }

    async fn start_jwks_mock(kid: &str) -> wiremock::MockServer {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };
        let (n, e) = jwk_n_e();
        let body = serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": kid,
                "n": n,
                "e": e,
            }]
        });
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        server
    }

    fn now_secs() -> i64 {
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock")
                .as_secs(),
        )
        .expect("unix epoch fits in i64")
    }

    /// The core regression: with the bypass ON, a real, validly-signed,
    /// real-org bearer must be verified and its REAL claims used — never
    /// silently replaced by the `org_placeholder` stub. This is the exact
    /// bug that stamped every Aquatiq crawl job with a placeholder org.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn dev_bypass_prefers_valid_jwt_over_stub_claims() {
        clear_env();
        reset_jwks_cache_for_test().await;
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");

        let kid = "test-kid-real";
        let server = start_jwks_mock(kid).await;
        std::env::set_var("AUTH_CORE_JWKS_URL", format!("{}/jwks", server.uri()));

        let claims = Claims {
            web_policy: None,
            sub: "user-real".into(),
            iss: "auth-core".into(),
            exp: now_secs() + 3600,
            org_id: "org-real-coresystem".into(),
            user_id: "user-real".into(),
            principal_type: Some("user".into()),
            service_id: None,
            nbf: None,
            aud: None,
            scopes: Vec::new(),
            zdr: None,
            key_id: None,
            acting_user_id: None,
        };
        let token = sign_jwt(&claims, kid);

        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let res = tower::ServiceExt::oneshot(echo_org_router(), req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"org-real-coresystem");

        clear_env();
        reset_jwks_cache_for_test().await;
    }

    /// Companion case: with the bypass ON, a bearer that genuinely fails
    /// verification (unknown kid — never issued by the configured JWKS)
    /// still falls back to the stub, preserving the legitimate local-dev
    /// escape hatch for non-JWT callers.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn dev_bypass_falls_back_to_stub_when_jwt_verification_fails() {
        clear_env();
        reset_jwks_cache_for_test().await;
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");

        let kid = "test-kid-real-2";
        let server = start_jwks_mock(kid).await;
        std::env::set_var("AUTH_CORE_JWKS_URL", format!("{}/jwks", server.uri()));

        // Signed with a kid the JWKS mock never advertises — verification
        // must fail (no matching JWK even after a forced refresh).
        let claims = Claims {
            web_policy: None,
            sub: "user-real".into(),
            iss: "auth-core".into(),
            exp: now_secs() + 3600,
            org_id: "org-real-coresystem".into(),
            user_id: "user-real".into(),
            principal_type: Some("user".into()),
            service_id: None,
            nbf: None,
            aud: None,
            scopes: Vec::new(),
            zdr: None,
            key_id: None,
            acting_user_id: None,
        };
        let token = sign_jwt(&claims, "unknown-kid");

        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let res = tower::ServiceExt::oneshot(echo_org_router(), req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"org_placeholder");

        clear_env();
        reset_jwks_cache_for_test().await;
    }

    // -----------------------------------------------------------------
    // Z-08 — the `zdr` claim is a floor for USER principals only.
    // -----------------------------------------------------------------

    fn claims_with(principal_type: Option<&str>, zdr: Option<bool>) -> Claims {
        let service = principal_type == Some("service");
        Claims {
            web_policy: None,
            sub: if service {
                "service:model-gateway".into()
            } else {
                "user-1".into()
            },
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: if service {
                "service:model-gateway".into()
            } else {
                "user-1".into()
            },
            principal_type: principal_type.map(str::to_owned),
            service_id: service.then(|| "service:model-gateway".to_owned()),
            nbf: None,
            aud: Some("quarry".into()),
            scopes: Vec::new(),
            zdr,
            key_id: None,
            acting_user_id: None,
        }
    }

    #[test]
    fn zdr_claim_is_absent_by_default_and_never_an_auth_failure() {
        let claims: Claims = serde_json::from_value(serde_json::json!({
            "sub": "u1",
            "iss": "auth-core",
            "exp": 1_900_000_000_i64,
            "org_id": "alpha",
            "user_id": "u1",
            "principal_type": "user"
        }))
        .expect("a token minted before the claim existed must still decode");
        assert_eq!(claims.zdr, None);
        assert!(!claims.zdr_floor());
        // An absent claim is not serialized back either.
        assert!(!serde_json::to_string(&claims).unwrap().contains("zdr"));
    }

    #[test]
    fn zdr_claim_decodes_true_and_false() {
        for (raw, expected) in [(true, Some(true)), (false, Some(false))] {
            let claims: Claims = serde_json::from_value(serde_json::json!({
                "sub": "u1",
                "iss": "auth-core",
                "exp": 1_900_000_000_i64,
                "org_id": "alpha",
                "user_id": "u1",
                "principal_type": "user",
                "zdr": raw
            }))
            .unwrap();
            assert_eq!(claims.zdr, expected);
        }
    }

    #[test]
    fn the_floor_comes_from_a_users_claim_and_never_from_a_services() {
        // user: the org's posture counts.
        assert!(claims_with(Some("user"), Some(true)).zdr_floor());
        assert!(!claims_with(Some("user"), Some(false)).zdr_floor());
        assert!(!claims_with(Some("user"), None).zdr_floor());
        // legacy user shape (no principal_type): validated as a user, so it counts.
        assert!(claims_with(None, Some(true)).zdr_floor());
        assert!(!claims_with(None, None).zdr_floor());
        // service: the claim is a registry property (zdr:true for every org on
        // every call for model-gateway / execution-core), so it never counts.
        assert!(!claims_with(Some("service"), Some(true)).zdr_floor());
        assert!(!claims_with(Some("service"), Some(false)).zdr_floor());
        assert!(!claims_with(Some("service"), None).zdr_floor());
    }

    #[test]
    fn effective_zdr_can_only_raise_the_posture() {
        let zdr_user = claims_with(Some("user"), Some(true));
        let plain_user = claims_with(Some("user"), Some(false));
        let registry_zdr_service = claims_with(Some("service"), Some(true));

        // body false cannot lower a ZDR org's user
        assert!(zdr_user.effective_zdr(false));
        assert!(zdr_user.effective_zdr(true));
        // body true always raises
        assert!(plain_user.effective_zdr(true));
        assert!(!plain_user.effective_zdr(false));
        // a service caller runs on its body flag in both directions
        assert!(registry_zdr_service.effective_zdr(true));
        assert!(!registry_zdr_service.effective_zdr(false));
    }

    #[test]
    fn effective_zdr_mode_reads_the_optional_body_field() {
        let zdr_user = claims_with(Some("user"), Some(true));
        let plain_user = claims_with(Some("user"), Some(false));
        assert_eq!(zdr_user.effective_zdr_mode(None), ZdrMode::On);
        assert_eq!(zdr_user.effective_zdr_mode(Some(false)), ZdrMode::On);
        assert_eq!(plain_user.effective_zdr_mode(None), ZdrMode::Off);
        assert_eq!(plain_user.effective_zdr_mode(Some(false)), ZdrMode::Off);
        assert_eq!(plain_user.effective_zdr_mode(Some(true)), ZdrMode::On);
    }

    fn effective_zdr_router() -> Router {
        async fn echo(Extension(claims): Extension<Claims>) -> String {
            format!(
                "floor={} effective_for_body_false={}",
                claims.zdr_floor(),
                claims.effective_zdr(false)
            )
        }
        Router::new()
            .route("/", get(echo))
            .layer(middleware::from_fn(require_auth))
    }

    async fn verified_posture(claims: Claims, kid: &str) -> String {
        let server = start_jwks_mock(kid).await;
        std::env::set_var("AUTH_CORE_JWKS_URL", format!("{}/jwks", server.uri()));
        let token = sign_jwt(&claims, kid);
        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let res = tower::ServiceExt::oneshot(effective_zdr_router(), req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(body.to_vec()).unwrap()
    }

    /// The claim survives real RS256 verification and normalization with the
    /// meaning the floor needs: a user's `zdr:true` lifts, a service's does not.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn verified_tokens_carry_the_zdr_claim_through_require_auth() {
        clear_env();
        reset_jwks_cache_for_test().await;
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");

        let mut zdr_user = claims_with(Some("user"), Some(true));
        zdr_user.exp = now_secs() + 3600;
        assert_eq!(
            verified_posture(zdr_user, "kid-zdr-user").await,
            "floor=true effective_for_body_false=true"
        );

        reset_jwks_cache_for_test().await;
        let mut service = claims_with(Some("service"), Some(true));
        service.exp = now_secs() + 3600;
        service.user_id = String::new();
        assert_eq!(
            verified_posture(service, "kid-zdr-service").await,
            "floor=false effective_for_body_false=false"
        );

        reset_jwks_cache_for_test().await;
        let mut absent = claims_with(Some("user"), None);
        absent.exp = now_secs() + 3600;
        assert_eq!(
            verified_posture(absent, "kid-zdr-absent").await,
            "floor=false effective_for_body_false=false"
        );

        clear_env();
        reset_jwks_cache_for_test().await;
    }
}
