//! Admission for the gateway's CONTENT caches (search results, scrape previews).
//!
//! The result cache holds two very different kinds of entry:
//!
//!   * identity and posture entries (session context, org ZDR posture, the live
//!     membership decision). These are keyed by something that already names the
//!     subject and are read through [`crate::cache::ResultCache::lookup`] and
//!     friends;
//!   * CONTENT entries, which are built from what an upstream returned for a
//!     caller. For search that includes an `answer` and `citations` computed
//!     from the caller's ORG-SCOPED private corpus by quarry-edge, so the entry
//!     belongs to an org, not to the query string.
//!
//! Content entries must therefore be keyed by org, never stored or served for a
//! Zero-Data-Retention caller, and never reused across a different request shape.
//! This module is the one place that decides it. Every content lookup and store
//! goes through a [`CacheAdmission`]; `tests/cache_admission_only.rs` fails the
//! build if a content handler reaches `cache.lookup(` or `cache.store(` directly.
//!
//! Fail-closed means "skip the cache" (read, write and stale fallback), never
//! "refuse the request": nothing the answer depends on is read from the cache.

use axum::http::HeaderMap;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    cache,
    config::AppState,
    middleware::AuthenticatedUser,
    zdr::{self, OrgPosture},
};

/// Default freshness window for a cached scrape preview, used when the caller
/// sends no `max_age_s`. Matches `cache::FRESH_SECS`, which this replaced for
/// the preview route so the caller's policy can override it.
pub(crate) const PREVIEW_FRESH_SECS: u64 = 4 * 60 * 60;

/// An org id longer than this is not a scope.
const MAX_SCOPE_LEN: usize = 128;

/// Body fields that make a scrape request depend on the caller's identity or on
/// per-request browser state. A response built under any of them must never be
/// shared through a cache, even within one org.
const SHAPE_DENY_KEYS: &[&str] = &[
    "headers",
    "actions",
    "actionsOnError",
    "viewport",
    "capture",
    "captures",
    "profile",
    "profile_id",
    "profileId",
    "session",
    "session_id",
    "sessionId",
    "cookies",
    "authorization",
    "proxy",
];

/// Why a request got no cache. The label is the `outcome` of the
/// `gateway_result_cache_total` counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Denied {
    /// `GATEWAY_RESULT_CACHE_CONTENT=off`.
    ContentCacheOff,
    /// The request or the org says Zero Data Retention.
    Zdr,
    /// org-core did not answer, so the posture is unknown: skip the cache.
    ZdrUnknown,
    /// No usable org scope on the authenticated session.
    NoScope,
    /// The request carries headers, actions, a profile or a session.
    RequestShape,
    /// The caller's own `cache` policy says bypass (or is not understood).
    Policy,
}

impl Denied {
    pub(crate) fn outcome(self) -> &'static str {
        match self {
            Self::ContentCacheOff => "bypass_content_off",
            Self::Zdr => "bypass_zdr",
            Self::ZdrUnknown => "bypass_zdr_unknown",
            Self::NoScope => "bypass_no_scope",
            Self::RequestShape => "bypass_request_shape",
            Self::Policy => "bypass_policy",
        }
    }
}

/// Whether the BFF may read and write its own copy of a content response, under
/// which org scope, and for how long a cached copy counts as fresh.
///
/// Deliberately shaped like quarry-edge's three admission helpers
/// (`should_read` / `should_write` / `effective_ttl`) so the two layers cannot
/// drift: `may_read`/`may_write` correspond to the first two, `max_age_secs` to
/// the third.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheAdmission {
    scope: Option<String>,
    pub(crate) may_read: bool,
    pub(crate) may_write: bool,
    pub(crate) max_age_secs: u64,
    zdr: bool,
    denial: Option<Denied>,
}

impl CacheAdmission {
    /// Nothing in, nothing out.
    fn denied(reason: Denied, zdr: bool) -> Self {
        Self {
            scope: None,
            may_read: false,
            may_write: false,
            max_age_secs: 0,
            zdr,
            denial: Some(reason),
        }
    }

