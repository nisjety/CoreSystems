//! Canonical URL helper for crawl identity (WS14 / E3.7.6, F-t2-041).
//!
//! Pure functions, no I/O, no tenant data. A URL is reduced to one canonical
//! string; a frontier request id is derived from that string, so the same page
//! reached through two spellings (case, default port, fragment, dot segments,
//! percent-encoding of unreserved bytes) maps to one `(org_id, request_id)` key.
//! A redirect is reconciled by canonicalising the final URL the same way.
//!
//! Default normalisation is conservative: it never changes what a server would
//! serve for a different spelling. Query order, trailing slashes, `www.` and
//! `index.html` are kept distinct on purpose. Tracking-parameter removal and
//! query sorting are opt-in through [`CanonicalOptions`].
//!
//! Errors carry a content-free class (`CanonicalError::code`), never the URL.

use url::Url;

/// Version tag mixed into the request-id hash. Changing the normalisation
/// rules means bumping this tag, so old and new ids never collide silently.
pub const CANONICAL_VERSION: &str = "canonical-url-v1";

/// Inputs longer than this are refused before parsing.
pub const MAX_INPUT_BYTES: usize = 8 * 1024;

/// Prefix of every request id produced by [`request_id_for_canonical`].
pub const REQUEST_ID_PREFIX: &str = "cu1-";

/// Hex digits of the blake3 digest kept in a request id (128 bits).
const REQUEST_ID_HEX_LEN: usize = 32;

/// Query parameters removed when `strip_tracking_params` is set. Matched
/// case-insensitively; every key that starts with `utm_` is also removed.
const TRACKING_PARAMS: &[&str] = &[
    "gclid", "fbclid", "msclkid", "yclid", "mc_cid", "mc_eid", "_ga", "_gl",
];

/// Opt-in normalisation. The `Default` value is the conservative mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CanonicalOptions {
    /// Sort query pairs by (key, value). Changes the URL for servers that
    /// read parameter order; only for callers that accept that risk.
    pub sort_query: bool,
    /// Remove common tracking parameters (`utm_*`, `gclid`, `fbclid`, ...).
    pub strip_tracking_params: bool,
}

/// Why a URL has no canonical form. None of the variants carries the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalError {
    /// Input is longer than [`MAX_INPUT_BYTES`].
    TooLong,
    /// The URL does not parse as an absolute URL.
    Unparseable,
    /// Scheme is not `http` or `https`.
    UnsupportedScheme,
    /// The URL carries a username or password. Refused, because a crawl must
    /// not treat `http://host@other/` and `http://other/` as the same target.
    Credentials,
    /// The URL has no host.
    NoHost,
}

impl CanonicalError {
    /// Content-free class name, safe to log and to put in an error envelope.
    pub fn code(&self) -> &'static str {
        match self {
            CanonicalError::TooLong => "url_too_long",
            CanonicalError::Unparseable => "url_unparseable",
            CanonicalError::UnsupportedScheme => "url_scheme_unsupported",
            CanonicalError::Credentials => "url_credentials",
            CanonicalError::NoHost => "url_no_host",
        }
    }
}

impl std::fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for CanonicalError {}

/// Canonical form of `input`. Idempotent: canonicalising the output again
/// returns the same string.
pub fn canonicalize(input: &str, opts: CanonicalOptions) -> Result<String, CanonicalError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(CanonicalError::TooLong);
    }
    let mut url = Url::parse(input).map_err(|_| CanonicalError::Unparseable)?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(CanonicalError::UnsupportedScheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CanonicalError::Credentials);
    }
    if url.host().is_none() {
        return Err(CanonicalError::NoHost);
    }

    // `url` already lowercases scheme and host, drops default ports, resolves
    // dot segments and gives an empty path "/". Fragments are not part of the
    // resource identity, so they go.
    url.set_fragment(None);

    // A trailing dot names the same DNS host: `example.com.` is `example.com`.
    let trimmed_host = match url.host() {
        Some(url::Host::Domain(d)) if d.ends_with('.') && !d.trim_end_matches('.').is_empty() => {
            Some(d.trim_end_matches('.').to_string())
        }
        _ => None,
    };
    if let Some(host) = trimmed_host {
        url.set_host(Some(host.as_str()))
            .map_err(|_| CanonicalError::Unparseable)?;
    }

    // RFC 3986 percent-encoding normalisation: decode escapes of unreserved
    // bytes and upper-case the hex of the others. Reserved escapes such as
    // `%2F` stay encoded, because decoding them changes the path.
    let path = url.path().to_string();
    let norm_path = normalize_percent_encoding(&path);
    if norm_path != path {
        url.set_path(&norm_path);
    }

    if opts.sort_query || opts.strip_tracking_params {
        if let Some(raw) = url.query().map(str::to_string) {
            let mut pairs: Vec<(String, String)> = url::form_urlencoded::parse(raw.as_bytes())
                .into_owned()
                .collect();
            if opts.strip_tracking_params {
                pairs.retain(|(key, _)| !is_tracking_param(key));
            }
            if opts.sort_query {
                pairs.sort();
            }
            if pairs.is_empty() {
                url.set_query(None);
            } else {
                url.query_pairs_mut()
                    .clear()
                    .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
            }
        }
    }

    // Conservative mode keeps the raw query order and bytes, apart from the
    // percent-encoding fix and dropping an empty `?`.
    if url.query() == Some("") {
        url.set_query(None);
    } else if !(opts.sort_query || opts.strip_tracking_params) {
        if let Some(raw) = url.query().map(str::to_string) {
            let norm = normalize_percent_encoding(&raw);
            if norm != raw {
                url.set_query(Some(norm.as_str()));
            }
        }
    }

    Ok(url.to_string())
}

