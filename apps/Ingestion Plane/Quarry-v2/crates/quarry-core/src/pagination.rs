//! Cursor-based pagination + common list-filter contract.
//!
//! Cycle 22 / cluster #4 part 1.
//!
//! Every Quarry list endpoint uses the same shape:
//!
//! ```text
//! GET /v1/<resource>?status=...&created_before=...&created_after=...
//!                    &limit=25&cursor=<opaque>&sort=newest
//!
//! 200 {
//!   "items": [...],
//!   "next_cursor": "..." | null,
//!   "total_estimated": 1234 | null
//! }
//! ```
//!
//! Cursors are opaque base64 of a `(created_at, id)` pair so consumers
//! don't need offset arithmetic. Clients that pass a stale cursor get
//! a fresh result-window starting at the cursor's position — no
//! pagination gaps even if rows are inserted between calls.

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, QuarryError, QuarryResult};

/// Hard ceiling on a single page. Protects the server from a malicious
/// `?limit=1000000` request and keeps memory bounded.
pub const MAX_PAGE_LIMIT: u32 = 100;

/// Default page size when the caller omits `limit`.
pub const DEFAULT_PAGE_LIMIT: u32 = 25;

/// Direction for sort. `Newest`/`Oldest` are convenience aliases for
/// `Desc`/`Asc` on `created_at`; the underlying SQL is identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    #[default]
    Newest,
    Oldest,
    Asc,
    Desc,
}

impl SortDirection {
    /// Returns `true` if the sort wants newest-first / largest-created_at
    /// first. Backends consume this to decide ORDER BY direction.
    pub fn is_descending(self) -> bool {
        matches!(self, Self::Newest | Self::Desc)
    }
}

/// Optional status filter. We avoid an enum here because each resource
/// has its own status taxonomy; the string is interpreted by the
/// backend that owns the resource.
pub type StatusFilter = String;

/// Common filter model used by every list endpoint. Each field is
/// optional so the same struct works for "list all" through "filtered
/// + paginated" without conditional types.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ListFilter {
    /// Status filter (e.g. `"running"`, `"completed"`, `"failed"`).
    /// Free-form because each resource defines its own taxonomy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<StatusFilter>,

    /// Inclusive upper bound on `created_at` (RFC3339 string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_before: Option<chrono::DateTime<chrono::Utc>>,

    /// Inclusive lower bound on `created_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_after: Option<chrono::DateTime<chrono::Utc>>,

    /// Page size. Defaulted + clamped via [`ListFilter::effective_limit`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,

    /// Opaque pagination token from a prior [`Page::next_cursor`].
    /// Server-only; clients treat it as a black box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,

    #[serde(default)]
    pub sort: SortDirection,
}

impl ListFilter {
    /// Page size, clamped to `[1, MAX_PAGE_LIMIT]`. Backends MUST use
    /// this helper rather than `limit` directly so malicious or buggy
    /// callers can't blow out memory.
    pub fn effective_limit(&self) -> u32 {
        let n = self.limit.unwrap_or(DEFAULT_PAGE_LIMIT);
        n.clamp(1, MAX_PAGE_LIMIT)
    }

    /// Decode the cursor if present. Returns `None` when the cursor
    /// is absent OR malformed (consumers treat a bad cursor as
    /// "start from the top" rather than 4xx — UX is gentler).
    pub fn decoded_cursor(&self) -> Option<Cursor> {
        self.cursor.as_deref().and_then(|s| Cursor::decode(s).ok())
    }
}

/// Encoded position into a result set. We embed `created_at` (RFC3339)
/// and `id` so the backend can resume exactly where the previous page
/// left off without offset arithmetic. Base64-URL encoded so the value
/// is safe to round-trip through query strings, cookies, or JSON
/// without escaping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub id: String,
}

impl Cursor {
    /// Build a new cursor pointing at `(created_at, id)`. The
    /// corresponding page should start AT THIS ROW (inclusive on
    /// `id`) for a stable resume.
    pub fn new(created_at: chrono::DateTime<chrono::Utc>, id: impl Into<String>) -> Self {
        Self {
            created_at,
            id: id.into(),
        }
    }

    /// Encode to a base64-URL string suitable for query params.
    pub fn encode(&self) -> String {
        // The cursor is JSON-shaped first so we can grow fields later
        // without breaking older clients (extra fields are ignored).
        let json = serde_json::to_vec(self).expect("Cursor is always serializable");
        // Use the URL-safe alphabet so we don't need to URL-encode
        // the result when callers stick it in a query string.
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
    }

