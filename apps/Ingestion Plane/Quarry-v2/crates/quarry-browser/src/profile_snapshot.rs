//! Snapshot schema v2 types and the pure logic of capture v2 (WS17.C1).
//!
//! Everything here compiles without the `chromiumoxide` feature so the
//! default `cargo test -p quarry-browser` covers it. The CDP side lives in
//! `profile_capture.rs`.
//!
//! - wire types: [`SameSite`], [`CookiePriority`], [`SourceScheme`], [`PartitionKey`],
//!   [`OriginState`], [`IdbDatabase`], [`IdbStore`], [`IdbRecord`];
//! - the tagged JSON encoding of IndexedDB keys and values ([`IdbValue`]); the in-page
//!   serializer in `profile_capture.rs` (`SERIALIZER_JS`) writes exactly this encoding and
//!   WS17.C2's seeding script reads it;
//! - [`ProfileCaps`] and [`merge_and_cap`]: the size caps, dropping the largest IndexedDB
//!   stores first and never a cookie;
//! - [`OriginJournal`]: the per-session record of visited origins and of the web storage
//!   a page flushed while it was alive;
//! - [`CaptureReport`]: counts only, never a name, a URL or a value;
//! - the flags `QUARRY_BROWSER_PROFILE_CAPTURE_V2` (default off), `QUARRY_BROWSER_PROFILE_IDB`
//!   and `QUARRY_BROWSER_PROFILE_MAX_BYTES`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::session::SessionSnapshot;

/// `SessionSnapshot::snapshot_version` written by capture v2.
pub const SNAPSHOT_VERSION: u8 = 2;
/// Plaintext cap of one snapshot (`QUARRY_BROWSER_PROFILE_MAX_BYTES` overrides it).
pub const DEFAULT_MAX_PLAINTEXT_BYTES: usize = 16 * 1024 * 1024;
/// Web storage plus IndexedDB of one origin.
pub const MAX_ORIGIN_BYTES: usize = 4 * 1024 * 1024;
/// Origins tracked per session and kept per snapshot.
pub const MAX_ORIGINS: usize = 64;
/// IndexedDB records per snapshot.
pub const MAX_IDB_RECORDS: usize = 50_000;
/// The largest `pagehide` flush the driver accepts from a page.
pub const MAX_FLUSH_PAYLOAD_BYTES: usize = 2 * 1024 * 1024;
/// Written into a page's localStorage by WS17.C2's seeding script; capture strips it.
pub const SEEDED_SENTINEL_KEY: &str = "__quarry_profile_seeded_v1";

/// Capture budget (plan WS17.C1): cookies first, then web storage, then IndexedDB.
pub const BUDGET_TOTAL_MS: u64 = 10_000;
pub const BUDGET_COOKIES_MS: u64 = 2_000;
pub const BUDGET_STORAGE_MS: u64 = 4_000;
pub const BUDGET_IDB_MS: u64 = 4_000;

pub const CAPTURE_V2_ENV: &str = "QUARRY_BROWSER_PROFILE_CAPTURE_V2";
pub const IDB_ENV: &str = "QUARRY_BROWSER_PROFILE_IDB";
pub const MAX_BYTES_ENV: &str = "QUARRY_BROWSER_PROFILE_MAX_BYTES";

/// A boolean flag value: `1`, `true`, `on`, `yes` are on; `0`, `false`, `off`, `no` are off;
/// anything else (or unset) is `default`.
pub fn parse_flag(raw: Option<&str>, default: bool) -> bool {
    match raw
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("1" | "true" | "on" | "yes") => true,
        Some("0" | "false" | "off" | "no") => false,
        _ => default,
    }
}

/// Capture v2 is on. Default off for one release: off keeps today's capture.
pub fn capture_v2_enabled() -> bool {
    parse_flag(std::env::var(CAPTURE_V2_ENV).ok().as_deref(), false)
}

/// IndexedDB capture, switchable on its own. Default on while capture v2 is on.
pub fn idb_capture_enabled() -> bool {
    parse_flag(std::env::var(IDB_ENV).ok().as_deref(), true)
}

// ---------------------------------------------------------------------------------------
// Cookie attributes
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