    /// Resolve admission for this request.
    ///
    /// Order (cheapest first, and the one that cannot be argued with first):
    /// 1. the request's own ZDR flag (`x-zdr` header or `zdr: true` body), no I/O;
    /// 2. the org scope from the live session membership, validated;
    /// 3. the org's standing posture from org-core (tri-state);
    /// 4. the content-cache lever;
    /// 5. for `check_request_shape`, the request shape deny list;
    /// 6. the caller's own `cache` policy.
    ///
    /// The posture is derived SERVER-side ([`crate::zdr`]): a client that never
    /// sends `zdr` but belongs to a ZDR workspace must still get no cache.
    pub(crate) async fn resolve(
        state: &AppState,
        user: &AuthenticatedUser,
        headers: &HeaderMap,
        body: &Value,
        check_request_shape: bool,
    ) -> Self {
        if zdr::request_zdr(headers, Some(body)) {
            return Self::denied(Denied::Zdr, true);
        }
        let org_id = crate::upstream::authorized_org_id(state, user).await;
        let Some(scope) = valid_org_scope(&org_id) else {
            return Self::denied(Denied::NoScope, false);
        };
        match zdr::org_posture(state, user).await {
            OrgPosture::Zdr => return Self::denied(Denied::Zdr, true),
            OrgPosture::Unknown => return Self::denied(Denied::ZdrUnknown, false),
            OrgPosture::NotZdr => {}
        }
        if !state.cache.content_enabled() {
            return Self::denied(Denied::ContentCacheOff, false);
        }
        if check_request_shape && request_shape_denied(body) {
            return Self::denied(Denied::RequestShape, false);
        }
        let mut admission = Self::from_policy(body.get("cache"));
        if !admission.may_read && !admission.may_write {
            admission.denial = Some(Denied::Policy);
        }
        admission.scope = Some(scope.to_owned());
        admission
    }

    /// Read the caller's `CachePolicy` (quarry-core's `{mode, max_age_s, …}`) out
    /// of the same body the BFF forwards to quarry-edge. An absent or
    /// unrecognised policy keeps the previous defaults, so a client that sends
    /// no policy behaves exactly as before.
    pub(crate) fn from_policy(policy: Option<&Value>) -> Self {
        let mode = policy
            .and_then(|policy| policy.get("mode"))
            .and_then(Value::as_str)
            .map(str::trim);
        let (may_read, may_write) = match mode {
            None => (true, true),
            Some("read_write") => (true, true),
            Some("read_only") => (true, false),
            Some("write_only") => (false, true),
            Some("bypass") => (false, false),
            // An unknown mode is not a licence to cache: quarry-edge would
            // reject the body outright, so the safe local reading is "no".
            Some(_) => (false, false),
        };
        let max_age_secs = policy
            .and_then(|policy| policy.get("max_age_s"))
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
            .unwrap_or(PREVIEW_FRESH_SECS);
        Self {
            scope: None,
            may_read,
            may_write,
            max_age_secs,
            zdr: false,
            denial: None,
        }
    }

    /// The validated org scope this admission was resolved under.
    pub(crate) fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    /// True only when ZDR is ESTABLISHED (the request says so, or org-core says
    /// so). An org-core outage is not an establishment: the cache is skipped but
    /// `zdr: true` is not forwarded because of it.
    pub(crate) fn zdr_established(&self) -> bool {
        self.zdr
    }

    #[cfg(test)]
    pub(crate) fn denial(&self) -> Option<Denied> {
        self.denial
    }

    /// The org-scoped key for this request, or `None` when this admission may
    /// neither read nor write (so there is nothing to look up or store).
    pub(crate) fn key(&self, namespace: &str, parts: &[&str]) -> Option<String> {
        if !self.may_read && !self.may_write {
            return None;
        }
        let scope = self.scope.as_deref()?;
        Some(cache::scoped_key(namespace, scope, parts))
    }

    /// Count a bypass, if this admission is one.
    pub(crate) fn record_bypass(&self, namespace: &str) {
        if let Some(reason) = self.denial {
            crate::observability::record_result_cache(namespace, reason.outcome());
        }
    }
}

