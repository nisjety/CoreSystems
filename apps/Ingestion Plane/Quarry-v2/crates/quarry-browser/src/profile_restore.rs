//! Restore of a saved profile into a fresh browser context (WS17.C2).
//!
//! The first tab of a context restores the whole snapshot BEFORE its first navigation, once
//! per context, with no reload:
//!
//! 1. the cookies go into the context's jar with `Storage.setCookies { browserContextId }`,
//!    in chunks of 100, with no page URL (so a blank first tab is no obstacle). Expired
//!    cookies and session cookies older than the policy are skipped and counted. A host-only
//!    cookie (captured domain without a leading dot) is restored with `url: https://<host>/`
//!    and no `domain`, so `__Host-` and host-only semantics survive. A cookie Chromium
//!    rejects is counted, never fatal (a rejected chunk is retried cookie by cookie). The
//!    acceptance is then CHECKED (D2-117): a refused chunk and a systematic refusal (every
//!    cookie refused) are counted, and the jar is read back so a cookie Chromium accepted
//!    and then dropped is moved from applied to rejected;
//! 2. web storage and IndexedDB come back through one `Page.addScriptToEvaluateOnNewDocument`
//!    script that holds an origin-keyed map and acts only when `location.origin` is in it and
//!    the sentinel `__quarry_profile_seeded_v1` is absent from that origin's localStorage;
//! 3. the locale and the timezone are emulated on the page. **The user agent is never
//!    overridden** (honest identity, ADR-5): a changed user agent is only recorded
//!    (`ua_changed`).
//!
//! The pure half (policy, the cookie plan, the seeding script, the report) compiles without
//! the `chromiumoxide` feature so `cargo test -p quarry-browser` covers it. The CDP half is
//! in [`cdp`]. Nothing here logs or reports a cookie name, an origin, a URL or a value: the
//! report is counts.

use std::time::Duration;

use chrono::{DateTime, Utc};
use quarry_core::contracts::ProfileRestoreSummary;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::page_emulation::PageEmulation;

use crate::profile_snapshot::{
    CookiePriority, OriginState, PartitionKey, SameSite, SourceScheme, SEEDED_SENTINEL_KEY,
};
use crate::session::{Cookie, SessionSnapshot};

/// A session cookie older than this many seconds (by the snapshot's capture time) is not
/// restored. Default 7 days. `0` restores no session cookie at all.
pub const SESSION_COOKIE_MAX_AGE_ENV: &str = "QUARRY_BROWSER_PROFILE_SESSION_COOKIE_MAX_AGE_S";
pub const DEFAULT_SESSION_COOKIE_MAX_AGE_S: u64 = 604_800;
/// Cookies per `Storage.setCookies` call.
pub const COOKIE_CHUNK: usize = 100;

// ---------------------------------------------------------------------------------------
// Loading: what a failed load means
// ---------------------------------------------------------------------------------------

/// Why a saved profile could not be loaded. The three cases are different decisions.
#[derive(Debug)]
pub enum ProfileLoadError {
    /// The key cannot be reached right now (the key provider or the Control Plane is
    /// down). The profile is NOT absent: the attach fails with the typed 503 and the caller
    /// retries; starting fresh would let the next save replace it.
    KeyUnavailable(QuarryError),
    /// The profile cannot be decoded, or the store failed in another way. The session
    /// starts fresh and `lease_restore_failed` is set, so the release does not overwrite
    /// what is stored.
    Corrupt(QuarryError),
    /// The key was destroyed (crypto-shredding): the profile is gone for good. Same as no
    /// profile.
    KeyDestroyed,
}

impl ProfileLoadError {
    /// Classify the error a [`crate::session::ProfileStore::load`] returned. The envelope
    /// store reports a key that cannot be used as `RuntimeNotReady` (`profile key
    /// unavailable` or `profile key destroyed`); everything else is treated as corrupt (the
    /// safe direction: start fresh, do not overwrite).
    #[must_use]
    pub fn classify(error: QuarryError) -> Self {
        if error.code == ErrorCode::RuntimeNotReady {
            if error.message.contains("key destroyed") {
                return Self::KeyDestroyed;
            }
            return Self::KeyUnavailable(error);
        }
        Self::Corrupt(error)
    }

    /// Whether a release must NOT save over the stored profile after this load failure.
    /// An unreadable profile and an unreachable key both mean "the stored profile is not
    /// known to be empty": starting fresh and saving would replace it (D2-028). A destroyed
    /// key is the same as no profile, so there is nothing to protect.
    #[must_use]
    pub fn blocks_save(&self) -> bool {
        !matches!(self, Self::KeyDestroyed)
    }

    /// The restore report for a session whose profile could not be loaded (D2-028): a
    /// typed, visible signal instead of a silent fresh start. `failed` is set when the
    /// stored profile must not be overwritten; the class is the same label the log carries.
    /// Counts and a class only, never the store's error text.
    #[must_use]
    pub fn restore_report(&self) -> RestoreReport {
        RestoreReport {
            failed: self.blocks_save(),
            load_failure: Some(self.class()),
            ..RestoreReport::default()
        }
    }

    /// A short class for logs. Never the error text (it can carry store detail).
    #[must_use]
    pub fn class(&self) -> &'static str {
        match self {
            Self::KeyUnavailable(_) => "key_unavailable",
            Self::Corrupt(_) => "corrupt",
            Self::KeyDestroyed => "key_destroyed",
        }
    }
}

// ---------------------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestorePolicy {
    pub session_cookie_max_age: Duration,
}

impl Default for RestorePolicy {
    fn default() -> Self {
        Self {
            session_cookie_max_age: Duration::from_secs(DEFAULT_SESSION_COOKIE_MAX_AGE_S),
        }
    }
}

impl RestorePolicy {
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_raw(std::env::var(SESSION_COOKIE_MAX_AGE_ENV).ok().as_deref())
    }

    /// An absent or unparsable value is the default.
    #[must_use]
    pub fn from_raw(raw: Option<&str>) -> Self {
        let seconds = raw
            .and_then(|text| text.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_SESSION_COOKIE_MAX_AGE_S);
        Self {
            session_cookie_max_age: Duration::from_secs(seconds),
        }
    }
}

// ---------------------------------------------------------------------------------------
// The report (counts only)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreReport {
    pub cookies_total: u32,
    pub cookies_applied: u32,
    pub skipped_expired: u32,
    pub skipped_session_stale: u32,
    /// Cookies Chromium refused (or that could not be mapped).
    pub cookies_rejected: u32,
    pub origins_seeded: u32,
    pub idb_databases: u32,
    pub ua_changed: bool,
    pub snapshot_age_s: Option<u64>,
    pub truncated: bool,
    /// The restore itself did not finish (a timeout or a CDP failure): the session runs
    /// with what was applied, and its release does not overwrite the stored profile.
    pub failed: bool,
    /// The profile could not even be loaded (D2-028): `corrupt`, `key_unavailable` or
    /// `key_destroyed`. A closed label (see [`ProfileLoadError::class`]), never error text.
    /// Not part of the wire summary yet.
    pub load_failure: Option<&'static str>,
    /// `Storage.setCookies` calls that Chromium refused as a whole chunk (each is then
    /// retried cookie by cookie). A nonzero count with `cookies_applied == 0` is a
    /// systematic refusal, not a few bad cookies.
    pub cookie_chunks_refused: u32,
    /// Cookies Chromium accepted that the read-back did not find (counted in
    /// `cookies_rejected` too, and removed from `cookies_applied`).
    pub cookies_dropped_after_accept: u32,
    /// The jar could not be read back, so acceptance is as reported by `setCookies` only.
    pub cookie_readback_unavailable: bool,
}