impl SameSite {
    /// The CDP spelling (`Strict`, `Lax`, `None`).
    pub fn as_cdp(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
            Self::None => "None",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CookiePriority {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceScheme {
    Unset,
    NonSecure,
    Secure,
}

/// A cookie partition key (CHIPS), as Chromium 114+ reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionKey {
    pub top_level_site: String,
    #[serde(default)]
    pub has_cross_site_ancestor: bool,
}

// ---------------------------------------------------------------------------------------
// Per-origin web storage and IndexedDB
// ---------------------------------------------------------------------------------------

/// What one origin holds: web storage and IndexedDB.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OriginState {
    pub origin: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_storage: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_storage: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexed_db: Vec<IdbDatabase>,
    /// Values the serializer could not carry (not structured-cloneable), counted not kept.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub dropped_values: u32,
    /// Something was cut short: a cap, the time budget, or a payload limit.
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IdbDatabase {
    pub name: String,
    #[serde(default)]
    pub version: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stores: Vec<IdbStore>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IdbStore {
    pub name: String,
    /// `null`, a string or an array of strings (the store's key path).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub key_path: Value,
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_increment: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IdbIndex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<IdbRecord>,
    /// The records were left out by a cap (the store's definition is kept).
    #[serde(default, skip_serializing_if = "is_false")]
    pub omitted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IdbIndex {
    pub name: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub key_path: Value,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unique: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub multi_entry: bool,
}

/// One record. `key` and `value` use the tagged encoding of [`IdbValue`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdbRecord {
    pub key: Value,
    pub value: Value,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

pub(crate) fn is_zero_u8(value: &u8) -> bool {
    *value == 0
}

/// Serialized size of any serde value; the unit of every cap.
fn json_len<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map_or(0, |bytes| bytes.len())
}

impl IdbStore {
    fn records_bytes(&self) -> usize {
        self.records.iter().map(json_len).sum()
    }
}

impl OriginState {
    /// Web storage plus IndexedDB size, in serialized bytes.
    pub fn bytes(&self) -> usize {
        json_len(self)
    }

    pub fn idb_record_count(&self) -> usize {
        self.indexed_db
            .iter()
            .flat_map(|db| db.stores.iter())
            .map(|store| store.records.len())
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.local_storage.is_empty()
            && self.session_storage.is_empty()
            && self.indexed_db.is_empty()
    }
}

// ---------------------------------------------------------------------------------------
// The tagged encoding of IndexedDB keys and values
// ---------------------------------------------------------------------------------------

/// A structured-clone value in the tagged JSON encoding the in-page serializer writes.
///
/// Plain JSON stays plain: `null`, booleans, finite numbers, strings, arrays, and objects
/// without a `$t` own key. Everything else is an object `{ "$t": <tag>, ... }`:
///
/// | tag | shape |
/// |---|---|
/// | `undefined` | `{ "$t": "undefined" }` |
/// | `num` | `{ "$t": "num", "v": "NaN" \| "Infinity" \| "-Infinity" \| "-0" }` |
/// | `bigint` | `{ "$t": "bigint", "v": "<decimal>" }` |
/// | `date` | `{ "$t": "date", "v": <ms since epoch> \| "invalid" }` |
/// | `regexp` | `{ "$t": "regexp", "source": .., "flags": .. }` |
/// | `bytes` | `{ "$t": "bytes", "k": "ArrayBuffer" \| "Uint8Array" \| .., "v": "<base64>" }` |
/// | `blob` | `{ "$t": "blob", "mime": .., "v": "<base64>" }` (a `File` is a blob) |
/// | `map` | `{ "$t": "map", "v": [[key, value], ..] }` |
/// | `set` | `{ "$t": "set", "v": [value, ..] }` |
/// | `obj` | `{ "$t": "obj", "v": { .. } }` (a plain object that has its own `$t` key) |
/// | `dropped` | `{ "$t": "dropped" }` (not structured-cloneable: a function, a cycle, ..) |
#[derive(Debug, Clone, PartialEq)]
pub enum IdbValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    BigInt(String),
    String(String),
    /// Milliseconds since the epoch, `None` for an invalid date.
    Date(Option<f64>),
    RegExp {
        source: String,
        flags: String,
    },
    Bytes {
        kind: String,
        base64: String,
    },
    Blob {
        mime: String,
        base64: String,
    },
    Array(Vec<IdbValue>),
    Map(Vec<(IdbValue, IdbValue)>),
    Set(Vec<IdbValue>),
    Object(BTreeMap<String, IdbValue>),
    Dropped,
}

/// A tagged value that does not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedError(pub &'static str);

impl std::fmt::Display for TaggedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid tagged value: {}", self.0)
    }
}

impl std::error::Error for TaggedError {}

const TAG: &str = "$t";