/// An org id that is safe to interpolate into a cache key: non-empty, at most
/// [`MAX_SCOPE_LEN`] characters, `[A-Za-z0-9_-]` only. Anything else (a
/// separator, a glob character, whitespace) is not a scope: it could make one
/// org's key pattern match another's.
pub(crate) fn valid_org_scope(org_id: &str) -> Option<&str> {
    let org_id = org_id.trim();
    let ok = !org_id.is_empty()
        && org_id.len() <= MAX_SCOPE_LEN
        && org_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    ok.then_some(org_id)
}

/// True when the request body asks for something that depends on the caller's
/// identity or on browser state, so the response must not be shared.
fn request_shape_denied(body: &Value) -> bool {
    if SHAPE_DENY_KEYS
        .iter()
        .any(|key| body.get(*key).is_some_and(|value| !value.is_null()))
    {
        return true;
    }
    body.get("ingest").and_then(Value::as_bool).unwrap_or(false)
}

/// SHA-256 of the request body minus the fields that are already part of the
/// key (`url`) or of admission (`cache`, `zdr`), with object keys sorted, so two
/// requests that differ in any option the upstream honours (formats, render
/// hints, waits, locale) never share an entry.
pub(crate) fn variant_digest(body: &Value) -> String {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut sorted: Vec<(&String, &Value)> = map.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(b.0));
                let mut out = Map::new();
                for (key, inner) in sorted {
                    out.insert(key.clone(), canonical(inner));
                }
                Value::Object(out)
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            other => other.clone(),
        }
    }
    let mut trimmed = body.clone();
    if let Some(map) = trimmed.as_object_mut() {
        for key in ["url", "cache", "zdr"] {
            map.remove(key);
        }
    }
    let encoded = serde_json::to_string(&canonical(&trimmed)).unwrap_or_default();
    hex::encode(Sha256::digest(encoded.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use serde_json::json;

    fn user_in(org: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some(org.to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: org.to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    fn preview_user() -> AuthenticatedUser {
        user_in("org-1")
    }

    async fn state_with_org_zdr(zdr: bool) -> (AppState, wiremock::MockServer) {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org-1",
                "metadata": { "interactiveRetention": { "zdr": zdr } }
            })))
            .mount(&org_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.org_core_url = org_core.uri();
        (state, org_core)
    }

    /// The BI-2 defect verbatim: a `zdr: true` scrape is forwarded to Quarry,
    /// Quarry refuses to cache it — and the BFF wrote the rendered page content
    /// into Frontend-Plane Redis for 24h regardless. Admission must be denied in
    /// BOTH directions, so a ZDR request also cannot be answered from an entry
    /// an earlier non-ZDR request left behind. An explicit `read_write` policy
    /// must not re-open it: the posture is resolved before the policy is read.
    #[tokio::test]
    async fn a_zdr_request_neither_reads_nor_writes_the_local_preview_cache() {
        let (state, _org_core) = state_with_org_zdr(false).await;
        let body = json!({
            "url": "https://example.invalid/",
            "zdr": true,
            "cache": { "mode": "read_write", "max_age_s": 3600 }
        });
        let admission =
            CacheAdmission::resolve(&state, &preview_user(), &HeaderMap::new(), &body, true).await;
        assert!(!admission.may_read && !admission.may_write);
        assert_eq!(admission.denial(), Some(Denied::Zdr));
        assert!(admission.zdr_established());
        assert!(admission.key("scrape-preview", &["x"]).is_none());

        // The `x-zdr` header alone does it too, with no `zdr` in the body.
        let mut headers = HeaderMap::new();
        headers.insert("x-zdr", HeaderValue::from_static("true"));
        let admission = CacheAdmission::resolve(
            &state,
            &preview_user(),
            &headers,
            &json!({ "url": "https://example.invalid/" }),
            true,
        )
        .await;
        assert!(!admission.may_read && !admission.may_write);
        assert_eq!(admission.denial(), Some(Denied::Zdr));
    }

    /// Org-wide ZDR is invisible to the request body: a client under a ZDR
    /// workspace sends an ordinary request. The posture has to be derived
    /// server-side or the content lands in Frontend-Plane Redis anyway.
    #[tokio::test]
    async fn an_org_under_zdr_caches_no_page_content_at_all() {
        let (state, _org_core) = state_with_org_zdr(true).await;
        let admission = CacheAdmission::resolve(
            &state,
            &preview_user(),
            &HeaderMap::new(),
            &json!({ "url": "https://example.invalid/" }),
            true,
        )
        .await;
        assert!(!admission.may_read && !admission.may_write);
        assert_eq!(admission.denial(), Some(Denied::Zdr));
        assert!(admission.zdr_established());
    }

    #[tokio::test]
    async fn an_ordinary_request_in_a_non_zdr_org_still_caches() {
        let (state, _org_core) = state_with_org_zdr(false).await;
        let admission = CacheAdmission::resolve(
            &state,
            &preview_user(),
            &HeaderMap::new(),
            &json!({ "url": "https://example.invalid/" }),
            true,
        )
        .await;
        assert!(admission.may_read && admission.may_write);
        assert_eq!(admission.max_age_secs, PREVIEW_FRESH_SECS);
        assert_eq!(admission.scope(), Some("org-1"));
        assert!(!admission.zdr_established());
    }

    /// A lookup failure must skip the cache in both directions but must not be
    /// reported to the upstream as ZDR: an org-core outage is not a posture.
    #[tokio::test]
    async fn org_lookup_failure_denies_both_directions() {
        // The default test state points org-core at a closed port.
        let state = crate::tests::test_state(false);
        let admission = CacheAdmission::resolve(
            &state,
            &preview_user(),
            &HeaderMap::new(),
            &json!({ "url": "https://example.invalid/" }),
            true,
        )
        .await;
        assert!(!admission.may_read && !admission.may_write);
        assert_eq!(admission.denial(), Some(Denied::ZdrUnknown));
        assert!(!admission.zdr_established());
    }

    #[tokio::test]
    async fn no_authorized_org_denies() {
        let state = crate::tests::test_state(false);
        let mut user = preview_user();
        user.authorized_membership = None;
        let admission = CacheAdmission::resolve(
            &state,
            &user,
            &HeaderMap::new(),
            &json!({ "url": "https://example.invalid/" }),
            true,
        )
        .await;
        assert!(!admission.may_read && !admission.may_write);
        assert_eq!(admission.denial(), Some(Denied::NoScope));
        assert!(admission.key("search-web", &["q"]).is_none());
    }

    #[tokio::test]
    async fn request_shape_with_headers_actions_profile_or_ingest_denies() {
        let (state, _org_core) = state_with_org_zdr(false).await;
        for extra in [
            json!({ "headers": { "x-a": "b" } }),
            json!({ "actions": [{ "type": "click" }] }),
            json!({ "profile": "p1" }),
            json!({ "profile_id": "p1" }),
            json!({ "session_id": "s1" }),
            json!({ "cookies": "a=b" }),
            json!({ "authorization": "Bearer x" }),
            json!({ "ingest": true }),
        ] {
            let mut body = json!({ "url": "https://example.invalid/" });
            body.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let admission =
                CacheAdmission::resolve(&state, &preview_user(), &HeaderMap::new(), &body, true)
                    .await;
            assert_eq!(admission.denial(), Some(Denied::RequestShape), "{body}");
            assert!(!admission.may_read && !admission.may_write);
        }
        // The same body is fine for a caller that does not ask for the shape check.
        let body = json!({ "url": "https://example.invalid/", "headers": { "x": "y" } });
        let admission =
            CacheAdmission::resolve(&state, &preview_user(), &HeaderMap::new(), &body, false).await;
        assert!(admission.may_read);
    }

    /// quarry-edge honours `{mode: "bypass"}`; this layer silently ignored it,
    /// so a caller who asked for a live fetch got a preview up to 4h old.
    #[test]
    fn the_callers_cache_mode_is_honoured_the_way_quarry_edge_honours_it() {
        let admission = |mode: &str| CacheAdmission::from_policy(Some(&json!({ "mode": mode })));

        let bypass = admission("bypass");
        assert!(!bypass.may_read && !bypass.may_write);

        let read_only = admission("read_only");
        assert!(read_only.may_read && !read_only.may_write);

        let write_only = admission("write_only");
        assert!(!write_only.may_read && write_only.may_write);

        let read_write = admission("read_write");
        assert!(read_write.may_read && read_write.may_write);

        // An unknown mode must not read as permission. quarry-edge rejects the
        // body outright; the safe local reading is "cache nothing".
        let unknown = admission("write_through");
        assert!(!unknown.may_read && !unknown.may_write);
    }

    #[test]
    fn no_policy_keeps_the_previous_defaults() {
        let default = CacheAdmission::from_policy(None);
        assert!(default.may_read && default.may_write);
        assert_eq!(default.max_age_secs, PREVIEW_FRESH_SECS);
        // An empty policy object is the same as none.
        let empty = CacheAdmission::from_policy(Some(&json!({})));
        assert!(empty.may_read && empty.may_write);
        assert_eq!(empty.max_age_secs, PREVIEW_FRESH_SECS);
    }

    /// `max_age_s` is the caller's freshness window, matching quarry-edge's
    /// `effective_ttl`. A zero/absent value falls back to the default rather
    /// than meaning "instantly stale".
    #[test]
    fn max_age_s_narrows_the_freshness_window() {
        let tight = CacheAdmission::from_policy(Some(&json!({ "max_age_s": 60 })));
        assert_eq!(tight.max_age_secs, 60);
        let zero = CacheAdmission::from_policy(Some(&json!({ "max_age_s": 0 })));
        assert_eq!(zero.max_age_secs, PREVIEW_FRESH_SECS);
    }

    #[test]
    fn scoped_keys_differ_by_org_and_variant() {
        let a = cache::scoped_key("search-web", "org-a", &["q", "10"]);
        let b = cache::scoped_key("search-web", "org-b", &["q", "10"]);
        assert_ne!(a, b);
        assert!(a.starts_with("verevon:gw:search-web:o:org-a:"));
        assert!(b.starts_with("verevon:gw:search-web:o:org-b:"));
        // Part boundaries are part of the key.
        assert_ne!(
            cache::scoped_key("n", "o", &["a", "bc"]),
            cache::scoped_key("n", "o", &["ab", "c"])
        );
        // Request variants (anything but url/cache/zdr) change the digest.
        let one = variant_digest(&json!({ "url": "u", "formats": ["markdown"] }));
        let two = variant_digest(&json!({ "url": "u", "formats": ["html"] }));
        assert_ne!(one, two);
        // ...and key order, `url`, `cache` and `zdr` do not.
        let same = variant_digest(&json!({
            "cache": { "mode": "bypass" }, "formats": ["markdown"], "url": "other", "zdr": false
        }));
        assert_eq!(one, same);
    }

    #[test]
    fn an_org_id_with_separator_or_glob_characters_is_not_a_scope() {
        assert_eq!(valid_org_scope("org-1_A"), Some("org-1_A"));
        assert_eq!(valid_org_scope("  org-1  "), Some("org-1"));
        for bad in [
            "", "   ", "org:1", "org*", "org?", "org[1]", "o/rg", "org 1", "org\n1",
        ] {
            assert!(valid_org_scope(bad).is_none(), "{bad:?}");
        }
        assert!(valid_org_scope(&"a".repeat(129)).is_none());
        assert!(valid_org_scope(&"a".repeat(128)).is_some());
    }

    #[tokio::test]
    async fn a_scope_tag_mismatch_is_a_miss_not_a_hit() {
        let cache = cache::ResultCache::in_memory();
        let key = cache::scoped_key("search-web", "org-a", &["q"]);
        cache.store_scoped(&key, "org-a", &json!({ "v": 1 })).await;
        assert!(cache.lookup_scoped(&key, "org-a").await.is_some());
        // The same key read under another scope is a miss.
        assert!(cache.lookup_scoped(&key, "org-b").await.is_none());
        assert!(cache
            .lookup_within_scoped(&key, "org-b", 60)
            .await
            .is_none());
        // An entry written without a scope tag (pre-scoping format) is a miss too.
        cache.store_for_secs(&key, &json!({ "v": 2 }), 60).await;
        assert!(cache.lookup_scoped(&key, "org-a").await.is_none());
    }
}