impl RestoreReport {
    #[must_use]
    pub fn summary(&self) -> ProfileRestoreSummary {
        ProfileRestoreSummary {
            cookies_total: self.cookies_total,
            cookies_applied: self.cookies_applied,
            skipped_expired: self.skipped_expired,
            skipped_session_stale: self.skipped_session_stale,
            cookies_rejected: self.cookies_rejected,
            origins_seeded: self.origins_seeded,
            idb_databases: self.idb_databases,
            ua_changed: self.ua_changed,
            snapshot_age_s: self.snapshot_age_s,
            truncated: self.truncated,
            failed: self.failed,
        }
    }
}

// ---------------------------------------------------------------------------------------
// The cookie plan
// ---------------------------------------------------------------------------------------

/// A cookie ready to hand to Chromium, free of any CDP type so it is testable everywhere.
#[derive(Clone, PartialEq)]
pub struct CookiePlan {
    pub name: String,
    pub value: String,
    /// Set for a host-only cookie (`https://<host>/`); then `domain` is `None`.
    pub url: Option<String>,
    /// Set for a domain cookie (leading dot); then `url` is `None`.
    pub domain: Option<String>,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<SameSite>,
    /// Seconds since the epoch; `None` for a session cookie.
    pub expires_s: Option<f64>,
    pub priority: Option<CookiePriority>,
    pub source_scheme: Option<SourceScheme>,
    pub partition_key: Option<PartitionKey>,
}

/// Never prints the value.
impl std::fmt::Debug for CookiePlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookiePlan")
            .field("host_only", &self.url.is_some())
            .field("session", &self.expires_s.is_none())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieSkip {
    Expired,
    SessionStale,
    /// No name or no host: it cannot be restored.
    Invalid,
}

/// Decide what to do with one saved cookie. `age` is the age of the snapshot at restore time
/// (`None` for a legacy snapshot that does not say when it was captured: a session cookie is
/// then kept, because nothing says it is stale).
pub fn plan_cookie(
    cookie: &Cookie,
    now: DateTime<Utc>,
    age: Option<Duration>,
    policy: &RestorePolicy,
) -> Result<CookiePlan, CookieSkip> {
    let host = cookie.domain.trim_start_matches('.');
    if cookie.name.is_empty() || host.is_empty() {
        return Err(CookieSkip::Invalid);
    }
    let session = cookie.session || cookie.expires.is_none();
    if !session {
        if let Some(expires) = cookie.expires {
            if expires <= now {
                return Err(CookieSkip::Expired);
            }
        }
    } else if age.is_some_and(|age| age > policy.session_cookie_max_age) {
        return Err(CookieSkip::SessionStale);
    }
    let host_only = !cookie.domain.starts_with('.');
    let (url, domain) = if host_only {
        let authority = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        (Some(format!("https://{authority}/")), None)
    } else {
        (None, Some(cookie.domain.clone()))
    };
    let expires_s = if session {
        None
    } else {
        cookie
            .expires
            .map(|at| at.timestamp_millis() as f64 / 1000.0)
    };
    Ok(CookiePlan {
        name: cookie.name.clone(),
        value: cookie.value.clone(),
        url,
        domain,
        path: if cookie.path.is_empty() {
            "/".to_owned()
        } else {
            cookie.path.clone()
        },
        secure: cookie.secure,
        http_only: cookie.http_only,
        same_site: cookie.same_site,
        expires_s,
        priority: cookie.priority,
        source_scheme: cookie
            .source_scheme
            .filter(|scheme| *scheme != SourceScheme::Unset),
        partition_key: cookie.partition_key.clone(),
    })
}

// ---------------------------------------------------------------------------------------
// Applying the cookies, and checking they were taken (D2-117)
// ---------------------------------------------------------------------------------------

/// Where cookies go: the CDP half implements it over `Storage.setCookies` /
/// `Storage.getCookies`, the tests over a mock jar.
#[async_trait::async_trait]
pub trait CookieSink: Send + Sync {
    /// Hand `cookies` to the jar in one call. `true` only when Chromium accepted the call
    /// (it refuses the whole call when one cookie is invalid).
    async fn set(&self, cookies: &[CookiePlan]) -> bool;
    /// The jar's cookies as (name, domain, path), or `None` when it cannot be read.
    async fn read_back(&self) -> Option<Vec<CookieKey>>;
}

/// What identifies a cookie in the jar: name, host and path. Compared in lower case without
/// a leading dot or IPv6 brackets, because Chromium reports a host-only cookie's domain
/// bare and a domain cookie's with a dot.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CookieKey {
    name: String,
    host: String,
    path: String,
}

impl CookieKey {
    #[must_use]
    pub fn new(name: &str, domain: &str, path: &str) -> Self {
        Self {
            name: name.to_owned(),
            host: normalise_host(domain),
            path: path.to_owned(),
        }
    }

    /// The key a planned cookie has once it is in the jar.
    #[must_use]
    pub fn of_plan(plan: &CookiePlan) -> Self {
        let host = match (plan.domain.as_deref(), plan.url.as_deref()) {
            (Some(domain), _) => domain.to_owned(),
            (None, Some(url)) => url
                .strip_prefix("https://")
                .unwrap_or(url)
                .split('/')
                .next()
                .unwrap_or_default()
                .to_owned(),
            (None, None) => String::new(),
        };
        Self::new(&plan.name, &host, &plan.path)
    }
}

/// Never prints a name or a host.
impl std::fmt::Debug for CookieKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieKey").finish_non_exhaustive()
    }
}

