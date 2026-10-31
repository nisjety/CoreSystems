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
//!    Web storage only: IndexedDB cannot be read from a `pagehide` handler. Whether the
//!    frame-less `IndexedDB` CDP domain can cover a closed tab's origin is the S0 spike's
//!    question (`tests/profile_cdp_spike.rs`); it is not built here.
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
    merge_and_cap, normalize_origin, CapOutcome, CaptureReport, CookiePriority, IdbDatabase,
    OriginJournal, OriginState, PartitionKey, ProfileCaps, SameSite, SourceScheme,
    BUDGET_COOKIES_MS, BUDGET_IDB_MS, BUDGET_STORAGE_MS, MAX_FLUSH_PAYLOAD_BYTES, SNAPSHOT_VERSION,
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
    let flushed = match journal.lock() {
        Ok(mut journal) => {
            for origin in &live_origins {
                journal.record_visit(origin);
            }
            journal.flushed_not_in(&live_origins)
        }
        Err(_) => Vec::new(),
    };
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
}
