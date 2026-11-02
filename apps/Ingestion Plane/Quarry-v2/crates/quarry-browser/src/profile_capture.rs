//! Capture v2: the whole context jar, web storage and IndexedDB of every visited origin
//! (WS17.C1). Compiled with the `chromiumoxide` feature; the pure parts (types, caps, the
//! journal) are in `profile_snapshot.rs`.
//!
//! Strategy (plan D16-12):
//!
//! 1. **Cookies**, first and alone: `Storage.getCookies { browserContextId }` through
//!    `Browser::execute` for the session's own context. The whole jar, with expiry, SameSite
//!    and partition key, with no tab open. A cookie failure aborts the capture (nothing is
//!    saved over the previous snapshot), so a slow IndexedDB can never lose authentication.
//! 2. **Web storage** of every live tab, by the in-page serializer (`SERIALIZER_JS`).
//! 3. **IndexedDB** of every live tab, by the same serializer, its own budget.
//! 4. Origins that no live tab covers any more: what their pages flushed on `pagehide` while
//!    alive (`FLUSH_JS`, delivered through `Runtime.addBinding`), kept in the [`OriginJournal`].
//!    Web storage only: IndexedDB cannot be read from a `pagehide` handler.
//! 4b. With `QUARRY_BROWSER_CAPTURE_CLOSED_ORIGINS` on (default off, D2-070): the IndexedDB of
//!    those same closed origins, read through the frame-less `IndexedDB` CDP domain on a live
//!    tab of the same browser context (S0 probe P3, `tests/profile_cdp_spike.rs`), bounded by
//!    its own time budget, a per-origin cap and an all-origins cap. A read that fails, finds
//!    nothing or runs out of time never erases what the journal already holds.
//!
//! Budget: 10 s overall (the caller wraps this in `Op::ProfileCapture`), cookies 2 s,
//! storage 4 s, IndexedDB 4 s. A phase that runs out is flagged `truncated` and the capture
//! carries on with what it has.
//!
//! Nothing here logs a URL, a cookie name, a storage key or a value; the report is counts.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ::chromiumoxide::cdp::browser_protocol::browser::BrowserContextId;
use ::chromiumoxide::cdp::browser_protocol::network as net;
use ::chromiumoxide::cdp::browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams;
use ::chromiumoxide::cdp::browser_protocol::storage::GetCookiesParams;
use ::chromiumoxide::cdp::js_protocol::runtime::{
    AddBindingParams, EvaluateParams, EventBindingCalled, EventExecutionContextCreated,
    ExecutionContextId,
};
use ::chromiumoxide::{Browser, Page};
use futures::StreamExt;
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use serde::Deserialize;
use serde_json::json;

use crate::deadlines::{timeout_error, with_deadline, Op};
use crate::profile_snapshot::{
    merge_and_cap, normalize_origin, parse_flag, CapOutcome, CaptureReport, CookiePriority,
    IdbDatabase, IdbIndex, IdbRecord, IdbStore, OriginJournal, OriginState, PartitionKey,
    ProfileCaps, SameSite, SourceScheme, BUDGET_COOKIES_MS, BUDGET_IDB_MS, BUDGET_STORAGE_MS,
    BUDGET_TOTAL_MS, MAX_FLUSH_PAYLOAD_BYTES, SNAPSHOT_VERSION,
};
use crate::session::{Cookie, SessionSnapshot};

/// The `Runtime.addBinding` name the `pagehide` handler calls.
pub(crate) const FLUSH_BINDING: &str = "__quarryProfileFlush";

/// Installed with `Page.addScriptToEvaluateOnNewDocument` on a profile-enabled tab: on
/// `pagehide` it hands the origin's localStorage and sessionStorage (capped at 1 MiB of
/// text) to the binding. Synchronous only: a `pagehide` handler cannot wait for IndexedDB.
pub(crate) const FLUSH_JS: &str = r#"(() => {
  try {
    if (window.__quarryProfileFlushOn) return;
    Object.defineProperty(window, '__quarryProfileFlushOn', { value: true });
  } catch (e) { return; }
  const SEED = '__quarry_profile_seeded_v1';
  const MAX = 1048576;
  const dump = (get, skipSeed) => {
    const entries = [];
    let truncated = false;
    try {
      const area = get();
      let n = 0;
      for (let i = 0; i < area.length; i++) {
        const k = area.key(i);
        if (k === null || (skipSeed && k === SEED)) continue;
        const v = area.getItem(k);
        if (v === null) continue;
        n += k.length + v.length;
        if (n > MAX) { truncated = true; break; }
        entries.push([k, v]);
      }
    } catch (e) { truncated = true; }
    return { entries, truncated };
  };
  const flush = () => {
    try {
      if (!/^https?:$/.test(location.protocol)) return;
      const send = window.__quarryProfileFlush;
      if (typeof send !== 'function') return;
      const local = dump(() => window.localStorage, true);
      const session = dump(() => window.sessionStorage, false);
      send(JSON.stringify({
        origin: location.origin,
        local: local.entries,
        session: session.entries,
        truncated: local.truncated || session.truncated,
      }));
    } catch (e) {}
  };
  addEventListener('pagehide', flush);
})();"#;

/// The in-page serializer: `await (SERIALIZER_JS)(opts)` returns a JSON string (or `null` on
/// a page that is not http(s)) shaped like [`RawPage`]. `opts`: `{ storage, idb, meta,
/// maxBytes, maxRecords, idbMs }`.
///
/// IndexedDB keys and values use the tagged encoding of `profile_snapshot::IdbValue` (Date,
/// ArrayBuffer and typed arrays as base64, Map, Set, BigInt, Blob and File as base64, RegExp,
/// non-finite numbers, `undefined`; anything not structured-cloneable becomes
/// `{"$t":"dropped"}` and is counted). Records are read with `getAllKeys` and `getAll`
/// inside one read-only transaction and encoded after it ends, so a Blob read cannot
/// auto-commit the transaction away.
pub(crate) const SERIALIZER_JS: &str = r#"async (opts) => {
  if (!/^https?:$/.test(location.protocol)) return null;
  const out = {
    origin: location.origin, local: [], session: [], idb: [],
    dropped: 0, truncated: false, ua: null, locale: null, tz: null,
  };
  const SEED = '__quarry_profile_seeded_v1';
  let bytes = 0;
  const over = () => bytes > opts.maxBytes;
  if (opts.meta) {
    out.ua = navigator.userAgent || null;
    out.locale = navigator.language || null;
    try { out.tz = Intl.DateTimeFormat().resolvedOptions().timeZone || null; } catch (e) {}
  }
  const dump = (get, into, skipSeed) => {
    try {
      const area = get();
      for (let i = 0; i < area.length; i++) {
        const k = area.key(i);
        if (k === null || (skipSeed && k === SEED)) continue;
        const v = area.getItem(k);
        if (v === null) continue;
        bytes += k.length + v.length + 8;
        if (over()) { out.truncated = true; return; }
        into.push([k, v]);
      }
    } catch (e) { out.truncated = true; }
  };
  if (opts.storage) {
    dump(() => window.localStorage, out.local, true);
    dump(() => window.sessionStorage, out.session, false);
  }
  if (!opts.idb) return JSON.stringify(out);

  const deadline = Date.now() + opts.idbMs;
  let records = 0;
  const DROPPED = { $t: 'dropped' };
  const b64 = (u8) => {
    let s = '';
    for (let i = 0; i < u8.length; i += 0x8000) {
      s += String.fromCharCode.apply(null, u8.subarray(i, i + 0x8000));
    }
    return btoa(s);
  };
  const TYPED = new Set(['Int8Array', 'Uint8Array', 'Uint8ClampedArray', 'Int16Array',
    'Uint16Array', 'Int32Array', 'Uint32Array', 'Float32Array', 'Float64Array',
    'BigInt64Array', 'BigUint64Array', 'DataView']);
  const drop = () => { out.dropped++; return DROPPED; };
  const enc = async (v, seen, depth) => {
    if (v === undefined) return { $t: 'undefined' };
    if (v === null) return null;
    switch (typeof v) {
      case 'boolean': case 'string': return v;
      case 'number':
        if (Number.isFinite(v) && !Object.is(v, -0)) return v;
        return { $t: 'num', v: Object.is(v, -0) ? '-0' : String(v) };
      case 'bigint': return { $t: 'bigint', v: v.toString() };
      case 'function': case 'symbol': return drop();
    }
    if (depth > 64 || seen.has(v)) return drop();
    seen.add(v);
    try {
      if (v instanceof Date) {
        const t = v.getTime();
        return { $t: 'date', v: Number.isNaN(t) ? 'invalid' : t };
      }
      if (v instanceof RegExp) return { $t: 'regexp', source: v.source, flags: v.flags };
      if (v instanceof ArrayBuffer) return { $t: 'bytes', k: 'ArrayBuffer', v: b64(new Uint8Array(v)) };
      if (ArrayBuffer.isView(v)) {
        const k = TYPED.has(v.constructor && v.constructor.name) ? v.constructor.name : 'Uint8Array';
        return { $t: 'bytes', k, v: b64(new Uint8Array(v.buffer, v.byteOffset, v.byteLength)) };
      }
      if (typeof Blob !== 'undefined' && v instanceof Blob) {
        if (v.size > opts.maxBytes - bytes) return drop();
        return { $t: 'blob', mime: v.type || '', v: b64(new Uint8Array(await v.arrayBuffer())) };
      }
      if (v instanceof Map) {
        const entries = [];
        for (const [k, val] of v) entries.push([await enc(k, seen, depth + 1), await enc(val, seen, depth + 1)]);
        return { $t: 'map', v: entries };
      }
      if (v instanceof Set) {
        const items = [];
        for (const item of v) items.push(await enc(item, seen, depth + 1));
        return { $t: 'set', v: items };
      }
      if (Array.isArray(v)) {
        const items = [];
        for (let i = 0; i < v.length; i++) items.push(await enc(v[i], seen, depth + 1));
        return items;
      }
      const proto = Object.getPrototypeOf(v);
      if (proto === Object.prototype || proto === null) {
        const o = {};
        for (const k of Object.keys(v)) o[k] = await enc(v[k], seen, depth + 1);
        return Object.prototype.hasOwnProperty.call(o, '$t') ? { $t: 'obj', v: o } : o;
      }
      return drop();
    } finally {
      seen.delete(v);
    }
  };
  const wrap = (r) => new Promise((res, rej) => {
    r.onsuccess = () => res(r.result);
    r.onerror = () => rej(r.error);
  });
  let infos = [];
  try { infos = await indexedDB.databases(); } catch (e) { out.truncated = true; return JSON.stringify(out); }
  for (const info of infos) {
    if (Date.now() > deadline || over()) { out.truncated = true; break; }
    if (!info || typeof info.name !== 'string') continue;
    let db;
    try {
      db = await new Promise((res, rej) => {
        const r = indexedDB.open(info.name);
        r.onsuccess = () => res(r.result);
        r.onerror = () => rej(r.error);
        r.onblocked = () => rej(new Error('blocked'));
        r.onupgradeneeded = () => { try { r.transaction.abort(); } catch (e) {} };
      });
    } catch (e) { out.truncated = true; continue; }
    try {
      const entry = { name: info.name, version: db.version, stores: [] };
      const names = Array.from(db.objectStoreNames);
      const raw = [];
      if (names.length) {
        const tx = db.transaction(names, 'readonly');
        const finished = new Promise((res, rej) => {
          tx.oncomplete = () => res();
          tx.onerror = () => rej(tx.error);
          tx.onabort = () => rej(tx.error);
        });
        const read = (async () => {
          for (const name of names) {
            const os = tx.objectStore(name);
            const left = Math.max(0, opts.maxRecords - records) + 1;
            const keys = await wrap(os.getAllKeys(null, left));
            const values = await wrap(os.getAll(null, left));
            if (keys.length >= left) out.truncated = true;
            raw.push({
              name,
              key_path: os.keyPath,
              auto_increment: os.autoIncrement,
              indexes: Array.from(os.indexNames).map((n) => {
                const ix = os.index(n);
                return { name: n, key_path: ix.keyPath, unique: ix.unique, multi_entry: ix.multiEntry };
              }),
              keys, values,
            });
          }
        })();
        await Promise.all([finished, read]);
      }
      for (const s of raw) {
        const store = { name: s.name, key_path: s.key_path, auto_increment: s.auto_increment, indexes: s.indexes, records: [] };
        for (let i = 0; i < s.keys.length; i++) {
          if (Date.now() > deadline) { out.truncated = true; break; }
          const key = await enc(s.keys[i], new WeakSet(), 0);
          const value = await enc(s.values[i], new WeakSet(), 0);
          bytes += JSON.stringify([key, value]).length;
          if (over() || records >= opts.maxRecords) { out.truncated = true; break; }
          store.records.push({ key, value });
          records++;
        }
        entry.stores.push(store);
      }
      out.idb.push(entry);
    } catch (e) {
      out.truncated = true;
    } finally {
      try { db.close(); } catch (e) {}
    }
  }
  return JSON.stringify(out);
}"#;