impl IdbValue {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Undefined => json!({ TAG: "undefined" }),
            Self::Null => Value::Null,
            Self::Bool(b) => Value::Bool(*b),
            Self::Number(n) => {
                if n.is_finite() && !(*n == 0.0 && n.is_sign_negative()) {
                    serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number)
                } else if n.is_nan() {
                    json!({ TAG: "num", "v": "NaN" })
                } else if *n == f64::INFINITY {
                    json!({ TAG: "num", "v": "Infinity" })
                } else if *n == f64::NEG_INFINITY {
                    json!({ TAG: "num", "v": "-Infinity" })
                } else {
                    json!({ TAG: "num", "v": "-0" })
                }
            }
            Self::BigInt(digits) => json!({ TAG: "bigint", "v": digits }),
            Self::String(s) => Value::String(s.clone()),
            Self::Date(Some(ms)) => json!({ TAG: "date", "v": ms }),
            Self::Date(None) => json!({ TAG: "date", "v": "invalid" }),
            Self::RegExp { source, flags } => {
                json!({ TAG: "regexp", "source": source, "flags": flags })
            }
            Self::Bytes { kind, base64 } => json!({ TAG: "bytes", "k": kind, "v": base64 }),
            Self::Blob { mime, base64 } => json!({ TAG: "blob", "mime": mime, "v": base64 }),
            Self::Array(items) => Value::Array(items.iter().map(Self::to_json).collect()),
            Self::Map(entries) => json!({
                TAG: "map",
                "v": entries
                    .iter()
                    .map(|(k, v)| Value::Array(vec![k.to_json(), v.to_json()]))
                    .collect::<Vec<_>>(),
            }),
            Self::Set(items) => json!({
                TAG: "set",
                "v": items.iter().map(Self::to_json).collect::<Vec<_>>(),
            }),
            Self::Object(fields) => {
                let object: serde_json::Map<String, Value> = fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_json()))
                    .collect();
                if object.contains_key(TAG) {
                    json!({ TAG: "obj", "v": Value::Object(object) })
                } else {
                    Value::Object(object)
                }
            }
            Self::Dropped => json!({ TAG: "dropped" }),
        }
    }

    pub fn from_json(value: &Value) -> Result<Self, TaggedError> {
        Ok(match value {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Bool(*b),
            Value::Number(n) => Self::Number(n.as_f64().ok_or(TaggedError("number"))?),
            Value::String(s) => Self::String(s.clone()),
            Value::Array(items) => Self::Array(
                items
                    .iter()
                    .map(Self::from_json)
                    .collect::<Result<_, _>>()?,
            ),
            Value::Object(fields) => match fields.get(TAG) {
                None => Self::Object(
                    fields
                        .iter()
                        .map(|(k, v)| Ok((k.clone(), Self::from_json(v)?)))
                        .collect::<Result<_, TaggedError>>()?,
                ),
                Some(Value::String(tag)) => Self::from_tagged(tag, fields)?,
                Some(_) => return Err(TaggedError("tag is not a string")),
            },
        })
    }

    fn from_tagged(
        tag: &str,
        fields: &serde_json::Map<String, Value>,
    ) -> Result<Self, TaggedError> {
        let text = |key: &str| -> Result<String, TaggedError> {
            fields
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(TaggedError("missing text field"))
        };
        let list = |key: &str| -> Result<&Vec<Value>, TaggedError> {
            fields
                .get(key)
                .and_then(Value::as_array)
                .ok_or(TaggedError("missing list field"))
        };
        Ok(match tag {
            "undefined" => Self::Undefined,
            "dropped" => Self::Dropped,
            "num" => match text("v")?.as_str() {
                "NaN" => Self::Number(f64::NAN),
                "Infinity" => Self::Number(f64::INFINITY),
                "-Infinity" => Self::Number(f64::NEG_INFINITY),
                "-0" => Self::Number(-0.0),
                _ => return Err(TaggedError("unknown number form")),
            },
            "bigint" => {
                let digits = text("v")?;
                let body = digits.strip_prefix('-').unwrap_or(&digits);
                if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(TaggedError("bigint digits"));
                }
                Self::BigInt(digits)
            }
            "date" => match fields.get("v") {
                Some(Value::String(s)) if s == "invalid" => Self::Date(None),
                Some(Value::Number(n)) => Self::Date(Some(n.as_f64().ok_or(TaggedError("date"))?)),
                _ => return Err(TaggedError("date value")),
            },
            "regexp" => Self::RegExp {
                source: text("source")?,
                flags: text("flags")?,
            },
            "bytes" => Self::Bytes {
                kind: text("k")?,
                base64: text("v")?,
            },
            "blob" => Self::Blob {
                mime: text("mime")?,
                base64: text("v")?,
            },
            "map" => Self::Map(
                list("v")?
                    .iter()
                    .map(|pair| match pair.as_array().map(Vec::as_slice) {
                        Some([k, v]) => Ok((Self::from_json(k)?, Self::from_json(v)?)),
                        _ => Err(TaggedError("map entry")),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            "set" => Self::Set(
                list("v")?
                    .iter()
                    .map(Self::from_json)
                    .collect::<Result<_, _>>()?,
            ),
            "obj" => match fields.get("v") {
                Some(Value::Object(inner)) => Self::Object(
                    inner
                        .iter()
                        .map(|(k, v)| Ok((k.clone(), Self::from_json(v)?)))
                        .collect::<Result<_, TaggedError>>()?,
                ),
                _ => return Err(TaggedError("obj value")),
            },
            _ => return Err(TaggedError("unknown tag")),
        })
    }
}

// ---------------------------------------------------------------------------------------
// Caps and merge_and_cap
// ---------------------------------------------------------------------------------------

/// The size caps of one snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileCaps {
    pub max_bytes: usize,
    pub max_origin_bytes: usize,
    pub max_origins: usize,
    pub max_idb_records: usize,
}

impl Default for ProfileCaps {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_PLAINTEXT_BYTES,
            max_origin_bytes: MAX_ORIGIN_BYTES,
            max_origins: MAX_ORIGINS,
            max_idb_records: MAX_IDB_RECORDS,
        }
    }
}

impl ProfileCaps {
    /// The defaults with `QUARRY_BROWSER_PROFILE_MAX_BYTES` applied (a positive integer;
    /// anything else is ignored).
    pub fn from_env() -> Self {
        Self::from_max_bytes(std::env::var(MAX_BYTES_ENV).ok().as_deref())
    }