/// Frontier request id for an already canonical URL. Deterministic; the same
/// canonical string always gives the same id. Shape: `cu1-` followed by 32
/// lower-case hex digits. Ids are scoped by `(org_id, request_id)` in the
/// queue, so the id is not a tenancy or authorisation key.
pub fn request_id_for_canonical(canonical: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(CANONICAL_VERSION.as_bytes());
    hasher.update(b"\n");
    hasher.update(canonical.as_bytes());
    let hex = hasher.finalize().to_hex();
    format!("{REQUEST_ID_PREFIX}{}", &hex.as_str()[..REQUEST_ID_HEX_LEN])
}

/// Canonicalise `input` and derive its request id in one call.
pub fn request_id_for_url(input: &str, opts: CanonicalOptions) -> Result<String, CanonicalError> {
    let canonical = canonicalize(input, opts)?;
    Ok(request_id_for_canonical(&canonical))
}

/// Result of matching a requested URL with the URL a fetch finally landed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconciliation {
    /// Canonical form of the URL that was requested.
    pub requested: String,
    /// Canonical form of the final URL after redirects.
    pub final_url: String,
    /// Request id of `requested`.
    pub requested_request_id: String,
    /// Request id of `final_url`.
    pub final_request_id: String,
    /// True when the canonical forms differ, that is the fetch redirected to
    /// a different resource under these options.
    pub redirected: bool,
    /// True when both canonical URLs name the same host (case and trailing
    /// dot already folded). `www.` is a different host here.
    pub same_host: bool,
}

impl Reconciliation {
    /// Request ids the frontier must treat as already seen: the requested id
    /// first, then the final id when it differs. Never contains duplicates.
    pub fn seen_request_ids(&self) -> Vec<&str> {
        if self.requested_request_id == self.final_request_id {
            vec![self.requested_request_id.as_str()]
        } else {
            vec![
                self.requested_request_id.as_str(),
                self.final_request_id.as_str(),
            ]
        }
    }
}

/// Reconcile a requested URL with the final URL of its fetch. Both inputs
/// must canonicalise; an error on either is returned, so the caller can treat
/// the fetch as unreconcilable instead of guessing.
pub fn reconcile(
    requested: &str,
    final_url: &str,
    opts: CanonicalOptions,
) -> Result<Reconciliation, CanonicalError> {
    let requested_canonical = canonicalize(requested, opts)?;
    let final_canonical = canonicalize(final_url, opts)?;
    let requested_request_id = request_id_for_canonical(&requested_canonical);
    let final_request_id = request_id_for_canonical(&final_canonical);
    let same_host = host_of(&requested_canonical) == host_of(&final_canonical);
    Ok(Reconciliation {
        redirected: requested_canonical != final_canonical,
        same_host,
        requested: requested_canonical,
        final_url: final_canonical,
        requested_request_id,
        final_request_id,
    })
}

/// Host of a canonical URL, lower-cased by `url`. Empty when none (never for
/// a canonical http(s) URL).
fn host_of(canonical: &str) -> String {
    Url::parse(canonical)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

fn is_tracking_param(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.starts_with("utm_") || TRACKING_PARAMS.contains(&lower.as_str())
}

/// Decode `%XX` escapes whose byte is unreserved (ALPHA, DIGIT, `-._~`) and
/// upper-case the hex digits of every other escape. Other bytes pass through.
/// Input comes from `url`, so it is ASCII; a non-ASCII result is refused and
/// the input is returned unchanged.
fn normalize_percent_encoding(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                let value = hi * 16 + lo;
                if is_unreserved(value) {
                    out.push(value);
                } else {
                    out.push(b'%');
                    out.push(upper_hex(hi));
                    out.push(upper_hex(lo));
                }
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    match String::from_utf8(out) {
        Ok(s) if s.is_ascii() => s,
        _ => input.to_string(),
    }
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn upper_hex(nibble: u8) -> u8 {
    match nibble {
        0..=9 => b'0' + nibble,
        _ => b'A' + (nibble - 10),
    }
}

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}