    /// Decode from a base64-URL string. Returns `Err(BadRequest)` if
    /// the bytes are not valid base64 OR don't decode to a `Cursor`.
    pub fn decode(s: &str) -> QuarryResult<Self> {
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(s.as_bytes())
            .map_err(|e| QuarryError::new(ErrorCode::BadRequest, format!("cursor decode: {e}")))?;
        serde_json::from_slice::<Self>(&raw)
            .map_err(|e| QuarryError::new(ErrorCode::BadRequest, format!("cursor json: {e}")))
    }
}

/// Page response envelope used by every list endpoint.
///
/// `next_cursor` is `Some` when there's at least one more page, `None`
/// when the caller has reached the end. `total_estimated` is optional
/// because exact counts on large tables can be expensive — backends
/// MAY omit it (in which case clients show "more available" rather
/// than a hard count).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_estimated: Option<u64>,
}

impl<T> Page<T> {
    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            next_cursor: None,
            total_estimated: Some(0),
        }
    }

    /// Convenience: emit a page from a complete vec + bookkeeping.
    /// `more_available` controls whether a cursor is produced from the
    /// last item; callers compute this by reading one extra row beyond
    /// `limit` to detect the boundary cheaply.
    pub fn new(items: Vec<T>, next_cursor: Option<Cursor>, total: Option<u64>) -> Self {
        Self {
            items,
            next_cursor: next_cursor.map(|c| c.encode()),
            total_estimated: total,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn default_limit_falls_in_safe_range() {
        let f = ListFilter::default();
        let n = f.effective_limit();
        assert!((1..=MAX_PAGE_LIMIT).contains(&n));
        assert_eq!(n, DEFAULT_PAGE_LIMIT);
    }

    #[test]
    fn effective_limit_clamps_excessive_request() {
        let f = ListFilter {
            limit: Some(99999),
            ..Default::default()
        };
        assert_eq!(f.effective_limit(), MAX_PAGE_LIMIT);
    }

    #[test]
    fn effective_limit_rejects_zero() {
        let f = ListFilter {
            limit: Some(0),
            ..Default::default()
        };
        // `clamp` snaps 0 → 1 (minimum).
        assert_eq!(f.effective_limit(), 1);
    }

    #[test]
    fn cursor_roundtrips_through_encode_decode() {
        let cur = Cursor::new(
            chrono::Utc.with_ymd_and_hms(2026, 5, 19, 12, 0, 0).unwrap(),
            "art_01H...",
        );
        let s = cur.encode();
        let back = Cursor::decode(&s).unwrap();
        assert_eq!(back, cur);
    }

    #[test]
    fn cursor_is_base64_url_safe() {
        let cur = Cursor::new(chrono::Utc::now(), "art_xyz/+=");
        let s = cur.encode();
        // base64-url-safe-no-pad uses `-` and `_`, never `/` or `+` or `=`.
        assert!(!s.contains('/'));
        assert!(!s.contains('+'));
        assert!(!s.contains('='));
    }

    #[test]
    fn bad_cursor_returns_bad_request() {
        let err = Cursor::decode("not-valid-base64!!!").unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[test]
    fn bad_cursor_via_filter_decodes_to_none() {
        // `decoded_cursor` is forgiving — a bad cursor is treated as
        // "start at the top" rather than 4xx.
        let f = ListFilter {
            cursor: Some("garbage".into()),
            ..Default::default()
        };
        assert!(f.decoded_cursor().is_none());
    }

    #[test]
    fn sort_direction_descending_helper() {
        assert!(SortDirection::Newest.is_descending());
        assert!(SortDirection::Desc.is_descending());
        assert!(!SortDirection::Oldest.is_descending());
        assert!(!SortDirection::Asc.is_descending());
    }

    #[test]
    fn page_empty_is_serializable_and_terminal() {
        let p: Page<String> = Page::empty();
        assert!(p.items.is_empty());
        assert!(p.next_cursor.is_none());
        let s = serde_json::to_string(&p).unwrap();
        // Empty page must not include null cursor fields — keeps wire
        // shape clean for OpenAPI consumers.
        assert!(!s.contains("\"next_cursor\""));
    }

    #[test]
    fn page_new_emits_encoded_cursor() {
        let cur = Cursor::new(chrono::Utc::now(), "x");
        let p = Page::new(vec!["a".to_string()], Some(cur.clone()), Some(1));
        assert!(p.next_cursor.is_some());
        let decoded = Cursor::decode(&p.next_cursor.unwrap()).unwrap();
        assert_eq!(decoded, cur);
    }
}