    pub fn from_max_bytes(raw: Option<&str>) -> Self {
        let mut caps = Self::default();
        if let Some(bytes) = raw
            .map(str::trim)
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|bytes| *bytes > 0)
        {
            caps.max_bytes = bytes;
        }
        caps
    }
}

/// What `merge_and_cap` did.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct CapOutcome {
    /// Something was dropped or cut: the snapshot says `truncated` on the origin(s) hit.
    pub truncated: bool,
    pub stores_dropped: u32,
    pub storage_entries_dropped: u32,
    pub origins_dropped: u32,
    /// Cookies plus the origins left still exceed `max_bytes` (the cookies alone, since
    /// everything droppable was dropped): the caller must not save.
    pub over_cap: bool,
}

/// Merge the origins captured from live tabs with the ones flushed while their pages were
/// alive, then bring the whole under the caps.
///
/// - one entry per origin, a live capture winning over a flushed one;
/// - at most `max_origins`, live origins first;
/// - per origin at most `max_origin_bytes`, and over all `max_bytes` (with `cookies_bytes`
///   counted first) and `max_idb_records`: the largest IndexedDB stores lose their
///   records first (the store's definition stays, `omitted`), then the largest web storage
///   entries; every origin hit says `truncated`;
/// - a cookie is never dropped: `cookies_bytes` is only counted. When the cookies alone
///   exceed `max_bytes` the outcome says `over_cap` and the caller refuses to save.
pub fn merge_and_cap(
    live: Vec<OriginState>,
    flushed: Vec<OriginState>,
    cookies_bytes: usize,
    caps: &ProfileCaps,
) -> (Vec<OriginState>, CapOutcome) {
    let mut outcome = CapOutcome::default();
    let mut origins: Vec<OriginState> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for state in live.into_iter().chain(flushed) {
        if state.origin.is_empty() || !seen.insert(state.origin.clone()) {
            continue;
        }
        if origins.len() >= caps.max_origins {
            outcome.origins_dropped += 1;
            outcome.truncated = true;
            continue;
        }
        origins.push(state);
    }

    // Per origin.
    for state in &mut origins {
        if state.bytes() > caps.max_origin_bytes {
            shrink_origin(state, caps.max_origin_bytes, &mut outcome);
        }
    }

    // Records over all.
    let mut records: usize = origins.iter().map(OriginState::idb_record_count).sum();
    if records > caps.max_idb_records {
        let mut stores = idb_stores_by_size(&origins);
        for slot in stores.drain(..) {
            if records <= caps.max_idb_records {
                break;
            }
            records -= omit_store(&mut origins, &slot, &mut outcome);
        }
    }

    // Bytes over all.
    let mut total = cookies_bytes + origins.iter().map(OriginState::bytes).sum::<usize>();
    if total > caps.max_bytes {
        let mut stores = idb_stores_by_size(&origins);
        for slot in stores.drain(..) {
            if total <= caps.max_bytes {
                break;
            }
            omit_store(&mut origins, &slot, &mut outcome);
            total = cookies_bytes + origins.iter().map(OriginState::bytes).sum::<usize>();
        }
    }
    while total > caps.max_bytes {
        let Some((index, side, at)) = largest_storage_entry(&origins) else {
            break;
        };
        let state = &mut origins[index];
        let list = if side == 0 {
            &mut state.local_storage
        } else {
            &mut state.session_storage
        };
        list.remove(at);
        state.truncated = true;
        outcome.truncated = true;
        outcome.storage_entries_dropped += 1;
        total = cookies_bytes + origins.iter().map(OriginState::bytes).sum::<usize>();
    }
    outcome.over_cap = total > caps.max_bytes;
    (origins, outcome)
}

/// One store's place in a snapshot, with its size at the time of listing.
struct StoreSlot {
    origin: usize,
    database: usize,
    store: usize,
    bytes: usize,
}

fn idb_stores_by_size(origins: &[OriginState]) -> Vec<StoreSlot> {
    let mut slots = Vec::new();
    for (origin, state) in origins.iter().enumerate() {
        for (database, db) in state.indexed_db.iter().enumerate() {
            for (store, entry) in db.stores.iter().enumerate() {
                if !entry.records.is_empty() {
                    slots.push(StoreSlot {
                        origin,
                        database,
                        store,
                        bytes: entry.records_bytes(),
                    });
                }
            }
        }
    }
    slots.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    slots
}

/// Empty a store's records (keeping its definition). Returns how many records went.
fn omit_store(origins: &mut [OriginState], slot: &StoreSlot, outcome: &mut CapOutcome) -> usize {
    let state = &mut origins[slot.origin];
    let store = &mut state.indexed_db[slot.database].stores[slot.store];
    let removed = store.records.len();
    store.records = Vec::new();
    store.omitted = true;
    state.truncated = true;
    outcome.truncated = true;
    outcome.stores_dropped += 1;
    removed
}

fn largest_storage_entry(origins: &[OriginState]) -> Option<(usize, u8, usize)> {
    let mut best: Option<(usize, usize, u8, usize)> = None;
    for (index, state) in origins.iter().enumerate() {
        for (side, list) in [(0u8, &state.local_storage), (1u8, &state.session_storage)] {
            for (at, (key, value)) in list.iter().enumerate() {
                let size = key.len() + value.len();
                let bigger = match best {
                    Some((current, ..)) => size > current,
                    None => true,
                };
                if bigger {
                    best = Some((size, index, side, at));
                }
            }
        }
    }
    best.map(|(_, index, side, at)| (index, side, at))
}