fn normalise_host(domain: &str) -> String {
    domain
        .trim()
        .trim_start_matches('.')
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Apply `plans` to `sink`, in chunks of [`COOKIE_CHUNK`], and check the outcome. Fills the
/// cookie counts of `report` (it adds to them; the planner's skips stay).
///
/// 1. A chunk the jar accepts counts as applied. A chunk it refuses is counted in
///    `cookie_chunks_refused` and retried cookie by cookie, so one bad cookie costs one
///    cookie.
/// 2. When two or more cookies were planned and NONE was applied, the refusal is systematic
///    (the call itself is wrong, not a cookie): `failed` is set, so the release does not
///    save the cookie-less session over the stored profile.
/// 3. The jar is read back. An accepted cookie it does not hold was dropped after the
///    acceptance: it moves from applied to rejected and into `cookies_dropped_after_accept`.
///    An unreadable jar sets `cookie_readback_unavailable` and changes no count.
pub async fn apply_cookies(sink: &dyn CookieSink, plans: &[CookiePlan], report: &mut RestoreReport) {
    let mut accepted: Vec<&CookiePlan> = Vec::with_capacity(plans.len());
    for chunk in plans.chunks(COOKIE_CHUNK) {
        if sink.set(chunk).await {
            report.cookies_applied = report.cookies_applied.saturating_add(count(chunk.len()));
            accepted.extend(chunk.iter());
            continue;
        }
        report.cookie_chunks_refused += 1;
        for plan in chunk {
            if sink.set(std::slice::from_ref(plan)).await {
                report.cookies_applied += 1;
                accepted.push(plan);
            } else {
                report.cookies_rejected += 1;
            }
        }
    }
    if plans.len() >= 2 && accepted.is_empty() {
        report.failed = true;
    }
    if accepted.is_empty() {
        return;
    }
    match sink.read_back().await {
        None => report.cookie_readback_unavailable = true,
        Some(present) => {
            let present: std::collections::HashSet<CookieKey> = present.into_iter().collect();
            let dropped = accepted
                .iter()
                .filter(|plan| !present.contains(&CookieKey::of_plan(plan)))
                .count();
            let dropped = count(dropped);
            report.cookies_applied = report.cookies_applied.saturating_sub(dropped);
            report.cookies_rejected = report.cookies_rejected.saturating_add(dropped);
            report.cookies_dropped_after_accept =
                report.cookies_dropped_after_accept.saturating_add(dropped);
        }
    }
}

// ---------------------------------------------------------------------------------------
// Emulation
// ---------------------------------------------------------------------------------------

/// The locale and timezone a restored profile asks the page to emulate. The user agent is
/// deliberately absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileEmulation {
    /// ICU style (`en_US`).
    pub locale: Option<String>,
    /// IANA name (`Europe/Oslo`).
    pub timezone: Option<String>,
}

impl ProfileEmulation {
    #[must_use]
    pub fn from_snapshot(snapshot: &SessionSnapshot) -> Self {
        let locale = snapshot
            .locale
            .as_deref()
            .map(str::trim)
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 35
                    && value
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
            .map(|value| value.replace('-', "_"));
        let timezone = snapshot
            .timezone
            .as_deref()
            .map(str::trim)
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 64
                    && value
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '+' | '-'))
            })
            .map(str::to_owned);
        Self { locale, timezone }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locale.is_none() && self.timezone.is_none()
    }

    /// What the profile emulates once the page's own request sets some of it: a field the request
    /// sets is the request's to emulate, not the profile's (E4.4-a).
    #[must_use]
    pub fn without(&self, request: &PageEmulation) -> Self {
        Self {
            locale: if request.locale.is_some() {
                None
            } else {
                self.locale.clone()
            },
            timezone: if request.timezone.is_some() {
                None
            } else {
                self.timezone.clone()
            },
        }
    }
}

// ---------------------------------------------------------------------------------------
// The plan of a whole restore
// ---------------------------------------------------------------------------------------

pub struct RestorePlan {
    pub cookies: Vec<CookiePlan>,
    /// Counts decided without Chromium: totals, skips, the origins and databases to seed.
    pub report: RestoreReport,
    /// `None` when no origin has anything to seed.
    pub seeding_script: Option<String>,
    pub emulation: ProfileEmulation,
    /// The user agent the snapshot was captured with; compared with the live one.
    pub saved_user_agent: Option<String>,
}

/// Counts only: the plan holds values (cookies, the seeding script), so it never prints them.
impl std::fmt::Debug for RestorePlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestorePlan")
            .field("cookies", &self.cookies.len())
            .field("report", &self.report)
            .field("seeding_script", &self.seeding_script.is_some())
            .finish_non_exhaustive()
    }
}

/// Plan the restore of `snapshot` into a fresh context whose first navigation is
/// `initial_url`. A legacy flat snapshot seeds into `initial_url`'s origin.
#[must_use]
pub fn plan_restore(
    snapshot: &SessionSnapshot,
    initial_url: &str,
    now: DateTime<Utc>,
    policy: &RestorePolicy,
) -> RestorePlan {
    let age = snapshot_age(snapshot, now);
    let mut report = RestoreReport {
        cookies_total: u32::try_from(snapshot.cookies.len()).unwrap_or(u32::MAX),
        snapshot_age_s: age.map(|age| age.as_secs()),
        ..RestoreReport::default()
    };
    let mut cookies = Vec::with_capacity(snapshot.cookies.len());
    for cookie in &snapshot.cookies {
        match plan_cookie(cookie, now, age, policy) {
            Ok(plan) => cookies.push(plan),
            Err(CookieSkip::Expired) => report.skipped_expired += 1,
            Err(CookieSkip::SessionStale) => report.skipped_session_stale += 1,
            Err(CookieSkip::Invalid) => report.cookies_rejected += 1,
        }
    }
    let origins = snapshot.effective_origins(initial_url);
    let seeded: Vec<&OriginState> = origins
        .iter()
        .filter(|origin| origin_has_seed(origin))
        .collect();
    report.origins_seeded = u32::try_from(seeded.len()).unwrap_or(u32::MAX);
    report.idb_databases = seeded
        .iter()
        .map(|origin| u32::try_from(origin.indexed_db.len()).unwrap_or(u32::MAX))
        .fold(0u32, u32::saturating_add);
    report.truncated = origins
        .iter()
        .any(|origin| origin.truncated || origin.dropped_values > 0);
    RestorePlan {
        cookies,
        report,
        seeding_script: build_seeding_script(&origins),
        emulation: ProfileEmulation::from_snapshot(snapshot),
        saved_user_agent: snapshot.user_agent.clone(),
    }
}

/// How old the snapshot is. `None` for a legacy snapshot without a capture time, and for a
/// capture time in the future (clock skew: treated as unknown, not as fresh).
#[must_use]
pub fn snapshot_age(snapshot: &SessionSnapshot, now: DateTime<Utc>) -> Option<Duration> {
    let captured = snapshot.captured_at?;
    (now - captured).to_std().ok()
}

fn origin_has_seed(origin: &OriginState) -> bool {
    origin
        .local_storage
        .iter()
        .any(|(key, _)| key != SEEDED_SENTINEL_KEY)
        || !origin.session_storage.is_empty()
        || !origin.indexed_db.is_empty()
}

// ---------------------------------------------------------------------------------------
// The seeding script
// ---------------------------------------------------------------------------------------