/// What the serializer returns for one page.
#[derive(Debug, Deserialize)]
struct RawPage {
    origin: String,
    #[serde(default)]
    local: Vec<(String, String)>,
    #[serde(default)]
    session: Vec<(String, String)>,
    #[serde(default)]
    idb: Vec<IdbDatabase>,
    #[serde(default)]
    dropped: u32,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    ua: Option<String>,
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    tz: Option<String>,
}

/// One page's capture: its origin's state and the page-level facts of the active tab.
#[derive(Debug)]
pub(crate) struct PageCapture {
    pub state: OriginState,
    pub user_agent: Option<String>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
}

/// Which parts the serializer reads on one call.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Phase {
    pub storage: bool,
    pub idb: bool,
    /// User agent, locale and timezone (the active tab only).
    pub meta: bool,
}

/// A live tab of the session.
pub(crate) struct LiveTab {
    pub page: Page,
    pub active: bool,
}

/// The result of [`capture_profile`].
#[derive(Debug)]
pub(crate) struct ProfileCapture {
    pub snapshot: SessionSnapshot,
    pub report: CaptureReport,
    pub outcome: CapOutcome,
    /// What the closed-origin phase did (all zero with the flag off). `capture_profile` logs
    /// it itself; the caller (chromiumoxide.rs) may surface it later.
    #[allow(dead_code)]
    pub closed: ClosedReport,
}

// ---------------------------------------------------------------------------------------
// Cookies
// ---------------------------------------------------------------------------------------

/// A CDP cookie as the snapshot stores it. `expires` is `None` for a session cookie
/// (Chromium reports `session: true` and `-1`). The domain is kept as reported: a leading
/// dot is a domain cookie, no dot is host-only (WS17.C2 restores the two differently).
pub(crate) fn cookie_from_cdp(cookie: &net::Cookie) -> Cookie {
    let expires = if cookie.session || !cookie.expires.is_finite() || cookie.expires <= 0.0 {
        None
    } else {
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis((cookie.expires * 1000.0) as i64)
    };
    Cookie {
        name: cookie.name.clone(),
        value: cookie.value.clone(),
        domain: cookie.domain.clone(),
        path: cookie.path.clone(),
        secure: cookie.secure,
        http_only: cookie.http_only,
        expires,
        same_site: cookie.same_site.as_ref().map(|same_site| match same_site {
            net::CookieSameSite::Strict => SameSite::Strict,
            net::CookieSameSite::Lax => SameSite::Lax,
            net::CookieSameSite::None => SameSite::None,
        }),
        session: cookie.session,
        priority: Some(match cookie.priority {
            net::CookiePriority::Low => CookiePriority::Low,
            net::CookiePriority::Medium => CookiePriority::Medium,
            net::CookiePriority::High => CookiePriority::High,
        }),
        // An opaque partition key (Chromium reports no site for it) cannot be restored as a
        // key; the cookie is kept, unpartitioned in the snapshot.
        partition_key: match (&cookie.partition_key, cookie.partition_key_opaque) {
            (Some(key), Some(false) | None) => Some(PartitionKey {
                top_level_site: key.top_level_site.clone(),
                has_cross_site_ancestor: key.has_cross_site_ancestor,
            }),
            _ => None,
        },
        source_scheme: Some(match cookie.source_scheme {
            net::CookieSourceScheme::Unset => SourceScheme::Unset,
            net::CookieSourceScheme::NonSecure => SourceScheme::NonSecure,
            net::CookieSourceScheme::Secure => SourceScheme::Secure,
        }),
    }
}

/// The whole jar of the session's own browser context: `Storage.getCookies` with a
/// `browserContextId`. Works with no tab open.
pub(crate) async fn capture_context_cookies(
    browser: &Browser,
    context: &BrowserContextId,
) -> QuarryResult<Vec<Cookie>> {
    let response = browser
        .execute(GetCookiesParams {
            browser_context_id: Some(context.clone()),
        })
        .await
        .map_err(|error| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide context cookie capture failed",
            )
            .with_details(json!({ "error": error.to_string() }))
        })?;
    Ok(response
        .result
        .cookies
        .iter()
        .map(cookie_from_cdp)
        .collect())
}

// ---------------------------------------------------------------------------------------
// The page serializer
// ---------------------------------------------------------------------------------------

/// Run the in-page serializer on `page` for `phase`. `Ok(None)` for a page that is not
/// http(s) (a blank tab, an error page). The IndexedDB walk stops itself `idb_ms` after it
/// starts; the caller still bounds the whole call.
pub(crate) async fn capture_origin_state(
    page: &Page,
    phase: Phase,
    idb_ms: u64,
    caps: &ProfileCaps,
) -> QuarryResult<Option<PageCapture>> {
    let options = json!({
        "storage": phase.storage,
        "idb": phase.idb,
        "meta": phase.meta,
        "maxBytes": caps.max_origin_bytes,
        "maxRecords": caps.max_idb_records,
        "idbMs": idb_ms,
    });
    let mut params = EvaluateParams::new(format!("({SERIALIZER_JS})({options})"));
    params.await_promise = Some(true);
    params.return_by_value = Some(true);
    let response = page.execute(params).await.map_err(|error| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            "chromiumoxide origin state capture failed",
        )
        .with_details(json!({ "error": error.to_string() }))
    })?;
    if response.result.exception_details.is_some() {
        return Err(QuarryError::new(
            ErrorCode::DriverFailed,
            "chromiumoxide origin state serializer threw",
        ));
    }
    let Some(serde_json::Value::String(text)) = response.result.result.value else {
        return Ok(None);
    };
    let raw: RawPage = serde_json::from_str(&text).map_err(|_| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            "chromiumoxide origin state serializer output did not parse",
        )
    })?;
    let Some(origin) = normalize_origin(&raw.origin) else {
        return Ok(None);
    };
    Ok(Some(PageCapture {
        state: OriginState {
            origin,
            local_storage: raw.local,
            session_storage: raw.session,
            indexed_db: raw.idb,
            dropped_values: raw.dropped,
            truncated: raw.truncated,
        },
        user_agent: raw.ua,
        locale: raw.locale,
        timezone: raw.tz,
    }))
}

// ---------------------------------------------------------------------------------------
// Closed-tab origins (D2-070)
// ---------------------------------------------------------------------------------------

/// `QUARRY_BROWSER_CAPTURE_CLOSED_ORIGINS`: capture the IndexedDB of origins the session
/// visited whose tabs have closed (or navigated away) before this capture. Default off.
pub(crate) const CAPTURE_CLOSED_ORIGINS_ENV: &str = "QUARRY_BROWSER_CAPTURE_CLOSED_ORIGINS";