/// Bring one origin under `cap` bytes: IndexedDB stores (largest first), then web storage.
fn shrink_origin(state: &mut OriginState, cap: usize, outcome: &mut CapOutcome) {
    let mut one = vec![std::mem::take(state)];
    for slot in idb_stores_by_size(&one) {
        if one[0].bytes() <= cap {
            break;
        }
        omit_store(&mut one, &slot, outcome);
    }
    while one[0].bytes() > cap {
        let Some((_, side, at)) = largest_storage_entry(&one) else {
            break;
        };
        let list = if side == 0 {
            &mut one[0].local_storage
        } else {
            &mut one[0].session_storage
        };
        list.remove(at);
        one[0].truncated = true;
        outcome.truncated = true;
        outcome.storage_entries_dropped += 1;
    }
    *state = one.remove(0);
}

// ---------------------------------------------------------------------------------------
// The origin journal
// ---------------------------------------------------------------------------------------

/// The `scheme://host[:port]` of an http(s) URL, or `None` for anything else (`about:`,
/// `data:`, `file:`, `chrome:`, a URL that does not parse).
pub fn normalize_origin(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return None;
    }
    Some(parsed.origin().ascii_serialization())
}

/// Per-session record of the origins a profile-enabled session visited, and of the web
/// storage their pages flushed on `pagehide` while alive. In memory only; it goes with the
/// session. It records nothing until [`OriginJournal::enable`] (a session without a
/// persistent profile never keeps an origin list).
#[derive(Debug, Default)]
pub struct OriginJournal {
    enabled: bool,
    visited: BTreeSet<String>,
    flushed: BTreeMap<String, OriginState>,
    /// WS17.C2: what the loaded profile held for each origin. It stays in the next capture
    /// for an origin no live tab covers (and keeps its IndexedDB when a page flushed only
    /// web storage), so a session that never revisits an origin does not lose its state.
    carried: BTreeMap<String, OriginState>,
    /// Origins refused because the journal was full.
    overflow: u32,
}

impl OriginJournal {
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Record a visit. False when nothing was recorded (disabled, not http(s), already
    /// known counts as true, journal full).
    pub fn record_visit(&mut self, url_or_origin: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(origin) = normalize_origin(url_or_origin) else {
            return false;
        };
        if self.visited.contains(&origin) {
            return true;
        }
        if self.visited.len() >= MAX_ORIGINS {
            self.overflow = self.overflow.saturating_add(1);
            return false;
        }
        self.visited.insert(origin)
    }

    /// Keep what a page flushed on `pagehide`. Only for an origin the journal knows (or has
    /// room for); the latest flush of an origin replaces the earlier one.
    pub fn record_flush(&mut self, mut state: OriginState) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(origin) = normalize_origin(&state.origin) else {
            return false;
        };
        if !self.visited.contains(&origin) && !self.record_visit(&origin) {
            return false;
        }
        state.origin = origin.clone();
        state.indexed_db = Vec::new();
        state
            .local_storage
            .retain(|(key, _)| key != SEEDED_SENTINEL_KEY);
        if state.bytes() > MAX_ORIGIN_BYTES {
            return false;
        }
        self.flushed.insert(origin, state);
        true
    }

    /// WS17.C2: keep what a restored profile held per origin (see `carried`). Bounded by
    /// [`MAX_ORIGINS`]; an origin over [`MAX_ORIGIN_BYTES`] is not carried.
    pub fn carry(&mut self, states: impl IntoIterator<Item = OriginState>) {
        if !self.enabled {
            return;
        }
        for mut state in states {
            let Some(origin) = normalize_origin(&state.origin) else {
                continue;
            };
            if self.carried.len() >= MAX_ORIGINS && !self.carried.contains_key(&origin) {
                self.overflow = self.overflow.saturating_add(1);
                continue;
            }
            state.origin = origin.clone();
            state
                .local_storage
                .retain(|(key, _)| key != SEEDED_SENTINEL_KEY);
            if state.bytes() > MAX_ORIGIN_BYTES {
                continue;
            }
            self.carried.insert(origin, state);
        }
    }

    pub fn visited(&self) -> impl Iterator<Item = &String> {
        self.visited.iter()
    }

    pub fn visited_count(&self) -> usize {
        self.visited.len()
    }

    pub fn overflow(&self) -> u32 {
        self.overflow
    }

    /// The states of origins that no live tab covers: what their pages flushed on
    /// `pagehide` (with the IndexedDB the loaded profile held for them), else what the
    /// loaded profile held.
    pub fn flushed_not_in(&self, live: &BTreeSet<String>) -> Vec<OriginState> {
        let mut out: Vec<OriginState> = Vec::new();
        for (origin, state) in &self.flushed {
            if live.contains(origin) {
                continue;
            }
            let mut state = state.clone();
            if let Some(carried) = self.carried.get(origin) {
                state.indexed_db = carried.indexed_db.clone();
            }
            out.push(state);
        }
        for (origin, state) in &self.carried {
            if !live.contains(origin) && !self.flushed.contains_key(origin) {
                out.push(state.clone());
            }
        }
        out
    }