/// The script `Page.addScriptToEvaluateOnNewDocument` runs in every frame at document start.
/// `__QUARRY_ORIGIN_MAP__` is replaced by the origin-keyed map. It does nothing unless
/// `location.origin` is a key of the map and the sentinel is absent from the origin's
/// localStorage; then it sets the sentinel first, restores web storage, and recreates the
/// IndexedDB databases inside the `upgradeneeded` of its own `open` request, so a page that
/// opens the same database next queues behind the seeding and finds the records.
///
/// The decoder reads exactly the tagged encoding of `profile_snapshot::IdbValue`.
pub(crate) const SEEDING_JS_TEMPLATE: &str = r#"(() => {
  const MAP = __QUARRY_ORIGIN_MAP__;
  try {
    const origin = location.origin;
    if (!Object.prototype.hasOwnProperty.call(MAP, origin)) return;
    const st = MAP[origin];
    const SEED = '__quarry_profile_seeded_v1';
    let ls = null;
    try { ls = window.localStorage; } catch (e) { return; }
    if (ls.getItem(SEED) !== null) return;
    ls.setItem(SEED, '1');
    const put = (area, entries) => {
      for (const kv of entries || []) {
        try { if (kv[0] !== SEED || area !== ls) area.setItem(kv[0], kv[1]); } catch (e) {}
      }
    };
    put(ls, st.l);
    try { put(window.sessionStorage, st.s); } catch (e) {}
    if (!st.i || !st.i.length || typeof indexedDB === 'undefined') return;
    const unb64 = (s) => {
      const bin = atob(s);
      const u8 = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
      return u8;
    };
    const dec = (v) => {
      if (v === null || typeof v !== 'object') return v;
      if (Array.isArray(v)) return v.map(dec);
      if (Object.prototype.hasOwnProperty.call(v, '$t')) {
        switch (v.$t) {
          case 'undefined': return undefined;
          case 'dropped': return undefined;
          case 'num':
            return v.v === 'NaN' ? NaN : v.v === 'Infinity' ? Infinity : v.v === '-Infinity' ? -Infinity : -0;
          case 'bigint': return BigInt(v.v);
          case 'date': return new Date(v.v === 'invalid' ? NaN : v.v);
          case 'regexp': return new RegExp(v.source, v.flags);
          case 'bytes': {
            const u8 = unb64(v.v);
            if (v.k === 'ArrayBuffer') return u8.buffer;
            if (v.k === 'DataView') return new DataView(u8.buffer);
            const C = globalThis[v.k];
            if (typeof C === 'function' && C.BYTES_PER_ELEMENT && u8.length % C.BYTES_PER_ELEMENT === 0) {
              return new C(u8.buffer);
            }
            return u8;
          }
          case 'blob': return new Blob([unb64(v.v)], { type: v.mime || '' });
          case 'map': return new Map(v.v.map((p) => [dec(p[0]), dec(p[1])]));
          case 'set': return new Set(v.v.map(dec));
          case 'obj': {
            const o = {};
            for (const k of Object.keys(v.v)) o[k] = dec(v.v[k]);
            return o;
          }
        }
        return undefined;
      }
      const o = {};
      for (const k of Object.keys(v)) o[k] = dec(v[k]);
      return o;
    };
    for (const d of st.i) {
      try {
        const req = indexedDB.open(d.name, d.version || 1);
        req.onupgradeneeded = () => {
          try {
            const db = req.result;
            for (const s of d.stores || []) {
              try {
                if (db.objectStoreNames.contains(s.name)) continue;
                const opts = {};
                if (s.key_path !== undefined && s.key_path !== null) opts.keyPath = s.key_path;
                if (s.auto_increment) opts.autoIncrement = true;
                const os = db.createObjectStore(s.name, opts);
                for (const ix of s.indexes || []) {
                  try {
                    os.createIndex(ix.name, ix.key_path, { unique: !!ix.unique, multiEntry: !!ix.multi_entry });
                  } catch (e) {}
                }
                if (s.omitted) continue;
                const inline = os.keyPath !== null;
                for (const r of s.records || []) {
                  try {
                    if (r.value && r.value.$t === 'dropped') continue;
                    const value = dec(r.value);
                    const rq = inline ? os.put(value) : os.put(value, dec(r.key));
                    rq.onerror = (ev) => { try { ev.preventDefault(); ev.stopPropagation(); } catch (e) {} };
                  } catch (e) {}
                }
              } catch (e) {}
            }
          } catch (e) {}
        };
        req.onsuccess = () => { try { req.result.close(); } catch (e) {} };
        req.onerror = (ev) => { try { ev.preventDefault(); } catch (e) {} };
      } catch (e) {}
    }
  } catch (e) {}
})();"#;

/// The seeding script for `origins`, or `None` when no origin has anything to seed. The map
/// holds only origins that have web storage or IndexedDB: an origin without a seed is not in
/// the script at all, so the script reveals nothing about it.
#[must_use]
pub fn build_seeding_script(origins: &[OriginState]) -> Option<String> {
    let mut map = serde_json::Map::new();
    for origin in origins {
        if !origin_has_seed(origin) || map.contains_key(&origin.origin) {
            continue;
        }
        let local: Vec<&(String, String)> = origin
            .local_storage
            .iter()
            .filter(|(key, _)| key != SEEDED_SENTINEL_KEY)
            .collect();
        map.insert(
            origin.origin.clone(),
            serde_json::json!({
                "l": local,
                "s": origin.session_storage,
                "i": origin.indexed_db,
            }),
        );
    }
    if map.is_empty() {
        return None;
    }
    let json = serde_json::to_string(&serde_json::Value::Object(map)).ok()?;
    // `<` only occurs inside JSON strings: its escape is valid there and keeps the script
    // from ever containing a closing tag. U+2028 and U+2029 likewise.
    let json = json
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Some(SEEDING_JS_TEMPLATE.replace("__QUARRY_ORIGIN_MAP__", &json))
}

// ---------------------------------------------------------------------------------------
// The CDP half
// ---------------------------------------------------------------------------------------

#[cfg(feature = "chromiumoxide")]
pub(crate) use cdp::{apply_page_emulation, apply_profile_emulation, restore_into_context};

#[cfg(feature = "chromiumoxide")]
mod cdp {
    use ::chromiumoxide::cdp::browser_protocol::browser::{
        BrowserContextId, PermissionDescriptor, PermissionSetting, SetPermissionParams,
    };
    use ::chromiumoxide::cdp::browser_protocol::emulation::{
        SetGeolocationOverrideParams, SetLocaleOverrideParams, SetTimezoneOverrideParams,
    };
    use ::chromiumoxide::cdp::browser_protocol::network as net;
    use ::chromiumoxide::cdp::browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams;
    use ::chromiumoxide::cdp::browser_protocol::storage::{GetCookiesParams, SetCookiesParams};
    use ::chromiumoxide::{Browser, Page};
    use chrono::{DateTime, Utc};
    use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
    use quarry_core::identity::strip_product_token;
    use quarry_core::page_emulation::{
        accept_language_for, Location, PageEmulation, DEFAULT_ACCURACY_M,
    };
    use serde_json::json;

    use super::{
        apply_cookies, plan_restore, CookieKey, CookiePlan, CookieSink, ProfileEmulation,
        RestorePolicy, RestoreReport,
    };
    use crate::profile_snapshot::{CookiePriority, SameSite, SourceScheme};
    use crate::session::SessionSnapshot;