/// The closed-origin phase's own budget. It also never runs past [`BUDGET_TOTAL_MS`] minus a
/// margin, so it cannot cost the capture its cookies to the caller's overall deadline.
pub(crate) const BUDGET_CLOSED_MS: u64 = 2_000;
const CLOSED_MARGIN_MS: u64 = 500;
/// Origins read per capture (the journal's sorted order decides which).
pub(crate) const MAX_CLOSED_ORIGINS: usize = 16;
/// Databases read per origin.
pub(crate) const MAX_CLOSED_DATABASES: usize = 16;
/// Serialized bytes kept per closed origin.
pub(crate) const MAX_CLOSED_ORIGIN_BYTES: usize = 2 * 1024 * 1024;
/// Serialized bytes kept over all closed origins of one capture.
pub(crate) const MAX_CLOSED_TOTAL_BYTES: usize = 8 * 1024 * 1024;
/// Records kept per closed origin.
pub(crate) const MAX_CLOSED_RECORDS: usize = 10_000;
/// Records asked for in one `IndexedDB.requestData`.
pub(crate) const CLOSED_PAGE_SIZE: usize = 100;

/// The switch. Off by default: today's capture, unchanged.
pub(crate) fn closed_origins_capture_enabled() -> bool {
    closed_flag(std::env::var(CAPTURE_CLOSED_ORIGINS_ENV).ok().as_deref())
}

fn closed_flag(raw: Option<&str>) -> bool {
    parse_flag(raw, false)
}

/// What one closed-origin read may keep. Derived from the snapshot caps, never above them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClosedLimits {
    pub origins: usize,
    pub databases: usize,
    pub origin_bytes: usize,
    pub total_bytes: usize,
    pub origin_records: usize,
    pub page_size: usize,
}

impl ClosedLimits {
    pub(crate) fn from_caps(caps: &ProfileCaps) -> Self {
        Self {
            origins: MAX_CLOSED_ORIGINS,
            databases: MAX_CLOSED_DATABASES,
            origin_bytes: MAX_CLOSED_ORIGIN_BYTES.min(caps.max_origin_bytes),
            total_bytes: MAX_CLOSED_TOTAL_BYTES.min(caps.max_bytes / 2),
            origin_records: MAX_CLOSED_RECORDS.min(caps.max_idb_records),
            page_size: CLOSED_PAGE_SIZE,
        }
    }
}

/// One page of an object store's records, already in the tagged encoding.
#[derive(Debug, Default)]
pub(crate) struct RecordPage {
    pub records: Vec<IdbRecord>,
    /// Values that could not be carried (counted, not kept).
    pub dropped: u32,
    pub has_more: bool,
}

/// Where a closed origin's IndexedDB is read from. The real source is the frame-less
/// `IndexedDB` CDP domain on a live tab of the same browser context
/// ([`CdpClosedIdb`]); the unit tests use a mock.
pub(crate) trait ClosedIdbSource {
    async fn database_names(&self, origin: &str) -> QuarryResult<Vec<String>>;
    /// The database's version and its stores' definitions (no records).
    async fn database(&self, origin: &str, name: &str) -> QuarryResult<IdbDatabase>;
    async fn records(
        &self,
        origin: &str,
        database: &str,
        store: &str,
        skip: usize,
        page_size: usize,
    ) -> QuarryResult<RecordPage>;
}

/// What the closed-origin phase did. Counts only.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ClosedReport {
    pub enabled: bool,
    /// Visited origins no live tab covers.
    pub candidates: u32,
    /// Candidates left out by [`MAX_CLOSED_ORIGINS`] or by running out of budget.
    pub skipped: u32,
    pub origins_read: u32,
    /// Origins whose read failed (their earlier state is kept).
    pub origins_failed: u32,
    pub databases: u32,
    pub records: u32,
    pub truncated: bool,
    pub timed_out: bool,
    /// No live tab to send the CDP calls through.
    pub no_page: bool,
}

/// The IndexedDB read for one closed origin.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ClosedOrigin {
    pub origin: String,
    pub idb: Vec<IdbDatabase>,
    pub dropped: u32,
    pub truncated: bool,
}

/// The visited origins that no live tab covers, at most `max` of them (sorted, so the choice
/// is stable), and how many were left out.
pub(crate) fn closed_origin_candidates<'a>(
    visited: impl Iterator<Item = &'a String>,
    live: &BTreeSet<String>,
    max: usize,
) -> (Vec<String>, usize) {
    let all: Vec<String> = visited.filter(|origin| !live.contains(*origin)).cloned().collect();
    let skipped = all.len().saturating_sub(max);
    (all.into_iter().take(max).collect(), skipped)
}

enum Stop {
    Timeout,
    Failed,
}

async fn within<T>(
    until: Instant,
    call: impl std::future::Future<Output = QuarryResult<T>>,
) -> Result<T, Stop> {
    match tokio::time::timeout(remaining(until), call).await {
        Err(_) => Err(Stop::Timeout),
        Ok(Err(_)) => Err(Stop::Failed),
        Ok(Ok(value)) => Ok(value),
    }
}

fn record_bytes(record: &IdbRecord) -> usize {
    serde_json::to_vec(record).map_or(0, |bytes| bytes.len())
}

/// Read one origin: every database (up to the cap), every store, records page by page until
/// a cap. A failure or the deadline anywhere in the origin discards the whole origin (a
/// half-read database must not replace a complete one); a cap keeps what was read and says
/// `truncated`.
async fn read_closed_origin<S: ClosedIdbSource>(
    source: &S,
    origin: &str,
    limits: &ClosedLimits,
    until: Instant,
    total_left: usize,
) -> Result<ClosedOrigin, Stop> {
    let mut out = ClosedOrigin {
        origin: origin.to_owned(),
        ..Default::default()
    };
    let byte_budget = limits.origin_bytes.min(total_left);
    let mut bytes = 0usize;
    let mut records = 0usize;

    let mut names = within(until, source.database_names(origin)).await?;
    names.sort();
    names.dedup();
    if names.len() > limits.databases {
        names.truncate(limits.databases);
        out.truncated = true;
    }
    for name in names {
        let mut db = within(until, source.database(origin, &name)).await?;
        for store in &mut db.stores {
            store.records = Vec::new();
            let mut skip = 0usize;
            'pages: loop {
                if records >= limits.origin_records || bytes >= byte_budget {
                    out.truncated = true;
                    break;
                }
                let want = limits.page_size.min(limits.origin_records - records);
                let page = within(
                    until,
                    source.records(origin, &db.name, &store.name, skip, want),
                )
                .await?;
                let fetched = page.records.len();
                out.dropped += page.dropped;
                for record in page.records {
                    let size = record_bytes(&record);
                    if bytes + size > byte_budget || records >= limits.origin_records {
                        out.truncated = true;
                        break 'pages;
                    }
                    bytes += size;
                    records += 1;
                    store.records.push(record);
                }
                skip += fetched;
                if !page.has_more || fetched == 0 {
                    break;
                }
            }
            // A store cut before it kept one record keeps its definition only, flagged as
            // the caps do, so a restore does not recreate an empty store as if it were whole.
            if store.records.is_empty() && out.truncated {
                store.omitted = true;
            }
        }
        out.idb.push(db);
    }
    Ok(out)
}

/// Read the IndexedDB of `origins` (the closed ones) from `source`, until `until`.
pub(crate) async fn capture_closed_origins<S: ClosedIdbSource>(
    source: &S,
    origins: &[String],
    limits: &ClosedLimits,
    until: Instant,
) -> (Vec<ClosedOrigin>, ClosedReport) {
    let mut report = ClosedReport {
        enabled: true,
        candidates: origins.len() as u32,
        ..Default::default()
    };
    let mut done: Vec<ClosedOrigin> = Vec::new();
    let mut total_left = limits.total_bytes;
    for (index, origin) in origins.iter().enumerate() {
        if total_left == 0 {
            report.truncated = true;
            report.skipped += (origins.len() - index) as u32;
            break;
        }
        match read_closed_origin(source, origin, limits, until, total_left).await {
            Ok(read) => {
                report.origins_read += 1;
                report.databases += read.idb.len() as u32;
                let kept: usize = read
                    .idb
                    .iter()
                    .flat_map(|db| db.stores.iter())
                    .flat_map(|store| store.records.iter())
                    .map(record_bytes)
                    .sum();
                report.records += read
                    .idb
                    .iter()
                    .flat_map(|db| db.stores.iter())
                    .map(|store| store.records.len() as u32)
                    .sum::<u32>();
                report.truncated |= read.truncated;
                total_left = total_left.saturating_sub(kept);
                done.push(read);
            }
            Err(Stop::Failed) => report.origins_failed += 1,
            Err(Stop::Timeout) => {
                report.timed_out = true;
                report.truncated = true;
                report.skipped += (origins.len() - index) as u32;
                break;
            }
        }
    }
    (done, report)
}

/// Fold the closed origins' IndexedDB into the states the journal holds for them. A read that
/// found no database never erases what the journal (or the loaded profile) held: only a
/// positive read replaces it. An origin the journal has no web storage for becomes a state
/// of its own.
pub(crate) fn merge_closed_idb(
    mut states: Vec<OriginState>,
    closed: Vec<ClosedOrigin>,
) -> Vec<OriginState> {
    for read in closed {
        if read.idb.is_empty() {
            continue;
        }
        match states.iter_mut().find(|state| state.origin == read.origin) {
            Some(state) => {
                state.indexed_db = read.idb;
                state.dropped_values += read.dropped;
                state.truncated |= read.truncated;
            }
            None => states.push(OriginState {
                origin: read.origin,
                indexed_db: read.idb,
                dropped_values: read.dropped,
                truncated: read.truncated,
                ..Default::default()
            }),
        }
    }
    states
}

/// A remote object that is not an object (no `objectId`) as the tagged encoding. `None` for a
/// value that cannot be carried (a symbol, a function). Pure.
pub(crate) fn primitive_to_tagged(
    kind: &str,
    value: Option<&serde_json::Value>,
    unserializable: Option<&str>,
) -> Option<serde_json::Value> {
    match kind {
        "undefined" => Some(json!({ "$t": "undefined" })),
        "boolean" | "string" => value.cloned(),
        "number" => match unserializable {
            Some(text @ ("NaN" | "Infinity" | "-Infinity" | "-0")) => {
                Some(json!({ "$t": "num", "v": text }))
            }
            Some(_) => None,
            None => value.cloned(),
        },
        "bigint" => unserializable
            .map(|text| text.strip_suffix('n').unwrap_or(text))
            .map(|digits| json!({ "$t": "bigint", "v": digits })),
        // `null` arrives as an object without an id.
        "object" => Some(serde_json::Value::Null),
        _ => None,
    }
}