    pub fn flushed_count(&self) -> usize {
        self.flushed.len()
    }
}

// ---------------------------------------------------------------------------------------
// The capture report and the empty-capture guard
// ---------------------------------------------------------------------------------------

/// What one capture did. Counts and timings only: no cookie name, origin, URL or value.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureReport {
    pub cookies: u32,
    pub origins_live: u32,
    pub origins_flushed: u32,
    pub idb_databases: u32,
    pub idb_records: u32,
    pub local_storage_entries: u32,
    pub dropped_values: u32,
    pub truncated: bool,
    pub storage_timed_out: bool,
    pub idb_timed_out: bool,
    pub idb_skipped: bool,
    pub stores_dropped: u32,
    pub empty_skipped: bool,
    pub cookies_ms: u64,
    pub storage_ms: u64,
    pub idb_ms: u64,
}

impl CaptureReport {
    pub fn fill_from(&mut self, origins: &[OriginState], outcome: &CapOutcome) {
        self.idb_databases = origins.iter().map(|o| o.indexed_db.len() as u32).sum();
        self.idb_records = origins.iter().map(|o| o.idb_record_count() as u32).sum();
        self.local_storage_entries = origins.iter().map(|o| o.local_storage.len() as u32).sum();
        self.dropped_values = origins.iter().map(|o| o.dropped_values).sum();
        self.truncated |= outcome.truncated || origins.iter().any(|o| o.truncated);
        self.stores_dropped = outcome.stores_dropped;
    }
}