    /// A [`CookiePlan`] as the CDP parameter object.
    fn cookie_param(plan: &CookiePlan) -> net::CookieParam {
        let mut param = net::CookieParam::new(plan.name.clone(), plan.value.clone());
        param.url = plan.url.clone();
        param.domain = plan.domain.clone();
        param.path = Some(plan.path.clone());
        param.secure = Some(plan.secure);
        param.http_only = Some(plan.http_only);
        param.same_site = plan.same_site.map(|same_site| match same_site {
            SameSite::Strict => net::CookieSameSite::Strict,
            SameSite::Lax => net::CookieSameSite::Lax,
            SameSite::None => net::CookieSameSite::None,
        });
        param.expires = plan.expires_s.map(net::TimeSinceEpoch::new);
        param.priority = plan.priority.map(|priority| match priority {
            CookiePriority::Low => net::CookiePriority::Low,
            CookiePriority::Medium => net::CookiePriority::Medium,
            CookiePriority::High => net::CookiePriority::High,
        });
        param.source_scheme = plan.source_scheme.map(|scheme| match scheme {
            SourceScheme::Unset => net::CookieSourceScheme::Unset,
            SourceScheme::NonSecure => net::CookieSourceScheme::NonSecure,
            SourceScheme::Secure => net::CookieSourceScheme::Secure,
        });
        param.partition_key = plan.partition_key.as_ref().map(|key| {
            net::CookiePartitionKey::new(key.top_level_site.clone(), key.has_cross_site_ancestor)
        });
        param
    }

    /// The context's cookie jar over `Storage.setCookies` / `Storage.getCookies`.
    struct ContextJar<'a> {
        browser: &'a Browser,
        context: &'a BrowserContextId,
    }

    #[async_trait::async_trait]
    impl CookieSink for ContextJar<'_> {
        async fn set(&self, cookies: &[CookiePlan]) -> bool {
            let mut params = SetCookiesParams::new(cookies.iter().map(cookie_param).collect());
            params.browser_context_id = Some(self.context.clone());
            self.browser.execute(params).await.is_ok()
        }

        async fn read_back(&self) -> Option<Vec<CookieKey>> {
            let params = GetCookiesParams {
                browser_context_id: Some(self.context.clone()),
            };
            let response = self.browser.execute(params).await.ok()?;
            Some(
                response
                    .result
                    .cookies
                    .iter()
                    .map(|cookie| CookieKey::new(&cookie.name, &cookie.domain, &cookie.path))
                    .collect(),
            )
        }
    }

    /// Restore `snapshot` into `context` through `page` (the context's first tab, still on
    /// `about:blank`, before its first navigation). Returns the report and what the page
    /// should emulate. The caller bounds the whole call with its deadline.
    ///
    /// Never fatal: a cookie Chromium rejects is counted; a failed seeding script install
    /// sets `failed`. The only error is the one the caller turns into `failed` too.
    pub(crate) async fn restore_into_context(
        browser: &Browser,
        context: &BrowserContextId,
        page: &Page,
        snapshot: &SessionSnapshot,
        initial_url: &str,
        now: DateTime<Utc>,
        policy: &RestorePolicy,
    ) -> (RestoreReport, ProfileEmulation) {
        let plan = plan_restore(snapshot, initial_url, now, policy);
        let mut report = plan.report.clone();

        // 1. Cookies, 100 at a time; a refused chunk is retried one by one; the acceptance
        // is checked against the jar (D2-117).
        let jar = ContextJar { browser, context };
        apply_cookies(&jar, &plan.cookies, &mut report).await;

        // 2. Web storage and IndexedDB, seeded by the page itself at document start.
        if let Some(script) = plan.seeding_script {
            if page
                .evaluate_on_new_document(AddScriptToEvaluateOnNewDocumentParams::new(script))
                .await
                .is_err()
            {
                report.failed = true;
                report.origins_seeded = 0;
                report.idb_databases = 0;
            }
        }

        // 3. Is the browser the one that captured the profile? Recorded, never overridden.
        // E4.4-a2: a trailing product token (QUARRY_BROWSER__PRODUCT_TOKEN) is not a different
        // browser, so it is stripped from both sides before they are compared.
        if let Some(saved) = plan.saved_user_agent.as_deref() {
            if let Ok(result) = page.evaluate("navigator.userAgent").await {
                if let Ok(live) = result.into_value::<String>() {
                    report.ua_changed = strip_product_token(&live) != strip_product_token(saved);
                }
            }
        }
        (report, plan.emulation)
    }

    /// Emulate the profile's locale and timezone on `page`. Warn-only: a page that cannot
    /// emulate still works. The user agent is never touched.
    pub(crate) async fn apply_profile_emulation(page: &Page, emulation: &ProfileEmulation) {
        if let Some(locale) = emulation.locale.as_ref() {
            let result = page
                .execute(SetLocaleOverrideParams {
                    locale: Some(locale.clone()),
                })
                .await;
            if let Err(error) = result {
                tracing::warn!(
                    event = "profile_locale_override_failed",
                    error_class = emulation_cdp_class(&error.to_string()),
                    "profile locale could not be emulated"
                );
            }
        }
        if let Some(timezone) = emulation.timezone.as_ref() {
            let result = page
                .execute(SetTimezoneOverrideParams::new(timezone.clone()))
                .await;
            if let Err(error) = result {
                tracing::warn!(
                    event = "profile_timezone_override_failed",
                    error_class = emulation_cdp_class(&error.to_string()),
                    "profile timezone could not be emulated"
                );
            }
        }
    }

    /// E4.4-a: the request's own emulation, on `page` before its first navigation. Unlike the
    /// profile's, a failure is an error: the caller asked for this page to be emulated, and a page
    /// that quietly has the host's locale is not that. The position is granted to the page's origin,
    /// since a site checks that permission before it reads the position; a page with no origin
    /// (`about:blank`) gets the override and no grant.
    pub(crate) async fn apply_page_emulation(
        browser: &Browser,
        page: &Page,
        context: &BrowserContextId,
        initial_url: &str,
        emulation: &PageEmulation,
    ) -> QuarryResult<()> {
        if let Some(locale) = emulation.locale.as_ref() {
            page.execute(SetLocaleOverrideParams {
                locale: Some(locale.clone()),
            })
            .await
            .map_err(|error| emulation_failed("locale", &error.to_string()))?;
            // E4.4-a2: `Accept-Language` derived from the checked locale, set once here from the
            // request, never in reaction to a response (ADR 5 rules 1 and 3). Only this header is
            // added; the user agent is never touched.
            if emulation.send_accept_language {
                let value = accept_language_for(locale).ok_or_else(|| {
                    emulation_failed("accept_language", "Invalid accept-language value")
                })?;
                page.execute(net::SetExtraHttpHeadersParams::new(net::Headers::new(
                    json!({ "Accept-Language": value }),
                )))
                .await
                .map_err(|error| emulation_failed("accept_language", &error.to_string()))?;
            }
        }
        if let Some(timezone) = emulation.timezone.as_ref() {
            page.execute(SetTimezoneOverrideParams::new(timezone.clone()))
                .await
                .map_err(|error| emulation_failed("timezone", &error.to_string()))?;
        }
        if let Some(location) = emulation.location.as_ref() {
            page.execute(geolocation_override(location))
                .await
                .map_err(|error| emulation_failed("location", &error.to_string()))?;
            if let Some(origin) = origin_of(initial_url) {
                let mut grant = SetPermissionParams::new(
                    PermissionDescriptor::new("geolocation"),
                    PermissionSetting::Granted,
                );
                grant.origin = Some(origin);
                grant.browser_context_id = Some(context.clone());
                browser
                    .execute(grant)
                    .await
                    .map_err(|error| emulation_failed("location", &error.to_string()))?;
            }
        }
        Ok(())
    }

    /// The CDP override for a location. The accuracy is always sent: without one Chromium reports
    /// the position as unavailable.
    fn geolocation_override(location: &Location) -> SetGeolocationOverrideParams {
        SetGeolocationOverrideParams {
            latitude: Some(location.latitude),
            longitude: Some(location.longitude),
            accuracy: Some(location.accuracy.unwrap_or(DEFAULT_ACCURACY_M)),
            ..Default::default()
        }
    }

    /// The origin of `url` as a permission is granted to it; `None` for a URL with no origin.
    fn origin_of(url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        let origin = parsed.origin();
        origin.is_tuple().then(|| origin.ascii_serialization())
    }

    /// The typed failure of one emulated field. The CDP text is classed, never echoed: it can carry
    /// the value, and the value is the caller's.
    fn emulation_failed(field: &'static str, cdp_message: &str) -> QuarryError {
        let class = emulation_cdp_class(cdp_message);
        let code = if class == "invalid_value" {
            ErrorCode::BadRequest
        } else {
            ErrorCode::DriverFailed
        };
        QuarryError::new(code, "chromiumoxide page emulation failed").with_details(json!({
            "reason": "page_emulation_failed",
            "field": field,
            "error_class": class,
        }))
    }

    /// The CDP error text can echo the value; only a class is logged. The name ends in
    /// `cdp_class` so the log-field lint (R5) reads the value as a closed label.
    fn emulation_cdp_class(message: &str) -> &'static str {
        if message.contains("already in effect") {
            "already_set"
        } else if message.contains("Invalid") || message.contains("invalid") {
            "invalid_value"
        } else {
            "cdp_error"
        }
    }
}