/// A `KeyPath` of the IndexedDB domain as the snapshot stores it.
fn key_path_value(path: &::chromiumoxide::cdp::browser_protocol::indexed_db::KeyPath) -> serde_json::Value {
    use ::chromiumoxide::cdp::browser_protocol::indexed_db::KeyPathType;
    match path.r#type {
        KeyPathType::Null => serde_json::Value::Null,
        KeyPathType::String => path
            .string
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::String),
        KeyPathType::Array => json!(path.array.clone().unwrap_or_default()),
    }
}

/// Run on a `requestData` entry's object (as `this`): the structured-clone value in the tagged
/// encoding, as a JSON string `{ v, dropped }`. The same encoding as [`SERIALIZER_JS`].
const ENCODE_REMOTE_JS: &str = r#"async function () {
  const MAX = __MAX_BYTES__;
  let dropped = 0;
  let bytes = 0;
  const DROPPED = { $t: 'dropped' };
  const drop = () => { dropped++; return DROPPED; };
  const b64 = (u8) => {
    let s = '';
    for (let i = 0; i < u8.length; i += 0x8000) {
      s += String.fromCharCode.apply(null, u8.subarray(i, i + 0x8000));
    }
    return btoa(s);
  };
  const TYPED = new Set(['Int8Array', 'Uint8Array', 'Uint8ClampedArray', 'Int16Array',
    'Uint16Array', 'Int32Array', 'Uint32Array', 'Float32Array', 'Float64Array',
    'BigInt64Array', 'BigUint64Array', 'DataView']);
  const enc = async (v, seen, depth) => {
    if (v === undefined) return { $t: 'undefined' };
    if (v === null) return null;
    switch (typeof v) {
      case 'boolean': case 'string': return v;
      case 'number':
        if (Number.isFinite(v) && !Object.is(v, -0)) return v;
        return { $t: 'num', v: Object.is(v, -0) ? '-0' : String(v) };
      case 'bigint': return { $t: 'bigint', v: v.toString() };
      case 'function': case 'symbol': return drop();
    }
    if (depth > 64 || seen.has(v)) return drop();
    seen.add(v);
    try {
      if (v instanceof Date) {
        const t = v.getTime();
        return { $t: 'date', v: Number.isNaN(t) ? 'invalid' : t };
      }
      if (v instanceof RegExp) return { $t: 'regexp', source: v.source, flags: v.flags };
      if (v instanceof ArrayBuffer) {
        bytes += v.byteLength;
        if (bytes > MAX) return drop();
        return { $t: 'bytes', k: 'ArrayBuffer', v: b64(new Uint8Array(v)) };
      }
      if (ArrayBuffer.isView(v)) {
        bytes += v.byteLength;
        if (bytes > MAX) return drop();
        const k = TYPED.has(v.constructor && v.constructor.name) ? v.constructor.name : 'Uint8Array';
        return { $t: 'bytes', k, v: b64(new Uint8Array(v.buffer, v.byteOffset, v.byteLength)) };
      }
      if (typeof Blob !== 'undefined' && v instanceof Blob) {
        bytes += v.size;
        if (bytes > MAX) return drop();
        return { $t: 'blob', mime: v.type || '', v: b64(new Uint8Array(await v.arrayBuffer())) };
      }
      if (v instanceof Map) {
        const entries = [];
        for (const [k, val] of v) entries.push([await enc(k, seen, depth + 1), await enc(val, seen, depth + 1)]);
        return { $t: 'map', v: entries };
      }
      if (v instanceof Set) {
        const items = [];
        for (const item of v) items.push(await enc(item, seen, depth + 1));
        return { $t: 'set', v: items };
      }
      if (Array.isArray(v)) {
        const items = [];
        for (let i = 0; i < v.length; i++) items.push(await enc(v[i], seen, depth + 1));
        return items;
      }
      const proto = Object.getPrototypeOf(v);
      if (proto === Object.prototype || proto === null) {
        const o = {};
        for (const k of Object.keys(v)) o[k] = await enc(v[k], seen, depth + 1);
        return Object.prototype.hasOwnProperty.call(o, '$t') ? { $t: 'obj', v: o } : o;
      }
      return drop();
    } finally {
      seen.delete(v);
    }
  };
  const v = await enc(this, new WeakSet(), 0);
  return JSON.stringify({ v, dropped });
}"#;

/// The real source: the `IndexedDB` CDP domain, asked through a live tab of the session's
/// browser context with the closed origin named as `securityOrigin`. The domain needs no
/// frame for that origin (S0 probe P3a/P3b); it does need a target to send through.
pub(crate) struct CdpClosedIdb<'a> {
    pub page: &'a Page,
}

impl CdpClosedIdb<'_> {
    fn cdp_error(what: &str, error: &impl std::fmt::Display) -> QuarryError {
        QuarryError::new(ErrorCode::DriverFailed, format!("chromiumoxide {what} failed"))
            .with_details(json!({ "error": error.to_string() }))
    }

    /// One remote object (a record's key or value) as the tagged encoding plus how many
    /// parts of it were dropped.
    async fn encode(
        &self,
        object: &::chromiumoxide::cdp::js_protocol::runtime::RemoteObject,
    ) -> QuarryResult<(serde_json::Value, u32)> {
        use ::chromiumoxide::cdp::js_protocol::runtime::CallFunctionOnParams;
        let dropped = || (json!({ "$t": "dropped" }), 1u32);
        let Some(object_id) = object.object_id.clone() else {
            let encoded = primitive_to_tagged(
                object.r#type.as_ref(),
                object.value.as_ref(),
                object.unserializable_value.as_ref().map(AsRef::as_ref),
            );
            return Ok(encoded.map_or_else(dropped, |value| (value, 0)));
        };
        let mut params = CallFunctionOnParams::new(
            ENCODE_REMOTE_JS.replace("__MAX_BYTES__", &MAX_CLOSED_ORIGIN_BYTES.to_string()),
        );
        params.object_id = Some(object_id);
        params.await_promise = Some(true);
        params.return_by_value = Some(true);
        let response = self
            .page
            .execute(params)
            .await
            .map_err(|error| Self::cdp_error("closed-origin value encode", &error))?;
        if response.result.exception_details.is_some() {
            return Ok(dropped());
        }
        let Some(serde_json::Value::String(text)) = response.result.result.value else {
            return Ok(dropped());
        };
        #[derive(Deserialize)]
        struct Encoded {
            v: serde_json::Value,
            #[serde(default)]
            dropped: u32,
        }
        match serde_json::from_str::<Encoded>(&text) {
            Ok(encoded) => Ok((encoded.v, encoded.dropped)),
            Err(_) => Ok(dropped()),
        }
    }
}

impl ClosedIdbSource for CdpClosedIdb<'_> {
    async fn database_names(&self, origin: &str) -> QuarryResult<Vec<String>> {
        use ::chromiumoxide::cdp::browser_protocol::indexed_db::RequestDatabaseNamesParams;
        let response = self
            .page
            .execute(RequestDatabaseNamesParams {
                security_origin: Some(origin.to_owned()),
                storage_key: None,
                storage_bucket: None,
            })
            .await
            .map_err(|error| Self::cdp_error("IndexedDB.requestDatabaseNames", &error))?;
        Ok(response.result.database_names.clone())
    }

    async fn database(&self, origin: &str, name: &str) -> QuarryResult<IdbDatabase> {
        use ::chromiumoxide::cdp::browser_protocol::indexed_db::RequestDatabaseParams;
        let mut params = RequestDatabaseParams::new(name);
        params.security_origin = Some(origin.to_owned());
        let response = self
            .page
            .execute(params)
            .await
            .map_err(|error| Self::cdp_error("IndexedDB.requestDatabase", &error))?;
        let db = &response.result.database_with_object_stores;
        Ok(IdbDatabase {
            name: db.name.clone(),
            version: db.version.max(0.0) as u64,
            stores: db
                .object_stores
                .iter()
                .map(|store| IdbStore {
                    name: store.name.clone(),
                    key_path: key_path_value(&store.key_path),
                    auto_increment: store.auto_increment,
                    indexes: store
                        .indexes
                        .iter()
                        .map(|index| IdbIndex {
                            name: index.name.clone(),
                            key_path: key_path_value(&index.key_path),
                            unique: index.unique,
                            multi_entry: index.multi_entry,
                        })
                        .collect(),
                    records: Vec::new(),
                    omitted: false,
                })
                .collect(),
        })
    }

    async fn records(
        &self,
        origin: &str,
        database: &str,
        store: &str,
        skip: usize,
        page_size: usize,
    ) -> QuarryResult<RecordPage> {
        use ::chromiumoxide::cdp::browser_protocol::indexed_db::RequestDataParams;
        let response = self
            .page
            .execute(RequestDataParams {
                security_origin: Some(origin.to_owned()),
                storage_key: None,
                storage_bucket: None,
                database_name: database.to_owned(),
                object_store_name: store.to_owned(),
                index_name: None,
                skip_count: skip as i64,
                page_size: page_size as i64,
                key_range: None,
            })
            .await
            .map_err(|error| Self::cdp_error("IndexedDB.requestData", &error))?;
        let mut page = RecordPage {
            has_more: response.result.has_more,
            ..Default::default()
        };
        for entry in &response.result.object_store_data_entries {
            let (key, key_dropped) = self.encode(&entry.key).await?;
            let (value, value_dropped) = self.encode(&entry.value).await?;
            page.dropped += key_dropped + value_dropped;
            // A record without a key it can be put back under is worth nothing.
            if key == json!({ "$t": "dropped" }) {
                continue;
            }
            page.records.push(IdbRecord { key, value });
        }
        Ok(page)
    }
}

// ---------------------------------------------------------------------------------------
// The flush: web storage of a page that is about to go away
// ---------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FlushPayload {
    origin: String,
    #[serde(default)]
    local: Vec<(String, String)>,
    #[serde(default)]
    session: Vec<(String, String)>,
    #[serde(default)]
    truncated: bool,
}