/// True when the capture must not replace what the store holds: it found no cookie and no
/// origin while the profile loaded at attach had some (a renderer that died, a context that
/// went away). The caller emits `profile_capture_empty_skipped` and does not save.
pub fn should_skip_empty_save(loaded_had_state: bool, captured: &SessionSnapshot) -> bool {
    loaded_had_state && captured.cookies.is_empty() && captured.origins.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(key: i64, value: Value) -> IdbRecord {
        IdbRecord {
            key: json!(key),
            value,
        }
    }

    fn store(name: &str, records: Vec<IdbRecord>) -> IdbStore {
        IdbStore {
            name: name.to_owned(),
            records,
            ..Default::default()
        }
    }

    fn origin_with_stores(origin: &str, stores: Vec<IdbStore>) -> OriginState {
        OriginState {
            origin: origin.to_owned(),
            indexed_db: vec![IdbDatabase {
                name: "db".to_owned(),
                version: 1,
                stores,
            }],
            ..Default::default()
        }
    }

    fn blob_of(len: usize) -> Value {
        json!({ "payload": "x".repeat(len) })
    }

    #[test]
    fn tagged_serializer_roundtrip() {
        let mut fields = BTreeMap::new();
        fields.insert("plain".to_owned(), IdbValue::String("a".to_owned()));
        fields.insert("n".to_owned(), IdbValue::Number(1.5));
        fields.insert("neg_zero".to_owned(), IdbValue::Number(-0.0));
        fields.insert("when".to_owned(), IdbValue::Date(Some(1_700_000_000_000.0)));
        fields.insert("bad_when".to_owned(), IdbValue::Date(None));
        let value = IdbValue::Array(vec![
            IdbValue::Undefined,
            IdbValue::Null,
            IdbValue::Bool(true),
            IdbValue::Number(f64::INFINITY),
            IdbValue::Number(f64::NEG_INFINITY),
            IdbValue::BigInt("-12345678901234567890".to_owned()),
            IdbValue::RegExp {
                source: "a+b".to_owned(),
                flags: "gi".to_owned(),
            },
            IdbValue::Bytes {
                kind: "Uint8Array".to_owned(),
                base64: "AQID".to_owned(),
            },
            IdbValue::Bytes {
                kind: "ArrayBuffer".to_owned(),
                base64: String::new(),
            },
            IdbValue::Blob {
                mime: "text/plain".to_owned(),
                base64: "aGk=".to_owned(),
            },
            IdbValue::Map(vec![(
                IdbValue::String("k".to_owned()),
                IdbValue::Set(vec![IdbValue::Number(1.0), IdbValue::Number(2.0)]),
            )]),
            IdbValue::Object(fields),
            IdbValue::Dropped,
        ]);
        let encoded = value.to_json();
        let text = serde_json::to_string(&encoded).unwrap();
        let decoded = IdbValue::from_json(&serde_json::from_str(&text).unwrap()).unwrap();
        assert_eq!(decoded, value);
        // NaN != NaN: check it on its own.
        let nan = IdbValue::from_json(&IdbValue::Number(f64::NAN).to_json()).unwrap();
        assert!(matches!(nan, IdbValue::Number(n) if n.is_nan()));
    }

    #[test]
    fn tagged_object_with_its_own_tag_key_is_escaped_and_restored() {
        let mut fields = BTreeMap::new();
        fields.insert("$t".to_owned(), IdbValue::String("date".to_owned()));
        fields.insert("v".to_owned(), IdbValue::Number(1.0));
        let value = IdbValue::Object(fields);
        let encoded = value.to_json();
        assert_eq!(encoded["$t"], "obj");
        assert_eq!(IdbValue::from_json(&encoded).unwrap(), value);
    }

    #[test]
    fn tagged_decoder_rejects_malformed_values() {
        for bad in [
            json!({ "$t": "bigint", "v": "12x" }),
            json!({ "$t": "num", "v": "huge" }),
            json!({ "$t": "map", "v": [[1]] }),
            json!({ "$t": "nope" }),
            json!({ "$t": 3 }),
            json!({ "$t": "bytes", "k": "Uint8Array" }),
        ] {
            assert!(IdbValue::from_json(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn merge_and_cap_drops_largest_idb_first_and_sets_truncated() {
        let small = store("small", vec![record(1, blob_of(10))]);
        let big = store("big", (0..20).map(|i| record(i, blob_of(2_000))).collect());
        let medium = store(
            "medium",
            (0..5).map(|i| record(i, blob_of(2_000))).collect(),
        );
        let origin = origin_with_stores("https://a.test", vec![small, big, medium]);
        let before = origin.bytes();
        let caps = ProfileCaps {
            max_bytes: before - 5_000,
            ..Default::default()
        };
        let (origins, outcome) = merge_and_cap(vec![origin], vec![], 100, &caps);
        assert!(outcome.truncated);
        assert!(!outcome.over_cap);
        assert_eq!(outcome.stores_dropped, 1);
        let stores = &origins[0].indexed_db[0].stores;
        let by_name = |name: &str| stores.iter().find(|s| s.name == name).unwrap();
        assert!(by_name("big").omitted && by_name("big").records.is_empty());
        assert!(!by_name("medium").omitted);
        assert!(!by_name("small").omitted);
        assert!(origins[0].truncated);
    }

    #[test]
    fn per_origin_cap_applies_before_the_total() {
        let big = store("big", (0..40).map(|i| record(i, blob_of(2_000))).collect());
        let origin = origin_with_stores("https://a.test", vec![big]);
        let caps = ProfileCaps {
            max_origin_bytes: 10_000,
            ..Default::default()
        };
        let (origins, outcome) = merge_and_cap(vec![origin], vec![], 0, &caps);
        assert!(outcome.truncated);
        assert!(origins[0].bytes() <= 10_000);
    }

    #[test]
    fn cookies_are_never_dropped_by_caps() {
        // The cookies alone are over the cap: nothing droppable is left, nothing else is
        // touched, and the outcome says over_cap so the caller refuses to save.
        let big = store("big", (0..4).map(|i| record(i, blob_of(2_000))).collect());
        let origin = origin_with_stores("https://a.test", vec![big]);
        let caps = ProfileCaps {
            max_bytes: 1_000,
            ..Default::default()
        };
        let (origins, outcome) = merge_and_cap(vec![origin], vec![], 5_000, &caps);
        assert!(outcome.over_cap);
        assert!(origins[0].indexed_db[0].stores[0].omitted);
        // merge_and_cap has no cookie parameter it could drop from: only a size.
    }

    #[test]
    fn record_cap_drops_largest_stores_until_under() {
        let a = store("a", (0..30).map(|i| record(i, json!(i))).collect());
        let b = store("b", (0..10).map(|i| record(i, blob_of(500))).collect());
        let origin = origin_with_stores("https://a.test", vec![a, b]);
        let caps = ProfileCaps {
            max_idb_records: 20,
            ..Default::default()
        };
        let (origins, outcome) = merge_and_cap(vec![origin], vec![], 0, &caps);
        let kept: usize = origins.iter().map(OriginState::idb_record_count).sum();
        assert!(kept <= 20);
        assert!(outcome.truncated);
    }

    #[test]
    fn live_capture_wins_over_a_flushed_one_and_origins_are_capped() {
        let live = OriginState {
            origin: "https://a.test".to_owned(),
            local_storage: vec![("k".to_owned(), "live".to_owned())],
            ..Default::default()
        };
        let flushed_same = OriginState {
            origin: "https://a.test".to_owned(),
            local_storage: vec![("k".to_owned(), "old".to_owned())],
            ..Default::default()
        };
        let (origins, _) =
            merge_and_cap(vec![live], vec![flushed_same], 0, &ProfileCaps::default());
        assert_eq!(origins.len(), 1);
        assert_eq!(origins[0].local_storage[0].1, "live");

        let many: Vec<OriginState> = (0..70)
            .map(|i| OriginState {
                origin: format!("https://o{i}.test"),
                ..Default::default()
            })
            .collect();
        let (origins, outcome) = merge_and_cap(vec![], many, 0, &ProfileCaps::default());
        assert_eq!(origins.len(), MAX_ORIGINS);
        assert_eq!(outcome.origins_dropped, 6);
        assert!(outcome.truncated);
    }

    #[test]
    fn journal_caps_at_64_origins_and_ignores_non_http() {
        let mut journal = OriginJournal::default();
        assert!(
            !journal.record_visit("https://a.test/"),
            "disabled records nothing"
        );
        journal.enable();
        for url in [
            "about:blank",
            "data:text/html,hi",
            "file:///etc/passwd",
            "chrome://version",
            "not a url",
        ] {
            assert!(!journal.record_visit(url), "{url}");
        }
        assert_eq!(journal.visited_count(), 0);
        for i in 0..64 {
            assert!(journal.record_visit(&format!("https://o{i}.test/path?q=1")));
        }
        assert!(!journal.record_visit("https://one-too-many.test/"));
        assert_eq!(journal.visited_count(), 64);
        assert_eq!(journal.overflow(), 1);
        // A known origin stays recordable, and a path or query never makes a new origin.
        assert!(journal.record_visit("https://o0.test/other"));
        assert_eq!(journal.visited_count(), 64);
    }

    #[test]
    fn journal_flush_keeps_web_storage_only_and_strips_the_seed_sentinel() {
        let mut journal = OriginJournal::default();
        journal.enable();
        let state = OriginState {
            origin: "https://a.test".to_owned(),
            local_storage: vec![
                ("k".to_owned(), "v".to_owned()),
                (SEEDED_SENTINEL_KEY.to_owned(), "1".to_owned()),
            ],
            indexed_db: vec![IdbDatabase::default()],
            ..Default::default()
        };
        assert!(journal.record_flush(state));
        let live: BTreeSet<String> = BTreeSet::new();
        let flushed = journal.flushed_not_in(&live);
        assert_eq!(flushed.len(), 1);
        assert_eq!(
            flushed[0].local_storage,
            vec![("k".to_owned(), "v".to_owned())]
        );
        assert!(flushed[0].indexed_db.is_empty());
        let covered: BTreeSet<String> = ["https://a.test".to_owned()].into();
        assert!(journal.flushed_not_in(&covered).is_empty());
    }

    #[test]
    fn empty_capture_does_not_replace_a_non_empty_profile() {
        let empty = SessionSnapshot::default();
        assert!(should_skip_empty_save(true, &empty));
        assert!(!should_skip_empty_save(false, &empty));
        let with_cookie = SessionSnapshot {
            cookies: vec![crate::session::Cookie::default()],
            ..Default::default()
        };
        assert!(!should_skip_empty_save(true, &with_cookie));
        let with_origin = SessionSnapshot {
            origins: vec![OriginState {
                origin: "https://a.test".to_owned(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(!should_skip_empty_save(true, &with_origin));
    }

    #[test]
    fn flags_parse_and_default() {
        assert!(!parse_flag(None, false));
        assert!(parse_flag(Some("1"), false));
        assert!(parse_flag(Some(" On "), false));
        assert!(!parse_flag(Some("0"), true));
        assert!(parse_flag(Some("garbage"), true));
        assert_eq!(ProfileCaps::from_max_bytes(Some("4096")).max_bytes, 4096);
        assert_eq!(
            ProfileCaps::from_max_bytes(Some("0")).max_bytes,
            DEFAULT_MAX_PLAINTEXT_BYTES
        );
        assert_eq!(
            ProfileCaps::from_max_bytes(Some("lots")).max_bytes,
            DEFAULT_MAX_PLAINTEXT_BYTES
        );
    }

    #[test]
    fn journal_carries_loaded_origins_and_keeps_their_indexeddb() {
        let state = |origin: &str, key: &str| OriginState {
            origin: origin.to_owned(),
            local_storage: vec![(key.to_owned(), "v".to_owned())],
            indexed_db: vec![IdbDatabase {
                name: "db".to_owned(),
                version: 1,
                stores: vec![],
            }],
            ..Default::default()
        };
        let mut journal = OriginJournal::default();
        // Disabled journal: nothing is carried.
        journal.carry([state("https://a.test", "k")]);
        assert!(journal.flushed_not_in(&BTreeSet::new()).is_empty());
        journal.enable();
        journal.carry([
            state("https://a.test", "k"),
            state("https://b.test", SEEDED_SENTINEL_KEY),
            state("about:blank", "x"),
        ]);
        // A live tab covers a.test; b.test is carried (without the sentinel); about: is not.
        let live: BTreeSet<String> = ["https://a.test".to_owned()].into();
        let rest = journal.flushed_not_in(&live);
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].origin, "https://b.test");
        assert!(rest[0].local_storage.is_empty());
        // A page flushed web storage for b.test: that wins, with the carried IndexedDB.
        journal.record_flush(OriginState {
            origin: "https://b.test".to_owned(),
            local_storage: vec![("fresh".to_owned(), "1".to_owned())],
            ..Default::default()
        });
        let rest = journal.flushed_not_in(&live);
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].local_storage, vec![("fresh".to_owned(), "1".to_owned())]);
        assert_eq!(rest[0].indexed_db.len(), 1);
    }

    #[test]
    fn normalize_origin_keeps_scheme_host_and_port_only() {
        assert_eq!(
            normalize_origin("https://Example.com:8443/a/b?c=d#e").as_deref(),
            Some("https://example.com:8443")
        );
        assert_eq!(
            normalize_origin("http://a.test").as_deref(),
            Some("http://a.test")
        );
        assert_eq!(normalize_origin("ftp://a.test"), None);
    }
}