// ---------------------------------------------------------------------------------------
// Tests (written in the build phase, run in the wave's test phase)
// ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_snapshot::{IdbDatabase, IdbRecord, IdbStore};
    use serde_json::json;

    fn now() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_900_000_000, 0).expect("timestamp")
    }

    fn cookie(name: &str, domain: &str) -> Cookie {
        Cookie {
            name: name.to_owned(),
            value: "SECRET-VALUE".to_owned(),
            domain: domain.to_owned(),
            path: "/".to_owned(),
            secure: true,
            http_only: true,
            expires: Some(now() + chrono::Duration::days(30)),
            ..Default::default()
        }
    }

    #[test]
    fn host_only_cookie_restores_with_url_not_domain() {
        let host_only = plan_cookie(
            &cookie("__Host-sid", "app.example.com"),
            now(),
            None,
            &RestorePolicy::default(),
        )
        .expect("planned");
        assert_eq!(host_only.url.as_deref(), Some("https://app.example.com/"));
        assert_eq!(host_only.domain, None);
        assert!(host_only.secure);
        assert_eq!(host_only.path, "/");

        let domain = plan_cookie(
            &cookie("sid", ".example.com"),
            now(),
            None,
            &RestorePolicy::default(),
        )
        .expect("planned");
        assert_eq!(domain.url, None);
        assert_eq!(domain.domain.as_deref(), Some(".example.com"));

        // An IPv6 host keeps its brackets in the URL.
        let v6 = plan_cookie(&cookie("a", "::1"), now(), None, &RestorePolicy::default())
            .expect("planned");
        assert_eq!(v6.url.as_deref(), Some("https://[::1]/"));
    }

    #[test]
    fn expired_cookie_is_skipped_and_counted() {
        let mut expired = cookie("old", ".example.com");
        expired.expires = Some(now() - chrono::Duration::seconds(1));
        assert_eq!(
            plan_cookie(&expired, now(), None, &RestorePolicy::default()).unwrap_err(),
            CookieSkip::Expired
        );
        let snapshot = SessionSnapshot {
            cookies: vec![expired, cookie("live", ".example.com")],
            captured_at: Some(now() - chrono::Duration::hours(1)),
            ..Default::default()
        };
        let plan = plan_restore(&snapshot, "about:blank", now(), &RestorePolicy::default());
        assert_eq!(plan.report.cookies_total, 2);
        assert_eq!(plan.report.skipped_expired, 1);
        assert_eq!(plan.cookies.len(), 1);
        assert_eq!(plan.report.snapshot_age_s, Some(3600));
    }

    #[test]
    fn stale_session_cookie_is_skipped() {
        let mut session = cookie("s", ".example.com");
        session.expires = None;
        session.session = true;
        let policy = RestorePolicy::from_raw(Some("3600"));
        // Younger than the limit: restored, with no expiry.
        let fresh = plan_cookie(&session, now(), Some(Duration::from_secs(60)), &policy)
            .expect("fresh session cookie");
        assert_eq!(fresh.expires_s, None);
        // Older: skipped.
        assert_eq!(
            plan_cookie(&session, now(), Some(Duration::from_secs(7200)), &policy).unwrap_err(),
            CookieSkip::SessionStale
        );
        // A snapshot that does not say when it was captured keeps its session cookies.
        assert!(plan_cookie(&session, now(), None, &policy).is_ok());
        // The default is seven days; an unparsable value is the default.
        assert_eq!(
            RestorePolicy::from_raw(Some("soon")).session_cookie_max_age,
            Duration::from_secs(604_800)
        );
        assert_eq!(
            RestorePolicy::from_raw(None).session_cookie_max_age,
            Duration::from_secs(DEFAULT_SESSION_COOKIE_MAX_AGE_S)
        );
    }

    fn origin_with(origin: &str, key: &str) -> OriginState {
        OriginState {
            origin: origin.to_owned(),
            local_storage: vec![(key.to_owned(), "v".to_owned())],
            ..Default::default()
        }
    }

    #[test]
    fn seeding_script_embeds_only_matching_origin_map() {
        let mut idb = origin_with("https://b.example.org", "kb");
        idb.indexed_db = vec![IdbDatabase {
            name: "app".to_owned(),
            version: 3,
            stores: vec![IdbStore {
                name: "items".to_owned(),
                records: vec![IdbRecord {
                    key: json!(1),
                    value: json!({ "$t": "date", "v": 1_700_000_000_000_i64 }),
                }],
                ..Default::default()
            }],
        }];
        let empty = OriginState {
            origin: "https://empty.example.net".to_owned(),
            ..Default::default()
        };
        let script = build_seeding_script(&[
            origin_with("https://a.example.com", "ka"),
            idb,
            empty,
        ])
        .expect("script");
        // The two origins with something to seed are keys of the map, the empty one is not.
        assert!(script.contains("https://a.example.com"));
        assert!(script.contains("https://b.example.org"));
        assert!(!script.contains("empty.example.net"));
        assert!(script.contains("location.origin"));
        assert!(!script.contains("__QUARRY_ORIGIN_MAP__"));
        // Nothing to seed, no script.
        assert!(build_seeding_script(&[OriginState {
            origin: "https://x.example.com".to_owned(),
            ..Default::default()
        }])
        .is_none());
        // A value cannot close the script element.
        let script = build_seeding_script(&[origin_with("https://a.example.com", "</script>")])
            .expect("script");
        assert!(!script.contains("</script>"));
    }

    #[test]
    fn sentinel_prevents_reseeding() {
        // The sentinel is read before anything is written, and written before the data.
        let read = SEEDING_JS_TEMPLATE.find("ls.getItem(SEED)").expect("sentinel read");
        let write = SEEDING_JS_TEMPLATE.find("ls.setItem(SEED, '1')").expect("sentinel write");
        let data = SEEDING_JS_TEMPLATE.find("put(ls, st.l)").expect("data write");
        assert!(read < write && write < data);
        assert!(SEEDING_JS_TEMPLATE.contains(SEEDED_SENTINEL_KEY));
        // The sentinel never travels inside the map.
        let script = build_seeding_script(&[origin_with(
            "https://a.example.com",
            SEEDED_SENTINEL_KEY,
        )]);
        assert!(script.is_none(), "an origin holding only the sentinel has nothing to seed");
    }

    #[test]
    fn restore_report_never_contains_values() {
        let mut snapshot = SessionSnapshot {
            cookies: vec![cookie("SESSION-NAME", ".example.com")],
            origins: vec![origin_with("https://a.example.com", "STORAGE-KEY")],
            user_agent: Some("UA-STRING".to_owned()),
            ..Default::default()
        };
        snapshot.captured_at = Some(now());
        let plan = plan_restore(&snapshot, "https://a.example.com/", now(), &RestorePolicy::default());
        let summary = serde_json::to_string(&plan.report.summary()).expect("json");
        for secret in ["SECRET-VALUE", "SESSION-NAME", "STORAGE-KEY", "example.com", "UA-STRING"] {
            assert!(!summary.contains(secret), "summary leaked {secret}");
        }
        let debug = format!("{:?} {:?}", plan.report, plan.cookies);
        assert!(!debug.contains("SECRET-VALUE") && !debug.contains("SESSION-NAME"));
        assert_eq!(plan.report.origins_seeded, 1);
    }

    #[test]
    fn legacy_flat_entries_target_initial_origin() {
        let snapshot = SessionSnapshot {
            local_storage: vec![("token".to_owned(), "t".to_owned())],
            session_storage: vec![("tab".to_owned(), "1".to_owned())],
            ..Default::default()
        };
        let plan = plan_restore(&snapshot, "https://app.example.com/home", now(), &RestorePolicy::default());
        assert_eq!(plan.report.origins_seeded, 1);
        let script = plan.seeding_script.expect("script");
        assert!(script.contains("https://app.example.com"));
        // A blank initial URL has no origin to seed into.
        let blank = plan_restore(&snapshot, "about:blank", now(), &RestorePolicy::default());
        assert!(blank.seeding_script.is_none());
        assert_eq!(blank.report.origins_seeded, 0);
    }

    #[test]
    fn load_errors_are_classified_by_what_to_do_next() {
        let unavailable = QuarryError::new(ErrorCode::RuntimeNotReady, "profile key unavailable");
        assert!(matches!(
            ProfileLoadError::classify(unavailable),
            ProfileLoadError::KeyUnavailable(_)
        ));
        let destroyed = QuarryError::new(ErrorCode::RuntimeNotReady, "profile key destroyed");
        assert!(matches!(
            ProfileLoadError::classify(destroyed),
            ProfileLoadError::KeyDestroyed
        ));
        let corrupt = QuarryError::new(ErrorCode::Internal, "profile snapshot decode failed");
        assert!(matches!(
            ProfileLoadError::classify(corrupt),
            ProfileLoadError::Corrupt(_)
        ));
    }

    #[test]
    fn emulation_takes_locale_and_timezone_only_when_they_look_right() {
        let snapshot = SessionSnapshot {
            locale: Some("nb-NO".to_owned()),
            timezone: Some("Europe/Oslo".to_owned()),
            user_agent: Some("anything".to_owned()),
            ..Default::default()
        };
        let emulation = ProfileEmulation::from_snapshot(&snapshot);
        assert_eq!(emulation.locale.as_deref(), Some("nb_NO"));
        assert_eq!(emulation.timezone.as_deref(), Some("Europe/Oslo"));
        let bad = SessionSnapshot {
            locale: Some("en US; drop table".to_owned()),
            timezone: Some("../etc".to_owned() + &"x".repeat(80)),
            ..Default::default()
        };
        assert!(ProfileEmulation::from_snapshot(&bad).is_empty());
    }

    #[test]
    fn a_page_request_replaces_the_profiles_locale_and_zone_field_by_field() {
        let profile = ProfileEmulation {
            locale: Some("nb_NO".to_owned()),
            timezone: Some("Europe/Oslo".to_owned()),
        };
        let request = PageEmulation {
            locale: Some("en_US".to_owned()),
            ..PageEmulation::default()
        };
        let left = profile.without(&request);
        assert_eq!(left.locale, None);
        assert_eq!(left.timezone.as_deref(), Some("Europe/Oslo"));
        assert_eq!(profile.without(&PageEmulation::default()), profile);
    }

    // ---- D2-028: a failed load is a visible, typed report that blocks the save ----

    #[test]
    fn corrupt_load_is_a_typed_failed_report_that_blocks_the_save() {
        let failure = ProfileLoadError::classify(QuarryError::new(
            ErrorCode::Internal,
            "decode failed for org-SECRET-ORG",
        ));
        assert!(failure.blocks_save());
        let report = failure.restore_report();
        assert!(report.failed);
        assert_eq!(report.load_failure, Some("corrupt"));
        assert!(report.summary().failed);
        // A class and counts, never the store's error text.
        assert!(!format!("{report:?}").contains("SECRET-ORG"));
    }

    #[test]
    fn unreachable_key_also_blocks_the_save_and_names_its_class() {
        let failure = ProfileLoadError::classify(QuarryError::new(
            ErrorCode::RuntimeNotReady,
            "profile key unavailable",
        ));
        assert!(failure.blocks_save());
        let report = failure.restore_report();
        assert!(report.failed);
        assert_eq!(report.load_failure, Some("key_unavailable"));
    }

    #[test]
    fn destroyed_key_is_signalled_but_does_not_block_the_save() {
        let failure = ProfileLoadError::KeyDestroyed;
        assert!(!failure.blocks_save());
        let report = failure.restore_report();
        assert!(!report.failed);
        assert_eq!(report.load_failure, Some("key_destroyed"));
    }

    #[test]
    fn a_normal_restore_report_carries_no_load_failure() {
        let snapshot = SessionSnapshot::default();
        let plan = plan_restore(&snapshot, "about:blank", now(), &RestorePolicy::default());
        assert_eq!(plan.report.load_failure, None);
        assert!(!plan.report.failed);
    }

    // ---- D2-117: setCookies acceptance is checked, not assumed ----

    /// A mock jar: refuses a call that holds a cookie named in `invalid`, or every call when
    /// `refuse_all`; drops the cookies named in `drop_after_accept` from what it holds.
    #[derive(Default)]
    struct MockJar {
        invalid: Vec<String>,
        refuse_all: bool,
        drop_after_accept: Vec<String>,
        readback_down: bool,
        held: std::sync::Mutex<Vec<CookieKey>>,
        calls: std::sync::Mutex<Vec<usize>>,
    }

    #[async_trait::async_trait]
    impl CookieSink for MockJar {
        async fn set(&self, cookies: &[CookiePlan]) -> bool {
            self.calls.lock().unwrap().push(cookies.len());
            if self.refuse_all || cookies.iter().any(|c| self.invalid.contains(&c.name)) {
                return false;
            }
            let mut held = self.held.lock().unwrap();
            for plan in cookies {
                if !self.drop_after_accept.contains(&plan.name) {
                    held.push(CookieKey::of_plan(plan));
                }
            }
            true
        }

        async fn read_back(&self) -> Option<Vec<CookieKey>> {
            if self.readback_down {
                None
            } else {
                Some(self.held.lock().unwrap().clone())
            }
        }
    }

    fn plans(names: &[&str], domain: &str) -> Vec<CookiePlan> {
        names
            .iter()
            .map(|name| {
                plan_cookie(&cookie(name, domain), now(), None, &RestorePolicy::default())
                    .expect("plan")
            })
            .collect()
    }

    fn numbered(n: usize, domain: &str) -> Vec<CookiePlan> {
        let names: Vec<String> = (0..n).map(|i| format!("c{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        plans(&refs, domain)
    }

    #[tokio::test]
    async fn accepted_chunks_count_as_applied_and_are_sent_in_hundreds() {
        let jar = MockJar::default();
        let mut report = RestoreReport::default();
        apply_cookies(&jar, &numbered(250, ".example.com"), &mut report).await;
        assert_eq!(*jar.calls.lock().unwrap(), vec![100, 100, 50]);
        assert_eq!(report.cookies_applied, 250);
        assert_eq!(report.cookies_rejected, 0);
        assert_eq!(report.cookie_chunks_refused, 0);
        assert!(!report.failed);
        assert!(!report.cookie_readback_unavailable);
    }

    #[tokio::test]
    async fn one_bad_cookie_costs_one_cookie() {
        let jar = MockJar {
            invalid: vec!["bad".to_owned()],
            ..MockJar::default()
        };
        let mut report = RestoreReport::default();
        apply_cookies(&jar, &plans(&["a", "bad", "b"], ".example.com"), &mut report).await;
        assert_eq!(report.cookies_applied, 2);
        assert_eq!(report.cookies_rejected, 1);
        assert_eq!(report.cookie_chunks_refused, 1);
        assert!(!report.failed, "a single bad cookie is not a systematic refusal");
    }

    #[tokio::test]
    async fn a_refusal_of_everything_is_systematic_and_marks_the_restore_failed() {
        let jar = MockJar {
            refuse_all: true,
            ..MockJar::default()
        };
        let mut report = RestoreReport::default();
        apply_cookies(&jar, &plans(&["a", "b", "c"], ".example.com"), &mut report).await;
        assert_eq!(report.cookies_applied, 0);
        assert_eq!(report.cookies_rejected, 3);
        assert_eq!(report.cookie_chunks_refused, 1);
        assert!(report.failed, "the release must not save a cookie-less session over the profile");
    }

    #[tokio::test]
    async fn a_lone_refused_cookie_is_not_called_systematic() {
        let jar = MockJar {
            refuse_all: true,
            ..MockJar::default()
        };
        let mut report = RestoreReport::default();
        apply_cookies(&jar, &plans(&["a"], ".example.com"), &mut report).await;
        assert_eq!(report.cookies_rejected, 1);
        assert!(!report.failed);
    }

    #[tokio::test]
    async fn no_cookies_means_no_calls_and_no_failure() {
        let jar = MockJar::default();
        let mut report = RestoreReport::default();
        apply_cookies(&jar, &[], &mut report).await;
        assert!(jar.calls.lock().unwrap().is_empty());
        assert!(!report.failed);
        assert_eq!(report, RestoreReport::default());
    }

    #[tokio::test]
    async fn a_cookie_accepted_then_dropped_is_moved_to_rejected() {
        let jar = MockJar {
            drop_after_accept: vec!["gone".to_owned()],
            ..MockJar::default()
        };
        let mut report = RestoreReport::default();
        apply_cookies(&jar, &plans(&["kept", "gone"], ".example.com"), &mut report).await;
        assert_eq!(report.cookies_applied, 1);
        assert_eq!(report.cookies_rejected, 1);
        assert_eq!(report.cookies_dropped_after_accept, 1);
        assert!(!report.failed);
    }

    #[tokio::test]
    async fn an_unreadable_jar_changes_no_count_and_says_so() {
        let jar = MockJar {
            readback_down: true,
            drop_after_accept: vec!["gone".to_owned()],
            ..MockJar::default()
        };
        let mut report = RestoreReport::default();
        apply_cookies(&jar, &plans(&["kept", "gone"], ".example.com"), &mut report).await;
        assert_eq!(report.cookies_applied, 2);
        assert_eq!(report.cookies_rejected, 0);
        assert!(report.cookie_readback_unavailable);
    }

    #[tokio::test]
    async fn host_only_and_domain_cookies_match_the_jars_spelling() {
        // Chromium reports a host-only cookie's domain bare and a domain cookie's with a dot.
        let host_only = plans(&["h"], "App.Example.com");
        let domain = plans(&["d"], ".example.com");
        assert_eq!(
            CookieKey::of_plan(&host_only[0]),
            CookieKey::new("h", "app.example.com", "/")
        );
        assert_eq!(
            CookieKey::of_plan(&domain[0]),
            CookieKey::new("d", ".example.com", "/")
        );
        let v6 = plans(&["v"], "::1");
        assert_eq!(CookieKey::of_plan(&v6[0]), CookieKey::new("v", "::1", "/"));
        let mut all = host_only;
        all.extend(domain);
        all.extend(v6);
        let jar = MockJar::default();
        let mut report = RestoreReport::default();
        apply_cookies(&jar, &all, &mut report).await;
        assert_eq!(report.cookies_applied, 3);
        assert_eq!(report.cookies_dropped_after_accept, 0);
    }

    #[test]
    fn cookie_keys_never_print_names_or_hosts() {
        let key = CookieKey::new("session-SECRET", "secret.example.com", "/");
        let text = format!("{key:?}");
        assert!(!text.contains("SECRET") && !text.contains("secret"));
    }
}