/// What a `pagehide` flush becomes in the journal, or `None` when it is not trusted: too
/// large, not parseable, or its claimed origin is not the origin of the execution context
/// that sent it (`context_origin`, which Chromium reports and a page cannot forge).
pub(crate) fn flush_to_state(payload: &str, context_origin: Option<&str>) -> Option<OriginState> {
    if payload.len() > MAX_FLUSH_PAYLOAD_BYTES {
        return None;
    }
    let parsed: FlushPayload = serde_json::from_str(payload).ok()?;
    let claimed = normalize_origin(&parsed.origin)?;
    let sender = normalize_origin(context_origin?)?;
    if claimed != sender {
        return None;
    }
    Some(OriginState {
        origin: claimed,
        local_storage: parsed.local,
        session_storage: parsed.session,
        truncated: parsed.truncated,
        ..Default::default()
    })
}

/// Install the `pagehide` flush on `page` (before its first navigation) and feed what it
/// sends into `journal`. Best effort by design: a page that is killed, crashes or is frozen
/// sends nothing, and the origin's state then comes from a live capture or not at all.
pub(crate) async fn install_profile_flush(
    page: &Page,
    journal: Arc<Mutex<OriginJournal>>,
) -> QuarryResult<()> {
    // Listeners first, so no event between the binding and the first navigation is missed.
    let mut bindings = page
        .event_listener::<EventBindingCalled>()
        .await
        .map_err(|error| flush_error("binding listener", &error))?;
    let mut contexts = page
        .event_listener::<EventExecutionContextCreated>()
        .await
        .map_err(|error| flush_error("context listener", &error))?;
    with_deadline(Op::TabAttach, async {
        page.execute(AddBindingParams::new(FLUSH_BINDING))
            .await
            .map_err(|error| flush_error("binding", &error))?;
        page.evaluate_on_new_document(AddScriptToEvaluateOnNewDocumentParams::new(FLUSH_JS))
            .await
            .map_err(|error| flush_error("script", &error))?;
        Ok(())
    })
    .await?;
    tokio::spawn(async move {
        // context id -> its security origin, as Chromium reports it.
        let mut origins: BTreeMap<i64, String> = BTreeMap::new();
        loop {
            tokio::select! {
                Some(event) = contexts.next() => {
                    if origins.len() >= 256 {
                        origins.clear();
                    }
                    origins.insert(*event.context.id.inner(), event.context.origin.clone());
                }
                Some(event) = bindings.next() => {
                    if event.name != FLUSH_BINDING {
                        continue;
                    }
                    let sender: ExecutionContextId = event.execution_context_id;
                    let context_origin = origins.get(sender.inner()).map(String::as_str);
                    let Some(state) = flush_to_state(&event.payload, context_origin) else {
                        tracing::debug!("profile flush from a page was not accepted");
                        continue;
                    };
                    if let Ok(mut journal) = journal.lock() {
                        journal.record_flush(state);
                    }
                }
                else => break,
            }
        }
    });
    Ok(())
}

fn flush_error(what: &str, error: &impl std::fmt::Display) -> QuarryError {
    QuarryError::new(
        ErrorCode::DriverFailed,
        format!("chromiumoxide profile flush {what} failed"),
    )
    .with_details(json!({ "error": error.to_string() }))
}

// ---------------------------------------------------------------------------------------
// The capture
// ---------------------------------------------------------------------------------------

fn remaining(until: Instant) -> Duration {
    until.saturating_duration_since(Instant::now())
}

/// Capture the session's profile: cookies, then web storage, then IndexedDB, then the
/// journal's flushed origins, merged and brought under `caps`.
///
/// `tabs` are the session's live tabs, the active one first. `idb` is the
/// `QUARRY_BROWSER_PROFILE_IDB` switch. The caller bounds the whole call with
/// `Op::ProfileCapture` (10 s) and drops the `browser` read guard when this returns.
///
/// An `Err` means nothing may be saved: the cookie phase failed or timed out.
pub(crate) async fn capture_profile(
    browser: &Browser,
    context: &BrowserContextId,
    tabs: Vec<LiveTab>,
    journal: &Arc<Mutex<OriginJournal>>,
    caps: ProfileCaps,
    idb: bool,
) -> QuarryResult<ProfileCapture> {
    let mut report = CaptureReport::default();
    let capture_started = Instant::now();

    // 1. Cookies. Everything else is optional; this is not.
    let started = Instant::now();
    let limit = Duration::from_millis(BUDGET_COOKIES_MS);
    let cookies = tokio::time::timeout(limit, capture_context_cookies(browser, context))
        .await
        .map_err(|_| timeout_error(Op::ProfileCapture, limit))??;
    report.cookies_ms = started.elapsed().as_millis() as u64;
    report.cookies = cookies.len() as u32;

    // 2. Web storage of every live tab (the active tab is first, so it wins an origin two
    // tabs share).
    let started = Instant::now();
    let until = started + Duration::from_millis(BUDGET_STORAGE_MS);
    let mut live: Vec<(OriginState, usize)> = Vec::new();
    let mut live_origins: BTreeSet<String> = BTreeSet::new();
    let mut meta: Option<PageCapture> = None;
    for (index, tab) in tabs.iter().enumerate() {
        let phase = Phase {
            storage: true,
            idb: false,
            meta: tab.active,
        };
        let step = tokio::time::timeout(
            remaining(until),
            capture_origin_state(&tab.page, phase, 0, &caps),
        )
        .await;
        match step {
            Err(_) => {
                report.storage_timed_out = true;
                report.truncated = true;
                break;
            }
            // A tab that cannot be read (closed, crashed) is skipped; the rest still count.
            Ok(Err(_)) | Ok(Ok(None)) => {}
            Ok(Ok(Some(capture))) => {
                if tab.active && meta.is_none() {
                    meta = Some(PageCapture {
                        state: OriginState::default(),
                        user_agent: capture.user_agent.clone(),
                        locale: capture.locale.clone(),
                        timezone: capture.timezone.clone(),
                    });
                }
                if live_origins.insert(capture.state.origin.clone()) {
                    live.push((capture.state, index));
                }
            }
        }
    }
    report.storage_ms = started.elapsed().as_millis() as u64;

    // 3. IndexedDB of those same tabs, its own budget.
    let started = Instant::now();
    if idb {
        let until = started + Duration::from_millis(BUDGET_IDB_MS);
        for (state, index) in &mut live {
            let phase = Phase {
                storage: false,
                idb: true,
                meta: false,
            };
            // The page's own walk stops half a second before the budget does.
            let idb_ms = remaining(until).as_millis().saturating_sub(500) as u64;
            let step = tokio::time::timeout(
                remaining(until),
                capture_origin_state(&tabs[*index].page, phase, idb_ms, &caps),
            )
            .await;
            match step {
                Err(_) => {
                    report.idb_timed_out = true;
                    report.truncated = true;
                    state.truncated = true;
                    break;
                }
                Ok(Err(_)) | Ok(Ok(None)) => {}
                Ok(Ok(Some(capture))) => {
                    // The tab navigated between the two calls: its IndexedDB is another
                    // origin's now.
                    if capture.state.origin == state.origin {
                        state.indexed_db = capture.state.indexed_db;
                        state.dropped_values += capture.state.dropped_values;
                        state.truncated |= capture.state.truncated;
                    }
                }
            }
        }
    } else {
        report.idb_skipped = true;
    }
    report.idb_ms = started.elapsed().as_millis() as u64;

    // 4. Origins no live tab covers: what their pages flushed while alive.
    let want_closed = idb && closed_origins_capture_enabled();
    let limits = ClosedLimits::from_caps(&caps);
    let mut closed_candidates: Vec<String> = Vec::new();
    let mut closed_skipped = 0usize;
    let mut flushed = match journal.lock() {
        Ok(mut journal) => {
            for origin in &live_origins {
                journal.record_visit(origin);
            }
            if want_closed {
                (closed_candidates, closed_skipped) =
                    closed_origin_candidates(journal.visited(), &live_origins, limits.origins);
            }
            journal.flushed_not_in(&live_origins)
        }
        Err(_) => Vec::new(),
    };

    // 4b. The IndexedDB of those closed origins (D2-070), flag-gated and budgeted. It goes
    // through a live tab (the active one first) because the IndexedDB domain needs a target.
    let mut closed_report = ClosedReport::default();
    if want_closed && !closed_candidates.is_empty() {
        let overall_end = capture_started
            + Duration::from_millis(BUDGET_TOTAL_MS.saturating_sub(CLOSED_MARGIN_MS));
        let until = (Instant::now() + Duration::from_millis(BUDGET_CLOSED_MS)).min(overall_end);
        if let Some(tab) = tabs.first() {
            if Instant::now() < until {
                let source = CdpClosedIdb { page: &tab.page };
                let (reads, phase) =
                    capture_closed_origins(&source, &closed_candidates, &limits, until).await;
                closed_report = phase;
                flushed = merge_closed_idb(flushed, reads);
            } else {
                closed_report.enabled = true;
                closed_report.candidates = closed_candidates.len() as u32;
                closed_report.timed_out = true;
                closed_report.truncated = true;
                closed_report.skipped = closed_candidates.len() as u32;
            }
        } else {
            closed_report.enabled = true;
            closed_report.candidates = closed_candidates.len() as u32;
            closed_report.no_page = true;
            closed_report.skipped = closed_candidates.len() as u32;
        }
        closed_report.skipped += closed_skipped as u32;
        report.truncated |= closed_report.truncated;
        tracing::info!(
            event = "profile_capture_closed_origins",
            candidates = closed_report.candidates,
            read = closed_report.origins_read,
            failed = closed_report.origins_failed,
            skipped = closed_report.skipped,
            databases = closed_report.databases,
            records = closed_report.records,
            timed_out = closed_report.timed_out,
            truncated = closed_report.truncated,
            no_page = closed_report.no_page,
            "closed-tab origin IndexedDB captured"
        );
    }
    report.origins_live = live.len() as u32;
    report.origins_flushed = flushed.len() as u32;

    // 5. Merge and cap. Cookies are counted, never dropped.
    let cookies_bytes = serde_json::to_vec(&cookies).map_or(0, |bytes| bytes.len());
    let active_origin = live
        .iter()
        .find(|(_, index)| tabs[*index].active)
        .or_else(|| live.first())
        .map(|(state, _)| state.origin.clone());
    let live_states: Vec<OriginState> = live.into_iter().map(|(state, _)| state).collect();
    let (origins, outcome) = merge_and_cap(live_states, flushed, cookies_bytes, &caps);
    report.fill_from(&origins, &outcome);

    // The active origin's entries also go into the legacy flat fields, so a binary that
    // predates v2 still restores something.
    let (flat_local, flat_session) = active_origin
        .as_deref()
        .and_then(|origin| origins.iter().find(|state| state.origin == origin))
        .map(|state| (state.local_storage.clone(), state.session_storage.clone()))
        .unwrap_or_default();
    let meta = meta.unwrap_or(PageCapture {
        state: OriginState::default(),
        user_agent: None,
        locale: None,
        timezone: None,
    });
    let snapshot = SessionSnapshot {
        snapshot_version: SNAPSHOT_VERSION,
        cookies,
        local_storage: flat_local,
        session_storage: flat_session,
        user_agent: meta.user_agent,
        locale: meta.locale,
        timezone: meta.timezone,
        origins,
        ..Default::default()
    };
    Ok(ProfileCapture {
        snapshot,
        report,
        outcome,
        closed: closed_report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cdp_cookie() -> net::Cookie {
        net::Cookie {
            name: "sid".to_owned(),
            value: "v".to_owned(),
            domain: ".example.com".to_owned(),
            path: "/".to_owned(),
            expires: 1_900_000_000.5,
            size: 4,
            http_only: true,
            secure: true,
            session: false,
            same_site: Some(net::CookieSameSite::Lax),
            priority: net::CookiePriority::High,
            source_scheme: net::CookieSourceScheme::Secure,
            source_port: 443,
            partition_key: None,
            partition_key_opaque: None,
        }
    }

    #[test]
    fn cookie_mapping_expiry_session_samesite_partition() {
        // A persistent cookie keeps its expiry (to the millisecond), SameSite, priority and
        // source scheme, and its domain exactly as Chromium reported it.
        let persistent = cookie_from_cdp(&cdp_cookie());
        assert_eq!(persistent.domain, ".example.com");
        assert_eq!(
            persistent.expires.map(|at| at.timestamp_millis()),
            Some(1_900_000_000_500)
        );
        assert_eq!(persistent.same_site, Some(SameSite::Lax));
        assert_eq!(persistent.priority, Some(CookiePriority::High));
        assert_eq!(persistent.source_scheme, Some(SourceScheme::Secure));
        assert!(!persistent.session && persistent.partition_key.is_none());

        // A session cookie has no expiry, whatever number Chromium sent with it.
        let session = cookie_from_cdp(&net::Cookie {
            session: true,
            expires: -1.0,
            same_site: Some(net::CookieSameSite::None),
            domain: "host.example.com".to_owned(),
            ..cdp_cookie()
        });
        assert!(session.session && session.expires.is_none());
        assert_eq!(session.same_site, Some(SameSite::None));
        assert_eq!(
            session.domain, "host.example.com",
            "host-only keeps no leading dot"
        );

        // A non-finite or non-positive expiry is no expiry, never a panic.
        for bad in [f64::NAN, f64::INFINITY, 0.0, -5.0] {
            let cookie = cookie_from_cdp(&net::Cookie {
                expires: bad,
                ..cdp_cookie()
            });
            assert!(cookie.expires.is_none(), "{bad}");
        }

        // A partition key is carried; an opaque one is not invented.
        let partitioned = cookie_from_cdp(&net::Cookie {
            partition_key: Some(net::CookiePartitionKey::new("https://top.test", true)),
            partition_key_opaque: Some(false),
            ..cdp_cookie()
        });
        assert_eq!(
            partitioned.partition_key,
            Some(PartitionKey {
                top_level_site: "https://top.test".to_owned(),
                has_cross_site_ancestor: true,
            })
        );
        let opaque = cookie_from_cdp(&net::Cookie {
            partition_key: Some(net::CookiePartitionKey::new("", false)),
            partition_key_opaque: Some(true),
            ..cdp_cookie()
        });
        assert!(opaque.partition_key.is_none());

        // No SameSite attribute stays none.
        let unset = cookie_from_cdp(&net::Cookie {
            same_site: None,
            ..cdp_cookie()
        });
        assert!(unset.same_site.is_none());
    }

    #[test]
    fn flush_is_trusted_only_from_the_context_it_names() {
        let payload =
            r#"{"origin":"https://a.test","local":[["k","v"]],"session":[],"truncated":false}"#;
        let state = flush_to_state(payload, Some("https://a.test")).expect("same origin");
        assert_eq!(state.origin, "https://a.test");
        assert_eq!(state.local_storage, vec![("k".to_owned(), "v".to_owned())]);
        // The page claims another origin than the one Chromium says it runs in.
        assert!(flush_to_state(payload, Some("https://evil.test")).is_none());
        // A context Chromium never told us about.
        assert!(flush_to_state(payload, None).is_none());
        // Not JSON, not http(s), too large.
        assert!(flush_to_state("not json", Some("https://a.test")).is_none());
        let opaque = r#"{"origin":"null","local":[],"session":[]}"#;
        assert!(flush_to_state(opaque, Some("null")).is_none());
        let huge = format!(
            r#"{{"origin":"https://a.test","local":[["k","{}"]],"session":[]}}"#,
            "x".repeat(MAX_FLUSH_PAYLOAD_BYTES)
        );
        assert!(flush_to_state(&huge, Some("https://a.test")).is_none());
    }

    #[test]
    fn serializer_js_is_one_async_function_and_carries_the_seed_sentinel() {
        // The two scripts are strings handed to Chromium; a cheap shape check keeps an edit
        // that breaks the wrapper from reaching a live run.
        assert!(SERIALIZER_JS.trim_start().starts_with("async (opts) =>"));
        assert!(SERIALIZER_JS.trim_end().ends_with('}'));
        assert!(SERIALIZER_JS.contains(crate::profile_snapshot::SEEDED_SENTINEL_KEY));
        assert!(FLUSH_JS.contains(crate::profile_snapshot::SEEDED_SENTINEL_KEY));
        assert!(FLUSH_JS.contains(FLUSH_BINDING));
        assert_eq!(
            SERIALIZER_JS.matches('{').count(),
            SERIALIZER_JS.matches('}').count(),
            "balanced braces"
        );
    }

    // -----------------------------------------------------------------------------------
    // Closed-tab origins (D2-070): the mock source stands in for the CDP domain
    // -----------------------------------------------------------------------------------

    use std::collections::HashMap;

    #[derive(Default, Clone)]
    struct MockOrigin {
        /// (database name, store name, record count)
        dbs: Vec<(String, Vec<(String, usize)>)>,
        fail: bool,
        hang: bool,
    }

    #[derive(Default)]
    struct MockSource {
        origins: HashMap<String, MockOrigin>,
        calls: Mutex<Vec<String>>,
    }

    impl MockSource {
        fn with(mut self, origin: &str, spec: MockOrigin) -> Self {
            self.origins.insert(origin.to_owned(), spec);
            self
        }

        fn origin(&self, origin: &str, what: &str) -> QuarryResult<MockOrigin> {
            self.calls.lock().unwrap().push(format!("{what}:{origin}"));
            let spec = self.origins.get(origin).cloned().unwrap_or_default();
            if spec.fail {
                return Err(QuarryError::new(ErrorCode::DriverFailed, "mock failure"));
            }
            Ok(spec)
        }

        fn calls(&self, prefix: &str) -> usize {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call.starts_with(prefix))
                .count()
        }
    }

    fn record(n: usize, pad: usize) -> IdbRecord {
        IdbRecord {
            key: json!(n),
            value: json!({ "n": n, "pad": "x".repeat(pad) }),
        }
    }

    impl ClosedIdbSource for MockSource {
        async fn database_names(&self, origin: &str) -> QuarryResult<Vec<String>> {
            let spec = self.origin(origin, "names")?;
            if spec.hang {
                std::future::pending::<()>().await;
            }
            Ok(spec.dbs.iter().map(|(name, _)| name.clone()).collect())
        }

        async fn database(&self, origin: &str, name: &str) -> QuarryResult<IdbDatabase> {
            let spec = self.origin(origin, "db")?;
            let (_, stores) = spec
                .dbs
                .iter()
                .find(|(db, _)| db == name)
                .expect("database exists");
            Ok(IdbDatabase {
                name: name.to_owned(),
                version: 3,
                stores: stores
                    .iter()
                    .map(|(store, _)| IdbStore {
                        name: store.clone(),
                        key_path: json!("id"),
                        ..Default::default()
                    })
                    .collect(),
            })
        }

        async fn records(
            &self,
            origin: &str,
            database: &str,
            store: &str,
            skip: usize,
            page_size: usize,
        ) -> QuarryResult<RecordPage> {
            let spec = self.origin(origin, "records")?;
            let total = spec
                .dbs
                .iter()
                .find(|(db, _)| db == database)
                .and_then(|(_, stores)| stores.iter().find(|(name, _)| name == store))
                .map(|(_, count)| *count)
                .expect("store exists");
            let end = (skip + page_size).min(total);
            Ok(RecordPage {
                records: (skip..end).map(|n| record(n, 8)).collect(),
                dropped: 0,
                has_more: end < total,
            })
        }
    }

    fn one_db(stores: &[(&str, usize)]) -> MockOrigin {
        MockOrigin {
            dbs: vec![(
                "app".to_owned(),
                stores
                    .iter()
                    .map(|(name, count)| ((*name).to_owned(), *count))
                    .collect(),
            )],
            ..Default::default()
        }
    }

    fn limits() -> ClosedLimits {
        ClosedLimits::from_caps(&ProfileCaps::default())
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(30)
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn closed_origin_flag_defaults_off_and_parses_like_the_other_profile_flags() {
        assert!(!closed_flag(None));
        assert!(!closed_flag(Some("")));
        assert!(!closed_flag(Some("maybe")));
        assert!(closed_flag(Some("1")));
        assert!(closed_flag(Some(" On ")));
        assert!(closed_flag(Some("true")));
        assert!(!closed_flag(Some("0")));
        assert!(!closed_flag(Some("off")));
        assert_eq!(
            CAPTURE_CLOSED_ORIGINS_ENV,
            "QUARRY_BROWSER_CAPTURE_CLOSED_ORIGINS"
        );
    }

    #[test]
    fn closed_limits_never_exceed_the_snapshot_caps() {
        let defaults = ClosedLimits::from_caps(&ProfileCaps::default());
        assert_eq!(defaults.origin_bytes, MAX_CLOSED_ORIGIN_BYTES);
        assert_eq!(defaults.total_bytes, MAX_CLOSED_TOTAL_BYTES);
        assert_eq!(defaults.origin_records, MAX_CLOSED_RECORDS);
        let tight = ClosedLimits::from_caps(&ProfileCaps {
            max_bytes: 1_000,
            max_origin_bytes: 300,
            max_origins: 4,
            max_idb_records: 7,
        });
        assert_eq!(tight.origin_bytes, 300);
        assert_eq!(tight.total_bytes, 500);
        assert_eq!(tight.origin_records, 7);
    }

    #[test]
    fn candidates_are_the_visited_origins_no_live_tab_covers() {
        let visited: BTreeSet<String> =
            ["https://a.test", "https://b.test", "https://c.test", "https://d.test"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
        let live: BTreeSet<String> = ["https://b.test".to_owned()].into_iter().collect();
        let (all, skipped) = closed_origin_candidates(visited.iter(), &live, 10);
        assert_eq!(all, names(&["https://a.test", "https://c.test", "https://d.test"]));
        assert_eq!(skipped, 0);
        let (two, skipped) = closed_origin_candidates(visited.iter(), &live, 2);
        assert_eq!(two, names(&["https://a.test", "https://c.test"]));
        assert_eq!(skipped, 1);
        let (none, skipped) = closed_origin_candidates(visited.iter(), &visited, 10);
        assert!(none.is_empty() && skipped == 0);
    }

    #[tokio::test]
    async fn a_closed_origin_is_read_page_by_page() {
        let source = MockSource::default().with(
            "https://a.test",
            one_db(&[("orders", 250), ("meta", 3)]),
        );
        let (reads, report) =
            capture_closed_origins(&source, &names(&["https://a.test"]), &limits(), soon()).await;
        assert_eq!(reads.len(), 1);
        let db = &reads[0].idb[0];
        assert_eq!((db.name.as_str(), db.version), ("app", 3));
        let orders = db.stores.iter().find(|s| s.name == "orders").unwrap();
        assert_eq!(orders.records.len(), 250);
        assert_eq!(orders.records[249].key, json!(249));
        assert_eq!(orders.key_path, json!("id"));
        assert!(!reads[0].truncated && !orders.omitted);
        // 250 records at 100 per page is three requests; the 3-record store one.
        assert_eq!(source.calls("records:"), 4);
        assert_eq!(report.origins_read, 1);
        assert_eq!((report.databases, report.records), (1, 253));
        assert!(!report.truncated && !report.timed_out && report.origins_failed == 0);
    }

    #[tokio::test]
    async fn the_per_origin_record_cap_keeps_what_was_read_and_says_truncated() {
        let source = MockSource::default().with("https://a.test", one_db(&[("orders", 500)]));
        let capped = ClosedLimits {
            origin_records: 120,
            ..limits()
        };
        let (reads, report) =
            capture_closed_origins(&source, &names(&["https://a.test"]), &capped, soon()).await;
        let store = &reads[0].idb[0].stores[0];
        assert_eq!(store.records.len(), 120);
        assert!(reads[0].truncated && report.truncated);
        assert!(!store.omitted, "records were kept");
        // The second request asked for 20, not a whole page.
        assert_eq!(source.calls("records:"), 2);
    }

    #[tokio::test]
    async fn the_per_origin_byte_cap_stops_the_read() {
        let source = MockSource::default().with("https://a.test", one_db(&[("orders", 500)]));
        let one = record_bytes(&record(0, 8));
        let capped = ClosedLimits {
            origin_bytes: one * 10 + 5,
            ..limits()
        };
        let (reads, _) =
            capture_closed_origins(&source, &names(&["https://a.test"]), &capped, soon()).await;
        let kept: usize = reads[0].idb[0].stores[0].records.iter().map(record_bytes).sum();
        assert!(kept <= capped.origin_bytes);
        assert!(reads[0].idb[0].stores[0].records.len() >= 9);
        assert!(reads[0].truncated);
    }

    #[tokio::test]
    async fn a_store_cut_before_its_first_record_keeps_its_definition_flagged_omitted() {
        let source = MockSource::default().with("https://a.test", one_db(&[("orders", 5)]));
        let capped = ClosedLimits {
            origin_bytes: 1,
            ..limits()
        };
        let (reads, _) =
            capture_closed_origins(&source, &names(&["https://a.test"]), &capped, soon()).await;
        let store = &reads[0].idb[0].stores[0];
        assert!(store.records.is_empty() && store.omitted);
        assert!(reads[0].truncated);
    }

    #[tokio::test]
    async fn the_all_origins_byte_cap_leaves_later_origins_out() {
        let source = MockSource::default()
            .with("https://a.test", one_db(&[("s", 40)]))
            .with("https://b.test", one_db(&[("s", 40)]))
            .with("https://c.test", one_db(&[("s", 40)]));
        let one = record_bytes(&record(0, 8));
        let capped = ClosedLimits {
            origin_bytes: one * 60,
            total_bytes: one * 45,
            ..limits()
        };
        let all = names(&["https://a.test", "https://b.test", "https://c.test"]);
        let (reads, report) = capture_closed_origins(&source, &all, &capped, soon()).await;
        let total: usize = reads
            .iter()
            .flat_map(|r| r.idb.iter())
            .flat_map(|d| d.stores.iter())
            .flat_map(|s| s.records.iter())
            .map(record_bytes)
            .sum();
        assert!(total <= capped.total_bytes, "{total} > {}", capped.total_bytes);
        assert_eq!(reads[0].idb[0].stores[0].records.len(), 40);
        assert!(report.truncated);
        assert!(reads.len() < 3 || reads[2].idb[0].stores[0].records.len() < 40);
    }

    #[tokio::test]
    async fn databases_over_the_per_origin_cap_are_left_out() {
        let many = MockOrigin {
            dbs: (0..20)
                .map(|n| (format!("db{n:02}"), vec![("s".to_owned(), 1)]))
                .collect(),
            ..Default::default()
        };
        let source = MockSource::default().with("https://a.test", many);
        let (reads, report) =
            capture_closed_origins(&source, &names(&["https://a.test"]), &limits(), soon()).await;
        assert_eq!(reads[0].idb.len(), MAX_CLOSED_DATABASES);
        assert!(reads[0].truncated && report.truncated);
        assert_eq!(reads[0].idb[0].name, "db00");
    }

    #[tokio::test]
    async fn one_failing_origin_costs_only_itself() {
        let source = MockSource::default()
            .with("https://a.test", one_db(&[("s", 2)]))
            .with(
                "https://b.test",
                MockOrigin {
                    fail: true,
                    ..Default::default()
                },
            )
            .with("https://c.test", one_db(&[("s", 2)]));
        let all = names(&["https://a.test", "https://b.test", "https://c.test"]);
        let (reads, report) = capture_closed_origins(&source, &all, &limits(), soon()).await;
        let read: Vec<&str> = reads.iter().map(|r| r.origin.as_str()).collect();
        assert_eq!(read, ["https://a.test", "https://c.test"]);
        assert_eq!((report.origins_read, report.origins_failed), (2, 1));
        assert!(!report.timed_out);
    }

    #[tokio::test]
    async fn a_failure_in_a_later_page_discards_the_whole_origin() {
        // The mock fails at the records call: names and database succeed first.
        struct FailsOnRecords;
        impl ClosedIdbSource for FailsOnRecords {
            async fn database_names(&self, _: &str) -> QuarryResult<Vec<String>> {
                Ok(vec!["app".to_owned()])
            }
            async fn database(&self, _: &str, name: &str) -> QuarryResult<IdbDatabase> {
                Ok(IdbDatabase {
                    name: name.to_owned(),
                    version: 1,
                    stores: vec![IdbStore {
                        name: "s".to_owned(),
                        ..Default::default()
                    }],
                })
            }
            async fn records(
                &self,
                _: &str,
                _: &str,
                _: &str,
                _: usize,
                _: usize,
            ) -> QuarryResult<RecordPage> {
                Err(QuarryError::new(ErrorCode::DriverFailed, "gone"))
            }
        }
        let (reads, report) = capture_closed_origins(
            &FailsOnRecords,
            &names(&["https://a.test"]),
            &limits(),
            soon(),
        )
        .await;
        assert!(reads.is_empty());
        assert_eq!(report.origins_failed, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn running_out_of_time_keeps_finished_origins_and_skips_the_rest() {
        let source = MockSource::default()
            .with("https://a.test", one_db(&[("s", 2)]))
            .with(
                "https://b.test",
                MockOrigin {
                    hang: true,
                    ..Default::default()
                },
            )
            .with("https://c.test", one_db(&[("s", 2)]));
        let all = names(&["https://a.test", "https://b.test", "https://c.test"]);
        let until = Instant::now() + Duration::from_millis(BUDGET_CLOSED_MS);
        let (reads, report) = capture_closed_origins(&source, &all, &limits(), until).await;
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].origin, "https://a.test");
        assert!(report.timed_out && report.truncated);
        assert_eq!(report.skipped, 2, "the hung origin and the one after it");
        assert_eq!(source.calls("names:https://c.test"), 0);
    }

    #[tokio::test]
    async fn an_origin_with_no_database_reads_empty() {
        let source = MockSource::default().with("https://a.test", MockOrigin::default());
        let (reads, report) =
            capture_closed_origins(&source, &names(&["https://a.test"]), &limits(), soon()).await;
        assert_eq!(reads.len(), 1);
        assert!(reads[0].idb.is_empty());
        assert_eq!((report.origins_read, report.databases, report.records), (1, 0, 0));
    }

    fn web_state(origin: &str) -> OriginState {
        OriginState {
            origin: origin.to_owned(),
            local_storage: vec![("k".to_owned(), "v".to_owned())],
            ..Default::default()
        }
    }

    fn db(name: &str, records: usize) -> IdbDatabase {
        IdbDatabase {
            name: name.to_owned(),
            version: 1,
            stores: vec![IdbStore {
                name: "s".to_owned(),
                records: (0..records).map(|n| record(n, 1)).collect(),
                ..Default::default()
            }],
        }
    }

    #[test]
    fn merge_replaces_idb_keeps_web_storage_and_adds_origins_the_journal_lacked() {
        let mut stale = web_state("https://a.test");
        stale.indexed_db = vec![db("old", 1)];
        let closed = vec![
            ClosedOrigin {
                origin: "https://a.test".to_owned(),
                idb: vec![db("new", 2)],
                dropped: 3,
                truncated: true,
            },
            ClosedOrigin {
                origin: "https://z.test".to_owned(),
                idb: vec![db("z", 1)],
                ..Default::default()
            },
        ];
        let merged = merge_closed_idb(vec![stale], closed);
        assert_eq!(merged.len(), 2);
        let a = &merged[0];
        assert_eq!(a.indexed_db.len(), 1);
        assert_eq!(a.indexed_db[0].name, "new");
        assert_eq!(a.local_storage, vec![("k".to_owned(), "v".to_owned())]);
        assert_eq!(a.dropped_values, 3);
        assert!(a.truncated);
        let z = &merged[1];
        assert_eq!(z.origin, "https://z.test");
        assert!(z.local_storage.is_empty());
        assert_eq!(z.indexed_db[0].name, "z");
    }

    #[test]
    fn an_empty_read_never_erases_the_idb_the_journal_holds() {
        let mut carried = web_state("https://a.test");
        carried.indexed_db = vec![db("kept", 4)];
        let closed = vec![
            ClosedOrigin {
                origin: "https://a.test".to_owned(),
                ..Default::default()
            },
            // A read that found nothing for an origin with no state adds no empty state.
            ClosedOrigin {
                origin: "https://nothing.test".to_owned(),
                ..Default::default()
            },
        ];
        let merged = merge_closed_idb(vec![carried.clone()], closed);
        assert_eq!(merged, vec![carried]);
    }

    #[test]
    fn merged_closed_idb_goes_through_the_same_caps_as_a_live_origin() {
        let closed = vec![ClosedOrigin {
            origin: "https://a.test".to_owned(),
            idb: vec![db("big", 100)],
            ..Default::default()
        }];
        let flushed = merge_closed_idb(Vec::new(), closed);
        let caps = ProfileCaps {
            max_idb_records: 10,
            ..ProfileCaps::default()
        };
        let (origins, outcome) = merge_and_cap(Vec::new(), flushed, 0, &caps);
        assert_eq!(origins.len(), 1);
        assert!(origins[0].idb_record_count() <= 10);
        assert!(outcome.truncated);
    }

    #[test]
    fn remote_primitives_become_the_tagged_encoding() {
        let some = |v: serde_json::Value| Some(v);
        assert_eq!(
            primitive_to_tagged("undefined", None, None),
            some(json!({ "$t": "undefined" }))
        );
        assert_eq!(
            primitive_to_tagged("string", Some(&json!("hi")), None),
            some(json!("hi"))
        );
        assert_eq!(
            primitive_to_tagged("boolean", Some(&json!(true)), None),
            some(json!(true))
        );
        assert_eq!(
            primitive_to_tagged("number", Some(&json!(1.5)), None),
            some(json!(1.5))
        );
        assert_eq!(
            primitive_to_tagged("number", None, Some("NaN")),
            some(json!({ "$t": "num", "v": "NaN" }))
        );
        assert_eq!(
            primitive_to_tagged("number", None, Some("-0")),
            some(json!({ "$t": "num", "v": "-0" }))
        );
        assert_eq!(
            primitive_to_tagged("bigint", None, Some("123n")),
            some(json!({ "$t": "bigint", "v": "123" }))
        );
        assert_eq!(
            primitive_to_tagged("object", Some(&serde_json::Value::Null), None),
            some(serde_json::Value::Null)
        );
        assert_eq!(primitive_to_tagged("symbol", None, None), None);
        assert_eq!(primitive_to_tagged("function", None, None), None);
        assert_eq!(primitive_to_tagged("number", None, Some("weird")), None);
    }

    #[test]
    fn the_encoded_values_round_trip_through_the_snapshot_decoder() {
        // What the encoder writes is what restore reads: every shape above decodes.
        for value in [
            json!({ "$t": "undefined" }),
            json!({ "$t": "num", "v": "Infinity" }),
            json!({ "$t": "bigint", "v": "123" }),
            json!({ "$t": "date", "v": 1_700_000_000_000u64 }),
            json!("plain"),
            json!(null),
        ] {
            assert!(
                crate::profile_snapshot::IdbValue::from_json(&value).is_ok(),
                "{value}"
            );
        }
    }

    #[test]
    fn the_remote_encoder_is_one_async_function_with_its_budget_placeholder() {
        assert!(ENCODE_REMOTE_JS.trim_start().starts_with("async function ()"));
        assert!(ENCODE_REMOTE_JS.trim_end().ends_with('}'));
        assert!(ENCODE_REMOTE_JS.contains("__MAX_BYTES__"));
        assert_eq!(
            ENCODE_REMOTE_JS.matches('{').count(),
            ENCODE_REMOTE_JS.matches('}').count(),
            "balanced braces"
        );
        let ready = ENCODE_REMOTE_JS.replace("__MAX_BYTES__", "2097152");
        assert!(!ready.contains("__MAX_BYTES__"));
        // The tags it writes are the ones the snapshot decoder reads.
        for tag in ["undefined", "num", "bigint", "date", "regexp", "bytes", "blob", "map", "set", "obj", "dropped"] {
            assert!(ENCODE_REMOTE_JS.contains(&format!("$t: '{tag}'")), "tag {tag}");
        }
    }

    /// Live: a visited origin whose tab closed is still readable through the IndexedDB
    /// domain from another tab of the same context. Needs a pinned Chromium; not run in the
    /// unit lane.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "live Chromium: CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1, run with --ignored"]
    async fn live_closed_tab_origin_idb_is_captured_through_another_tab() {
        use ::chromiumoxide::cdp::browser_protocol::target::{
            CreateBrowserContextParams, CreateTargetParams,
        };
        use ::chromiumoxide::handler::viewport::Viewport;
        use ::chromiumoxide::BrowserConfig;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        if !crate::live_gate::live_enabled() {
            return;
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = socket.read(&mut buf).await;
                    let body = "<!doctype html><title>t</title>";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        let profile = tempfile::tempdir().expect("profile dir");
        let mut builder = BrowserConfig::builder()
            .user_data_dir(profile.path())
            .arg("--disable-dev-shm-usage")
            .viewport(None::<Viewport>);
        if std::env::var("QUARRY_BROWSER_NO_SANDBOX").ok().as_deref() == Some("1") {
            builder = builder.no_sandbox();
        }
        let (browser, mut handler) =
            Browser::launch(builder.build().expect("config")).await.expect("launch");
        tokio::spawn(async move { while handler.next().await.is_some() {} });
        let context = browser
            .create_browser_context(CreateBrowserContextParams::default())
            .await
            .expect("context");
        let mut params = CreateTargetParams::new("about:blank");
        params.browser_context_id = Some(context.clone());
        let carrier = browser.new_page(params.clone()).await.expect("carrier tab");
        let victim = browser.new_page(params).await.expect("victim tab");
        let origin = format!("http://127.0.0.1:{port}");
        victim.goto(format!("{origin}/")).await.expect("goto");
        let mut setup = EvaluateParams::new(
            r#"(async () => {
              const db = await new Promise((res, rej) => {
                const r = indexedDB.open('closed-live', 2);
                r.onupgradeneeded = () => r.result.createObjectStore('items', { keyPath: 'id' });
                r.onsuccess = () => res(r.result);
                r.onerror = () => rej(r.error);
              });
              await new Promise((res, rej) => {
                const tx = db.transaction('items', 'readwrite');
                tx.objectStore('items').put({ id: 1, when: new Date(1700000000000), n: 7n });
                tx.objectStore('items').put({ id: 2, bytes: new Uint8Array([1, 2, 3]) });
                tx.oncomplete = () => res();
                tx.onerror = () => rej(tx.error);
              });
              db.close();
              return 'stored';
            })()"#,
        );
        setup.await_promise = Some(true);
        setup.return_by_value = Some(true);
        victim.execute(setup).await.expect("seed IndexedDB");
        victim.close().await.expect("close the tab");

        let source = CdpClosedIdb { page: &carrier };
        let until = Instant::now() + Duration::from_secs(10);
        let (reads, report) =
            capture_closed_origins(&source, std::slice::from_ref(&origin), &limits(), until).await;
        server.abort();
        assert_eq!(report.origins_read, 1, "{report:?}");
        let db = &reads[0].idb[0];
        assert_eq!((db.name.as_str(), db.version), ("closed-live", 2));
        let store = &db.stores[0];
        assert_eq!(store.name, "items");
        assert_eq!(store.key_path, json!("id"));
        assert_eq!(store.records.len(), 2, "{:?}", store.records);
        let first = &store.records[0];
        assert_eq!(first.key, json!(1));
        assert_eq!(first.value["when"], json!({ "$t": "date", "v": 1_700_000_000_000u64 }));
        assert_eq!(first.value["n"], json!({ "$t": "bigint", "v": "7" }));
        assert_eq!(
            store.records[1].value["bytes"],
            json!({ "$t": "bytes", "k": "Uint8Array", "v": "AQID" })
        );
    }
}
